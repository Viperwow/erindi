use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::claude::{self, ClaudeMode, ClaudeRequest, Session};

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
        Agent::Codex => unimplemented!("Task 2 {cwd}"),
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
    match req.agent {
        Agent::Claude => claude::run_in_terminal(program, cwd, &claude_request(req)?, prompt)
            .map_err(|e| match e {
                claude::InvalidTerminalRun::Cwd => InvalidRequest::Cwd,
                claude::InvalidTerminalRun::Model => InvalidRequest::Model,
            }),
        Agent::Codex => unimplemented!("Task 2"),
    }
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
        Agent::Codex => unimplemented!("Task 2"),
    }
}

pub fn env(
    agent: Agent,
    vars: impl IntoIterator<Item = (String, String)>,
) -> Vec<(String, String)> {
    match agent {
        Agent::Claude => claude::claude_env(vars),
        Agent::Codex => unimplemented!("Task 2"),
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
}
