use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use process_wrap::tokio::*;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio_util::sync::CancellationToken;

/// Longest stdout line passed to the caller; longer lines are dropped.
pub const MAX_LINE: usize = 1 << 20;
/// How much of stderr is kept for error reporting.
pub const STDERR_TAIL: usize = 4 << 10;

#[derive(Debug, Clone)]
pub struct RunSpec {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    /// The complete child environment; nothing is inherited.
    pub env: Vec<(String, String)>,
    pub stdin: String,
    pub timeout: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunEnd {
    Exited { success: bool },
    Cancelled,
    TimedOut,
}

#[derive(Debug)]
pub struct RunOutcome {
    pub end: RunEnd,
    pub stderr_tail: String,
}

/// Runs `spec` as a process tree that is killed as a whole on cancel, timeout or drop.
pub async fn run(
    spec: RunSpec,
    cancel: CancellationToken,
    mut on_line: impl FnMut(&str),
) -> std::io::Result<RunOutcome> {
    let mut command = tokio::process::Command::new(&spec.program);
    command
        .args(&spec.args)
        .current_dir(&spec.cwd)
        .env_clear()
        .envs(spec.env.iter().map(|(k, v)| (k, v)))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut wrap = CommandWrap::from(command);
    wrap.wrap(KillOnDrop);
    #[cfg(windows)]
    {
        use windows::Win32::System::Threading::CREATE_NO_WINDOW;
        wrap.wrap(CreationFlags(CREATE_NO_WINDOW)).wrap(JobObject);
    }
    #[cfg(unix)]
    wrap.wrap(ProcessGroup::leader());
    let mut child = wrap.spawn()?;

    let mut stdin = child.stdin().take().expect("piped stdin");
    let input = spec.stdin;
    // A child that exits without reading stdin makes this write fail; the exit status reports that.
    let stdin_task = tokio::spawn(async move {
        let _ = stdin.write_all(input.as_bytes()).await;
    });

    let mut stderr = child.stderr().take().expect("piped stderr");
    let stderr_task = tokio::spawn(async move {
        let mut tail = Vec::new();
        let mut buf = [0u8; 4096];
        while let Ok(n @ 1..) = stderr.read(&mut buf).await {
            tail.extend_from_slice(&buf[..n]);
            if tail.len() > STDERR_TAIL {
                tail.drain(..tail.len() - STDERR_TAIL);
            }
        }
        String::from_utf8_lossy(&tail).into_owned()
    });

    let mut stdout = BufReader::new(child.stdout().take().expect("piped stdout"));
    let mut line = Vec::new();
    let deadline = tokio::time::sleep(spec.timeout);
    tokio::pin!(deadline);

    let interrupted = loop {
        tokio::select! {
            read = read_line(&mut stdout, &mut line) => match read {
                Ok(true) => on_line(&String::from_utf8_lossy(&line)),
                Ok(false) | Err(_) => break None,
            },
            _ = cancel.cancelled() => break Some(RunEnd::Cancelled),
            _ = &mut deadline => break Some(RunEnd::TimedOut),
        }
    };

    let end = match interrupted {
        Some(end) => {
            Box::into_pin(child.kill()).await?;
            end
        }
        None => RunEnd::Exited {
            success: child.wait().await?.success(),
        },
    };
    stdin_task.abort();
    let stderr_tail = stderr_task.await.unwrap_or_default();
    Ok(RunOutcome { end, stderr_tail })
}

/// Reads the next line of at most [`MAX_LINE`] bytes into `line`, skipping longer ones.
/// Returns `false` at end of stream.
async fn read_line(
    reader: &mut (impl AsyncBufRead + Unpin),
    line: &mut Vec<u8>,
) -> std::io::Result<bool> {
    line.clear();
    let mut oversized = false;
    loop {
        let chunk = reader.fill_buf().await?;
        if chunk.is_empty() {
            return Ok(!line.is_empty() && !oversized);
        }
        let (part, used, done) = match chunk.iter().position(|&b| b == b'\n') {
            Some(i) => (&chunk[..i], i + 1, true),
            None => (chunk, chunk.len(), false),
        };
        if !oversized && line.len() + part.len() <= MAX_LINE {
            line.extend_from_slice(part);
        } else {
            oversized = true;
            line.clear();
        }
        reader.consume(used);
        if done {
            if !oversized {
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                return Ok(true);
            }
            oversized = false;
        }
    }
}
