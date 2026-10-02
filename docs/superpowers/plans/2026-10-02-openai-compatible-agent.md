# OpenAI-compatible Agent Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A fourth agent, `Agent::Api`, that sends each phrase to an OpenAI-compatible `/chat/completions` endpoint, streams the reply into the bubble and keeps the conversation in Erindi's history.

**Architecture:** `erindi-core` gains `api.rs`, a blocking `ureq` client that turns an SSE stream into the existing `RunEvent`s, so the controller, bubble, queue and cancel stay as they are. The desktop runtime branches in `start_run`: `Agent::Api` runs `api::stream_chat` on a blocking thread instead of spawning a CLI. Settings hold name, base URL and model; the key lives in the OS credential store through `keyring`. History prompts gain an optional reply, which feeds the next request and the Sessions question → answer list.

**Tech Stack:** Rust (`ureq` 3 already in core, `keyring` new in the desktop crate), Tauri 2, Preact + Tailwind + TypeScript.

**Spec:** `docs/superpowers/specs/2026-10-02-openai-compatible-agent-design.md`

## Global Constraints

- Default name "Local model"; default base URL `http://localhost:1234/v1`; requests to `{base}/chat/completions` and `{base}/models`, a trailing `/` on base ignored.
- Spoken patterns default to "модель…" and "local model", editable on the Commands tab.
- Key in the OS credential store, service `com.viperwow.erindi`, user `api-key`; `settings.json` never holds it.
- `stream: true`; no system prompt; the project folder is not sent.
- Error texts, verbatim: "Cannot reach {base} — is the server running?", "The API key was rejected", "Choose a model in Settings", "This agent has no terminal".
- 10 s connect timeout; the whole run shares `RUN_TIMEOUT`.
- No new HTTP dependency; tests use a mock server on a local `TcpListener`.
- Claude, Codex and Pi behave exactly as before.
- Every UI change ends with before/after screenshots and a user path for each (project memory).
- Full check list, Windows and Mac: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `pnpm -C apps/desktop test`, `tsc --noEmit`.

## Review Focus

1. SSE keep-alive comment lines (`: ping`) and blank lines between events — ignored, the reply is unaffected.
2. A server that ignores `stream: true` and returns one JSON body (`choices[0].message.content`) — its text becomes the result.
3. An error object inside the stream (`data: {"error":{"message":"..."}}`, as Ollama and LM Studio send on a bad model) — the run fails with that message, not with an empty success.
4. Chunks with no text (a role-only first chunk, a `finish_reason` chunk, `reasoning_content` from a thinking model) — skipped, no empty `Reply` events.
5. A base URL entered with a trailing slash, or with spaces around it — the same requests as the clean URL.

---

### Task 1: OpenAI-compatible client in core

**Files:**
- Create: `crates/core/src/api.rs`
- Modify: `crates/core/src/lib.rs` (add `pub mod api;`)

**Interfaces:**
- Produces:
  - `pub struct ApiConfig { pub base_url: String, pub key: Option<String>, pub model: String }`
  - `pub struct Turn { pub prompt: String, pub reply: Option<String> }`
  - `pub fn messages(history: &[Turn], prompt: &str) -> Vec<serde_json::Value>` — `{"role","content"}` objects; a turn without a reply contributes only its user message.
  - `pub fn stream_chat(config: &ApiConfig, messages: &[serde_json::Value], cancel: &AtomicBool, on_event: impl FnMut(RunEvent)) -> RunEnd` — emits `RunEvent::Reply { text }` with the text so far after each non-empty chunk, then exactly one `RunEvent::Result`; returns `RunEnd::Cancelled` when `cancel` was set, else `RunEnd::Exited { success }`.
  - `pub fn list_models(base_url: &str, key: Option<&str>) -> Result<Vec<String>, String>`

- [ ] **Step 1: Write the failing tests** in `api.rs` `#[cfg(test)]`, with a helper `serve(response: &'static str) -> (String, JoinHandle<String>)` that binds `127.0.0.1:0`, answers one request with `response` verbatim and returns the base URL `http://127.0.0.1:{port}/v1` and the request it read.

