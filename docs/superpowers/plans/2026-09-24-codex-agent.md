# Codex Agent Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Erindi runs Codex next to Claude Code: a default agent in Settings, a spoken agent name that starts a new session with that agent, sessions that always continue with their own agent, and a Sessions tab that shows each session's agent, current model and current permission.

**Architecture:** `erindi-core` gets `enum Agent { Claude, Codex }` with one module per agent (`claude.rs`, new `codex.rs`) and a dispatch module `agent.rs`. Erindi keeps its own UUID per session and stores the agent's native ID in history; Codex reports its ID in `thread.started`. Agent CLIs are found on a PATH read fresh from the registry, and agent state lives in Rust and reaches the UI through events.

**Tech Stack:** Rust 2024 (tokio, serde, serde_json, regex, uuid, `windows` 0.62), Tauri 2, Preact + Tailwind + TypeScript.

**Spec:** `docs/superpowers/specs/2026-09-24-codex-agent-design.md`

## Global Constraints

- Windows only. `cfg(windows)` code needs a non-Windows stub only where the crate already builds on other targets.
- Prompts reach an agent only through stdin for headless runs, and only after `--` for terminal runs. Never build a shell command from user text.
- Default model and default permission pass no flag.
- Model IDs are rejected when empty, when they start with `-`, or when they contain whitespace.
- A continued session passes no model and no permission flags, for every agent.
- Codex headless: `codex exec --json …` for a new session, `codex exec resume <id> --json` to continue, the prompt on stdin.
- Codex permissions: `read-only`, `workspace-write`, `danger-full-access`, passed as `-s <value>`.
- Claude permissions: `acceptEdits`, `auto`, `plan`, `dontAsk`, `bypassPermissions`, passed as `--permission-mode <value>`.
- Claude model list: Default, Fable, Opus, Sonnet, Haiku (`fable`, `opus`, `sonnet`, `haiku`), Custom model ID.
- Codex model list: Default (config.toml), models with `visibility: "list"` from `codex debug models`, Custom model ID.
- Missing CLI text: `"<Label> CLI not found. Install it, then press Re-check in Settings."`
- Rust owns state; the UI renders it and re-renders on `agents-changed` and `sessions-changed`.
- Commit messages follow Conventional Commits with a lowercase subject (commitlint runs on every commit).
- Test first, then code, then commit; one task per commit unless a task says otherwise.
- Run `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` before each commit; run `pnpm build` in `apps/desktop` before committing UI changes.

## Review Focus

1. **Codex installed through npm is `codex.cmd`, not `codex.exe`.** `Command::new("codex")` on Windows only finds `.exe`, so a plain name fails silently. Expected: Erindi runs the resolved full path. Pinned by the `cmd_files_are_found_through_pathext` test in Task 5 and by passing `program` everywhere in Tasks 1, 2 and 11.
2. **A session log that is large or still being written.** Expected: reading the newest entry never loads a multi-megabyte file whole and tolerates a half-written last line. Pinned by `newest_entry_ignores_a_half_written_last_line` and `reads_only_the_tail_of_big_logs` in Task 6.
3. **A phrase such as "Claude.md обнови" or "codex review this".** Expected: agent patterns match only at the start of a phrase and never at the end; the rest keeps the words after the name. Pinned by `agent_names_count_only_at_the_start` in Task 7.
4. **A Codex run that ends before `thread.started`** (no network, bad login). Expected: the session is stored without a native ID, is not active afterwards, and cannot be continued. Pinned by `codex_session_without_native_id_is_forgotten` in Task 11 and `continue_session` returning an error.
5. **Old files.** Expected: a `settings.json` with `mode`/`model` and a `sessions.json` without `agent` keep working as Claude. Pinned by `old_mode_and_model_move_to_claude` in Task 9 and `entries_without_agent_are_claude` in Task 10.

---

## File Structure

| File | Responsibility |
|------|----------------|
| `crates/core/src/agent.rs` (new) | `Agent`, `Target`, `AgentRequest`, validation, dispatch to the agent modules, `EventParser` |
| `crates/core/src/claude.rs` | Claude arguments; terminal functions take the resolved program path |
| `crates/core/src/codex.rs` (new) | Codex arguments, env allowlist, `--json` parser, model catalog parser |
| `crates/core/src/stream.rs` | `RunEvent` gains `SessionStarted`; Claude parser unchanged |
| `crates/core/src/cli.rs` (new) | Finding an agent CLI on a PATH; reading the current PATH from the registry |
| `crates/core/src/transcript.rs` (new) | Current model and permission from an agent's session log; readable model names |
| `crates/core/src/commands.rs` | `Command::Claude`, `Command::Codex`, start-only matching |
| `crates/core/src/session.rs` | `Active` remembers its agent |
| `crates/core/src/controller.rs` | Picks the agent per phrase; effects carry the agent |
| `crates/core/examples/codex-smoke.rs` (new) | Manual check against the real Codex CLI |
| `apps/desktop/src-tauri/src/settings.rs` | `agent`, `agents` per agent, migration, validation |
| `apps/desktop/src-tauri/src/history.rs` | `agent`, `nativeId`, `startedModel`, `startedPermission` |
| `apps/desktop/src-tauri/src/agents.rs` (new) | Agent state: CLI path, model list, errors; `agents-changed` |
| `apps/desktop/src-tauri/src/runtime.rs` | Runs through `AgentRequest`, stores native IDs, reports a missing CLI |
| `apps/desktop/src-tauri/src/lib.rs`, `build.rs`, `capabilities/settings.json` | Commands `agent_status`, `recheck_agents`; session details in `list_sessions` |
| `apps/desktop/src/controls.tsx` | Types and the per-agent lists |
| `apps/desktop/src/settings.tsx` | Agent section |
| `apps/desktop/src/commands.tsx` | Claude and Codex commands |
| `apps/desktop/src/sessions.tsx` | Agent icon, current model and permission, "can't resume" |
| `apps/desktop/src/icons/claude.svg`, `openai.svg` (new) | Agent icons |

---

### Task 1: `Agent`, `AgentRequest` and Claude dispatch

**Files:**
- Create: `crates/core/src/agent.rs`
- Modify: `crates/core/src/lib.rs`, `crates/core/src/claude.rs:131-162` and its terminal tests

**Interfaces:**
- Produces:
  - `pub enum Agent { Claude, Codex }` (`Copy`, `Default = Claude`, serde `"claude"`/`"codex"`), `Agent::ALL: [Agent; 2]`, `fn label(self) -> &'static str`, `fn cli(self) -> &'static str`, `fn permissions(self) -> &'static [&'static str]`
  - `pub enum Target { New(Uuid), Resume(String) }` — `Resume` holds the agent's native ID
  - `pub struct AgentRequest { pub agent: Agent, pub model: Option<String>, pub permission: Option<String>, pub target: Target }`
  - `pub enum InvalidRequest { Model, Permission, Cwd, NativeId }`
  - `pub fn valid_model(model: &str) -> bool`
  - `pub fn headless_args(req: &AgentRequest, cwd: &str) -> Result<Vec<String>, InvalidRequest>`
  - `pub fn terminal_args(program: &str, cwd: &str, req: &AgentRequest, prompt: &str) -> Result<Vec<String>, InvalidRequest>`
  - `pub fn resume_in_terminal(program: &str, cwd: &str, agent: Agent, native_id: &str) -> Result<Vec<String>, InvalidRequest>`
  - `pub fn env(agent: Agent, vars: impl IntoIterator<Item = (String, String)>) -> Vec<(String, String)>`
  - `claude::run_in_terminal(program: &str, cwd, req, prompt)` and `claude::resume_in_terminal(program: &str, cwd, session_id)` now take `program` first.

- [ ] **Step 1: Change the Claude terminal tests to take the program path**

In `crates/core/src/claude.rs` tests, replace the terminal calls and expectations:

```rust
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
```

and in `terminal_run_passes_the_prompt_after_the_options` call `run_in_terminal("claude", "C:/p", &req, …)` in all three places (the expected third element stays `"claude"`).

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test -p erindi-core claude::`
Expected: compile errors, `resume_in_terminal` takes 2 arguments.

- [ ] **Step 3: Add `program` to the Claude terminal functions**

In `crates/core/src/claude.rs`:

```rust
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
```

Update the callers so the workspace compiles: in `apps/desktop/src-tauri/src/runtime.rs` pass `"claude"` as the first argument to both functions, and in `apps/desktop/src-tauri/src/settings.rs:103` call `resume_in_terminal("claude", &self.cwd, Uuid::nil())`. Task 11 replaces these calls.

- [ ] **Step 4: Run the tests to see them pass**

Run: `cargo test -p erindi-core claude::`
Expected: PASS.

- [ ] **Step 5: Write the failing `agent.rs` tests**

Create `crates/core/src/agent.rs` with only the tests module and `pub mod agent;` in `lib.rs`:

```rust
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
        assert_eq!(serde_json::from_str::<Agent>("\"claude\"").unwrap(), Agent::Claude);
    }

    #[test]
    fn claude_new_session_passes_its_id_model_and_permission() {
        let req = claude(Some("opus"), Some("plan"), Target::New(Uuid::nil()));
        assert_eq!(
            headless_args(&req, "C:/p").unwrap(),
            ["-p", "--output-format", "stream-json", "--verbose", "--session-id", NIL,
             "--permission-mode", "plan", "--model", "opus"]
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
            assert_eq!(headless_args(&req, "C:/p"), Err(InvalidRequest::Model), "{bad}");
        }
    }

    #[test]
    fn permission_lists_match_the_clis() {
        assert_eq!(
            Agent::Claude.permissions(),
            ["acceptEdits", "auto", "plan", "dontAsk", "bypassPermissions"]
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
        let vars = [("PATH", "x"), ("OPENAI_API_KEY", "leak"), ("ANTHROPIC_API_KEY", "k")]
            .map(|(k, v)| (k.to_string(), v.to_string()));
        let kept: Vec<_> = env(Agent::Claude, vars).into_iter().map(|(k, _)| k).collect();
        assert_eq!(kept, ["PATH", "ANTHROPIC_API_KEY"]);
    }
}
```

- [ ] **Step 6: Run them to see them fail**

Run: `cargo test -p erindi-core agent::`
Expected: compile errors, `Agent` not found.

- [ ] **Step 7: Implement `agent.rs` for Claude**

Above the tests in `crates/core/src/agent.rs`:

```rust
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::claude::{self, ClaudeMode, ClaudeRequest, Session};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
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
            Agent::Claude => &["acceptEdits", "auto", "plan", "dontAsk", "bypassPermissions"],
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
        Agent::Claude => claude::claude_args(&claude_request(req)?).map_err(|_| InvalidRequest::Model),
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

