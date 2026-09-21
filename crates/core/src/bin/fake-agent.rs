//! Test double for agent CLIs. The first argument selects the behavior.

use std::io::{Read, Write};
use std::time::Duration;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = args.first().map(String::as_str).unwrap_or("");
    let mut out = std::io::stdout();
    match mode {
        "echo" => {
            let mut stdin = String::new();
            std::io::stdin().read_to_string(&mut stdin).unwrap();
            let report = serde_json::json!({
                "stdin": stdin,
                "args": args,
                "cwd": std::env::current_dir().unwrap(),
                "env": std::env::vars().collect::<std::collections::BTreeMap<_, _>>(),
            });
            writeln!(out, "{report}").unwrap();
        }
        "tree" => {
            let child = std::process::Command::new(std::env::current_exe().unwrap())
                .arg("sleep")
                .spawn()
                .unwrap();
            writeln!(out, "{}", child.id()).unwrap();
            out.flush().unwrap();
            std::thread::sleep(Duration::from_secs(60));
        }
        "sleep" => std::thread::sleep(Duration::from_secs(60)),
        "flood" => {
            writeln!(out, "{}", "x".repeat(4 << 20)).unwrap();
            writeln!(out, "tail").unwrap();
        }
        "fail" => {
            eprintln!("{}boom", "noise ".repeat(10_000));
            std::process::exit(3);
        }
        other => panic!("unknown mode {other:?}"),
    }
}
