# Codex as a second agent

Date: 2026-09-24. Branch: `feat/codex-agent`.

## Goal

Erindi runs Codex the way it runs Claude Code: speak a task, the agent runs it in the background, the session continues by voice and opens in a terminal. The agent code gets one shape so that Pi joins later as one more module, without reworking the controller, settings or history.

Order of work: Codex now; the user checks it by hand; then Pi; then LM Studio and OpenAI-compatible servers. Only the first step is in this spec.

## Decisions

| Topic | Decision |
|-------|----------|
| Agents | Claude Code and Codex, each through its own installed CLI. Windows only. |
| Code shape | `enum Agent { Claude, Codex }` in `erindi-core`, one module per agent, dispatch by `match`. A plugin system is deferred (`ROADMAP.md`). |
| Codex interface | `codex exec --json` per utterance, `codex exec resume <id>` to continue. Codex app-server and the Agent Client Protocol are deferred (`ROADMAP.md`). |
| Default agent | Chosen in Settings. New sessions without a spoken agent name use it. |
| Spoken agent name | "claude" or "codex" at the start of a phrase always starts a new session with that agent, for that phrase only. The default agent does not change. |
| Continuing | A session always continues with the agent that started it, with no model or permission flags, so the agent keeps its prompt cache and any permissions the user changed inside the session. |
| Settings per agent | Model and permission are stored per agent. Switching the default agent and back restores what the user chose. |
| Defaults | Default model and default permission pass no flag; the agent uses the user's own configuration. |
| Model choice | A dropdown. Codex lists the models from `codex debug models`. Claude lists the aliases Claude Code accepts. Every agent also offers "Custom model ID". |
| Session details | The Sessions tab shows the agent icon, and the model and permission the session has now, read from the agent's own session log. |
| Missing CLI | Erindi says so and asks the user to install it and press Re-check. No restart is needed. |
| UI state | Rust owns agent state and history; the UI renders events. |

## Architecture

### `erindi-core`

- `agent.rs` (new): `Agent`, `AgentRequest { agent, model, permission, session }`, and dispatch functions: `headless_args`, `parse_line`, `terminal_args`, `resume_in_terminal`, `permissions`, `env`.
- `claude.rs`: today's `claude.rs` and `stream.rs`, renamed to fit the dispatch. Argument building and event parsing stay the same, and so do their tests.
- `codex.rs` (new): arguments, `--json` parser, terminal arguments, permission list, env allowlist, model catalog parser.
- `transcript.rs` (new): reads the newest entry of an agent's session log and returns the current model and permission, or `None`.
- `RunEvent` gains `SessionStarted { native_id }`. Claude never sends it, because Erindi picks Claude's session ID in advance.

### Session IDs

Erindi keeps its own UUID per session, as today, and stores the agent and the agent's native ID next to it.

| Agent | Native ID |
|-------|-----------|
| Claude | Equal to Erindi's UUID, passed with `--session-id`. |
| Codex | `thread_id` from the first `thread.started` event. |

The controller keeps working with Erindi's UUID. Continuing a session runs its agent with the native ID.

### Controller

- The active session remembers its agent.
- A new voice command per agent ("claude", "codex") with editable patterns on the Commands tab, matched only at the start of a phrase. It starts a new session with that agent. Only the patterns recognise it; the local command model keeps its current three commands.
- Without it: continue the active session with its agent; with no active session, start one with the default agent.

### Desktop

- `runtime.rs` builds an `AgentRequest` instead of a `ClaudeRequest`. On `SessionStarted` it writes the native ID into history.
- Agent state in Rust: per agent, whether the CLI is found, its path, the model list and the last error. An `agents-changed` event reaches the window on every change.

## Settings

```json
{
  "agent": "claude",
  "agents": {
    "claude": { "model": null, "permission": "default" },
    "codex":  { "model": { "listed": "gpt-5.6-sol" }, "permission": "workspace-write" }
  }
}
```

- `model` is `null` (Default), `{ "listed": "<id>" }` or `{ "custom": "<id>" }`.
- On first read, the old `mode` and `model` fields move into `agents.claude`.

### Model dropdown

| Agent | Entries |
|-------|---------|
| Claude | Default, Fable, Opus, Sonnet, Haiku (aliases passed as `--model fable` and so on), Custom model ID |
| Codex | Default (config.toml), every model with `visibility: "list"` from `codex debug models` by `display_name`, Custom model ID |

- The Codex list is read when Settings opens, on Re-check, and cached while Erindi runs.
- If the list cannot be read, Settings says "Couldn't read Codex models: <reason>" and offers Default and Custom model ID.
- "Custom model ID" shows a text field. Saving fails with "Enter a model ID" when it is empty, and with "Invalid model ID" when it starts with `-` or contains whitespace. Rust checks both on save.

Exact Claude versions in the list need the Anthropic Models API and an API key; they are out of scope. Custom model ID covers them.

### Permissions

| Agent | Entries | Flag |
|-------|---------|------|
| Claude | Default, `acceptEdits`, `auto`, `plan`, `dontAsk`, `bypassPermissions` | `--permission-mode <value>` |
| Codex | Default, `read-only`, `workspace-write`, `danger-full-access` | `-s <value>` |

`bypassPermissions` and `danger-full-access` show a warning under the field.

### Agent section in Settings

1. Agent: Claude or Codex, with its icon.
2. Model and Permission of the selected agent.
3. A Re-check button. It searches for the CLI and reloads the model list.
4. When the CLI is missing: "Codex CLI not found. Install it, then press Re-check." Saving still works.

## Run flow

1. The controller picks the session and its agent (see Controller).
2. The runtime builds the request. A new session takes model and permission from the agent's settings; a continued session passes neither. For Claude this is a change: today a continued run repeats `--model` and `--permission-mode`.
3. The runtime starts the process:

