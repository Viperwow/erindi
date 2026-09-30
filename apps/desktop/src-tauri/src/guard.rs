//! macOS has no job objects: a child outlives an Erindi that crashed or was killed. A second
//! Erindi process holds a pipe to the first and kills the tracked process groups when it closes.

use std::io::BufRead;
use std::process::ChildStdin;
use std::sync::Mutex;

static PIPE: Mutex<Option<ChildStdin>> = Mutex::new(None);

/// Starts the guard on macOS; Windows ties children to Erindi with job objects instead.
pub fn start() {
    #[cfg(target_os = "macos")]
    {
        let spawned = std::env::current_exe().and_then(|exe| {
            std::process::Command::new(exe)
                .arg("--guard")
                .stdin(std::process::Stdio::piped())
                .spawn()
        });
        match spawned {
            Ok(mut guard) => *PIPE.lock().unwrap() = guard.stdin.take(),
            Err(e) => eprintln!("Cannot start the process guard: {e}"),
        }
    }
}

/// Hands process group `pgid` to the guard.
pub fn track(pgid: u32) {
    send(&format!("+{pgid}"));
}

/// Takes back a group that ended, so a later process reusing its id is never killed.
pub fn untrack(pgid: u32) {
    send(&format!("-{pgid}"));
}

fn send(line: &str) {
    use std::io::Write;
    if let Some(pipe) = PIPE.lock().unwrap().as_mut() {
        let _ = writeln!(pipe, "{line}");
    }
}

/// The guard process: reads group ids until Erindi's end of the pipe closes, then kills them.
pub fn run() {
    watch(std::io::stdin().lock());
}

fn watch(input: impl BufRead) {
    let mut live = std::collections::BTreeSet::new();
    for line in input.lines().map_while(Result::ok) {
        let (op, id) = line.trim().split_at_checked(1).unwrap_or_default();
        match (op, id.parse::<u32>()) {
            ("+", Ok(id)) => live.insert(id),
            ("-", Ok(id)) => live.remove(&id),
            _ => false,
        };
    }
    if live.is_empty() {
        return;
    }
    let groups: Vec<String> = live.iter().map(|id| format!("-{id}")).collect();
    signal("-TERM", &groups);
    std::thread::sleep(std::time::Duration::from_secs(1));
    signal("-KILL", &groups);
}

fn signal(sig: &str, groups: &[String]) {
    let _ = std::process::Command::new("kill")
        .arg(sig)
        .arg("--")
        .args(groups)
        .stderr(std::process::Stdio::null())
        .status();
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn closing_the_pipe_kills_tracked_groups() {
        use std::os::unix::process::CommandExt;
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .process_group(0)
            .spawn()
            .unwrap();
        let pid = child.id();
        let (reader, mut writer) = std::io::pipe().unwrap();
        let guard = std::thread::spawn(move || watch(std::io::BufReader::new(reader)));
        use std::io::Write;
        writeln!(writer, "+{pid}").unwrap();
        drop(writer);
        guard.join().unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while child.try_wait().unwrap().is_none() {
            assert!(
                std::time::Instant::now() < deadline,
                "the child survived the guard"
            );
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }

    #[test]
    fn an_untracked_group_is_left_alone() {
        use std::os::unix::process::CommandExt;
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .process_group(0)
            .spawn()
            .unwrap();
        let pid = child.id();
        let (reader, mut writer) = std::io::pipe().unwrap();
        use std::io::Write;
        writeln!(
            writer,
            "+{pid}
-{pid}"
        )
        .unwrap();
        drop(writer);
        watch(std::io::BufReader::new(reader));
        assert!(child.try_wait().unwrap().is_none());
        child.kill().unwrap();
    }
}