```rust
#[test] fn messages_alternate_user_and_assistant()   // 2 turns with replies + "c" → roles [user, assistant, user, assistant, user]
#[test] fn a_streamed_reply_grows_then_ends()         // 3 chunks "Hel","lo"," you" + [DONE] → Reply "Hel","Hello","Hello you"; Result{ok:true,text:"Hello you"}; Exited{success:true}
#[test] fn the_request_names_the_model_and_streams()  // request line "POST /v1/chat/completions", body has "model":"m","stream":true, Authorization "Bearer k" when key is Some, absent when None
#[test] fn keep_alives_and_empty_chunks_are_skipped() // ": ping", blank lines, a role-only chunk, a finish_reason chunk, a reasoning_content chunk → only the 2 text chunks become Replies
#[test] fn a_plain_json_body_is_the_reply()           // non-SSE 200 body {"choices":[{"message":{"content":"hi"}}]} → Result{ok:true,text:"hi"}
#[test] fn an_error_in_the_stream_fails_the_run()     // data: {"error":{"message":"model not found"}} → Result{ok:false,text contains "model not found"}
#[test] fn a_rejected_key_says_so()                   // HTTP 401 → Result{ok:false,text:"The API key was rejected"}
#[test] fn other_statuses_show_the_server_message()   // HTTP 500 body {"error":{"message":"boom"}} → text contains "500" and "boom"
#[test] fn an_unreachable_server_names_the_address()  // base on a closed port → text == "Cannot reach {base} — is the server running?"
#[test] fn a_cut_stream_keeps_its_text()              // 1 chunk "part", connection closed without [DONE] → Result{ok:true,text:"part"}
#[test] fn cancel_ends_without_a_result()             // cancel set from on_event after the first Reply → RunEnd::Cancelled, no Result{ok:true}
#[test] fn a_trailing_slash_is_ignored()              // base "  http://127.0.0.1:{port}/v1/ " → request path "/v1/chat/completions"
#[test] fn models_are_listed()                        // GET /v1/models {"data":[{"id":"a"},{"id":"b"}]} → Ok(["a","b"])
```

- [ ] **Step 2: Run** — `cargo test -p erindi-core --lib api` — Expected: FAIL (module missing).
- [ ] **Step 3: Implement** with `ureq` (connect timeout 10 s, `http_status_as_error(false)` so the body of an error status can be read); read the body line by line, act only on `data: ` lines, stop at `data: [DONE]`; check `cancel` between lines and return `Cancelled` without a `Result`. When the content type is not `text/event-stream`, parse the whole body as one completion.
- [ ] **Step 4: Run** — same command — Expected: PASS, 13 tests.
- [ ] **Step 5: Commit** — `feat: OpenAI-compatible chat client`

### Task 2: `Agent::Api` in core

**Files:**
- Modify: `crates/core/src/agent.rs`, `crates/core/src/commands.rs`, `crates/core/src/transcript.rs`