| Agent | New session | Continue |
|-------|-------------|----------|
| Claude | `claude -p --output-format stream-json --verbose --session-id <uuid> [--model] [--permission-mode]` | `claude -p … --resume <id>` |
| Codex | `codex exec --json -C <cwd> [-m] [-s]` | `codex exec resume <id> --json`, started in the session folder |

   The prompt goes through stdin, never as an argument. Codex reads a piped prompt when no prompt argument is given.

4. Codex events map to `RunEvent`:

| Codex event | `RunEvent` |
|-------------|------------|
| `thread.started` | `SessionStarted { native_id: thread_id }` |
| `item.started` with a command or file change | `ToolUse` |
| `item.completed` with `agent_message` | kept as the latest reply |
| `turn.completed` | `Result { ok: true, text: latest reply }` |
| `turn.failed`, `error` | `Result { ok: false, text: message }` |

   Codex writes MCP and other diagnostics to stderr; they do not affect the result.

5. Cancel kills the process tree through the Windows Job Object, as today.

## History and Sessions tab

Each history entry gains:

```json
{ "agent": "codex", "nativeId": "01a0d2c0-…", "startedModel": "gpt-5.6-sol", "startedPermission": "workspace-write" }
```

Entries without `agent` are Claude entries whose native ID equals the Erindi UUID.

Each row on the Sessions tab shows:

- the agent icon;
- the current model and permission from the agent's log, such as "GPT-5.6-Sol · workspace-write", with a readable model name such as "Claude Opus 5.5" for `claude-opus-5-5`;
- the values from history marked "at start" when the log gives nothing.

| Agent | Log | Model | Permission |
|-------|-----|-------|------------|
| Claude | `~/.claude/projects/<folder>/<id>.jsonl` | `message.model` of the newest assistant entry | `permissionMode` of the newest entry |
| Codex | `~/.codex/sessions/**/rollout-*<id>.jsonl` | `model` of the newest `turn_context` | `sandbox_policy.type` of the newest `turn_context` |

Logs are read when the tab opens and when the window gains focus. The formats are undocumented; any read failure falls back to "at start" and writes one log line.

Open in terminal runs `claude --resume <id>` or `codex resume <id>` in Windows Terminal. A new terminal session with a prompt runs `claude -- <prompt>` or `codex -- <prompt>`.

Agent icons are SVG files in `apps/desktop/src/icons/`, used as labels for the agents.

## Keeping the UI current

- The CLI is searched on every agent run, on Re-check, and when the Settings window gains focus if the last check is older than 30 seconds.
- The search reads the current PATH from the registry (`HKCU\Environment` and `HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\Environment`), because the process PATH is fixed when Erindi starts. A CLI installed while Erindi runs is found without a restart.
- Any change in agent state emits `agents-changed`; any change in history emits the existing history event. Settings and Sessions re-render from them.

## Errors

| Case | User sees | Erindi does |
|------|-----------|-------------|
| CLI missing at run time | Bubble: "Codex CLI not found. Install it, then press Re-check in Settings." | No session is created and nothing is added to history. |
| CLI missing in Settings | The hint under Agent | Saving still works. |
| `codex debug models` fails | "Couldn't read Codex models: <reason>" | Default and Custom model ID stay available. |
| No `thread.started` before the run ends | The result as usual | The session is stored without a native ID, marked "can't resume", and the next phrase starts a new session. |
| `turn.failed` or a non-zero exit | Error bubble with the agent's message | As for Claude today. |
| Session log missing or changed | Values marked "at start" | One log line; the UI keeps working. |
| A spoken agent without a CLI | The "not found" bubble | The phrase is not sent to any other agent. |

Security stays as today: prompts only through stdin, the project folder checked before `wt`, model IDs rejected when they start with `-` or contain whitespace, and a per-agent env allowlist (Codex adds `CODEX_HOME` and `OPENAI_API_KEY`).

## Testing

Test first, then code, then commit, one step per commit.

- `codex.rs`: new and continued session arguments; Default passes no flags; a model cannot smuggle a flag; terminal arguments; env allowlist.
- Codex `--json` parser on fixtures recorded from a real run: `thread.started`, command and file change items, `agent_message`, `turn.completed`, `turn.failed`, unknown and malformed lines.
- `codex debug models`: only `visibility: "list"` models; malformed JSON is an error, never a panic.
- `transcript.rs`: Claude and Codex log fixtures give the newest model and permission; empty or broken files give `None`.
- `claude.rs` and `stream.rs` tests stay unchanged; a new test checks that a continued Claude run passes no model or permission flags.
- `controller.rs`: a spoken agent starts a new session with that agent; a plain phrase continues the active session with its agent; with no active session the default agent is used; an agent without a CLI sends nothing.
- `settings.rs`: old `mode` and `model` move to `agents.claude`; the new format round-trips; an empty or flag-like custom model ID is rejected.
- `history.rs`: entries without `agent` read as Claude.
- CLI search on a fake set of registry PATH values, without the real registry.
- `capabilities/*.json` tests cover the new commands (`agent_status`, `recheck_agents`).

Manual check before the pull request:

1. `cargo run -p erindi-core --example codex-smoke -- <folder> <prompt>`, a new example next to `claude-smoke`.
2. In the app: "codex, …", continue by voice, open in terminal, change the permission in the terminal and see it on the Sessions tab, remove `codex` from PATH, see the hint, restore it and press Re-check.

## Out of scope

- Pi, LM Studio and OpenAI-compatible servers (next specs).
- Exact Claude versions in the model list (needs the Anthropic Models API).
- Codex app-server, the Agent Client Protocol and a plugin system (`ROADMAP.md`).
- Install steps per agent and system (`ROADMAP.md`).
- macOS.
