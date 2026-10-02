use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::claude::{self, ClaudeMode, ClaudeRequest, Session};
use crate::codex;
use crate::cursor;
use crate::pi;
use crate::stream::{self, RunEvent};

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum Agent {
    #[default]
    Claude,
    Codex,
    Pi,
    Cursor,
    /// A model behind an OpenAI-compatible API, reached over HTTP instead of a CLI.
    Api,
}

impl Agent {
    pub const ALL: [Agent; 5] = [
        Agent::Claude,
        Agent::Codex,
        Agent::Pi,
        Agent::Cursor,
        Agent::Api,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Agent::Claude => "Claude",
            Agent::Codex => "Codex",
            Agent::Pi => "Pi",
            Agent::Cursor => "Cursor",
            Agent::Api => "Local model",
        }
    }

    /// The command name searched on PATH.
    pub fn cli(self) -> &'static str {
        match self {
            Agent::Claude => "claude",
            Agent::Codex => "codex",
            Agent::Pi => "pi",
            Agent::Cursor => "cursor-agent",
            Agent::Api => "",
        }
    }

    pub fn is_cli(self) -> bool {
        self != Agent::Api
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
            Agent::Cursor => &["plan", "ask", "force"],
            // Pi has no permission modes; it always runs with all its tools.
            Agent::Pi | Agent::Api => &[],
        }
    }

    /// New sessions take Erindi's ID, so they can be resumed before the agent reports anything.
    pub fn uses_erindi_id(self) -> bool {
        matches!(self, Agent::Claude | Agent::Pi | Agent::Api)
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
    /// The agent has no command line to run.
    NotCli,
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

/// Native IDs are UUIDs for every agent; anything else could be read as an option.
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
        Agent::Api => Err(InvalidRequest::NotCli),
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
        Agent::Pi => {
            check(req)?;
            let (id, new) = pi_session(&req.target)?;
            Ok(pi::print_args(req.model.as_deref(), &id, new))
        }
        Agent::Cursor => {
            check(req)?;
            Ok(cursor::print_args(
                req.model.as_deref(),
                req.permission.as_deref(),
                cursor_resume(&req.target)?,
            ))
        }
    }
}

/// Cursor picks the ID of a new session and reports it when the run starts.
fn cursor_resume(target: &Target) -> Result<Option<&str>, InvalidRequest> {
    match target {
        Target::New(_) => Ok(None),
        Target::Resume(id) => native_ok(id).map(|()| Some(id.as_str())),
    }
}

fn pi_session(target: &Target) -> Result<(String, bool), InvalidRequest> {
    match target {
        Target::New(id) => Ok((id.to_string(), true)),
        Target::Resume(id) => native_ok(id).map(|()| (id.clone(), false)),
    }
}

/// An interactive agent to open in a terminal: its folder and its command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalCommand {
    pub cwd: String,
    pub argv: Vec<String>,
}

impl TerminalCommand {
    /// Windows Terminal arguments; wt splits commands at `;` even inside one argument.
    pub fn wt_args(&self) -> Vec<String> {
        let argv = self.argv.iter().map(|a| a.replace(';', r"\;"));
        ["-d".to_string(), self.cwd.clone()]
            .into_iter()
            .chain(argv)
            .collect()
    }

    /// A `.command` script for Terminal.app. Every word is single-quoted, so the login shell
    /// runs the agent without expanding anything in the prompt.
    pub fn command_script(&self) -> String {
        let q = |s: &str| format!("'{}'", s.replace('\'', r"'\''"));
        let argv: Vec<String> = self.argv.iter().map(|a| q(a)).collect();
        format!(
            "#!/bin/zsh -il\nrm -f -- \"$0\"\ncd {} && exec {}\n",
            q(&self.cwd),
            argv.join(" ")
        )
    }
}

/// The interactive agent whose first message is `prompt`.
pub fn terminal_args(
    program: &str,
    cwd: &str,
    req: &AgentRequest,
    prompt: &str,
) -> Result<TerminalCommand, InvalidRequest> {
    cwd_ok(cwd)?;
    let prompt = &cmd_safe(program, prompt);
    let argv = match req.agent {
        Agent::Api => Err(InvalidRequest::NotCli),
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
                req.model.as_deref(),
                req.permission.as_deref(),
                &req.target,
                prompt,
            ))
        }
        Agent::Pi => {
            check(req)?;
            let (id, new) = pi_session(&req.target)?;
            Ok(pi::terminal_args(
                program,
                req.model.as_deref(),
                &id,
                new,
                prompt,
            ))
        }
        Agent::Cursor => {
            check(req)?;
            Ok(cursor::terminal_args(
                program,
                req.model.as_deref(),
                req.permission.as_deref(),
                cursor_resume(&req.target)?,
                prompt,
            ))
        }
    }?;
    Ok(TerminalCommand {
        cwd: cwd.into(),
        argv,
    })
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

