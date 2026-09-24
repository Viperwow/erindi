use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::claude::{self, ClaudeMode, ClaudeRequest, Session};
use crate::codex;
use crate::stream::{self, RunEvent};

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum Agent {
    #[default]
    Claude,
    Codex,
}

impl Agent {
    pub const ALL: [Agent; 2] = [Agent::Claude, Agent::Codex];

    pub fn label(self) -> &'static str {
        match self {
            Agent::Claude => "Claude",
            Agent::Codex => "Codex",
        }
    }

    /// The command name searched on PATH.
    pub fn cli(self) -> &'static str {
        match self {
            Agent::Claude => "claude",
            Agent::Codex => "codex",
        }
    }

    /// Permission values besides the default, as the CLI spells them.
    pub fn permissions(self) -> &'static [&'static str] {
        match self {
            Agent::Claude => &[
                "acceptEdits",
                "auto",
                "plan",
                "dontAsk",
                "bypassPermissions",
            ],
            Agent::Codex => &["read-only", "workspace-write", "danger-full-access"],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModelOption {
    pub id: String,
    pub label: String,
}

/// Claude Code has no command that lists models; its aliases always point at the latest one.
pub fn claude_models() -> Vec<ModelOption> {
    [
        ("fable", "Fable"),
        ("opus", "Opus"),
        ("sonnet", "Sonnet"),
        ("haiku", "Haiku"),
    ]
    .map(|(id, label)| ModelOption {
        id: id.into(),
        label: label.into(),
    })
    .into()
}

/// Where a run goes: a new session with Erindi's ID, or an existing one by the agent's native ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    New(Uuid),
    Resume(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentRequest {
    pub agent: Agent,
    pub model: Option<String>,
    pub permission: Option<String>,
    pub target: Target,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvalidRequest {
    Model,
    Permission,
    Cwd,
    NativeId,
}

pub fn valid_model(model: &str) -> bool {
    !model.is_empty() && !model.starts_with('-') && !model.chars().any(char::is_whitespace)
}

fn check(req: &AgentRequest) -> Result<(), InvalidRequest> {
    if req.model.as_deref().is_some_and(|m| !valid_model(m)) {
        return Err(InvalidRequest::Model);
    }
    if req
        .permission
        .as_deref()
        .is_some_and(|p| !req.agent.permissions().contains(&p))
    {
        return Err(InvalidRequest::Permission);
    }
    Ok(())
}

fn claude_request(req: &AgentRequest) -> Result<ClaudeRequest, InvalidRequest> {
    check(req)?;
    let mode = match &req.permission {
        None => ClaudeMode::Default,
        Some(p) => serde_json::from_value(serde_json::Value::String(p.clone()))
            .map_err(|_| InvalidRequest::Permission)?,
    };
    let session = match &req.target {
        Target::New(id) => Session::New(*id),
        Target::Resume(native) => {
            Session::Resume(Uuid::parse_str(native).map_err(|_| InvalidRequest::NativeId)?)
        }
    };
    Ok(ClaudeRequest {
        mode,
        model: req.model.clone(),
        session,
    })
}

/// Native IDs are UUIDs for both agents; anything else could be read as an option.
fn native_ok(id: &str) -> Result<(), InvalidRequest> {
    let ok = !id.is_empty()
        && !id.starts_with('-')
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    if ok {
        Ok(())
    } else {
        Err(InvalidRequest::NativeId)
    }
}

fn cwd_ok(cwd: &str) -> Result<(), InvalidRequest> {
    if cwd.is_empty() || cwd.starts_with('-') || cwd.contains(';') {
        return Err(InvalidRequest::Cwd);
    }
    Ok(())
}

/// Arguments for a headless run in `cwd`. The prompt goes to stdin.
pub fn headless_args(req: &AgentRequest, cwd: &str) -> Result<Vec<String>, InvalidRequest> {
    match req.agent {
        // Claude runs in the process folder; `cwd` matters only to Codex (`-C`).
        Agent::Claude => {
            claude::claude_args(&claude_request(req)?).map_err(|_| InvalidRequest::Model)
        }
        Agent::Codex => {
            check(req)?;
            if let Target::Resume(id) = &req.target {
                native_ok(id)?;
            }
            Ok(codex::exec_args(
                req.model.as_deref(),
                req.permission.as_deref(),
                &req.target,
                cwd,
            ))
        }
    }
}

/// Windows Terminal arguments for an interactive agent whose first message is `prompt`.
pub fn terminal_args(
    program: &str,
    cwd: &str,
    req: &AgentRequest,
    prompt: &str,
) -> Result<Vec<String>, InvalidRequest> {
    cwd_ok(cwd)?;
    let prompt = &cmd_safe(program, prompt);
    match req.agent {
        Agent::Claude => claude::run_in_terminal(program, cwd, &claude_request(req)?, prompt)
            .map_err(|e| match e {
                claude::InvalidTerminalRun::Cwd => InvalidRequest::Cwd,
                claude::InvalidTerminalRun::Model => InvalidRequest::Model,
            }),
        Agent::Codex => {
            check(req)?;
            if let Target::Resume(id) = &req.target {
                native_ok(id)?;
            }
            Ok(codex::terminal_args(
                program,
                cwd,
                req.model.as_deref(),
                req.permission.as_deref(),
                &req.target,
                prompt,
            ))
        }
    }
}

/// npm installs CLIs as `.cmd` shims, which Windows runs through `cmd /c`; cmd ignores the
/// argument quoting and would run the text after `&`, `|` or a redirect as commands of its own.
fn cmd_safe(program: &str, prompt: &str) -> String {
    let lower = program.to_ascii_lowercase();
    if !(lower.ends_with(".cmd") || lower.ends_with(".bat")) {
        return prompt.to_string();
    }
    prompt
        .chars()
        .map(|c| match c {
            '"' | '&' | '|' | '<' | '>' | '^' | '%' => ' ',
            c => c,
        })
        .collect()
}

/// Windows Terminal arguments that reopen session `native_id` interactively.
pub fn resume_in_terminal(
    program: &str,
    cwd: &str,
    agent: Agent,
    native_id: &str,
) -> Result<Vec<String>, InvalidRequest> {
    cwd_ok(cwd)?;
    match agent {
        Agent::Claude => {
            let id = Uuid::parse_str(native_id).map_err(|_| InvalidRequest::NativeId)?;
            claude::resume_in_terminal(program, cwd, id).map_err(|_| InvalidRequest::Cwd)
        }
        Agent::Codex => {
            native_ok(native_id)?;
            Ok(codex::resume_in_terminal(program, cwd, native_id))
        }
    }
}

pub fn env(
    agent: Agent,
    vars: impl IntoIterator<Item = (String, String)>,
) -> Vec<(String, String)> {
    match agent {
        Agent::Claude => claude::claude_env(vars),
        Agent::Codex => codex::codex_env(vars),
    }
}

/// Parses one run's output. Codex sends the reply before the result, so the parser keeps it.
pub struct EventParser {
    agent: Agent,
    reply: String,
}

impl EventParser {
    pub fn new(agent: Agent) -> Self {
        Self {
            agent,
            reply: String::new(),
        }
    }

    pub fn feed(&mut self, line: &str) -> Vec<RunEvent> {
        let events = match self.agent {
            Agent::Claude => stream::parse_line(line),
            Agent::Codex => codex::parse_line(line),
        };
        events
            .into_iter()
            .filter_map(|e| match e {
                RunEvent::Reply { text } => {
                    self.reply = text;
                    None
                }
                RunEvent::Result { ok, text } if text.is_empty() => Some(RunEvent::Result {
                    ok,
                    text: std::mem::take(&mut self.reply),
                }),
                e => Some(e),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claude(model: Option<&str>, permission: Option<&str>, target: Target) -> AgentRequest {
        AgentRequest {
            agent: Agent::Claude,
            model: model.map(str::to_string),
            permission: permission.map(str::to_string),
            target,
        }
    }

    const NIL: &str = "00000000-0000-0000-0000-000000000000";

    #[test]
    fn serde_names_are_lowercase() {
        assert_eq!(serde_json::to_string(&Agent::Codex).unwrap(), "\"codex\"");
        assert_eq!(
            serde_json::from_str::<Agent>("\"claude\"").unwrap(),
            Agent::Claude
        );
    }

    #[test]
    fn claude_new_session_passes_its_id_model_and_permission() {
        let req = claude(Some("opus"), Some("plan"), Target::New(Uuid::nil()));
        assert_eq!(
            headless_args(&req, "C:/p").unwrap(),
            [
                "-p",
                "--output-format",
                "stream-json",
                "--verbose",
                "--session-id",
                NIL,
                "--permission-mode",
                "plan",
                "--model",
                "opus"
            ]
        );
    }

    #[test]
    fn claude_resume_uses_the_native_id() {
        let req = claude(None, None, Target::Resume(NIL.into()));
        let args = headless_args(&req, "C:/p").unwrap();
        assert_eq!(args[4..], ["--resume", NIL]);
    }

    #[test]
    fn claude_resume_rejects_a_native_id_that_is_not_a_uuid() {
        let req = claude(None, None, Target::Resume("--help".into()));
        assert_eq!(headless_args(&req, "C:/p"), Err(InvalidRequest::NativeId));
    }

    #[test]
    fn unknown_permission_and_bad_models_are_rejected() {
        let req = claude(None, Some("yolo"), Target::New(Uuid::nil()));
        assert_eq!(headless_args(&req, "C:/p"), Err(InvalidRequest::Permission));
        for bad in ["", "-m", "a b"] {
            let req = claude(Some(bad), None, Target::New(Uuid::nil()));
            assert_eq!(
                headless_args(&req, "C:/p"),
                Err(InvalidRequest::Model),
                "{bad}"
            );
        }
    }

    #[test]
    fn permission_lists_match_the_clis() {
        assert_eq!(
            Agent::Claude.permissions(),
            [
                "acceptEdits",
                "auto",
                "plan",
                "dontAsk",
                "bypassPermissions"
            ]
        );
        assert_eq!(
            Agent::Codex.permissions(),
            ["read-only", "workspace-write", "danger-full-access"]
        );
    }

    #[test]
    fn claude_terminal_uses_the_program_path() {
        let req = claude(None, None, Target::New(Uuid::nil()));
        let args = terminal_args(r"C:\bin\claude.exe", "C:/p", &req, "fix it").unwrap();
        assert_eq!(args[..3], ["-d", "C:/p", r"C:\bin\claude.exe"]);
        assert_eq!(args[args.len() - 2..], ["--", "fix it"]);
        let args = resume_in_terminal("claude", "C:/p", Agent::Claude, NIL).unwrap();
        assert_eq!(args, ["-d", "C:/p", "claude", "--resume", NIL]);
        assert_eq!(
            resume_in_terminal("claude", "C:/p;calc", Agent::Claude, NIL),
            Err(InvalidRequest::Cwd)
        );
    }

    #[test]
    fn claude_env_is_the_claude_allowlist() {
        let vars = [
            ("PATH", "x"),
            ("OPENAI_API_KEY", "leak"),
            ("ANTHROPIC_API_KEY", "k"),
        ]
        .map(|(k, v)| (k.to_string(), v.to_string()));
        let kept: Vec<_> = env(Agent::Claude, vars)
            .into_iter()
            .map(|(k, _)| k)
            .collect();
        assert_eq!(kept, ["PATH", "ANTHROPIC_API_KEY"]);
    }

    fn codex(model: Option<&str>, permission: Option<&str>, target: Target) -> AgentRequest {
        AgentRequest {
            agent: Agent::Codex,
            model: model.map(str::to_string),
            permission: permission.map(str::to_string),
            target,
        }
    }

    #[test]
    fn codex_new_session_runs_exec_in_the_folder() {
        let req = codex(None, None, Target::New(Uuid::nil()));
        assert_eq!(
            headless_args(&req, r"C:\p").unwrap(),
            ["exec", "--json", "-C", r"C:\p"]
        );
        let req = codex(
            Some("gpt-5.5"),
            Some("workspace-write"),
            Target::New(Uuid::nil()),
        );
        assert_eq!(
            headless_args(&req, r"C:\p").unwrap(),
            [
                "exec",
                "--json",
                "-C",
                r"C:\p",
                "-m",
                "gpt-5.5",
                "-s",
                "workspace-write"
            ]
        );
    }

    #[test]
    fn codex_resume_passes_only_the_native_id() {
        let req = codex(
            None,
            None,
            Target::Resume("01a0d2c0-0c6d-7dc0-90c7-da4ffbaf65a2".into()),
        );
        assert_eq!(
            headless_args(&req, r"C:\p").unwrap(),
            [
                "exec",
                "resume",
                "01a0d2c0-0c6d-7dc0-90c7-da4ffbaf65a2",
                "--json"
            ]
        );
    }

    #[test]
    fn codex_native_id_cannot_be_a_flag() {
        let req = codex(None, None, Target::Resume("--last".into()));
        assert_eq!(headless_args(&req, "C:/p"), Err(InvalidRequest::NativeId));
        assert_eq!(
            resume_in_terminal("codex", "C:/p", Agent::Codex, "--last"),
            Err(InvalidRequest::NativeId)
        );
    }

    #[test]
    fn codex_permission_must_be_a_sandbox_mode() {
        let req = codex(None, Some("plan"), Target::New(Uuid::nil()));
        assert_eq!(headless_args(&req, "C:/p"), Err(InvalidRequest::Permission));
    }

    #[test]
    fn codex_terminal_task_continues_an_existing_session() {
        let req = codex(Some("gpt-5.5"), None, Target::Resume("abc-1".into()));
        let args = terminal_args("codex", "C:/p", &req, "fix it").unwrap();
        assert_eq!(
            args,
            ["-d", "C:/p", "codex", "resume", "abc-1", "--", "fix it"]
        );
        let req = codex(None, None, Target::Resume("--last".into()));
        assert_eq!(
            terminal_args("codex", "C:/p", &req, "x"),
            Err(InvalidRequest::NativeId)
        );
    }

    #[test]
    fn codex_terminal_runs_the_interactive_cli() {
        let req = codex(Some("gpt-5.5"), Some("read-only"), Target::New(Uuid::nil()));
        let args = terminal_args(r"C:\npm\codex.cmd", "C:/p", &req, "fix; it").unwrap();
        assert_eq!(
            args,
            [
                "-d",
                "C:/p",
                r"C:\npm\codex.cmd",
                "-m",
                "gpt-5.5",
                "-s",
                "read-only",
                "--",
                r"fix\; it"
            ]
        );
        let args = resume_in_terminal("codex", "C:/p", Agent::Codex, "abc-1").unwrap();
        assert_eq!(args, ["-d", "C:/p", "codex", "resume", "abc-1"]);
    }

    #[test]
    fn codex_env_keeps_openai_and_codex_vars_only() {
        let vars = [
            ("Path", "x"),
            ("CODEX_HOME", "C:/c"),
            ("OPENAI_API_KEY", "k"),
            ("OPENAI_BASE_URL", "u"),
            ("ANTHROPIC_API_KEY", "leak"),
            ("GITHUB_TOKEN", "leak"),
        ]
        .map(|(k, v)| (k.to_string(), v.to_string()));
        let kept: Vec<_> = env(Agent::Codex, vars)
            .into_iter()
            .map(|(k, _)| k)
            .collect();
        assert_eq!(
            kept,
            ["Path", "CODEX_HOME", "OPENAI_API_KEY", "OPENAI_BASE_URL"]
        );
    }

    #[test]
    fn parser_fills_an_empty_result_with_the_latest_reply() {
        let mut p = EventParser::new(Agent::Codex);
        let mut events = vec![];
        for line in [
            r#"{"type":"item.completed","item":{"type":"agent_message","text":"first"}}"#,
            r#"{"type":"item.completed","item":{"type":"agent_message","text":"done"}}"#,
            r#"{"type":"turn.completed"}"#,
        ] {
            events.extend(p.feed(line));
        }
        assert_eq!(
            events.last(),
            Some(&RunEvent::Result {
                ok: true,
                text: "done".into()
            })
        );
        assert!(!events.iter().any(|e| matches!(e, RunEvent::Reply { .. })));
    }

    #[test]
    fn parser_passes_claude_lines_through() {
        let mut p = EventParser::new(Agent::Claude);
        let line = r#"{"type":"result","subtype":"success","is_error":false,"result":"ok"}"#;
        assert_eq!(
            p.feed(line),
            [RunEvent::Result {
                ok: true,
                text: "ok".into()
            }]
        );
    }

    #[test]
    fn claude_models_are_the_aliases() {
        let ids: Vec<_> = claude_models().into_iter().map(|m| m.id).collect();
        assert_eq!(ids, ["fable", "opus", "sonnet", "haiku"]);
    }

    #[test]
    fn cmd_shims_get_prompts_cmd_cannot_misread() {
        let req = codex(None, None, Target::New(Uuid::nil()));
        let prompt = r#"rename "foo" & calc | more > out ^ 50%"#;
        let args = terminal_args(r"C:\npm\codex.CMD", "C:/p", &req, prompt).unwrap();
        let sent = args.last().unwrap();
        assert!(
            !sent.contains(['"', '&', '|', '<', '>', '^', '%']),
            "{sent}"
        );
        assert!(sent.contains("rename") && sent.contains("calc"), "{sent}");
        let args = terminal_args(r"C:\bin\codex.exe", "C:/p", &req, "a & b").unwrap();
        assert_eq!(args.last().unwrap(), "a & b");
    }
}