pub fn env(agent: Agent, vars: impl IntoIterator<Item = (String, String)>) -> Vec<(String, String)> {
    match agent {
        Agent::Claude => claude::claude_env(vars),
        Agent::Codex => unimplemented!("Task 2"),
    }
}
```

The `unimplemented!` arms exist only until Task 2 replaces them in the same branch; no commit ships them to `main`.

- [ ] **Step 8: Run the tests**

Run: `cargo test -p erindi-core`
Expected: PASS.

- [ ] **Step 9: Commit**

```bash
git add crates/core apps/desktop/src-tauri/src
git commit -m "feat(core): agent requests with Claude as the first agent"
```

---

### Task 2: Codex arguments, terminal and env

**Files:**
- Create: `crates/core/src/codex.rs`
- Modify: `crates/core/src/agent.rs` (Codex arms), `crates/core/src/lib.rs`

**Interfaces:**
- Consumes: `AgentRequest`, `Target`, `InvalidRequest`, `check`, `cwd_ok` from Task 1.
- Produces: `codex::exec_args(model: Option<&str>, sandbox: Option<&str>, target: &Target, cwd: &str) -> Vec<String>`, `codex::terminal_args(program, cwd, model, sandbox, prompt) -> Vec<String>`, `codex::resume_in_terminal(program, cwd, native_id) -> Vec<String>`, `codex::codex_env(vars)`; the `agent.rs` Codex arms.

- [ ] **Step 1: Write the failing tests** in `crates/core/src/agent.rs` tests:

```rust
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
        assert_eq!(headless_args(&req, r"C:\p").unwrap(), ["exec", "--json", "-C", r"C:\p"]);
        let req = codex(Some("gpt-5.5"), Some("workspace-write"), Target::New(Uuid::nil()));
        assert_eq!(
            headless_args(&req, r"C:\p").unwrap(),
            ["exec", "--json", "-C", r"C:\p", "-m", "gpt-5.5", "-s", "workspace-write"]
        );
    }

    #[test]
    fn codex_resume_passes_only_the_native_id() {
        let req = codex(None, None, Target::Resume("01a0d2c0-0c6d-7dc0-90c7-da4ffbaf65a2".into()));
        assert_eq!(
            headless_args(&req, r"C:\p").unwrap(),
            ["exec", "resume", "01a0d2c0-0c6d-7dc0-90c7-da4ffbaf65a2", "--json"]
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
    fn codex_terminal_runs_the_interactive_cli() {
        let req = codex(Some("gpt-5.5"), Some("read-only"), Target::New(Uuid::nil()));
        let args = terminal_args(r"C:\npm\codex.cmd", "C:/p", &req, "fix; it").unwrap();
        assert_eq!(
            args,
            ["-d", "C:/p", r"C:\npm\codex.cmd", "-m", "gpt-5.5", "-s", "read-only", "--", r"fix\; it"]
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
        let kept: Vec<_> = env(Agent::Codex, vars).into_iter().map(|(k, _)| k).collect();
        assert_eq!(kept, ["Path", "CODEX_HOME", "OPENAI_API_KEY", "OPENAI_BASE_URL"]);
    }
```

- [ ] **Step 2: Run to see them fail**

Run: `cargo test -p erindi-core agent::`
Expected: panics `not implemented: Task 2`.

- [ ] **Step 3: Implement `codex.rs`**

`crates/core/src/codex.rs`:

```rust
use crate::agent::Target;

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
        Target::Resume(id) => ["exec", "resume", id.as_str(), "--json"].map(String::from).into(),
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
    ["-d", cwd, program, "resume", native_id].map(String::from).into()
}
```

In `crates/core/src/claude.rs`, split the allowlist so Codex reuses the Windows basics without Claude's prefixes:

```rust
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
```

In `crates/core/src/agent.rs`, add `use crate::codex;`, a native ID check, and replace the four `unimplemented!` arms:

```rust
/// Native IDs are UUIDs for both agents; anything else could be read as an option.
fn native_ok(id: &str) -> Result<(), InvalidRequest> {
    let ok = !id.is_empty()
        && !id.starts_with('-')
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    if ok { Ok(()) } else { Err(InvalidRequest::NativeId) }
}
```

```rust
        Agent::Codex => {
            check(req)?;
            if let Target::Resume(id) = &req.target {
                native_ok(id)?;
            }
            Ok(codex::exec_args(req.model.as_deref(), req.permission.as_deref(), &req.target, cwd))
        }
```

(in `headless_args`), and in `terminal_args`:

```rust
        Agent::Codex => {
            check(req)?;
            Ok(codex::terminal_args(program, cwd, req.model.as_deref(), req.permission.as_deref(), prompt))
        }
```

in `resume_in_terminal`:

```rust
        Agent::Codex => {
            native_ok(native_id)?;
            Ok(codex::resume_in_terminal(program, cwd, native_id))
        }
```

in `env`: `Agent::Codex => codex::codex_env(vars),`. Add `pub mod codex;` to `lib.rs`.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p erindi-core`
Expected: PASS, including the unchanged `env_keeps_only_allowlisted_vars`.

- [ ] **Step 5: Commit**

```bash
git add crates/core
git commit -m "feat(core): run Codex through codex exec --json"
```

---

### Task 3: Codex events and one parser per run

**Files:**
- Modify: `crates/core/src/stream.rs`, `crates/core/src/codex.rs`, `crates/core/src/agent.rs`, `crates/core/src/controller.rs:348-361`
- Create: `crates/core/tests/fixtures/codex-exec.jsonl`

**Interfaces:**
- Produces: `RunEvent::SessionStarted { native_id: String }`, `RunEvent::Reply { text: String }`; `codex::parse_line(line: &str) -> Vec<RunEvent>`; `agent::EventParser::new(agent)`, `fn feed(&mut self, line: &str) -> Vec<RunEvent>`.

- [ ] **Step 1: Record the fixture**

Create `crates/core/tests/fixtures/codex-exec.jsonl` (recorded from `codex exec --json` 0.156.1 and trimmed):

```
{"type":"thread.started","thread_id":"01a0d2c0-0c6d-7dc0-90c7-da4ffbaf65a2"}
{"type":"turn.started"}
{"type":"item.started","item":{"id":"item_0","type":"command_execution","command":"git diff","status":"in_progress"}}
{"type":"item.completed","item":{"id":"item_0","type":"command_execution","command":"git diff","exit_code":0,"status":"completed"}}
{"type":"item.started","item":{"id":"item_1","type":"file_change","changes":[{"path":"src/a.rs","kind":"update"}],"status":"in_progress"}}
{"type":"item.completed","item":{"id":"item_2","type":"agent_message","text":"pong"}}
{"type":"turn.completed","usage":{"input_tokens":21106,"cached_input_tokens":11392,"output_tokens":5}}
```

- [ ] **Step 2: Write the failing tests** in `crates/core/src/codex.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::stream::RunEvent;

    const FIXTURE: &str = include_str!("../tests/fixtures/codex-exec.jsonl");

    #[test]
    fn events_from_a_real_run() {
        let events: Vec<_> = FIXTURE.lines().flat_map(parse_line).collect();
        assert_eq!(
            events,
            [
                RunEvent::SessionStarted { native_id: "01a0d2c0-0c6d-7dc0-90c7-da4ffbaf65a2".into() },
                RunEvent::ToolUse { name: "git diff".into() },
                RunEvent::ToolUse { name: "Edit src/a.rs".into() },
                RunEvent::Reply { text: "pong".into() },
                RunEvent::Result { ok: true, text: String::new() },
            ]
        );
    }

    #[test]
    fn failures() {
        let line = r#"{"type":"turn.failed","error":{"message":"stream disconnected"}}"#;
        assert_eq!(parse_line(line), [RunEvent::Result { ok: false, text: "stream disconnected".into() }]);
        let line = r#"{"type":"error","message":"Not logged in"}"#;
        assert_eq!(parse_line(line), [RunEvent::Result { ok: false, text: "Not logged in".into() }]);
    }

    #[test]
    fn ignores_noise_and_garbage() {
        for line in ["", "x", "{\"type\":", "[1]", r#"{"type":"turn.started"}"#,
                     r#"{"type":"item.started","item":"oops"}"#,
                     r#"{"type":"item.completed","item":{"type":"reasoning","text":"…"}}"#] {
            assert_eq!(parse_line(line), [], "{line}");
        }
    }
}
```

and in `crates/core/src/agent.rs` tests:

```rust
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
        assert_eq!(events.last(), Some(&RunEvent::Result { ok: true, text: "done".into() }));
        assert!(!events.iter().any(|e| matches!(e, RunEvent::Reply { .. })));
    }

    #[test]
    fn parser_passes_claude_lines_through() {
        let mut p = EventParser::new(Agent::Claude);
        let line = r#"{"type":"result","subtype":"success","is_error":false,"result":"ok"}"#;
        assert_eq!(p.feed(line), [RunEvent::Result { ok: true, text: "ok".into() }]);
    }
```

- [ ] **Step 3: Run to see them fail**

Run: `cargo test -p erindi-core`
Expected: compile errors, no `SessionStarted`, `Reply`, `parse_line` in `codex`, `EventParser`.

- [ ] **Step 4: Implement**

In `crates/core/src/stream.rs` extend the enum (doc comment now covers both agents):

```rust
/// Progress events from an agent run, reduced to what Erindi uses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RunEvent {
    /// The agent's own ID for the session, when the agent picks it.
    SessionStarted { native_id: String },
    ToolUse { name: String },
    PermissionDenied { tool: String },
    /// A reply message before the run ends; the last one becomes the result text.
    Reply { text: String },
    Result { ok: bool, text: String },
}
```

In `crates/core/src/codex.rs`:

```rust
use serde_json::Value;

use crate::stream::RunEvent;

/// Unknown, malformed or irrelevant lines yield no events.
pub fn parse_line(line: &str) -> Vec<RunEvent> {
    let Ok(v) = serde_json::from_str::<Value>(line) else {
        return vec![];
    };
    let text = |v: &Value| v.as_str().unwrap_or_default().to_string();
    let item = &v["item"];
    match (v["type"].as_str(), item["type"].as_str()) {
        (Some("thread.started"), _) => match v["thread_id"].as_str() {
            Some(id) => vec![RunEvent::SessionStarted { native_id: id.into() }],
            None => vec![],
        },
        (Some("item.started"), Some("command_execution")) => {
            vec![RunEvent::ToolUse { name: text(&item["command"]) }]
        }
        (Some("item.started"), Some("file_change")) => {
            let path = text(&item["changes"][0]["path"]);
            vec![RunEvent::ToolUse { name: format!("Edit {path}") }]
        }
        (Some("item.completed"), Some("agent_message")) => {
            vec![RunEvent::Reply { text: text(&item["text"]) }]
        }
        (Some("turn.completed"), _) => vec![RunEvent::Result { ok: true, text: String::new() }],
        (Some("turn.failed"), _) => vec![RunEvent::Result { ok: false, text: text(&v["error"]["message"]) }],
        (Some("error"), _) => vec![RunEvent::Result { ok: false, text: text(&v["message"]) }],
        _ => vec![],
    }
}
```

In `crates/core/src/agent.rs`:

```rust
use crate::stream::{self, RunEvent};

/// Parses one run's output. Codex sends the reply before the result, so the parser keeps it.
pub struct EventParser {
    agent: Agent,
    reply: String,
}

impl EventParser {
    pub fn new(agent: Agent) -> Self {
        Self { agent, reply: String::new() }
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
```

In `crates/core/src/controller.rs`, the `Msg::Run` match gains an arm so the new variants compile (the runtime handles `SessionStarted`):

```rust
                RunEvent::SessionStarted { .. } | RunEvent::Reply { .. } => vec![],
```

- [ ] **Step 5: Run the tests**

Run: `cargo test -p erindi-core`
Expected: PASS; `stream::tests` unchanged.

- [ ] **Step 6: Commit**

```bash
git add crates/core
git commit -m "feat(core): read Codex progress, reply and session ID from its JSON events"
```

---

### Task 4: Model lists

**Files:**
- Modify: `crates/core/src/codex.rs`, `crates/core/src/agent.rs`
- Create: `crates/core/tests/fixtures/codex-models.json`

**Interfaces:**
- Produces: `#[derive(Serialize)] pub struct ModelOption { pub id: String, pub label: String }`; `codex::parse_models(json: &str) -> Result<Vec<ModelOption>, String>`; `agent::claude_models() -> Vec<ModelOption>`.

- [ ] **Step 1: Fixture** `crates/core/tests/fixtures/codex-models.json` (trimmed from `codex debug models`):

```json
{"models":[
 {"slug":"gpt-6-sol","display_name":"GPT-6-Sol","visibility":"list","supported_in_api":true},
 {"slug":"gpt-reserve","display_name":"GPT-Reserve","visibility":"hide","supported_in_api":true},
 {"slug":"gpt-5.5","display_name":"GPT-5.5","visibility":"list","supported_in_api":true}
]}
```

- [ ] **Step 2: Failing tests** in `codex.rs` tests:

```rust
    #[test]
    fn listed_models_only() {
        let models = parse_models(include_str!("../tests/fixtures/codex-models.json")).unwrap();
        let ids: Vec<_> = models.iter().map(|m| (m.id.as_str(), m.label.as_str())).collect();
        assert_eq!(ids, [("gpt-6-sol", "GPT-6-Sol"), ("gpt-5.5", "GPT-5.5")]);
    }

    #[test]
    fn broken_catalog_is_an_error() {
        for bad in ["", "{", r#"{"models":"x"}"#, r#"{"other":[]}"#] {
            assert!(parse_models(bad).is_err(), "{bad}");
        }
    }
```

and in `agent.rs` tests:

```rust
    #[test]
    fn claude_models_are_the_aliases() {
        let ids: Vec<_> = claude_models().into_iter().map(|m| m.id).collect();
        assert_eq!(ids, ["fable", "opus", "sonnet", "haiku"]);
    }
```

- [ ] **Step 3: Run to see them fail**

Run: `cargo test -p erindi-core`
Expected: compile errors.

- [ ] **Step 4: Implement**

In `agent.rs`:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModelOption {
    pub id: String,
    pub label: String,
}

/// Claude Code has no command that lists models; its aliases always point at the latest one.
pub fn claude_models() -> Vec<ModelOption> {
    [("fable", "Fable"), ("opus", "Opus"), ("sonnet", "Sonnet"), ("haiku", "Haiku")]
        .map(|(id, label)| ModelOption { id: id.into(), label: label.into() })
        .into()
}
```

In `codex.rs`:

```rust
use crate::agent::ModelOption;

/// Models from `codex debug models` that Codex shows in its own picker.
pub fn parse_models(json: &str) -> Result<Vec<ModelOption>, String> {
    let v: Value = serde_json::from_str(json).map_err(|e| format!("not JSON: {e}"))?;
    let list = v["models"].as_array().ok_or("no \"models\" list")?;
    Ok(list
        .iter()
        .filter(|m| m["visibility"] == "list")
        .filter_map(|m| {
            let id = m["slug"].as_str()?;
            let label = m["display_name"].as_str().unwrap_or(id);
            Some(ModelOption { id: id.into(), label: label.into() })
        })
        .collect())
}
```

- [ ] **Step 5: Run the tests** — `cargo test -p erindi-core`, expected PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/core
git commit -m "feat(core): model lists for Claude and Codex"
```

---

### Task 5: Finding an agent CLI on a fresh PATH

**Files:**
- Create: `crates/core/src/cli.rs`
- Modify: `crates/core/src/lib.rs`, `crates/core/Cargo.toml` (feature `Win32_System_Registry`)

**Interfaces:**
- Produces: `cli::find(name: &str, path: &str, pathext: &str) -> Option<PathBuf>`; `cli::current_path() -> String` (registry on Windows, `PATH` env elsewhere); `cli::locate(agent: Agent) -> Option<PathBuf>`.

- [ ] **Step 1: Failing tests** in `crates/core/src/cli.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn touch(dir: &std::path::Path, name: &str) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, "").unwrap();
        p
    }

    #[test]
    fn cmd_files_are_found_through_pathext() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let cmd = touch(b.path(), "codex.cmd");
        let path = format!("{};{}", a.path().display(), b.path().display());
        assert_eq!(find("codex", &path, ".COM;.EXE;.BAT;.CMD"), Some(cmd));
    }

    #[test]
    fn earlier_directories_win_and_exe_beats_cmd_in_one_directory() {
        let a = tempfile::tempdir().unwrap();
        let exe = touch(a.path(), "claude.exe");
        touch(a.path(), "claude.cmd");
        let path = format!("{};C:\\nowhere", a.path().display());
        assert_eq!(find("claude", &path, ".EXE;.CMD"), Some(exe));
    }

    #[test]
    fn missing_or_empty_entries_find_nothing() {
        assert_eq!(find("codex", ";;C:\\nowhere", ".EXE;.CMD"), None);
        assert_eq!(find("codex", "", ".EXE"), None);
    }

    #[test]
    fn quoted_entries_are_unquoted() {
        let a = tempfile::tempdir().unwrap();
        let exe = touch(a.path(), "codex.exe");
        let path = format!("\"{}\"", a.path().display());
        assert_eq!(find("codex", &path, ".EXE"), Some(exe));
    }

    #[test]
    fn merge_puts_system_entries_first() {
        assert_eq!(merge("C:\\sys;C:\\win", "C:\\user"), "C:\\sys;C:\\win;C:\\user");
        assert_eq!(merge("C:\\sys", ""), "C:\\sys");
    }
}
```

- [ ] **Step 2: Run to see them fail** — `cargo test -p erindi-core cli::`, compile errors.

- [ ] **Step 3: Implement**

`crates/core/Cargo.toml` Windows features become `["Win32_System_Threading", "Win32_System_JobObjects", "Win32_Security", "Win32_System_Registry"]`.

`crates/core/src/cli.rs`:

```rust
use std::path::PathBuf;

use crate::agent::Agent;

/// The first `name` + extension from `pathext` in the directories of `path`.
pub fn find(name: &str, path: &str, pathext: &str) -> Option<PathBuf> {
    let exts: Vec<&str> = pathext.split(';').filter(|e| !e.is_empty()).collect();
    path.split(';')
        .map(|d| d.trim().trim_matches('"'))
        .filter(|d| !d.is_empty())
        .find_map(|dir| {
            exts.iter()
                .map(|ext| PathBuf::from(dir).join(format!("{name}{}", ext.to_lowercase())))
                .find(|p| p.is_file())
        })
}

/// Windows builds a process PATH from the system value followed by the user value.
fn merge(system: &str, user: &str) -> String {
    [system, user].into_iter().filter(|p| !p.is_empty()).collect::<Vec<_>>().join(";")
}

/// The PATH a newly started program would get, so a CLI installed while Erindi runs is found.
#[cfg(windows)]
pub fn current_path() -> String {
    let system = registry_value(
        windows::Win32::System::Registry::HKEY_LOCAL_MACHINE,
        r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment",
    );
    let user = registry_value(windows::Win32::System::Registry::HKEY_CURRENT_USER, "Environment");
    match (system, user) {
        (None, None) => std::env::var("PATH").unwrap_or_default(),
        (s, u) => merge(&s.unwrap_or_default(), &u.unwrap_or_default()),
    }
}

#[cfg(not(windows))]
pub fn current_path() -> String {
    std::env::var("PATH").unwrap_or_default()
}

/// `Path` of `subkey`, with `%VARS%` expanded by the registry API.
#[cfg(windows)]
fn registry_value(root: windows::Win32::System::Registry::HKEY, subkey: &str) -> Option<String> {
    use windows::Win32::System::Registry::{RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ, RegGetValueW};
    use windows::core::HSTRING;
    let (key, value) = (HSTRING::from(subkey), HSTRING::from("Path"));
    let mut size = 0u32;
    let flags = RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ;
    unsafe { RegGetValueW(root, &key, &value, flags, None, None, Some(&mut size)) }.ok().ok()?;
    let mut buf = vec![0u16; (size as usize).div_ceil(2)];
    unsafe {
        RegGetValueW(root, &key, &value, flags, None, Some(buf.as_mut_ptr().cast()), Some(&mut size))
    }
    .ok()
    .ok()?;
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..len]))
}

pub fn locate(agent: Agent) -> Option<PathBuf> {
    let pathext = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into());
    find(agent.cli(), &current_path(), &pathext)
}
```

Add `pub mod cli;` to `lib.rs`. If `RegGetValueW`'s signature in `windows` 0.62 differs (argument order or `Option` wrapping), follow the compiler: the call reads `Path` under `subkey` of `root` into `buf` with `RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ`.

- [ ] **Step 4: Run the tests** — `cargo test -p erindi-core cli::`, expected PASS. Also run once by hand: `cargo test -p erindi-core cli:: -- --nocapture` and add a temporary `println!("{:?}", locate(Agent::Codex))` in a scratch test to see the real path; remove it before committing.

- [ ] **Step 5: Commit**

```bash
git add crates/core
git commit -m "feat(core): find agent CLIs on the PATH as it is now, not at launch"
```

---

### Task 6: Session details from the agents' logs

**Files:**
- Create: `crates/core/src/transcript.rs`, `crates/core/tests/fixtures/claude-log.jsonl`, `crates/core/tests/fixtures/codex-log.jsonl`
- Modify: `crates/core/src/lib.rs`

**Interfaces:**
- Produces: `#[derive(Serialize)] pub struct Details { pub model: Option<String>, pub permission: Option<String> }`; `transcript::read(agent, path: &Path) -> Option<Details>`; `transcript::find_logs(agent, home: &Path) -> HashMap<String, PathBuf>` (native ID → log path); `transcript::model_label(raw: &str) -> String`.

- [ ] **Step 1: Fixtures**

`crates/core/tests/fixtures/claude-log.jsonl`:

```
{"type":"user","permissionMode":"default","message":{"role":"user","content":"hi"}}
{"type":"assistant","message":{"model":"claude-opus-5","content":[]}}
{"type":"user","permissionMode":"plan","message":{"role":"user","content":"again"}}
{"type":"assistant","message":{"model":"claude-opus-5-5","content":[]}}
{"type":"assistant","message":{"model":"<synthetic>","content":[]}}
```

`crates/core/tests/fixtures/codex-log.jsonl`:

```
{"type":"session_meta","payload":{"id":"01a0d2c0-0c6d-7dc0-90c7-da4ffbaf65a2"}}
{"type":"turn_context","payload":{"model":"gpt-5.5","sandbox_policy":{"type":"read-only"},"approval_policy":"never"}}
{"type":"turn_context","payload":{"model":"gpt-5.6-sol","sandbox_policy":{"type":"workspace-write"},"approval_policy":"never"}}
{"type":"event_msg","payload":{"type":"task_complete"}}
```

- [ ] **Step 2: Failing tests** in `transcript.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, name: &str, text: &str) -> PathBuf {
        let p = dir.join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, text).unwrap();
        p
    }

    #[test]
    fn newest_claude_model_and_permission() {
        let d = tempfile::tempdir().unwrap();
        let p = write(d.path(), "a.jsonl", include_str!("../tests/fixtures/claude-log.jsonl"));
        let got = read(Agent::Claude, &p).unwrap();
        assert_eq!(got.model.as_deref(), Some("claude-opus-5-5"));
        assert_eq!(got.permission.as_deref(), Some("plan"));
    }

    #[test]
    fn newest_codex_turn_context() {
        let d = tempfile::tempdir().unwrap();
        let p = write(d.path(), "a.jsonl", include_str!("../tests/fixtures/codex-log.jsonl"));
        let got = read(Agent::Codex, &p).unwrap();
        assert_eq!(got.model.as_deref(), Some("gpt-5.6-sol"));
        assert_eq!(got.permission.as_deref(), Some("workspace-write"));
    }

    #[test]
    fn newest_entry_ignores_a_half_written_last_line() {
        let d = tempfile::tempdir().unwrap();
        let text = format!("{}{{\"type\":\"turn_con", include_str!("../tests/fixtures/codex-log.jsonl"));
        let p = write(d.path(), "a.jsonl", &text);
        assert_eq!(read(Agent::Codex, &p).unwrap().model.as_deref(), Some("gpt-5.6-sol"));
    }

    #[test]
    fn reads_only_the_tail_of_big_logs() {
        let d = tempfile::tempdir().unwrap();
        let filler = format!("{{\"type\":\"x\",\"pad\":\"{}\"}}\n", "a".repeat(1000)).repeat(2000);
        let text = format!("{}{filler}", include_str!("../tests/fixtures/codex-log.jsonl"));
        let p = write(d.path(), "a.jsonl", &text);
        // The only turn_context is 2 MB before the end, past the tail that is read.
        assert_eq!(read(Agent::Codex, &p), None);
    }

    #[test]
    fn empty_missing_or_broken_logs_give_none() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(read(Agent::Claude, &d.path().join("none.jsonl")), None);
        let p = write(d.path(), "e.jsonl", "");
        assert_eq!(read(Agent::Claude, &p), None);
        let p = write(d.path(), "b.jsonl", "not json\n[1]\n");
        assert_eq!(read(Agent::Codex, &p), None);
    }

    #[test]
    fn logs_are_found_by_native_id() {
        let d = tempfile::tempdir().unwrap();
        let c = write(d.path(), ".claude/projects/C--p/3f2a.jsonl", "");
        let x = write(
            d.path(),
            ".codex/sessions/2026/09/24/rollout-2026-09-24T13-29-56-01a0d2c0-0c6d-7dc0-90c7-da4ffbaf65a2.jsonl",
            "",
        );
        write(d.path(), ".codex/sessions/2026/09/24/notes.jsonl", "");
        assert_eq!(find_logs(Agent::Claude, d.path()).get("3f2a"), Some(&c));
        let codex = find_logs(Agent::Codex, d.path());
        assert_eq!(codex.get("01a0d2c0-0c6d-7dc0-90c7-da4ffbaf65a2"), Some(&x));
        assert_eq!(codex.len(), 1);
    }

    #[test]
    fn readable_model_names() {
        assert_eq!(model_label("claude-opus-5-5"), "Claude Opus 5.5");
        assert_eq!(model_label("claude-sonnet-4-6"), "Claude Sonnet 4.6");
        assert_eq!(model_label("claude-haiku-4-5-20251001"), "Claude Haiku 4.5");
        assert_eq!(model_label("claude-fable-5"), "Claude Fable 5");
        assert_eq!(model_label("gpt-5.6-sol"), "gpt-5.6-sol");
    }
}
```

- [ ] **Step 3: Run to see them fail** — `cargo test -p erindi-core transcript::`, compile errors.

- [ ] **Step 4: Implement** `crates/core/src/transcript.rs`:

```rust
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

use crate::agent::Agent;

/// How much of the end of a log is read; logs grow to many megabytes.
const TAIL: u64 = 512 << 10;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Details {
    pub model: Option<String>,
    pub permission: Option<String>,
}

/// The newest model and permission in `path`, or `None` when neither is found.
pub fn read(agent: Agent, path: &Path) -> Option<Details> {
    let text = tail(path).ok()?;
    let mut details = Details { model: None, permission: None };
    for line in text.lines().rev() {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        let (model, permission) = match agent {
            Agent::Claude => (
                v["message"]["model"].as_str().filter(|m| !m.starts_with('<')),
                v["permissionMode"].as_str(),
            ),
            Agent::Codex if v["type"] == "turn_context" => (
                v["payload"]["model"].as_str(),
                v["payload"]["sandbox_policy"]["type"].as_str(),
            ),
            Agent::Codex => (None, None),
        };
        details.model = details.model.or(model.map(String::from));
        details.permission = details.permission.or(permission.map(String::from));
        if details.model.is_some() && details.permission.is_some() {
            break;
        }
    }
    (details.model.is_some() || details.permission.is_some()).then_some(details)
}

/// The last `TAIL` bytes, starting at a line boundary.
fn tail(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let len = file.metadata()?.len();
    let start = len.saturating_sub(TAIL);
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    let text = String::from_utf8_lossy(&bytes).into_owned();
    Ok(if start > 0 {
        text.split_once('\n').map_or(String::new(), |(_, rest)| rest.to_string())
    } else {
        text
    })
}

/// Log files by native ID under the user's home folder.
pub fn find_logs(agent: Agent, home: &Path) -> HashMap<String, PathBuf> {
    let mut found = HashMap::new();
    match agent {
        // One folder per project, one `<id>.jsonl` per session.
        Agent::Claude => {
            for dir in read_dir(&home.join(".claude/projects")) {
                for file in read_dir(&dir) {
                    if let Some(id) = file.file_name().and_then(|n| n.to_str()).and_then(|n| n.strip_suffix(".jsonl")) {
                        found.insert(id.to_string(), file.clone());
                    }
                }
            }
        }
        // `sessions/YYYY/MM/DD/rollout-<time>-<id>.jsonl`; the ID is the UUID at the end.
        Agent::Codex => {
            let root = std::env::var_os("CODEX_HOME").map_or(home.join(".codex"), PathBuf::from);
            let mut stack = vec![root.join("sessions")];
            while let Some(dir) = stack.pop() {
                for entry in read_dir(&dir) {
                    if entry.is_dir() {
                        stack.push(entry);
                    } else if let Some(name) = entry.file_name().and_then(|n| n.to_str()) {
                        if let Some(id) = name.strip_suffix(".jsonl").and_then(uuid_suffix) {
                            found.insert(id.to_string(), entry.clone());
                        }
                    }
                }
            }
        }
    }
    found
}

/// `rollout-2026-09-24T13-29-56-01a0d2c0-0c6d-7dc0-90c7-da4ffbaf65a2` ends with the 36-character UUID.
fn uuid_suffix(stem: &str) -> Option<&str> {
    stem.strip_prefix("rollout-")?;
    let at = stem.len().checked_sub(36)?;
    let id = stem.get(at..)?;
    (stem[..at].ends_with('-') && uuid::Uuid::parse_str(id).is_ok()).then_some(id)
}

fn read_dir(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .map(|it| it.flatten().map(|e| e.path()).collect())
        .unwrap_or_default()
}

/// `claude-opus-5-5` reads as "Claude Opus 5.5"; other IDs stay as they are.
pub fn model_label(raw: &str) -> String {
    let Some(rest) = raw.strip_prefix("claude-") else {
        return raw.to_string();
    };
    let mut words = vec!["Claude".to_string()];
    let mut version = vec![];
    for part in rest.split('-') {
        if part.len() == 8 && part.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        if part.chars().all(|c| c.is_ascii_digit()) {
            version.push(part);
        } else {
            let mut chars = part.chars();
            let first = chars.next().map(|c| c.to_uppercase().collect::<String>()).unwrap_or_default();
            words.push(first + chars.as_str());
        }
    }
    if !version.is_empty() {
        words.push(version.join("."));
    }
    words.join(" ")
}
```

Real Codex log names look like `rollout-2026-09-24T13-29-56-01a0d2c0-0c6d-7dc0-90c7-da4ffbaf65a2.jsonl` (checked on Codex 0.156.1). Add `pub mod transcript;` to `lib.rs`.

- [ ] **Step 5: Run the tests** — `cargo test -p erindi-core transcript::`, expected PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/core
git commit -m "feat(core): current model and permission from the agents' session logs"
```

---

### Task 7: Spoken agent names

**Files:**
- Modify: `crates/core/src/commands.rs`, `apps/desktop/src/commands.tsx`, `apps/desktop/src/controls.tsx:11`

**Interfaces:**
- Produces: `Command::Claude`, `Command::Codex`; `Patterns { …, claude: Vec<String>, codex: Vec<String> }`; `Command::agent(self) -> Option<Agent>`.

- [ ] **Step 1: Failing tests** in `commands.rs` tests:

```rust
    #[test]
    fn agent_names_count_only_at_the_start() {
        let parser = Parser::new(&Patterns::default()).unwrap();
        assert_eq!(
            parser.parse("Codex, проверь diff"),
            (vec![Command::Codex], "проверь diff".to_string())
        );
        assert_eq!(
            parser.parse("в клоде напиши тесты"),
            (vec![Command::Claude], "напиши тесты".to_string())
        );
        for text in ["проверь diff в codex", "review this with claude"] {
            assert_eq!(parser.parse(text), (vec![], text.to_string()), "{text}");
        }
    }

    #[test]
    fn agent_and_terminal_combine() {
        let parser = Parser::new(&Patterns::default()).unwrap();
        let (mut commands, rest) = parser.parse("codex, открой в терминале, проверь diff");
        commands.sort_by_key(|c| *c as u8);
        assert_eq!(commands, [Command::OpenTerminal, Command::Codex]);
        assert_eq!(rest, "проверь diff");
    }

    #[test]
    fn commands_name_their_agent() {
        assert_eq!(Command::Codex.agent(), Some(crate::agent::Agent::Codex));
        assert_eq!(Command::NewSession.agent(), None);
    }
```

- [ ] **Step 2: Run to see them fail** — `cargo test -p erindi-core commands::`.

- [ ] **Step 3: Implement**

```rust
pub enum Command {
    NewSession,
    OpenTerminal,
    /// Only at the end of a phrase: nothing is sent.
    Cancel,
    /// Only at the start: a new session with this agent.
    Claude,
    Codex,
}

impl Command {
    pub fn agent(self) -> Option<crate::agent::Agent> {
        match self {
            Command::Claude => Some(crate::agent::Agent::Claude),
            Command::Codex => Some(crate::agent::Agent::Codex),
            _ => None,
        }
    }
}
```

`Patterns` gains `pub claude: Vec<String>, pub codex: Vec<String>` with defaults:

```rust
            claude: list(&[r"((в|с|через) )?(клод|claude)\w*", r"((in|with) )?claude"]),
            codex: list(&[r"((в|с|через) )?(кодекс|codex)\w*", r"((in|with) )?codex"]),
```

`Parser::new` adds `(Command::Claude, &patterns.claude)` and `(Command::Codex, &patterns.codex)` to `groups`. In `parse`, the end search excludes agents: change `self.at_end(&rest, |c| !cancel(c))` to `self.at_end(&rest, |c| !cancel(c) && c.agent().is_none())`.

In `apps/desktop/src/controls.tsx`: `export type Command = "newSession" | "openTerminal" | "cancel" | "claude" | "codex";`. In `apps/desktop/src/commands.tsx` add to `commands` and `names`:

```ts
  { id: "claude", name: "Claude", example: "claude, check the diff" },
  { id: "codex", name: "Codex", example: "codex, check the diff" },
```

```ts
  claude: "New Claude session",
  codex: "New Codex session",
```

and in the `TryPhrase` summary keep the existing text; the names above make it read "New Codex session · Claude gets: «…»" — change that fragment to `· the agent gets: «${result.rest}»` and "No command; the whole phrase goes to Claude." to "No command; the whole phrase goes to the agent."

- [ ] **Step 4: Run** — `cargo test -p erindi-core commands::` and `pnpm build` in `apps/desktop`; expected PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/core apps/desktop/src
git commit -m "feat: say claude or codex to start a session with that agent"
```

---

### Task 8: Controller picks the agent

**Files:**
- Modify: `crates/core/src/session.rs`, `crates/core/src/controller.rs`

**Interfaces:**
- Consumes: `Agent`, `Command::agent()`.
- Produces: `Active { id, cwd, last_used, agent: Agent }`; `Msg::Settings { …, agent: Agent }`; `Msg::SetActive { id, cwd, agent }`; `Effect::StartRun { op, prompt, session, cwd, agent }`; `Effect::RunInTerminal { session, cwd, prompt, agent }`; `Effect::OpenTerminal { id, cwd, agent }`.

- [ ] **Step 1: Update helpers and write the failing tests**

In `session.rs` tests, `active()` builds `Active { …, agent: Agent::Claude }` (`use crate::agent::Agent;`).

In `controller.rs` tests:
- `settings(policy, cwd)` gains `agent: Agent::Claude`;
- every `Msg::SetActive { id, cwd }` and `Effect::OpenTerminal { id, cwd }` literal gains `agent: Agent::Claude`;
- the `run_in_terminal` helper pattern gains `..` after `prompt`;
- `T::run()` says `"проверь diff"` instead of `"клод, проверь diff"`: the test dictionary turns "клод" into "Claude", which is now a command that forces a new session.

Add:

```rust
    fn with_default(t: &mut T, agent: Agent) {
        t.send(Msg::Settings { agent, ..settings(SessionPolicy::Continue, "C:/p") });
    }

    fn run_agent(fx: &[Effect]) -> Option<(Session, Agent)> {
        fx.iter().find_map(|e| match e {
            Effect::StartRun { session, agent, .. } => Some((*session, *agent)),
            _ => None,
        })
    }

    #[test]
    fn spoken_agent_starts_a_new_session_with_it() {
        let mut t = T::new();
        let first = t.finish_saying("проверь diff");
        let fx = say(&mut t, "codex, напиши тесты");
        let (session, agent) = run_agent(&fx).unwrap();
        assert!(matches!(session, Session::New(new) if new != id(first)));
        assert_eq!(agent, Agent::Codex);
    }

    #[test]
    fn plain_phrase_continues_with_the_session_agent() {
        let mut t = T::new();
        let first = t.finish_saying("codex, напиши тесты");
        let fx = say(&mut t, "а теперь поправь");
        let (session, agent) = run_agent(&fx).unwrap();
        assert_eq!(session, Session::Resume(id(first)));
        assert_eq!(agent, Agent::Codex);
    }

    #[test]
    fn default_agent_starts_new_sessions() {
        let mut t = T::new();
        with_default(&mut t, Agent::Codex);
        let (_, agent) = run_agent(&say(&mut t, "проверь diff")).unwrap();
        assert_eq!(agent, Agent::Codex);
    }

    #[test]
    fn picked_session_keeps_its_agent() {
        let mut t = T::new();
        t.send(Msg::SetActive { id: Uuid::from_u128(9), cwd: "C:/q".into(), agent: Agent::Codex });
        let (session, agent) = run_agent(&say(&mut t, "продолжай")).unwrap();
        assert_eq!(session, Session::Resume(Uuid::from_u128(9)));
        assert_eq!(agent, Agent::Codex);
    }
```

`T::new`, `T::finish_saying` (runs a phrase to a successful end and returns its session), `say` and `id` already exist in the test module.

- [ ] **Step 2: Run to see them fail** — `cargo test -p erindi-core controller::`, compile errors.

- [ ] **Step 3: Implement**

`session.rs`: `pub struct Active { pub id: Uuid, pub cwd: String, pub last_used: Instant, pub agent: Agent }`.

`controller.rs`:
- field `agent: Agent` (default agent) initialised to `Agent::Claude`;
- `Msg::Settings` gains `agent: Agent`, stored in the handler: `self.agent = agent;`;
- `Msg::SetActive { id, cwd, agent }` builds `Active { id, cwd, last_used: now, agent }`;
- `running: Option<(Session, String, Agent)>`; `remember` takes the agent and stores it in `Active`;
- `target(new, spoken, now) -> (Session, String, bool, Agent)`:

```rust
    /// The session a phrase goes to, its folder, whether it continues an earlier one, and its agent.
    /// A spoken agent always starts a new session with that agent.
    fn target(&self, new: bool, spoken: Option<Agent>, now: Instant) -> (Session, String, bool, Agent) {
        let resume = choose(self.policy, self.recent, new || spoken.is_some(), self.active.as_ref(), now);
        match (resume, &self.active) {
            (Some(id), Some(active)) => (Session::Resume(id), active.cwd.clone(), true, active.agent),
            _ => (Session::New(Uuid::new_v4()), self.cwd.clone(), false, spoken.unwrap_or(self.agent)),
        }
    }
```

- `act` computes `let spoken = commands.iter().find_map(|c| c.agent());` and passes it to `target` and `start_run`; `start_run(op, new, spoken, prompt, now)` puts `agent` into `Effect::StartRun` and `self.running`;
- `Effect::RunInTerminal` gets `agent` from `target`; the `Active` built there gets it too;
- `open_terminal` passes `agent: active.agent`.

- [ ] **Step 4: Keep the desktop crate compiling**

In `apps/desktop/src-tauri/src/settings.rs` `session_msg`, add `agent: erindi_core::agent::Agent::Claude,`. In `runtime.rs`, `Msg::SetActive { id, cwd }` becomes `Msg::SetActive { id, cwd, agent: Agent::Claude }`, and the `Effect::OpenTerminal`, `Effect::RunInTerminal` and `Effect::StartRun` patterns in `Executor::execute` end with `..` (the agent is used from Task 11).

- [ ] **Step 5: Run** — `cargo test --workspace`, expected PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/core apps/desktop/src-tauri/src
git commit -m "feat(core): sessions remember their agent; a spoken agent starts a new one"
```

Tasks 9, 10 and 11 change types the desktop runtime uses on every line it touches, so they share one commit at the end of Task 11. Run each task's own tests as you go with `cargo test -p erindi-desktop <module>::` once Task 11 Step 4 compiles; until then, write the tests and code of Tasks 9 and 10 and check them with `cargo check -p erindi-desktop --tests` after Task 11.

---

### Task 9: Settings per agent

**Files:**
- Modify: `apps/desktop/src-tauri/src/settings.rs`

**Interfaces:**
- Consumes: `Agent`, `valid_model`.
- Produces:
  - `#[serde(rename_all = "camelCase")] pub enum ModelChoice { Listed(String), Custom(String) }`
  - `pub struct AgentSettings { pub model: Option<ModelChoice>, pub permission: String }` (`"default"` passes no flag)
  - `Settings { …, agent: Agent, agents: BTreeMap<Agent, AgentSettings> }` without `mode` and `model`
  - `Settings::agent_settings(&self, agent) -> AgentSettings`, `AgentSettings::model_id(&self) -> Option<&str>`, `AgentSettings::permission_flag(&self) -> Option<&str>`

- [ ] **Step 1: Failing tests** in `settings.rs` tests (replace `roundtrip` and `partial_file_keeps_defaults_for_missing_fields`, and the `model:` case in `validation`):

```rust
    #[test]
    fn roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/settings.json");
        let mut settings = Settings { agent: Agent::Codex, ..Settings::default() };
        settings.agents.insert(Agent::Codex, AgentSettings {
            model: Some(ModelChoice::Custom("gpt-5.5".into())),
            permission: "workspace-write".into(),
        });
        settings.save(&path).unwrap();
        assert_eq!(Settings::load(&path), settings);
    }

    #[test]
    fn old_mode_and_model_move_to_claude() {
        let json = r#"{"mode":"plan","model":"opus","cwd":"C:/p"}"#;
        let s = Settings::from_json(json).unwrap();
        assert_eq!(s.agent, Agent::Claude);
        assert_eq!(
            s.agent_settings(Agent::Claude),
            AgentSettings { model: Some(ModelChoice::Listed("opus".into())), permission: "plan".into() }
        );
        let s = Settings::from_json(r#"{"model":"claude-opus-4-8"}"#).unwrap();
        assert_eq!(s.agent_settings(Agent::Claude).model, Some(ModelChoice::Custom("claude-opus-4-8".into())));
        let s = Settings::from_json(r#"{"mode":"default","model":""}"#).unwrap();
        assert_eq!(s.agent_settings(Agent::Claude), AgentSettings::default());
    }

    #[test]
    fn missing_agents_get_defaults() {
        let s = Settings::from_json("{}").unwrap();
        assert_eq!(s.agent_settings(Agent::Codex), AgentSettings::default());
        assert_eq!(s.agent_settings(Agent::Codex).permission_flag(), None);
        assert_eq!(s.agent_settings(Agent::Codex).model_id(), None);
    }

    #[test]
    fn model_ids_are_checked_on_save() {
        let dir = tempfile::tempdir().unwrap();
        let ok = Settings { cwd: dir.path().to_string_lossy().into(), ..Settings::default() };
        let with = |model: ModelChoice, permission: &str| {
            let mut s = ok.clone();
            s.agents.insert(Agent::Claude, AgentSettings { model: Some(model), permission: permission.into() });
            s.validate()
        };
        assert_eq!(with(ModelChoice::Custom(String::new()), "default"), Err("Enter a model ID for Claude".into()));
        assert_eq!(with(ModelChoice::Custom("--x".into()), "default"), Err("Invalid model ID for Claude: --x".into()));
        assert!(with(ModelChoice::Listed("a b".into()), "default").is_err());
        assert!(with(ModelChoice::Custom("claude-opus-4-8".into()), "plan").is_ok());
        assert_eq!(with(ModelChoice::Listed("opus".into()), "workspace-write"), Err("Unknown permission for Claude: workspace-write".into()));
    }
```

- [ ] **Step 2: Run to see them fail** — `cargo test -p erindi-desktop settings::` (the crate name is in `apps/desktop/src-tauri/Cargo.toml`; use it if it differs).

- [ ] **Step 3: Implement**

```rust
use std::collections::BTreeMap;

use erindi_core::agent::{Agent, valid_model};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ModelChoice {
    Listed(String),
    Custom(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AgentSettings {
    /// `None` passes no model flag.
    pub model: Option<ModelChoice>,
    /// `"default"` passes no permission flag.
    pub permission: String,
}

impl Default for AgentSettings {
    fn default() -> Self {
        Self { model: None, permission: "default".into() }
    }
}

impl AgentSettings {
    pub fn model_id(&self) -> Option<&str> {
        match &self.model {
            Some(ModelChoice::Listed(id) | ModelChoice::Custom(id)) => Some(id),
            None => None,
        }
    }

    pub fn permission_flag(&self) -> Option<&str> {
        (self.permission != "default").then_some(self.permission.as_str())
    }
}
```

`Settings` drops `mode` and `model`, adds `pub agent: Agent` and `pub agents: BTreeMap<Agent, AgentSettings>` (both `Default`; `Agent` needs `Ord` — derive `PartialOrd, Ord` on `Agent` in `crates/core/src/agent.rs`). Add:

```rust
    pub fn agent_settings(&self, agent: Agent) -> AgentSettings {
        self.agents.get(&agent).cloned().unwrap_or_default()
    }

    /// Reads a settings file, moving the old Claude-only `mode` and `model` into `agents.claude`.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        let mut value: serde_json::Value = serde_json::from_str(json)?;
        if let Some(obj) = value.as_object_mut() {
            let mode = obj.remove("mode");
            let model = obj.remove("model");
            let has_agents = obj.contains_key("agents");
            if !has_agents && (mode.is_some() || model.is_some()) {
                let aliases = erindi_core::agent::claude_models();
                let model = model
                    .and_then(|m| m.as_str().map(String::from))
                    .filter(|m| !m.is_empty())
                    .map(|m| match aliases.iter().any(|a| a.id == m) {
                        true => ModelChoice::Listed(m),
                        false => ModelChoice::Custom(m),
                    });
                let permission = mode.and_then(|m| m.as_str().map(String::from)).unwrap_or("default".into());
                obj.insert("agents".into(), serde_json::json!({ "claude": AgentSettings { model, permission } }));
            }
        }
        serde_json::from_value(value)
    }
```

`load` uses `from_json` instead of `serde_json::from_str`. `validate` replaces the `ClaudeRequest` block with:

```rust
        for (agent, s) in &self.agents {
            let name = agent.label();
            if let Some(id) = s.model_id() {
                if id.is_empty() {
                    return Err(format!("Enter a model ID for {name}"));
                }
                if !valid_model(id) {
                    return Err(format!("Invalid model ID for {name}: {id}"));
                }
            }
            if s.permission != "default" && !agent.permissions().contains(&s.permission.as_str()) {
                return Err(format!("Unknown permission for {name}: {}", s.permission));
            }
        }
```

and the folder check becomes `erindi_core::agent::resume_in_terminal("claude", &self.cwd, Agent::Claude, &Uuid::nil().to_string()).map_err(|_| "The folder path cannot contain ';' or start with '-'")?;`. `session_msg` adds `agent: self.agent`. Remove the old `claude::` import and the `model: "--dangerously-skip-permissions"` case in `validation` (the new test covers it).

- [ ] **Step 4: Run** — `cargo test -p erindi-desktop settings::` after Task 11 Step 4. No commit here; Task 11 commits Tasks 9–11.

---

### Task 10: History knows the agent

**Files:**
- Modify: `apps/desktop/src-tauri/src/history.rs`

**Interfaces:**
- Produces: `Entry { …, agent: Agent, native_id: Option<String>, started_model: Option<String>, started_permission: Option<String> }`; `History::record(id, cwd, prompt, now_ms, start: &Start)` where `pub struct Start { pub agent: Agent, pub native_id: Option<String>, pub model: Option<String>, pub permission: Option<String> }`; `History::set_native(id, native_id) -> Result<(), String>`; `Entry::native(&self) -> Option<&str>`.

- [ ] **Step 1: Failing tests**:

```rust
    fn claude_start(id: Uuid) -> Start {
        Start { agent: Agent::Claude, native_id: Some(id.to_string()), model: None, permission: None }
    }

    #[test]
    fn entries_without_agent_are_claude() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.json");
        let old = format!(r#"[{{"id":"{}","cwd":"C:/a","prompts":["x"],"createdMs":1,"updatedMs":1}}]"#, id(1));
        std::fs::write(&path, old).unwrap();
        let h = History::load(&path);
        let e = h.get(id(1)).unwrap();
        assert_eq!(e.agent, Agent::Claude);
        assert_eq!(e.native(), Some(id(1).to_string().as_str()));
    }

    #[test]
    fn codex_sessions_get_their_native_id_later() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.json");
        let mut h = History::load(&path);
        let start = Start { agent: Agent::Codex, native_id: None, model: Some("gpt-5.5".into()), permission: Some("read-only".into()) };
        h.record(id(1), "C:/a", plain("fix"), 10, &start).unwrap();
        assert_eq!(h.get(id(1)).unwrap().native(), None);
        h.set_native(id(1), "01a0").unwrap();
        let h = History::load(&path);
        let e = h.get(id(1)).unwrap();
        assert_eq!((e.agent, e.native()), (Agent::Codex, Some("01a0")));
        assert_eq!(e.started_model.as_deref(), Some("gpt-5.5"));
    }

    #[test]
    fn continuing_keeps_the_start_values() {
        let dir = tempfile::tempdir().unwrap();
        let mut h = History::load(&dir.path().join("s.json"));
        let start = Start { agent: Agent::Codex, native_id: Some("n".into()), model: Some("a".into()), permission: None };
        h.record(id(1), "C:/a", plain("one"), 1, &start).unwrap();
        let later = Start { agent: Agent::Codex, native_id: Some("n".into()), model: None, permission: None };
        h.record(id(1), "C:/a", plain("two"), 2, &later).unwrap();
        assert_eq!(h.get(id(1)).unwrap().started_model.as_deref(), Some("a"));
    }
```

and every existing `h.record(id(n), cwd, prompt, ms)` call in the tests gains `&claude_start(id(n))`.

- [ ] **Step 2: Run to see them fail.**

- [ ] **Step 3: Implement**

```rust
/// A session started from Erindi, with everything the user said in it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub id: Uuid,
    pub cwd: String,
    pub prompts: Vec<Prompt>,
    pub created_ms: u64,
    pub updated_ms: u64,
    #[serde(default)]
    pub agent: Agent,
    /// The agent's own session ID; `None` for a Codex run that never reported one.
    #[serde(default)]
    pub native_id: Option<String>,
    #[serde(default)]
    pub started_model: Option<String>,
    #[serde(default)]
    pub started_permission: Option<String>,
}

impl Entry {
    pub fn native(&self) -> Option<&str> {
        self.native_id.as_deref()
    }
}
```

In `History::load`, after parsing, give Claude entries from before agents existed their Erindi ID as the native ID:

```rust
        let mut entries: Vec<Entry> = /* the existing parsing */;
        for e in &mut entries {
            if e.agent == Agent::Claude && e.native_id.is_none() {
                e.native_id = Some(e.id.to_string());
            }
        }
```

`Start` and `record`:

```rust
pub struct Start {
    pub agent: Agent,
    pub native_id: Option<String>,
    pub model: Option<String>,
    pub permission: Option<String>,
}
```

`record` takes `start: &Start`; a new entry copies all four fields; an existing entry keeps its own. `set_native`:

```rust
    pub fn set_native(&mut self, id: Uuid, native_id: &str) -> Result<(), String> {
        if let Some(e) = self.entries.iter_mut().find(|e| e.id == id) {
            e.native_id = Some(native_id.to_string());
        }
        self.save()
    }
```

- [ ] **Step 4: Run** — `cargo test -p erindi-desktop history::` after Task 11 Step 4. No commit here; Task 11 commits Tasks 9–11.

---

### Task 11: Runtime, agent state and commands

**Files:**
- Create: `apps/desktop/src-tauri/src/agents.rs`
- Modify: `apps/desktop/src-tauri/src/runtime.rs`, `apps/desktop/src-tauri/src/lib.rs`, `apps/desktop/src-tauri/build.rs`, `apps/desktop/src-tauri/capabilities/settings.json`

**Interfaces:**
- Consumes: everything above.
- Produces:
  - `agents::Agents` (cloneable, `Arc<Mutex<…>>` inside) with `fn status(&self) -> Vec<AgentStatus>`, `fn recheck(&self, app: &AppHandle)`, `fn locate(&self, agent, app) -> Option<PathBuf>`
  - `#[derive(Serialize, Clone, PartialEq)] #[serde(rename_all = "camelCase")] pub struct AgentStatus { agent: Agent, label: &'static str, path: Option<String>, models: Vec<ModelOption>, models_error: Option<String>, permissions: Vec<&'static str> }`
  - Tauri commands `agent_status() -> Vec<AgentStatus>` and `recheck_agents()`; event `agents-changed` with `Vec<AgentStatus>`
  - `list_sessions` returns `{ entries, active, details: HashMap<Uuid, Details> }`

- [ ] **Step 1: Failing tests**

In `lib.rs` tests, extend the capability test list with `"allow-agent-status"` and `"allow-recheck-agents"`.

In `agents.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_cli_message_names_the_agent() {
        assert_eq!(
            missing(Agent::Codex),
            "Codex CLI not found. Install it, then press Re-check in Settings."
        );
    }

    #[test]
    fn status_reports_a_model_list_error() {
        let s = status_of(Agent::Codex, Some("C:/codex.cmd".into()), Err("not JSON: x".into()));
        assert_eq!(s.models, vec![]);
        assert_eq!(s.models_error.as_deref(), Some("Couldn't read Codex models: not JSON: x"));
        let s = status_of(Agent::Claude, None, Ok(vec![]));
        assert_eq!(s.path, None);
        assert_eq!(s.models_error, None);
    }
}
```

In `runtime.rs`:

```rust
    #[test]
    fn codex_session_without_native_id_is_forgotten() {
        assert!(forget_after_run(Agent::Codex, true, false));
        assert!(!forget_after_run(Agent::Codex, true, true));
        assert!(!forget_after_run(Agent::Claude, true, false));
        assert!(!forget_after_run(Agent::Codex, false, false));
    }
```

- [ ] **Step 2: Run to see them fail** — `cargo test -p erindi-desktop`.

- [ ] **Step 3: Implement `agents.rs`**

```rust
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use erindi_core::agent::{Agent, ModelOption, claude_models};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

/// How old a check may be before the Settings window re-checks on focus.
pub const STALE: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentStatus {
    pub agent: Agent,
    pub label: &'static str,
    pub path: Option<String>,
    pub models: Vec<ModelOption>,
    pub models_error: Option<String>,
    pub permissions: Vec<&'static str>,
}

#[derive(Clone, Default)]
pub struct Agents(Arc<Mutex<Option<(Instant, Vec<AgentStatus>)>>>);

pub fn missing(agent: Agent) -> String {
    format!("{} CLI not found. Install it, then press Re-check in Settings.", agent.label())
}

fn status_of(agent: Agent, path: Option<String>, models: Result<Vec<ModelOption>, String>) -> AgentStatus {
    let (models, models_error) = match models {
        Ok(m) => (m, None),
        Err(e) => (vec![], Some(format!("Couldn't read {} models: {e}", agent.label()))),
    };
    AgentStatus {
        agent,
        label: agent.label(),
        path,
        models,
        models_error,
        permissions: agent.permissions().to_vec(),
    }
}

fn check(agent: Agent) -> AgentStatus {
    let path = erindi_core::cli::locate(agent);
    let models = match (agent, &path) {
        (Agent::Claude, _) => Ok(claude_models()),
        (Agent::Codex, None) => Ok(vec![]),
        (Agent::Codex, Some(program)) => codex_models(program),
    };
    status_of(agent, path.map(|p| p.display().to_string()), models)
}

fn codex_models(program: &std::path::Path) -> Result<Vec<ModelOption>, String> {
    let mut command = std::process::Command::new(program);
    command.args(["debug", "models"]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let out = command.output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).lines().last().unwrap_or("failed").to_string());
    }
    erindi_core::codex::parse_models(&String::from_utf8_lossy(&out.stdout))
}

impl Agents {
    pub fn status(&self) -> Vec<AgentStatus> {
        self.0.lock().unwrap().as_ref().map(|(_, s)| s.clone()).unwrap_or_default()
    }

    /// Checks every agent off the calling thread and emits `agents-changed` when anything changed.
    pub fn recheck(&self, app: &AppHandle) {
        let (this, app) = (self.clone(), app.clone());
        std::thread::spawn(move || {
            let fresh: Vec<_> = Agent::ALL.into_iter().map(check).collect();
            let changed = this.status() != fresh;
            *this.0.lock().unwrap() = Some((Instant::now(), fresh.clone()));
            if changed {
                let _ = app.emit_to("settings", "agents-changed", fresh);
            }
        });
    }

    pub fn recheck_if_stale(&self, app: &AppHandle) {
        let stale = self.0.lock().unwrap().as_ref().is_none_or(|(at, _)| at.elapsed() > STALE);
        if stale {
            self.recheck(app);
        }
    }

    /// Finds the CLI now and updates the stored path when it moved or appeared.
    pub fn locate(&self, agent: Agent, app: &AppHandle) -> Option<PathBuf> {
        let path = erindi_core::cli::locate(agent);
        let known = self.status().into_iter().find(|s| s.agent == agent).and_then(|s| s.path);
        if known != path.as_ref().map(|p| p.display().to_string()) {
            self.recheck(app);
        }
        path
    }
}
```

- [ ] **Step 4: Rework `runtime.rs`**

- `Runtime` and `Executor` hold `agents: Agents`; `Runtime::start` takes it from `lib.rs`.
- Replace imports: `use erindi_core::agent::{self, Agent, AgentRequest, EventParser, Target};` and remove `claude::…` and `stream::parse_line`.
- `start_run(op, prompt, session, cwd, agent)`:

```rust
    fn start_run(&mut self, op: OpId, prompt: String, session: Session, cwd: String, agent: Agent) {
        let fail = |tx: &Sender<Msg>, stderr: String| {
            let _ = tx.send(Msg::RunExited { op, end: RunEnd::Exited { success: false }, stderr });
        };
        let Some(program) = self.agents.locate(agent, &self.app) else {
            return fail(&self.tx, crate::agents::missing(agent));
        };
        let settings = self.settings.read().unwrap().agent_settings(agent);
        let (target, start) = match session {
            Session::New(id) => (
                Target::New(id),
                Start {
                    agent,
                    native_id: (agent == Agent::Claude).then(|| id.to_string()),
                    model: settings.model_id().map(String::from),
                    permission: settings.permission_flag().map(String::from),
                },
            ),
            Session::Resume(id) => {
                let native = self.history.lock().unwrap().get(id).and_then(|e| e.native_id.clone());
                let Some(native) = native else {
                    return fail(&self.tx, "This session can't be resumed".into());
                };
                (Target::Resume(native.clone()), Start { agent, native_id: Some(native), model: None, permission: None })
            }
        };
        // A resumed session's `start` carries no model or permission, so none are passed.
        let request = AgentRequest {
            agent,
            model: start.model.clone(),
            permission: start.permission.clone(),
            target,
        };
        let args = match agent::headless_args(&request, &cwd) {
            Ok(args) => args,
            Err(e) => return fail(&self.tx, format!("Invalid {} settings: {e:?}", agent.label())),
        };
        let spec = RunSpec {
            program,
            args,
            cwd: cwd.clone().into(),
            env: agent::env(agent, std::env::vars().chain([("PATH".into(), erindi_core::cli::current_path())])),
            stdin: prompt.clone(),
            timeout: RUN_TIMEOUT,
        };
        let id = match session { Session::New(id) | Session::Resume(id) => id };
        *self.last_session.lock().unwrap() = Some(LastRun { op, id, cwd: cwd.clone() });
        self.remember_prompt(session, &cwd, prompt, &start);
        let token = CancellationToken::new();
        self.cancel = Some(token.clone());
        let (tx, history, app) = (self.tx.clone(), self.history.clone(), self.app.clone());
        let new = matches!(session, Session::New(_));
        tauri::async_runtime::spawn(async move {
            let mut parser = EventParser::new(agent);
            let mut native_seen = false;
            let outcome = run(spec, token, |line| {
                for event in parser.feed(line) {
                    if let RunEvent::SessionStarted { native_id } = &event {
                        native_seen = true;
                        if let Err(e) = history.lock().unwrap().set_native(id, native_id) {
                            eprintln!("cannot save session history: {e}");
                        }
                        let _ = app.emit_to("settings", "sessions-changed", ());
                    }
                    let _ = tx.send(Msg::Run { op, event });
                }
            })
            .await;
            let msg = match outcome {
                Ok(outcome) => Msg::RunExited { op, end: outcome.end, stderr: outcome.stderr_tail },
                Err(e) => Msg::RunExited {
                    op,
                    end: RunEnd::Exited { success: false },
                    stderr: format!("Cannot start {}: {e}", agent.cli()),
                },
            };
            let _ = tx.send(msg);
            if forget_after_run(agent, new, native_seen) {
                let _ = tx.send(Msg::Forget { id });
            }
        });
    }
```

`std::env::vars().chain(…)` puts the fresh PATH last; `agent::env` keeps both, and the child sees the last one because `Command::envs` applies them in order. Add:

```rust
/// A new Codex session that never reported its ID cannot be continued, so it must not stay active.
fn forget_after_run(agent: Agent, new: bool, native_seen: bool) -> bool {
    agent == Agent::Codex && new && !native_seen
}
```

- `remember_prompt(session, cwd, prompt, start: &Start)` passes `start` to `History::record`.
- `run_in_terminal(session, cwd, prompt, agent)`: locate the program (missing → `eprintln!` of `missing(agent)` and `Msg::Forget` for a new session, as today on failure); build `AgentRequest` like `start_run` (a resumed session uses its native ID from history); call `agent::terminal_args(&program.display().to_string(), &cwd, &request, &prompt)`; the rest stays.
- `open_terminal(cwd, id)` becomes a method that reads the entry's agent and native ID from history and calls `agent::resume_in_terminal(program, cwd, entry.agent, native)`; `Effect::OpenTerminal { id, cwd, agent }` and `open_history_session` and `open_session` use it. A missing native ID returns `Err("This session can't be resumed")`.
- `continue_session(id)` returns `Err("This session can't be resumed")` when `native_id` is `None`, else sends `Msg::SetActive { id, cwd, agent: entry.agent }`.
- `Effect::StartRun { op, prompt, session, cwd, agent }` and `Effect::RunInTerminal { session, cwd, prompt, agent }` pass `agent` through.

- [ ] **Step 5: Commands and capabilities**

`lib.rs`: `mod agents;`; in `setup` create `let agents = agents::Agents::default(); agents.recheck(app.handle()); app.manage(agents.clone());` and pass `agents` into `Runtime::start`. New commands:

```rust
#[tauri::command]
fn agent_status(agents: tauri::State<agents::Agents>) -> Vec<agents::AgentStatus> {
    agents.status()
}

/// `force` re-checks now; otherwise only when the last check is stale (window focus).
#[tauri::command]
fn recheck_agents(app: AppHandle, agents: tauri::State<agents::Agents>, force: bool) {
    if force { agents.recheck(&app) } else { agents.recheck_if_stale(&app) }
}
```

`Sessions` gains `details: std::collections::HashMap<uuid::Uuid, erindi_core::transcript::Details>`, filled in `list_sessions`:

```rust
    let home = std::env::var_os("USERPROFILE").map(PathBuf::from).unwrap_or_default();
    let logs: HashMap<_, _> = Agent::ALL.into_iter().map(|a| (a, transcript::find_logs(a, &home))).collect();
    let details = entries
        .iter()
        .filter_map(|e| {
            let path = logs[&e.agent].get(e.native_id.as_deref()?)?;
            Some((e.id, transcript::read(e.agent, path)?))
        })
        .collect();
```

Register `agent_status` and `recheck_agents` in `generate_handler!`, in `build.rs` `commands(&[…])`, and add `"allow-agent-status"` and `"allow-recheck-agents"` to `capabilities/settings.json`. Building once regenerates `permissions/autogenerated/*.toml`; commit those files.

`save_settings` also calls `app.state::<agents::Agents>().recheck(&app)` so a changed default agent shows its status at once.

- [ ] **Step 6: Run everything**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: PASS.

- [ ] **Step 7: Commit Tasks 9–11**

```bash
git add apps/desktop/src-tauri crates/core/src/agent.rs
git commit -m "feat(desktop): per-agent settings and history, and runs through the session's agent"
```

---

### Task 12: Settings, Commands and Sessions UI

**Files:**
- Create: `apps/desktop/src/icons/claude.svg`, `apps/desktop/src/icons/openai.svg`, `apps/desktop/src/agents.tsx`
- Modify: `apps/desktop/src/controls.tsx`, `apps/desktop/src/settings.tsx`, `apps/desktop/src/sessions.tsx`

**Interfaces:**
- Consumes: `agent_status`, `recheck_agents`, `agents-changed`, `list_sessions` with `details`.
- Produces: `AgentIcon`, `AgentFields`, `useAgents()` in `agents.tsx`.

- [ ] **Step 1: Icons**

```bash
curl -sL https://cdn.jsdelivr.net/npm/simple-icons@latest/icons/claude.svg -o apps/desktop/src/icons/claude.svg
curl -sL https://cdn.jsdelivr.net/npm/simple-icons@latest/icons/openai.svg -o apps/desktop/src/icons/openai.svg
```

Check both files start with `<svg`; if `claude.svg` is missing from the package, use `anthropic.svg`.

- [ ] **Step 2: Types** in `controls.tsx` — replace `Mode`, `modes`, `mode` and `model`:

```ts
export type Agent = "claude" | "codex";

export type ModelChoice = { listed: string } | { custom: string } | null;

export type AgentSettings = { model: ModelChoice; permission: string };

export type Settings = {
  talkHotkey: string;
  newSessionHotkey: string;
  terminalHotkey: string;
  patterns: Patterns;
  sessionPolicy: SessionPolicy;
  recentMinutes: number;
  cwd: string;
  agent: Agent;
  agents: Partial<Record<Agent, AgentSettings>>;
  microphone: string;
  silenceSecs: number;
  dictionary: [string, string][];
  modelCommands: boolean;
};

export type AgentStatus = {
  agent: Agent;
  label: string;
  path: string | null;
  models: { id: string; label: string }[];
  modelsError: string | null;
  permissions: string[];
};

export const unsafePermissions = ["bypassPermissions", "danger-full-access"];
```

- [ ] **Step 3: `agents.tsx`**

```tsx
import { useEffect, useState } from "preact/hooks";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { type Agent, type AgentSettings, type AgentStatus, Field, input, unsafePermissions } from "./controls";
import claudeIcon from "./icons/claude.svg";
import openaiIcon from "./icons/openai.svg";

export function AgentIcon(props: { agent: Agent; class?: string }) {
  const src = props.agent === "claude" ? claudeIcon : openaiIcon;
  return <img src={src} alt="" class={`dark:invert ${props.class ?? "h-4 w-4"}`} />;
}

/** Agent state from Rust, refreshed on `agents-changed` and re-checked when the window gains focus. */
export function useAgents() {
  const [agents, setAgents] = useState<AgentStatus[]>([]);
  useEffect(() => {
    invoke<AgentStatus[]>("agent_status").then(setAgents);
    const off = listen<AgentStatus[]>("agents-changed", (e) => setAgents(e.payload));
    const onFocus = () => invoke("recheck_agents", { force: false });
    window.addEventListener("focus", onFocus);
    onFocus();
    return () => {
      off.then((f) => f());
      window.removeEventListener("focus", onFocus);
    };
  }, []);
  return { agents, recheck: () => invoke("recheck_agents", { force: true }) };
}

const CUSTOM = "\u0000custom";
const DEFAULT = "";

export function AgentFields(props: {
  status: AgentStatus;
  value: AgentSettings;
  onChange: (v: AgentSettings) => void;
}) {
  const { status, value } = props;
  const model = value.model;
  const selected = model === null ? DEFAULT : "custom" in model ? CUSTOM : model.listed;
  const pick = (id: string) =>
    props.onChange({
      ...value,
      model: id === DEFAULT ? null : id === CUSTOM ? { custom: "" } : { listed: id },
    });
  return (
    <div class="space-y-3">
      <div class="grid grid-cols-2 gap-3">
        <Field label="Model" hint={status.modelsError ?? undefined}>
          <select class={input} value={selected} onChange={(e) => pick(e.currentTarget.value)}>
            <option value={DEFAULT}>{status.agent === "codex" ? "Default (config.toml)" : "Default"}</option>
            {status.models.map((m) => (
              <option value={m.id}>{m.label}</option>
            ))}
            <option value={CUSTOM}>Custom model ID…</option>
          </select>
        </Field>
        <Field
          label="Permission"
          hint={unsafePermissions.includes(value.permission) ? "The agent can change anything on this computer." : undefined}
        >
          <select
            class={input}
            value={value.permission}
            onChange={(e) => props.onChange({ ...value, permission: e.currentTarget.value })}
          >
            <option value="default">Default ({status.label} settings)</option>
            {status.permissions.map((p) => (
              <option value={p}>{p}</option>
            ))}
          </select>
        </Field>
      </div>
      {model !== null && "custom" in model && (
        <Field label="Model ID">
          <input
            class={input}
            value={model.custom}
            placeholder={status.agent === "codex" ? "gpt-5.5" : "claude-opus-4-8"}
            onInput={(e) => props.onChange({ ...value, model: { custom: e.currentTarget.value } })}
          />
        </Field>
      )}
    </div>
  );
}
```

`vite` resolves `.svg` imports to URLs the same way `logo.svg` is imported in `settings.tsx`.

- [ ] **Step 4: Agent section in `settings.tsx`**

Replace the "Permission mode" and "Model" fields in the Agent section with:

```tsx
        <div class="flex items-end gap-3">
          <Field label="Agent" hint="New sessions use it. Say “claude” or “codex” to pick one for a new session.">
            <select class={input} value={s.agent} onChange={(e) => set({ agent: e.currentTarget.value as Agent })}>
              {agents.map((a) => (
                <option value={a.agent}>{a.label}</option>
              ))}
            </select>
          </Field>
          <button type="button" class="shrink-0 rounded-md border border-neutral-300 px-3 py-1.5 hover:bg-neutral-100 dark:border-neutral-700 dark:hover:bg-neutral-800" onClick={recheck}>
            Re-check
          </button>
        </div>
        {status && !status.path && (
          <p class="text-xs text-red-600">{status.label} CLI not found. Install it, then press Re-check.</p>
        )}
        {status && (
          <AgentFields
            status={status}
            value={s.agents[s.agent] ?? { model: null, permission: "default" }}
            onChange={(v) => set({ agents: { ...s.agents, [s.agent]: v } })}
          />
        )}
```

with `const { agents, recheck } = useAgents();` and `const status = agents.find((a) => a.agent === s.agent);` at the top of `SettingsView`, and imports of `type Agent`, `useAgents`, `AgentFields`. Remove `type Mode`, `modes` imports. The Agent section description becomes "Which agent runs your requests and how."

- [ ] **Step 5: Sessions rows** in `sessions.tsx`

`Entry` gains `agent: Agent; nativeId: string | null; startedModel: string | null; startedPermission: string | null;`, `Sessions` gains `details: Record<string, { model: string | null; permission: string | null }>`. Reload on window focus: add `window.addEventListener("focus", load)` next to the `sessions-changed` listener and remove it in the cleanup.

Call `const { agents } = useAgents();` at the top of `SessionsView` so Codex models show their catalog names ("GPT-5.6-Sol"). Add a model label helper matching `transcript::model_label` for the rest:

```ts
const modelLabel = (raw: string) => {
  if (!raw.startsWith("claude-")) return raw;
  const parts = raw.slice(7).split("-").filter((p) => !/^\d{8}$/.test(p));
  const words = parts.filter((p) => !/^\d+$/.test(p)).map((p) => p[0].toUpperCase() + p.slice(1));
  const version = parts.filter((p) => /^\d+$/.test(p)).join(".");
  return ["Claude", ...words, version].filter(Boolean).join(" ");
};
```

In each row, before the prompt text:

```tsx
              {(() => {
                const live = data.details[entry.id];
                const model = live?.model ?? entry.startedModel;
                const listed = agents
                  .find((a) => a.agent === entry.agent)
                  ?.models.find((m) => m.id === model)?.label;
                const permission = live?.permission ?? entry.startedPermission ?? "default";
                const resumable = entry.nativeId !== null;
                return (
                  <p class="mb-1 flex items-center gap-1.5 text-xs text-neutral-600 dark:text-neutral-400">
                    <AgentIcon agent={entry.agent} />
                    {[model ? listed ?? modelLabel(model) : "Default model", permission].join(" · ")}
                    {!live && <span class="text-neutral-400">(at start)</span>}
                    {!resumable && <span class="text-red-600">· can't resume</span>}
                  </p>
                );
              })()}
```

Disable "Open in terminal" and "Continue by voice" when `entry.nativeId === null`.

- [ ] **Step 6: Build and check**

Run: `pnpm --dir apps/desktop build && pnpm --dir apps/desktop test`
Expected: both pass.

- [ ] **Step 7: Commit**

```bash
git add apps/desktop/src
git commit -m "feat(desktop): agent picker with model lists, and agent details on the Sessions tab"
```

---

### Task 13: Smoke example, docs and the manual check

**Files:**
- Create: `crates/core/examples/codex-smoke.rs`
- Modify: `README.md`, `CONTRIBUTING.md`, `ROADMAP.md`

- [ ] **Step 1: Smoke example** `crates/core/examples/codex-smoke.rs`, modelled on `claude-smoke.rs`:

```rust
//! Manual check against the real CLI: `cargo run --example codex-smoke -- <cwd> <prompt>`.
use std::time::Duration;

use erindi_core::agent::{self, Agent, AgentRequest, EventParser, Target};
use erindi_core::run::{RunSpec, run};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let cwd = args.next().expect("usage: codex-smoke <cwd> <prompt>");
    let prompt = args.collect::<Vec<_>>().join(" ");
    let program = erindi_core::cli::locate(Agent::Codex).expect("codex not found on PATH");
    let req = AgentRequest { agent: Agent::Codex, model: None, permission: Some("read-only".into()), target: Target::New(Uuid::new_v4()) };
    let spec = RunSpec {
        program,
        args: agent::headless_args(&req, &cwd).unwrap(),
        cwd: cwd.into(),
        env: agent::env(Agent::Codex, std::env::vars()),
        stdin: prompt,
        timeout: Duration::from_secs(300),
    };
    let mut parser = EventParser::new(Agent::Codex);
    let outcome = run(spec, CancellationToken::new(), |line| {
        for event in parser.feed(line) {
            println!("{event:?}");
        }
    })
    .await
    .unwrap();
    println!("{:?}\n{}", outcome.end, outcome.stderr_tail);
}
```

Check `claude-smoke.rs` for the runtime attribute it uses (`#[tokio::main]` needs the `macros` and `rt` features in `[dev-dependencies]`); copy its setup if it differs.

Run: `cargo run -p erindi-core --example codex-smoke -- . "Reply with the single word: pong"`
Expected: `SessionStarted`, then `Result { ok: true, text: "pong" }`, then `Exited { success: true }`.

- [ ] **Step 2: Docs**

- `README.md`: the note becomes "Erindi is a proof of concept: Windows only, Claude Code and Codex."; the Features list gains "**Claude Code or Codex.** Pick the default agent in Settings, or say “claude” or “codex” to start a session with one."; "Agent run and cancel" row becomes "`claude -p` stream-json and `codex exec --json`, Windows Job Objects".
- `CONTRIBUTING.md`: next to the `claude-smoke` line add "`cargo run -p erindi-core --example codex-smoke -- <folder> <prompt>` does the same for Codex."
- `ROADMAP.md`: under "Working with agents" add `- [x] **Codex.** Codex runs next to Claude Code: a default agent in Settings, a spoken agent name for a new session, and the agent, model and permission of each session on the Sessions tab.`; in "From the original plan" change the adapters line to "Adapters for Pi and Gemini, then Copilot, Qwen and Kimi."

- [ ] **Step 3: Commit**

```bash
git add crates/core/examples README.md CONTRIBUTING.md ROADMAP.md
git commit -m "docs: Codex in the README, contributing guide and roadmap"
```

- [ ] **Step 4: Manual check with the user**

Build `pnpm --dir apps/desktop tauri build --no-bundle`, start `target/release/erindi-desktop.exe`, and hand over this list:

1. Settings → Agent: Claude and Codex, Codex shows its model list; Custom model ID with an empty field refuses to save.
2. "codex, create a file hello.txt" → a Codex run in the overlay, a row with the OpenAI icon on the Sessions tab.
3. "now delete it" → continues the Codex session.
4. Open in terminal → `codex resume <id>`; change the sandbox inside Codex; back in Erindi, the Sessions row shows the new permission after the window gains focus.
5. Rename `codex.cmd` away → the Settings hint appears within 30 s of focus or at once on Re-check; put it back → Re-check clears it, no restart.
6. "claude, …" still works, and an old Claude session from before this change still continues.

Then open the pull request with `superpowers:finishing-a-development-branch`.
