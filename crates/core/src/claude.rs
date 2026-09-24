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

/// Which Claude session a run writes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Session {
    New(Uuid),
    Resume(Uuid),
}

#[derive(Debug, Clone)]
pub struct ClaudeRequest {
    pub mode: ClaudeMode,
    pub model: Option<String>,
    pub session: Session,
}

#[derive(Debug, PartialEq, Eq)]
pub struct InvalidModel;

/// Environment variables Claude Code needs on Windows; everything else stays with the launcher.
const ENV_ALLOW: &[&str] = &[
    "PATH",
    "PATHEXT",
    "SYSTEMROOT",
    "SYSTEMDRIVE",
    "WINDIR",
    "COMSPEC",
    "USERPROFILE",
    "HOMEDRIVE",
    "HOMEPATH",
    "HOME",
    "APPDATA",
    "LOCALAPPDATA",
    "PROGRAMDATA",
    "PROGRAMFILES",
    "PROGRAMFILES(X86)",
    "PROGRAMW6432",
    "COMMONPROGRAMFILES",
    "COMMONPROGRAMFILES(X86)",
    "COMMONPROGRAMW6432",
    "TEMP",
    "TMP",
    "USERNAME",
    "USERDOMAIN",
    "COMPUTERNAME",
    "NUMBER_OF_PROCESSORS",
    "PROCESSOR_ARCHITECTURE",
    "OS",
    "LANG",
    "LC_ALL",
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "NO_PROXY",
    "NODE_EXTRA_CA_CERTS",
    "SSL_CERT_FILE",
];
const ENV_ALLOW_PREFIX: &[&str] = &["ANTHROPIC_", "CLAUDE_"];

pub(crate) fn base_env_allowed(upper: &str) -> bool {
    ENV_ALLOW.contains(&upper)
}

pub fn claude_env(vars: impl IntoIterator<Item = (String, String)>) -> Vec<(String, String)> {
    vars.into_iter()
        .filter(|(k, _)| {
            let k = k.to_uppercase();
            base_env_allowed(&k) || ENV_ALLOW_PREFIX.iter().any(|p| k.starts_with(p))
        })
        .collect()
}

/// Arguments for a headless run. The prompt is written to stdin, never passed as an argument.
pub fn claude_args(req: &ClaudeRequest) -> Result<Vec<String>, InvalidModel> {
    let mut args: Vec<String> = ["-p", "--output-format", "stream-json", "--verbose"]
        .map(String::from)
        .into();
    args.extend(options(req)?);
    Ok(args)
}

