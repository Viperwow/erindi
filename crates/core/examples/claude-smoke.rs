//! Manual check against the real CLI: `cargo run --example claude-smoke -- <cwd> <prompt>`.

use std::time::Duration;

use erindi_core::claude::{ClaudeMode, ClaudeRequest, Session, claude_args, claude_env};
use erindi_core::run::{RunSpec, run};
use erindi_core::stream::parse_line;
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let cwd = args.next().expect("cwd");
    let prompt = args.collect::<Vec<_>>().join(" ");
    let session_id = uuid::Uuid::new_v4();
    let req = ClaudeRequest {
        mode: ClaudeMode::Plan,
        model: Some("haiku".into()),
        session: Session::New(session_id),
    };
    let spec = RunSpec {
        program: "claude".into(),
        args: claude_args(&req).unwrap(),
        cwd: cwd.into(),
        env: claude_env(std::env::vars()),
        stdin: prompt,
        timeout: Duration::from_secs(120),
    };
    let outcome = run(spec, CancellationToken::new(), |line| {
        for event in parse_line(line) {
            println!("{event:?}");
        }
    })
    .await
    .unwrap();
    println!(
        "{:?} session={session_id}\n{}",
        outcome.end, outcome.stderr_tail
    );
}
