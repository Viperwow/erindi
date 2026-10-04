use std::path::Path;

use serde_json::Value;

use crate::agent::ModelOption;
use crate::stream::RunEvent;

/// Cursor reads the prompt from stdin. A permission is a mode (`plan`, `ask`) or `force`, which
/// runs every command without asking; without one, Cursor asks, and in print mode that denies.
pub fn print_args(
    model: Option<&str>,
    permission: Option<&str>,
    resume: Option<&str>,
) -> Vec<String> {
    let mut args: Vec<String> = ["-p", "--output-format", "stream-json"]
        .map(String::from)
        .into();
    args.extend(shared(model, permission, resume));
    args
}

/// The prompt follows `--`, so it can never be read as an option.
pub fn terminal_args(
    program: &str,
    model: Option<&str>,
    permission: Option<&str>,
    resume: Option<&str>,
    prompt: &str,
) -> Vec<String> {
    let mut args = vec![program.to_string()];
    args.extend(shared(model, permission, resume));
    if !prompt.is_empty() {
        args.extend(["--".into(), prompt.into()]);
    }
    args
}

fn shared(model: Option<&str>, permission: Option<&str>, resume: Option<&str>) -> Vec<String> {
    let mut args = vec![];
    if let Some(model) = model {
        args.extend(["--model".into(), model.into()]);
    }
    match permission {
        Some("force") => args.push("--force".into()),
        Some(mode) => args.extend(["--mode".into(), mode.into()]),
        None => {}
    }
    if let Some(id) = resume {
        args.extend(["--resume".into(), id.into()]);
    }
    args
}

pub fn resume_in_terminal(program: &str, native_id: &str) -> Vec<String> {
    [program, "--resume", native_id].map(String::from).into()
}

/// Cursor keeps a marker file per trusted folder; the folder's path names its directory.
pub fn trusted(home: &Path, folder: &str) -> bool {
    let slug: String = folder
        .chars()
        .filter(|&c| c != ':')
        .map(|c| if c == '\\' || c == '/' { '-' } else { c })
        .collect();
    home.join(".cursor/projects")
        .join(slug.trim_matches('-'))
        .join(".workspace-trusted")
        .is_file()
}

/// `cursor-agent models` prints `id - Label` lines under a heading.
pub fn parse_models(out: &str) -> Vec<ModelOption> {
    out.lines()
        .filter_map(|line| line.split_once(" - "))
        .map(|(id, label)| ModelOption {
            id: id.trim().into(),
            label: label
                .trim()
                .trim_end_matches("(current, default)")
                .trim()
                .into(),
        })
        .filter(|m| !m.id.is_empty() && !m.id.contains(' '))
        .collect()
}

/// Cursor's `--output-format stream-json`. Unknown, malformed or irrelevant lines yield no events.
pub fn parse_line(line: &str) -> Vec<RunEvent> {
    let Ok(v) = serde_json::from_str::<Value>(line) else {
        return vec![];
    };
    match (v["type"].as_str(), v["subtype"].as_str()) {
        (Some("system"), Some("init")) => {
            let id = v["session_id"].as_str().map(|id| RunEvent::SessionStarted {
                native_id: id.into(),
            });
            let model = v["model"]
                .as_str()
                .map(|name| RunEvent::Model { name: name.into() });
            id.into_iter().chain(model).collect()
        }
        (Some("tool_call"), Some("started")) => tool_name(&v["tool_call"])
            .map(|name| vec![RunEvent::ToolUse { name }])
            .unwrap_or_default(),
        (Some("result"), _) => vec![RunEvent::Result {
            ok: v["is_error"] != true && v["subtype"] == "success",
            text: v["result"].as_str().unwrap_or_default().into(),
        }],
        _ => vec![],
    }
}

