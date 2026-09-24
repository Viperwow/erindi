use serde_json::Value;

use crate::agent::Target;
use crate::stream::RunEvent;

/// Windows basics shared with Claude, plus what Codex and its Node launcher read.
const ENV_ALLOW_PREFIX: &[&str] = &["CODEX_", "OPENAI_"];

pub fn codex_env(vars: impl IntoIterator<Item = (String, String)>) -> Vec<(String, String)> {
    vars.into_iter()
        .filter(|(k, _)| {
            let k = k.to_uppercase();
            crate::claude::base_env_allowed(&k) || ENV_ALLOW_PREFIX.iter().any(|p| k.starts_with(p))
        })
        .collect()
}

/// A native ID Codex could read as an option is refused by the caller before this runs.
pub fn exec_args(
    model: Option<&str>,
    sandbox: Option<&str>,
    target: &Target,
    cwd: &str,
) -> Vec<String> {
    match target {
        Target::Resume(id) => ["exec", "resume", id.as_str(), "--json"]
            .map(String::from)
            .into(),
        Target::New(_) => {
            let mut args: Vec<String> = ["exec", "--json", "-C", cwd].map(String::from).into();
            args.extend(options(model, sandbox));
            args
        }
    }
}

fn options(model: Option<&str>, sandbox: Option<&str>) -> Vec<String> {
    let mut args = vec![];
    if let Some(model) = model {
        args.extend(["-m".into(), model.to_string()]);
    }
    if let Some(sandbox) = sandbox {
        args.extend(["-s".into(), sandbox.to_string()]);
    }
    args
}

/// The prompt follows `--`, and its `;` is escaped because Windows Terminal splits commands there.
pub fn terminal_args(
    program: &str,
    cwd: &str,
    model: Option<&str>,
    sandbox: Option<&str>,
    prompt: &str,
) -> Vec<String> {
    let mut args = vec!["-d".into(), cwd.into(), program.into()];
    args.extend(options(model, sandbox));
    if !prompt.is_empty() {
        args.extend(["--".into(), prompt.replace(';', r"\;")]);
    }
    args
}

pub fn resume_in_terminal(program: &str, cwd: &str, native_id: &str) -> Vec<String> {
    ["-d", cwd, program, "resume", native_id]
        .map(String::from)
        .into()
}

/// Unknown, malformed or irrelevant lines yield no events.
pub fn parse_line(line: &str) -> Vec<RunEvent> {
    let Ok(v) = serde_json::from_str::<Value>(line) else {
        return vec![];
    };
    let text = |v: &Value| v.as_str().unwrap_or_default().to_string();
    let item = &v["item"];
    match (v["type"].as_str(), item["type"].as_str()) {
        (Some("thread.started"), _) => match v["thread_id"].as_str() {
            Some(id) => vec![RunEvent::SessionStarted {
                native_id: id.into(),
            }],
            None => vec![],
        },
        (Some("item.started"), Some("command_execution")) => vec![RunEvent::ToolUse {
            name: text(&item["command"]),
        }],
        (Some("item.started"), Some("file_change")) => {
            let path = text(&item["changes"][0]["path"]);
            vec![RunEvent::ToolUse {
                name: format!("Edit {path}"),
            }]
        }
        (Some("item.completed"), Some("agent_message")) => vec![RunEvent::Reply {
            text: text(&item["text"]),
        }],
        (Some("turn.completed"), _) => vec![RunEvent::Result {
            ok: true,
            text: String::new(),
        }],
        (Some("turn.failed"), _) => vec![RunEvent::Result {
            ok: false,
            text: text(&v["error"]["message"]),
        }],
        (Some("error"), _) => vec![RunEvent::Result {
            ok: false,
            text: text(&v["message"]),
        }],
        _ => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../tests/fixtures/codex-exec.jsonl");

    #[test]
    fn events_from_a_real_run() {
        let events: Vec<_> = FIXTURE.lines().flat_map(parse_line).collect();
        assert_eq!(
            events,
            [
                RunEvent::SessionStarted {
                    native_id: "01a0d2c0-0c6d-7dc0-90c7-da4ffbaf65a2".into()
                },
                RunEvent::ToolUse {
                    name: "git diff".into()
                },
                RunEvent::ToolUse {
                    name: "Edit src/a.rs".into()
                },
                RunEvent::Reply {
                    text: "pong".into()
                },
                RunEvent::Result {
                    ok: true,
                    text: String::new()
                },
            ]
        );
    }

    #[test]
    fn failures() {
        let line = r#"{"type":"turn.failed","error":{"message":"stream disconnected"}}"#;
        assert_eq!(
            parse_line(line),
            [RunEvent::Result {
                ok: false,
                text: "stream disconnected".into()
            }]
        );
        let line = r#"{"type":"error","message":"Not logged in"}"#;
        assert_eq!(
            parse_line(line),
            [RunEvent::Result {
                ok: false,
                text: "Not logged in".into()
            }]
        );
    }

    #[test]
    fn ignores_noise_and_garbage() {
        for line in [
            "",
            "x",
            "{\"type\":",
            "[1]",
            r#"{"type":"turn.started"}"#,
            r#"{"type":"item.started","item":"oops"}"#,
            r#"{"type":"item.completed","item":{"type":"reasoning","text":"…"}}"#,
        ] {
            assert_eq!(parse_line(line), [], "{line}");
        }
    }
}
