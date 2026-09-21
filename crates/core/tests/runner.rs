use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde_json::Value;
use tokio_util::sync::CancellationToken;
use whispio_core::run::{RunEnd, RunSpec, run};

fn spec(mode: &str, cwd: PathBuf) -> RunSpec {
    RunSpec {
        program: env!("CARGO_BIN_EXE_fake-agent").into(),
        args: vec![mode.into()],
        cwd,
        env: vec![("WHISPIO_ALLOWED".into(), "yes".into())],
        stdin: String::new(),
        timeout: Duration::from_secs(20),
    }
}

async fn collect(spec: RunSpec, cancel: CancellationToken) -> (RunEnd, Vec<String>, String) {
    let mut lines = vec![];
    let outcome = run(spec, cancel, |l| lines.push(l.to_string()))
        .await
        .unwrap();
    (outcome.end, lines, outcome.stderr_tail)
}

fn alive(pid: u32) -> bool {
    let out = std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).contains(&pid.to_string())
}

#[tokio::test]
async fn hostile_prompt_reaches_stdin_verbatim() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().join("dir with spaces");
    std::fs::create_dir(&cwd).unwrap();
    let prompt = format!(
        "; && || | `whoami` $(whoami) 'q' \"dq\"\nnext line\r\n\
         & del C:\\x %PATH% $env:PATH; Remove-Item -Recurse C:\\ \
         C:\\Program Files\\x 🚀 привет {}",
        "долгий ".repeat(20_000)
    );
    let mut s = spec("echo", cwd.clone());
    s.args.push("fixed-arg".into());
    s.stdin = prompt.clone();

    let (end, lines, _) = collect(s, CancellationToken::new()).await;

    assert_eq!(end, RunEnd::Exited { success: true });
    let report: Value = serde_json::from_str(&lines[0]).unwrap();
    assert_eq!(report["stdin"], prompt.as_str());
    assert_eq!(report["args"], serde_json::json!(["echo", "fixed-arg"]));
    assert_eq!(
        std::fs::canonicalize(report["cwd"].as_str().unwrap()).unwrap(),
        std::fs::canonicalize(&cwd).unwrap()
    );
}

#[tokio::test]
async fn child_gets_only_given_env() {
    let dir = tempfile::tempdir().unwrap();
    let (_, lines, _) = collect(spec("echo", dir.path().into()), CancellationToken::new()).await;
    let report: Value = serde_json::from_str(&lines[0]).unwrap();
    let env = report["env"].as_object().unwrap();
    assert_eq!(env["WHISPIO_ALLOWED"], "yes");
    assert!(!env.contains_key("PATH"), "inherited env leaked: {env:?}");
}

#[tokio::test]
async fn cancel_kills_whole_tree() {
    let dir = tempfile::tempdir().unwrap();
    let cancel = CancellationToken::new();
    let mut grandchild = None;
    let started = Instant::now();
    let c = cancel.clone();
    let outcome = run(spec("tree", dir.path().into()), cancel, |l| {
        grandchild = l.parse::<u32>().ok();
        c.cancel();
    })
    .await
    .unwrap();

    assert_eq!(outcome.end, RunEnd::Cancelled);
    assert!(started.elapsed() < Duration::from_secs(10));
    let pid = grandchild.expect("grandchild pid");
    assert!(!alive(pid), "grandchild {pid} survived cancel");
}

#[tokio::test]
async fn timeout_kills_run() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = spec("sleep", dir.path().into());
    s.timeout = Duration::from_millis(300);
    let (end, _, _) = collect(s, CancellationToken::new()).await;
    assert_eq!(end, RunEnd::TimedOut);
}

#[tokio::test]
async fn oversized_lines_are_dropped() {
    let dir = tempfile::tempdir().unwrap();
    let (end, lines, stderr) =
        collect(spec("flood", dir.path().into()), CancellationToken::new()).await;
    assert_eq!(end, RunEnd::Exited { success: true }, "{stderr}");
    assert_eq!(lines, ["tail"], "{stderr}");
}

#[tokio::test]
async fn failure_keeps_stderr_tail() {
    let dir = tempfile::tempdir().unwrap();
    let (end, _, stderr) = collect(spec("fail", dir.path().into()), CancellationToken::new()).await;
    assert_eq!(end, RunEnd::Exited { success: false });
    assert!(stderr.trim_end().ends_with("boom"));
    assert!(stderr.len() <= whispio_core::run::STDERR_TAIL);
}

#[tokio::test]
async fn missing_program_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = spec("echo", dir.path().into());
    s.program = dir.path().join("no-such-agent.exe");
    assert!(run(s, CancellationToken::new(), |_| {}).await.is_err());
}