/// Session, permission mode and model flags, shared by headless and terminal runs.
fn options(req: &ClaudeRequest) -> Result<Vec<String>, InvalidModel> {
    let mut args: Vec<String> = vec![];
    args.extend(match req.session {
        Session::New(id) => ["--session-id".into(), id.to_string()],
        Session::Resume(id) => ["--resume".into(), id.to_string()],
    });

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

#[derive(Debug, PartialEq, Eq)]
pub struct InvalidCwd;

#[derive(Debug, PartialEq, Eq)]
pub enum InvalidTerminalRun {
    Cwd,
    Model,
}

/// Windows Terminal arguments for an interactive Claude whose first message is `prompt`.
/// The prompt follows `--`, so it can never be read as an option, and its `;` is escaped because
/// Windows Terminal would otherwise split the command there.
pub fn run_in_terminal(
    program: &str,
    cwd: &str,
    req: &ClaudeRequest,
    prompt: &str,
) -> Result<Vec<String>, InvalidTerminalRun> {
    if cwd.is_empty() || cwd.starts_with('-') || cwd.contains(';') {
        return Err(InvalidTerminalRun::Cwd);
    }
    let mut args = vec!["-d".into(), cwd.into(), program.into()];
    args.extend(options(req).map_err(|_| InvalidTerminalRun::Model)?);
    if !prompt.is_empty() {
        args.extend(["--".into(), prompt.replace(';', r"\;")]);
    }
    Ok(args)
}

/// Windows Terminal arguments that reopen a headless session interactively.
pub fn resume_in_terminal(
    program: &str,
    cwd: &str,
    session_id: Uuid,
) -> Result<Vec<String>, InvalidCwd> {
    if cwd.is_empty() || cwd.starts_with('-') || cwd.contains(';') {
        return Err(InvalidCwd);
    }
    Ok(vec![
        "-d".into(),
        cwd.into(),
        program.into(),
        "--resume".into(),
        session_id.to_string(),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resume_opens_session_in_cwd() {
        let id = Uuid::nil();
        assert_eq!(
            resume_in_terminal(r"C:\bin\claude.exe", "C:\\My Projects\\app", id).unwrap(),
            [
                "-d",
                "C:\\My Projects\\app",
                r"C:\bin\claude.exe",
                "--resume",
                "00000000-0000-0000-0000-000000000000"
            ]
        );
    }

    #[test]
    fn resume_rejects_cwd_that_wt_would_split_or_parse() {
        // `wt` treats `;` as a command separator even inside one argument.
        for bad in ["C:\\a;calc", "", "-p evil"] {
            assert_eq!(
                resume_in_terminal("claude", bad, Uuid::nil()),
                Err(InvalidCwd),
                "{bad}"
            );
        }
    }

    fn req(mode: ClaudeMode, model: Option<&str>) -> ClaudeRequest {
        ClaudeRequest {
            mode,
            model: model.map(str::to_string),
            session: Session::New(Uuid::nil()),
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
    fn resume_continues_an_existing_session() {
        let mut r = req(ClaudeMode::Plan, None);
        r.session = Session::Resume(Uuid::nil());
        let args = claude_args(&r).unwrap();
        assert_eq!(
            args[4..6],
            ["--resume", "00000000-0000-0000-0000-000000000000"]
        );
        assert!(!args.contains(&"--session-id".to_string()));
        assert_eq!(args[6..], ["--permission-mode", "plan"]);
    }

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
    fn env_keeps_only_allowlisted_vars() {
        let vars = [
            ("Path", "C:\\bin"),
            ("SystemRoot", "C:\\Windows"),
            ("USERPROFILE", "C:\\Users\\me"),
            ("ANTHROPIC_API_KEY", "sk"),
            ("CLAUDE_CODE_GIT_BASH_PATH", "C:\\git\\bash.exe"),
            ("https_proxy", "http://proxy"),
            ("OPENAI_API_KEY", "leak"),
            ("GITHUB_TOKEN", "leak"),
            ("RUST_LOG", "debug"),
        ]
        .map(|(k, v)| (k.to_string(), v.to_string()));
        let kept: Vec<_> = claude_env(vars).into_iter().map(|(k, _)| k).collect();
        assert_eq!(
            kept,
            [
                "Path",
                "SystemRoot",
                "USERPROFILE",
                "ANTHROPIC_API_KEY",
                "CLAUDE_CODE_GIT_BASH_PATH",
                "https_proxy"
            ]
        );
    }

    #[test]
    fn serde_uses_cli_names() {
        assert_eq!(
            serde_json::to_string(&ClaudeMode::AcceptEdits).unwrap(),
            "\"acceptEdits\""
        );
    }

    #[test]
    fn terminal_run_passes_the_prompt_after_the_options() {
        let req = ClaudeRequest {
            mode: ClaudeMode::Plan,
            model: Some("opus".into()),
            session: Session::New(Uuid::nil()),
        };
        let args = run_in_terminal("claude", "C:/p", &req, "--help; rm -rf /").unwrap();
        assert_eq!(
            args,
            [
                "-d",
                "C:/p",
                "claude",
                "--session-id",
                &Uuid::nil().to_string(),
                "--permission-mode",
                "plan",
                "--model",
                "opus",
                "--",
                r"--help\; rm -rf /",
            ]
        );
        let args = run_in_terminal("claude", "C:/p", &req, "").unwrap();
        assert_eq!(args.last().unwrap(), "opus");
        assert!(run_in_terminal("claude", "C:/p;calc", &req, "x").is_err());
    }
}