**Interfaces:**
- Consumes: nothing from Task 1.
- Produces: `Agent::Api` (serde `"api"`), in `Agent::ALL`; `label()` → `"Local model"` (the default; the desktop shows the user's name); `Agent::is_cli(self) -> bool` (false only for Api); `permissions()` → `&[]`; `uses_erindi_id()` → true; `Command::Api` with `Patterns.api` (serde default) and `Command::Api.agent() == Some(Agent::Api)`; `InvalidRequest::NotCli`.

- [ ] **Step 1: Write the failing tests** — in `commands.rs`: `"модель, расскажи анекдот"` and `"local model, what time is it"` parse to `[Command::Api]`; a settings `patterns` JSON without `api` deserializes with the default patterns. In `agent.rs`: `!Agent::Api.is_cli()` and `Agent::Claude.is_cli()`.
- [ ] **Step 2: Run** — `cargo test -p erindi-core --lib` — Expected: FAIL to compile.
- [ ] **Step 3: Implement** — default patterns `r"((в|с|через) )?модел\w*"`, `r"((in|with) )?(the )?local model"`; the CLI-only paths return an empty or failing value for Api, so a mistaken call fails cleanly: `cli()` → `""`, `headless_args` and `terminal_args` → `Err(InvalidRequest::NotCli)` (a new variant), the `EventParser` ignores lines, transcript reading returns `None`.
- [ ] **Step 4: Run** — full core tests — Expected: PASS.
- [ ] **Step 5: Commit** — `feat: local model agent in core`

### Task 3: Connection settings and the stored key

**Files:**
- Modify: `apps/desktop/src-tauri/Cargo.toml` (`keyring = { version = "3", features = ["apple-native", "windows-native"] }`), `apps/desktop/src-tauri/src/settings.rs`
- Create: `apps/desktop/src-tauri/src/api_key.rs`

**Interfaces:**
- Produces:
  - `Settings.api_name: String` ("Local model"), `api_base_url: String` (`http://localhost:1234/v1`), `api_model: String` (""), all `#[serde(default)]`, camelCase in JSON.
  - `api_key::get() -> Option<String>`, `api_key::set(key: &str) -> Result<(), String>`, `api_key::clear() -> Result<(), String>`.
  - `Settings::api_config(&self) -> Result<erindi_core::api::ApiConfig, String>` — Err("Choose a model in Settings") when the model or base URL is blank.

- [ ] **Step 1: Write the failing tests** — `old_settings_get_the_api_defaults` (the existing old-settings fixture loads with the three defaults); `a_blank_model_asks_to_choose_one` (`api_config()` with `api_model: ""` → Err("Choose a model in Settings")); `#[ignore] fn the_key_round_trips_through_the_credential_store` (set "k", get == Some("k"), clear, get == None).
- [ ] **Step 2: Run** — `cargo test -p erindi-desktop settings` — Expected: FAIL.
- [ ] **Step 3: Implement** — `api_key` uses `keyring::Entry::new("com.viperwow.erindi", "api-key")`; `get` maps `NoEntry` to `None`. Tauri commands `has_api_key() -> bool`, `set_api_key(key: String)`, `clear_api_key()`, registered in `lib.rs` and in `build.rs`'s command list.
- [ ] **Step 4: Run** — same command, then `cargo test -p erindi-desktop -- --ignored the_key_round_trips` on Windows and on the Mac — Expected: PASS.
- [ ] **Step 5: Commit** — `feat: local model connection settings with the key in the credential store`

### Task 4: Replies in the session history

**Files:**
- Modify: `apps/desktop/src-tauri/src/history.rs`

**Interfaces:**
- Produces: `Prompt::Answered { text: String, reply: String }` (a third untagged variant, so old files still read); `Prompt::text(&self) -> &str`; `History::set_reply(&mut self, id: Uuid, reply: String) -> io::Result<()>` — turns the last prompt of `id` into `Answered`; `History::turns(&self, id: Uuid) -> Vec<erindi_core::api::Turn>`.

- [ ] **Step 1: Write the failing tests** — `old_history_still_loads` (a `sessions.json` with plain and refined prompts parses unchanged); `a_reply_attaches_to_the_last_prompt` (two prompts, `set_reply` → second is `Answered`, file round-trips); `turns_pair_prompts_with_replies`.
- [ ] **Step 2: Run** — `cargo test -p erindi-desktop history` — Expected: FAIL.
- [ ] **Step 3: Implement.** The frontend `Prompt` type in `sessions.tsx` gains `{ text: string; reply: string }`.
- [ ] **Step 4: Run** — same command — Expected: PASS.
- [ ] **Step 5: Commit** — `feat: session history keeps model replies`

### Task 5: Runs, terminal and status for the local model

**Files:**
- Modify: `apps/desktop/src-tauri/src/runtime.rs` (`start_run` near line 649, `request` near 549, `Effect::OpenTerminal` near 399, `open_history_session` near 138), `apps/desktop/src-tauri/src/agents.rs`

**Interfaces:**
- Consumes: `api::{stream_chat, messages, list_models}` (Task 1), `Settings::api_config`, `api_key::get` (Task 3), `History::{turns, set_reply}` (Task 4).
- Produces: `AgentStatus` for Api with `label` = `api_name`, `path` = `Some(base_url)`, `models` from `list_models`, `models_error` = the request error; a Tauri command `api_models(base_url: String) -> Result<Vec<String>, String>` for the Refresh button.

- [ ] **Step 1: Write the failing tests** — a `fn api_run_records_the_reply()` that drives the run function extracted below against the Task 1 mock server: two phrases in one session → the second request's body holds the first phrase and its reply; the history holds both replies.
- [ ] **Step 2: Run** — `cargo test -p erindi-desktop api_run` — Expected: FAIL.
- [ ] **Step 3: Implement** — extract `fn run_api(config, turns, prompt, cancel, on_event) -> (RunEnd, Option<String>)`; `start_run` for `Agent::Api` skips `request()`/`locate`, records the prompt as today, runs `run_api` in `tauri::async_runtime::spawn_blocking`, forwards events as `Msg::Run`, stores the reply with `set_reply`, sends `Msg::RunExited`; cancel sets the `AtomicBool` from the existing `CancellationToken`. `Effect::OpenTerminal` and `open_history_session` for an Api session send the failure text "This agent has no terminal".
- [ ] **Step 4: Run** — full check list on Windows and the Mac — Expected: PASS.
- [ ] **Step 5: Commit** — `feat: local model runs stream into the bubble`

### Task 6: Settings → Agent connection fields

**Files:**
- Modify: `apps/desktop/src/agent.ts` (`"api"` in `Agent`), `apps/desktop/src/agents.tsx`, `apps/desktop/src/settings.tsx`, `apps/desktop/src/controls.tsx` (`Settings` type)

**Interfaces:**
- Consumes: `has_api_key`, `set_api_key`, `clear_api_key`, `api_models` (Tasks 3, 5).
- Produces: `agentName(agent: Agent, settings: Settings): string` in `agent.ts` — `settings.apiName` for `"api"`, `agentLabels[agent]` otherwise.

- [ ] **Step 1: Write the failing test** in `apps/desktop/src/model.test.ts` (or a new `agent.test.ts`): `agentName("api", {apiName: "LM Studio"})` → `"LM Studio"`; `agentName("claude", …)` → `"Claude"`.
- [ ] **Step 2: Run** — `pnpm -C apps/desktop test` — Expected: FAIL.
- [ ] **Step 3: Implement** — when the selected agent is `api`, the Agent section shows Name, Base URL, API key (password field, placeholder "Stored" when `has_api_key`, a Clear button) and Model (a select filled by `api_models` with a Refresh button; a text field when the call fails). Saving the form calls `set_api_key` only when the key field is non-empty. Every agent label in Settings, Commands and Sessions goes through `agentName`.
- [ ] **Step 4: Run** — `pnpm test`, `tsc --noEmit`, `pnpm build` — Expected: PASS.
- [ ] **Step 5: Commit** — `feat: local model connection in Settings`

### Task 7: Sessions show the conversation

**Files:**
- Modify: `apps/desktop/src/sessions.tsx`, `apps/desktop/src/model.ts` (`textOf`)

- [ ] **Step 1: Write the failing test** — `textOf({ text: "q", reply: "a" })` → `"q"`; a new `replyOf(prompt)` → `"a"`, `null` for plain and refined prompts.
- [ ] **Step 2: Run** — `pnpm -C apps/desktop test` — Expected: FAIL.
- [ ] **Step 3: Implement** — an expanded Api session lists each phrase with its reply under it, the reply in muted text; the "Open in terminal" button is absent for `agent === "api"`; Continue and Delete stay.
- [ ] **Step 4: Run** — `pnpm test`, `tsc`, `pnpm build` — Expected: PASS.
- [ ] **Step 5: Commit** — `feat: local model sessions show questions and answers`

### Task 8: Roadmap, builds and the visual check

**Files:**
- Modify: `ROADMAP.md`

- [ ] **Step 1: Write** — tick a new item **Local model** under Working with agents ("Any OpenAI-compatible server, such as LM Studio or Ollama, as an agent: replies stream into the bubble and the conversation continues by voice"); add open items **Several saved model connections** and **Trim long conversations** ("drop the oldest turns to fit the model's context").
- [ ] **Step 2: Build** — Windows `pnpm -C apps/desktop tauri build --no-bundle`; the Mac over SSH per the manual-builds memory.
- [ ] **Step 3: Check by hand with LM Studio** on both: pick the model, ask two related questions by voice, cancel one mid-reply, open Sessions; stop LM Studio and ask again (the address error).
- [ ] **Step 4: Screenshots** — before (main) and after for Settings → Agent with Local model selected and for an expanded Api session; a user path for each.
- [ ] **Step 5: Commit** — `docs: roadmap marks the local model agent`
