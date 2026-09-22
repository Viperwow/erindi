//! Speed and quality of the prompt refiner.
//! cargo run -p erindi-core --release --example refine-bench -- <llama-server.exe> <model.gguf>

use std::path::Path;
use std::time::{Duration, Instant};

use erindi_core::llama::LlamaServer;
use erindi_core::refine::accept;
use erindi_core::session::{Intent, parse_intent};
use serde::Deserialize;

const BUDGET: Duration = Duration::from_millis(1500);

#[derive(Deserialize)]
struct Case {
    say: String,
    intent: String,
    keep: Vec<String>,
}

fn name(i: Intent) -> &'static str {
    match i {
        Intent::New => "new",
        Intent::Continue => "continue",
        Intent::Unspecified => "none",
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let server = LlamaServer::start(Path::new(&args[1]), Path::new(&args[2])).expect("server");
    let _ = server.refine(erindi_core::refine::WARM_UP);

    let cases: Vec<Case> = include_str!("refine-cases.jsonl")
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect(l))
        .collect();

    let (mut times, mut parser_ok, mut model_ok, mut kept) = (vec![], 0, 0, 0);
    for c in &cases {
        let (parsed, input) = parse_intent(&c.say);
        let started = Instant::now();
        let refined = server.refine(&input).ok().flatten();
        let took = started.elapsed();
        times.push(took);
        let model_intent = refined.as_ref().map_or("fail", |r| name(r.intent));
        let text = accept(&input, refined).map_or(input.clone(), |r| r.text);
        let lower = text.to_lowercase();
        let missing: Vec<&str> = c
            .keep
            .iter()
            .map(String::as_str)
            .filter(|k| !lower.contains(&k.to_lowercase()))
            .collect();
        parser_ok += usize::from(name(parsed) == c.intent);
        model_ok += usize::from(model_intent == c.intent);
        kept += usize::from(missing.is_empty());
        let warn = if took > BUDGET { "WARN" } else { "    " };
        println!(
            "{warn} {:>5} ms  parser={:<8} model={:<8} want={:<8} {}",
            took.as_millis(),
            name(parsed),
            model_intent,
            c.intent,
            text
        );
        if !missing.is_empty() {
            println!("      lost: {missing:?}");
        }
    }

    times.sort();
    let pct = |p: usize| times[(times.len() - 1) * p / 100].as_millis();
    let n = cases.len();
    println!(
        "\n{n} cases  p50 {} ms  p90 {} ms  max {} ms  over budget {}",
        pct(50),
        pct(90),
        pct(100),
        times.iter().filter(|t| **t > BUDGET).count()
    );
    println!("intent: parser {parser_ok}/{n}, model {model_ok}/{n}; meaning kept {kept}/{n}");
}
