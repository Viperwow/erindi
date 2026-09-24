//! Manual check against the real CLI: `cargo run --example codex-smoke -- <cwd> <prompt>`.

use std::time::Duration;

use erindi_core::agent::{self, Agent, AgentRequest, EventParser, Target};
use erindi_core::run::{RunSpec, run};
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let cwd = args.next().expect("cwd");
    let prompt = args.collect::<Vec<_>>().join(" ");
    let program = erindi_core::cli::locate(Agent::Codex).expect("codex not found on PATH");
    let req = AgentRequest {
        agent: Agent::Codex,
        model: None,
        permission: Some("read-only".into()),
        target: Target::New(uuid::Uuid::new_v4()),
    };
    let spec = RunSpec {
        program,
        args: agent::headless_args(&req, &cwd).unwrap(),
        cwd: cwd.into(),
        env: agent::env(Agent::Codex, std::env::vars()),
        stdin: prompt,
        timeout: Duration::from_secs(300),
    };
    let mut parser = EventParser::new(Agent::Codex);
    let outcome = run(spec, CancellationToken::new(), |line| {
        for event in parser.feed(line) {
            println!("{event:?}");
        }
    })
    .await
    .unwrap();
    println!("{:?}\n{}", outcome.end, outcome.stderr_tail);
}
