use serde_json::Value;

use crate::agent::ModelOption;
use crate::stream::RunEvent;

/// Windows basics shared with Claude, Pi's own settings, cloud credentials, and any provider's API key.
/// `--session-id` creates the session when it is missing, so new and continued runs look alike.
/// A continued session keeps its own model, so only a new one gets `--model`.
pub fn print_args(model: Option<&str>, session_id: &str, new: bool) -> Vec<String> {
    let mut args: Vec<String> = ["-p", "--mode", "json", "--session-id", session_id]
        .map(String::from)
        .into();
    if let Some(model) = model.filter(|_| new) {
        args.extend(["--model".into(), model.into()]);
    }
    args
}

/// The prompt follows `--`, so it can never be read as an option.
pub fn terminal_args(
    program: &str,
    model: Option<&str>,
    session_id: &str,
    new: bool,
    prompt: &str,
) -> Vec<String> {
    let mut args = vec![program.into(), "--session-id".into(), session_id.into()];
    if let Some(model) = model.filter(|_| new) {
        args.extend(["--model".into(), model.into()]);
    }
    if !prompt.is_empty() {
        args.extend(["--".into(), prompt.into()]);
    }
    args
}

pub fn resume_in_terminal(program: &str, native_id: &str) -> Vec<String> {
    [program, "--session", native_id].map(String::from).into()
}

/// Pi retries a failed request on its own, so a run fails only when its last reply failed.
#[derive(Default)]
pub struct Parser {
    error: Option<String>,
}

impl Parser {
    /// Unknown, malformed or irrelevant lines yield no events.
    pub fn feed(&mut self, line: &str) -> Vec<RunEvent> {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            return vec![];
        };
        match v["type"].as_str() {
            Some("session") => match v["id"].as_str() {
                Some(id) => vec![RunEvent::SessionStarted {
                    native_id: id.into(),
                }],
                None => vec![],
            },
            Some("message_end") if v["message"]["role"] == "assistant" => {
                self.assistant(&v["message"])
            }
            Some("agent_end") => vec![RunEvent::Result {
                ok: self.error.is_none(),
                text: self.error.clone().unwrap_or_default(),
            }],
            Some("auto_retry_end") if v["success"] == false => {
                let text = v["finalError"].as_str().unwrap_or_default().to_string();
                self.error = Some(text.clone());
                vec![RunEvent::Result { ok: false, text }]
            }
            _ => vec![],
        }
    }

    fn assistant(&mut self, message: &Value) -> Vec<RunEvent> {
        let mut events = vec![];
        let mut reply = String::new();
        for part in message["content"].as_array().into_iter().flatten() {
            match part["type"].as_str() {
                Some("text") => reply.push_str(part["text"].as_str().unwrap_or_default()),
                Some("toolCall") => events.push(RunEvent::ToolUse {
                    name: tool_name(part),
                }),
                _ => {}
            }
        }
        if !reply.trim().is_empty() {
            events.push(RunEvent::Reply {
                text: reply.trim().to_string(),
            });
        }
        self.error = match message["stopReason"].as_str() {
            Some("error" | "aborted") => Some(
                message["errorMessage"]
                    .as_str()
                    .unwrap_or("Pi failed")
                    .to_string(),
            ),
            _ => None,
        };
        events
    }
}

/// `bash` shows its command, file tools show their path, the rest show their name.
fn tool_name(call: &Value) -> String {
    let name = call["name"].as_str().unwrap_or_default();
    let args = &call["arguments"];
    if let Some(command) = args["command"].as_str() {
        return command.to_string();
    }
    match args["path"].as_str() {
        Some(path) => format!("{name} {path}"),
        None => name.to_string(),
    }
}

/// Models from the `pi --list-models` table as `provider/model`.
pub fn parse_models(table: &str) -> Vec<ModelOption> {
    table
        .lines()
        .skip_while(|l| !l.trim_start().starts_with("provider"))
        .skip(1)
        .filter_map(|l| {
            let mut cols = l.split_whitespace();
            let (provider, model) = (cols.next()?, cols.next()?);
            Some(ModelOption {
                id: format!("{provider}/{model}"),
                label: format!("{model} ({provider})"),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const RUN: &str = include_str!("../tests/fixtures/pi-print.jsonl");
    const FAILED: &str = include_str!("../tests/fixtures/pi-print-failed.jsonl");

    fn events(fixture: &str) -> Vec<RunEvent> {
        let mut parser = Parser::default();
        fixture.lines().flat_map(|l| parser.feed(l)).collect()
    }

    #[test]
    fn events_from_a_run_with_a_tool() {
        assert_eq!(
            events(RUN),
            [
                RunEvent::SessionStarted {
                    native_id: "01a0e976-6857-762c-a700-c31f66fa9cc0".into()
                },
                RunEvent::ToolUse {
                    name: "git --version".into()
                },
                RunEvent::Reply {
                    text: "done".into()
                },
                RunEvent::Result {
                    ok: true,
                    text: String::new()
                },
            ]
        );
    }

    #[test]
    fn a_run_fails_when_every_retry_failed() {
        let got = events(FAILED);
        assert_eq!(
            got.last(),
            Some(&RunEvent::Result {
                ok: false,
                text: "502 \"acp-proxy failure\"".into()
            })
        );
        assert!(
            !got.iter()
                .any(|e| matches!(e, RunEvent::Result { ok: true, .. }))
        );
    }

    #[test]
    fn a_successful_retry_clears_the_error() {
        let mut parser = Parser::default();
        let failed = r#"{"type":"message_end","message":{"role":"assistant","content":[],"stopReason":"error","errorMessage":"502"}}"#;
        let fine = r#"{"type":"message_end","message":{"role":"assistant","content":[{"type":"text","text":"ok"}],"stopReason":"stop"}}"#;
        parser.feed(failed);
        parser.feed(fine);
        assert_eq!(
            parser.feed(r#"{"type":"agent_end","messages":[]}"#),
            [RunEvent::Result {
                ok: true,
                text: String::new()
            }]
        );
    }

    #[test]
    fn args_pass_the_model_only_to_a_new_session() {
        assert_eq!(
            print_args(Some("lmstudio/m"), "abc", true),
            [
                "-p",
                "--mode",
                "json",
                "--session-id",
                "abc",
                "--model",
                "lmstudio/m"
            ]
        );
        assert_eq!(
            print_args(Some("lmstudio/m"), "abc", false),
            ["-p", "--mode", "json", "--session-id", "abc"]
        );
        assert_eq!(
            terminal_args("pi", None, "abc", true, "a; b"),
            ["pi", "--session-id", "abc", "--", "a; b"]
        );
        assert_eq!(resume_in_terminal("pi", "abc"), ["pi", "--session", "abc"]);
    }

    #[test]
    fn models_from_the_list_table() {
        let table = "provider  model                context  max-out  thinking  images\n\
                     lmstudio  prism-ml/bonsai-27b  128K     16.4K    no        yes\n\
                     anthropic claude-sonnet-5      1M       64K      yes       yes\n";
        assert_eq!(
            parse_models(table),
            [
                ModelOption {
                    id: "lmstudio/prism-ml/bonsai-27b".into(),
                    label: "prism-ml/bonsai-27b (lmstudio)".into()
                },
                ModelOption {
                    id: "anthropic/claude-sonnet-5".into(),
                    label: "claude-sonnet-5 (anthropic)".into()
                },
            ]
        );
        assert_eq!(parse_models("No models available.\n"), []);
    }
}