/// A call is keyed by its kind (`readToolCall`, `shellToolCall`) or is a named `function`.
fn tool_name(call: &Value) -> Option<String> {
    let (key, body) = call.as_object()?.iter().next()?;
    if key == "function" {
        return body["name"].as_str().map(String::from);
    }
    let kind = key.strip_suffix("ToolCall").unwrap_or(key);
    let mut chars = kind.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const RUN: &str = include_str!("../tests/fixtures/cursor-stream.jsonl");

    #[test]
    fn cursor_init_reports_the_model() {
        let events: Vec<_> = RUN.lines().flat_map(parse_line).collect();
        assert!(events.contains(&RunEvent::Model {
            name: "Auto".into()
        }));
    }

    #[test]
    fn a_run_reports_its_session_and_reply() {
        let events: Vec<RunEvent> = RUN
            .lines()
            .flat_map(parse_line)
            .filter(|e| !matches!(e, RunEvent::Model { .. }))
            .collect();
        assert_eq!(
            events,
            [
                RunEvent::SessionStarted {
                    native_id: "cf62eed9-a5f3-4b0f-827f-6404a8f8a4a1".into()
                },
                RunEvent::Result {
                    ok: true,
                    text: "ok".into()
                },
            ]
        );
    }

    #[test]
    fn tool_calls_name_the_tool() {
        let read = r#"{"type":"tool_call","subtype":"started","call_id":"1","tool_call":{"readToolCall":{"args":{"path":"a.rs"}}}}"#;
        let shell = r#"{"type":"tool_call","subtype":"started","tool_call":{"shellToolCall":{"args":{"command":"ls"}}}}"#;
        let function = r#"{"type":"tool_call","subtype":"started","tool_call":{"function":{"name":"grep","arguments":"{}"}}}"#;
        let done = r#"{"type":"tool_call","subtype":"completed","tool_call":{"readToolCall":{}}}"#;
        let names: Vec<RunEvent> = [read, shell, function, done]
            .into_iter()
            .flat_map(parse_line)
            .collect();
        let tool = |n: &str| RunEvent::ToolUse { name: n.into() };
        assert_eq!(names, [tool("Read"), tool("Shell"), tool("grep")]);
    }

    #[test]
    fn a_failed_run_fails() {
        let line = r#"{"type":"result","subtype":"error","is_error":true,"result":"Workspace Trust Required"}"#;
        assert_eq!(
            parse_line(line),
            [RunEvent::Result {
                ok: false,
                text: "Workspace Trust Required".into()
            }]
        );
    }

    #[test]
    fn arguments_carry_the_model_permission_and_session() {
        assert_eq!(
            print_args(Some("gpt-5.2"), Some("plan"), Some("abc")),
            [
                "-p",
                "--output-format",
                "stream-json",
                "--model",
                "gpt-5.2",
                "--mode",
                "plan",
                "--resume",
                "abc"
            ]
        );
        assert_eq!(
            print_args(None, Some("force"), None),
            ["-p", "--output-format", "stream-json", "--force"]
        );
        assert_eq!(
            terminal_args("agent.cmd", None, None, None, "--help me"),
            ["agent.cmd", "--", "--help me"]
        );
        assert_eq!(
            resume_in_terminal("agent", "abc"),
            ["agent", "--resume", "abc"]
        );
    }

    #[test]
    fn a_folder_is_trusted_once_cursor_marked_it() {
        let home = tempfile::tempdir().unwrap();
        assert!(!trusted(home.path(), r"D:\Projects\whispio"));
        let marker = home.path().join(".cursor/projects/D-Projects-whispio");
        std::fs::create_dir_all(&marker).unwrap();
        std::fs::write(marker.join(".workspace-trusted"), "").unwrap();
        assert!(trusted(home.path(), r"D:\Projects\whispio"));
        assert!(trusted(home.path(), r"D:\Projects\whispio\"));
        assert!(!trusted(home.path(), r"D:\Projects\other"));
        let mac = home.path().join(".cursor/projects/Users-me-app");
        std::fs::create_dir_all(&mac).unwrap();
        std::fs::write(mac.join(".workspace-trusted"), "").unwrap();
        assert!(trusted(home.path(), "/Users/me/app"));
    }

    #[test]
    fn models_are_read_from_the_list() {
        let out = "Available models\n\nauto - Auto (current, default)\ngpt-5.2 - GPT-5.2\n\nTip: use --model\n";
        assert_eq!(
            parse_models(out),
            [
                ModelOption {
                    id: "auto".into(),
                    label: "Auto".into()
                },
                ModelOption {
                    id: "gpt-5.2".into(),
                    label: "GPT-5.2".into()
                },
            ]
        );
    }
}
