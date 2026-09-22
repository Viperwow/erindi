//! How well patterns and the local model recognise voice commands.
//! cargo run -p erindi-core --release --example command-bench -- <llama-server.exe> <model.gguf>

use std::path::Path;
use std::time::{Duration, Instant};

use erindi_core::classify::{WARM_UP, accept};
use erindi_core::commands::{Command, Parser, Patterns};
use erindi_core::llama::LlamaServer;
use serde::Deserialize;

const BUDGET: Duration = Duration::from_millis(1500);

#[derive(Deserialize)]
struct Case {
    say: String,
    command: String,
}

fn name(command: Option<Command>) -> &'static str {
    match command {
        Some(Command::NewSession) => "new_session",
        Some(Command::OpenTerminal) => "open_terminal",
        Some(Command::Cancel) => "cancel",
        None => "none",
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let server = LlamaServer::start(Path::new(&args[1]), Path::new(&args[2])).expect("server");
    let _ = server.classify(WARM_UP);
    let parser = Parser::new(&Patterns::default()).unwrap();

    let cases: Vec<Case> = include_str!("command-cases.jsonl")
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect(l))
        .collect();

    let (mut times, mut parser_ok, mut final_ok, mut false_commands) = (vec![], 0, 0, 0);
    let (mut missed, mut model_fixed) = (0, 0);
    for c in &cases {
        let by_parser = parser.parse(&c.say).0.first().copied();
        let mut result = by_parser;
        let mut took = Duration::ZERO;
        if by_parser.is_none() {
            let started = Instant::now();
            let answer = server.classify(&c.say).ok().flatten();
            took = started.elapsed();
            times.push(took);
            result = accept(&c.say, answer).map(|(command, _)| command);
            if c.command != "none" {
                missed += 1;
                model_fixed += usize::from(name(result) == c.command);
            }
        }
        parser_ok += usize::from(name(by_parser) == c.command);
        final_ok += usize::from(name(result) == c.command);
        false_commands += usize::from(c.command == "none" && result.is_some());
        let warn = if took > BUDGET { "WARN" } else { "    " };
        let mark = if name(result) == c.command {
            "ok "
        } else {
            "BAD"
        };
        println!(
            "{warn} {mark} {:>4} ms  parser={:<13} final={:<13} want={:<13} {}",
            took.as_millis(),
            name(by_parser),
            name(result),
            c.command,
            c.say
        );
    }

    times.sort();
    let pct = |p: usize| {
        times
            .get((times.len().max(1) - 1) * p / 100)
            .map_or(0, |t| t.as_millis())
    };
    let n = cases.len();
    println!(
        "\n{n} cases; model asked {} times: p50 {} ms, p90 {} ms, max {} ms",
        times.len(),
        pct(50),
        pct(90),
        pct(100)
    );
    println!("patterns alone {parser_ok}/{n}; patterns + model {final_ok}/{n}");
    println!(
        "model found {model_fixed}/{missed} commands the patterns missed; false commands on plain tasks: {false_commands}"
    );
}
