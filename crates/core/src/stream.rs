use serde::Serialize;
use serde_json::Value;

/// Progress events from an agent run, reduced to what Erindi uses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RunEvent {
    /// The agent's own ID for the session, when the agent picks it.
    SessionStarted {
        native_id: String,
    },
    ToolUse {
        name: String,
    },
    PermissionDenied {
        tool: String,
    },
    /// A reply message before the run ends; the last one becomes the result text.
    Reply {
        text: String,
    },
    Result {
        ok: bool,
        text: String,
    },
}

/// Claude's `--output-format stream-json`. Unknown, malformed or irrelevant lines yield no events.
pub fn parse_line(line: &str) -> Vec<RunEvent> {
    let Ok(v) = serde_json::from_str::<Value>(line) else {
        return vec![];
    };
    let str_at = |key: &str| v[key].as_str().unwrap_or_default().to_string();
    match (v["type"].as_str(), v["subtype"].as_str()) {
        (Some("assistant"), _) => v["message"]["content"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|c| c["type"] == "tool_use")
            .filter_map(|c| c["name"].as_str())
            .map(|name| RunEvent::ToolUse { name: name.into() })
            .collect(),
        (Some("system"), Some("permission_denied")) => vec![RunEvent::PermissionDenied {
            tool: str_at("tool_name"),
        }],
        (Some("result"), subtype) => vec![RunEvent::Result {
            ok: subtype == Some("success") && v["is_error"] == false,
            text: str_at("result"),
        }],
        _ => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_use_from_assistant_message() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"thinking","thinking":""},{"type":"tool_use","id":"t1","name":"Write","input":{}}]},"session_id":"s"}"#;
        assert_eq!(
            parse_line(line),
            [RunEvent::ToolUse {
                name: "Write".into()
            }]
        );
    }

    #[test]
    fn permission_denied() {
        let line = r#"{"type":"system","subtype":"permission_denied","tool_name":"Bash","tool_use_id":"t2","message":"denied"}"#;
        assert_eq!(
            parse_line(line),
            [RunEvent::PermissionDenied {
                tool: "Bash".into()
            }]
        );
    }

    #[test]
    fn success_result() {
        let line = r#"{"type":"result","subtype":"success","is_error":false,"result":"Готово ✅","session_id":"s"}"#;
        assert_eq!(
            parse_line(line),
            [RunEvent::Result {
                ok: true,
                text: "Готово ✅".into()
            }]
        );
    }

    #[test]
    fn error_result() {
        let line = r#"{"type":"result","subtype":"success","is_error":true,"result":"Invalid API key · Please run /login"}"#;
        assert_eq!(
            parse_line(line),
            [RunEvent::Result {
                ok: false,
                text: "Invalid API key · Please run /login".into()
            }]
        );
        let line = r#"{"type":"result","subtype":"error_max_turns","is_error":false}"#;
        assert_eq!(
            parse_line(line),
            [RunEvent::Result {
                ok: false,
                text: String::new()
            }]
        );
    }

    #[test]
    fn ignores_noise_and_garbage() {
        for line in [
            "",
            "not json",
            "{\"type\":",
            "[1,2]",
            r#"{"type":"system","subtype":"hook_started"}"#,
            r#"{"type":"rate_limit_event"}"#,
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"hi"}]}}"#,
            r#"{"type":"assistant","message":"oops"}"#,
        ] {
            assert_eq!(parse_line(line), [], "line: {line}");
        }
    }
}