/// The interactive agent that reopens session `native_id`.
pub fn resume_in_terminal(
    program: &str,
    cwd: &str,
    agent: Agent,
    native_id: &str,
) -> Result<TerminalCommand, InvalidRequest> {
    cwd_ok(cwd)?;
    let argv = match agent {
        Agent::Api => Err(InvalidRequest::NotCli),
        Agent::Claude => {
            let id = Uuid::parse_str(native_id).map_err(|_| InvalidRequest::NativeId)?;
            claude::resume_in_terminal(program, cwd, id).map_err(|_| InvalidRequest::Cwd)
        }
        Agent::Codex => {
            native_ok(native_id)?;
            Ok(codex::resume_in_terminal(program, native_id))
        }
        Agent::Pi => {
            native_ok(native_id)?;
            Ok(pi::resume_in_terminal(program, native_id))
        }
        Agent::Cursor => {
            native_ok(native_id)?;
            Ok(cursor::resume_in_terminal(program, native_id))
        }
    }?;
    Ok(TerminalCommand {
        cwd: cwd.into(),
        argv,
    })
}

/// Parses one run's output. Codex sends the reply before the result, so the parser keeps it.
pub struct EventParser {
    agent: Agent,
    reply: String,
    pi: pi::Parser,
}

impl EventParser {
    pub fn new(agent: Agent) -> Self {
        Self {
            agent,
            reply: String::new(),
            pi: pi::Parser::default(),
        }
    }

    pub fn feed(&mut self, line: &str) -> Vec<RunEvent> {
        let events = match self.agent {
            Agent::Claude => stream::parse_line(line),
            Agent::Codex => codex::parse_line(line),
            Agent::Pi => self.pi.feed(line),
            Agent::Cursor => cursor::parse_line(line),
            Agent::Api => vec![],
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
    #[test]
    fn only_the_local_model_is_not_a_cli() {
        assert!(!Agent::Api.is_cli());
        assert!(Agent::Claude.is_cli() && Agent::Codex.is_cli() && Agent::Pi.is_cli());
        assert_eq!(
            headless_args(
                &AgentRequest {
                    agent: Agent::Api,
                    model: None,
                    permission: None,
                    target: Target::New(Uuid::nil())
                },
                "C:/p"
            ),
            Err(InvalidRequest::NotCli)
        );
    }

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
    fn a_command_script_quotes_every_word() {
        let cmd = TerminalCommand {
            cwd: "/Users/me/it's here".into(),
            argv: vec![
                "claude".into(),
                "--".into(),
                "a 'b' $HOME `x`\nпривет".into(),
            ],
        };
        assert_eq!(
            cmd.command_script(),
            concat!(
                "#!/bin/zsh -il\n",
                "rm -f -- \"$0\"\n",
                r"cd '/Users/me/it'\''s here' && exec 'claude' '--' 'a '\''b'\'' $HOME `x`",
                "\nпривет'\n"
            )
        );
    }

    #[test]
    fn claude_terminal_uses_the_program_path() {
        let req = claude(None, None, Target::New(Uuid::nil()));
        let args = terminal_args(r"C:\bin\claude.exe", "C:/p", &req, "fix it")
            .unwrap()
            .wt_args();
        assert_eq!(args[..3], ["-d", "C:/p", r"C:\bin\claude.exe"]);
        assert_eq!(args[args.len() - 2..], ["--", "fix it"]);
        let args = resume_in_terminal("claude", "C:/p", Agent::Claude, NIL)
            .unwrap()
            .wt_args();
        assert_eq!(args, ["-d", "C:/p", "claude", "--resume", NIL]);
        assert_eq!(
            resume_in_terminal("claude", "C:/p;calc", Agent::Claude, NIL),
            Err(InvalidRequest::Cwd)
        );
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
            ["exec", "--json", "--skip-git-repo-check", "-C", r"C:\p"]
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
                "--skip-git-repo-check",
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
                "--json",
                "--skip-git-repo-check"
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
        let args = terminal_args("codex", "C:/p", &req, "fix it")
            .unwrap()
            .wt_args();
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
        let args = terminal_args(r"C:\npm\codex.cmd", "C:/p", &req, "fix; it")
            .unwrap()
            .wt_args();
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
        let args = resume_in_terminal("codex", "C:/p", Agent::Codex, "abc-1")
            .unwrap()
            .wt_args();
        assert_eq!(args, ["-d", "C:/p", "codex", "resume", "abc-1"]);
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
        let args = terminal_args(r"C:\npm\codex.CMD", "C:/p", &req, prompt)
            .unwrap()
            .wt_args();
        let sent = args.last().unwrap();
        assert!(
            !sent.contains(['"', '&', '|', '<', '>', '^', '%']),
            "{sent}"
        );
        assert!(sent.contains("rename") && sent.contains("calc"), "{sent}");
        let args = terminal_args(r"C:\bin\codex.exe", "C:/p", &req, "a & b")
            .unwrap()
            .wt_args();
        assert_eq!(args.last().unwrap(), "a & b");
    }
}
