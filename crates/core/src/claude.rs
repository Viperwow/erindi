use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Claude Code permission modes. `Default` passes no flag so the user's own Claude settings apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ClaudeMode {
    #[default]
    Default,
    AcceptEdits,
    Auto,
    Plan,
    DontAsk,
    BypassPermissions,
}

#[derive(Debug, Clone)]
pub struct ClaudeRequest {
    pub mode: ClaudeMode,
    pub model: Option<String>,
    pub session_id: Uuid,
}

#[derive(Debug, PartialEq, Eq)]
pub struct InvalidModel;

/// Arguments for a headless run. The prompt is written to stdin, never passed as an argument.
pub fn claude_args(req: &ClaudeRequest) -> Result<Vec<String>, InvalidModel> {
    let mut args: Vec<String> = ["-p", "--output-format", "stream-json", "--verbose"]
        .map(String::from)
        .into();
    args.extend(["--session-id".into(), req.session_id.to_string()]);

    let mode = match req.mode {
        ClaudeMode::Default => None,
        ClaudeMode::AcceptEdits => Some("acceptEdits"),
        ClaudeMode::Auto => Some("auto"),
        ClaudeMode::Plan => Some("plan"),
        ClaudeMode::DontAsk => Some("dontAsk"),
        ClaudeMode::BypassPermissions => Some("bypassPermissions"),
    };
    if let Some(mode) = mode {
        args.extend(["--permission-mode".into(), mode.into()]);
    }

    if let Some(model) = &req.model {
        let valid =
            !model.is_empty() && !model.starts_with('-') && !model.chars().any(char::is_whitespace);
        if !valid {
            return Err(InvalidModel);
        }
        args.extend(["--model".into(), model.clone()]);
    }
    Ok(args)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(mode: ClaudeMode, model: Option<&str>) -> ClaudeRequest {
        ClaudeRequest {
            mode,
            model: model.map(str::to_string),
            session_id: Uuid::nil(),
        }
    }

    const BASE: [&str; 6] = [
        "-p",
        "--output-format",
        "stream-json",
        "--verbose",
        "--session-id",
        "00000000-0000-0000-0000-000000000000",
    ];

    #[test]
    fn default_mode_passes_no_permission_flag() {
        assert_eq!(claude_args(&req(ClaudeMode::Default, None)).unwrap(), BASE);
    }

    #[test]
    fn modes_map_to_cli_values() {
        let cases = [
            (ClaudeMode::AcceptEdits, "acceptEdits"),
            (ClaudeMode::Auto, "auto"),
            (ClaudeMode::Plan, "plan"),
            (ClaudeMode::DontAsk, "dontAsk"),
            (ClaudeMode::BypassPermissions, "bypassPermissions"),
        ];
        for (mode, value) in cases {
            let args = claude_args(&req(mode, None)).unwrap();
            assert_eq!(args[6..], ["--permission-mode", value]);
        }
    }

    #[test]
    fn model_is_a_separate_argument() {
        let args = claude_args(&req(ClaudeMode::Default, Some("opus"))).unwrap();
        assert_eq!(args[6..], ["--model", "opus"]);
    }

    #[test]
    fn model_cannot_smuggle_a_flag() {
        for bad in ["--dangerously-skip-permissions", "", "opus sonnet"] {
            assert_eq!(
                claude_args(&req(ClaudeMode::Default, Some(bad))),
                Err(InvalidModel)
            );
        }
    }

    #[test]
    fn serde_uses_cli_names() {
        assert_eq!(
            serde_json::to_string(&ClaudeMode::AcceptEdits).unwrap(),
            "\"acceptEdits\""
        );
    }
}
