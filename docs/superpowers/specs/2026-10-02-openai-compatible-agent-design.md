# OpenAI-compatible agent

Date: 2026-10-02. Branch: `feat/openai-compatible-agent`.

## Goal

Erindi works for someone who has no agent CLI, only a model server: LM Studio, Ollama, or a cloud service with an OpenAI-compatible API. They set the server address, an optional API key and a model in Settings, then talk to the model by voice. The reply streams into the overlay bubble, and the conversation continues phrase by phrase.

Success: a person with only LM Studio installs Erindi, enters `http://localhost:1234/v1`, picks a model and has a spoken conversation with it.

Out of scope: tools, file edits and commands (the model only answers); a chat window (Erindi stays voice-first); several saved connections (ROADMAP).

## Decisions

| # | Topic | Decision |
|---|-------|----------|
| 1 | Agent | A fourth agent, `Agent::Api`, next to Claude, Codex and Pi. It appears in the default agent list and on the Sessions tab like the others. |
| 2 | Connections | One connection: name, base URL, API key, model. Several saved connections go to ROADMAP. |
| 3 | Name | The user names the connection (`apiName`, default "Local model"). Settings, the bubble and Sessions show that name. The spoken patterns for choosing it default to "модель…" and "local model" and are edited on the Commands tab like the other agents' patterns. |
| 4 | Base URL | Default `http://localhost:1234/v1` (LM Studio). Requests go to `{base}/chat/completions` and `{base}/models`; a trailing `/` is ignored. |
| 5 | API key | Optional; local servers need none. It lives in the OS credential store (Windows Credential Manager, macOS Keychain) through the `keyring` crate, under service `com.viperwow.erindi` and user `api-key`. `settings.json` never holds it. Settings shows a password field: empty means "keep the stored key", and a Clear button removes it. |
| 6 | Model | A list read from `{base}/models`, with a Refresh button. When the server does not answer, the field becomes free text, so a model can still be typed. |
| 7 | Streaming | `stream: true`. Every `choices[0].delta.content` chunk extends the reply; the bubble shows the text as it grows. `data: [DONE]` ends the reply. |
| 8 | Conversation | Erindi keeps the conversation. Each prompt in the session history gains an optional `reply`. A new phrase in the same session sends every earlier phrase and reply as alternating `user` and `assistant` messages, then the new phrase. No system prompt, and the project folder is not sent. |
| 9 | Sessions tab | An API session shows its phrases and replies as a question → answer list. It has no "Open in terminal" button; Continue and Delete work as for other agents. The spoken "open in terminal" on an API session answers "This agent has no terminal". |
| 10 | Cancel | The cancel shortcut or the spoken "cancel" drops the connection at once, like it kills an agent process. |
| 11 | Errors | Every failure ends the run with `RunEvent::Result { ok: false, text }`: an unreachable server gives "Cannot reach {base} — is the server running?"; HTTP 401 or 403 gives "The API key was rejected"; any other error status gives the status and the server's `error.message` when present; a stream that ends without `[DONE]` keeps the text it got, and is an error only when no text came. A missing model or base URL stops the run before it starts with "Choose a model in Settings". |
| 12 | Timeouts | 10 s to connect. No limit on the reply while chunks arrive; the whole run shares the CLI agents' `RUN_TIMEOUT`. |
| 13 | Long conversations | No trimming. A model that refuses a long history returns its own error, and the user starts a new session. Trimming old turns to fit the context goes to ROADMAP. |
| 14 | HTTP | The `ureq` client already in `erindi-core`, run on a blocking thread. No new HTTP dependency. |

## Components

- `crates/core/src/api.rs`: `stream_chat(config, messages, cancel, on_event)` sends the request and turns the SSE stream into `RunEvent::Reply` (the text so far) and a final `RunEvent::Result`; `list_models(base, key)`; `messages(history, prompt)` builds the message list.
- `crates/core/src/agent.rs`: `Agent::Api` with its label and spoken patterns.
- `apps/desktop/src-tauri/src/settings.rs`: `apiName`, `apiBaseUrl`, `apiModel`; the key through `keyring`.
- `apps/desktop/src-tauri/src/runtime.rs`: `start_run` streams through `api::stream_chat` for `Agent::Api` instead of starting a process, and stores the reply in the history when the run ends.
- `apps/desktop/src-tauri/src/history.rs`: `Prompt.reply: Option<String>`, absent in older files.
- `apps/desktop/src`: the connection fields in Settings → Agent; the question → answer list and the missing terminal button on Sessions.
- `ROADMAP.md`: several saved OpenAI-compatible connections; trimming old turns to fit the context.

## Testing

- `api.rs` against a mock server on a local `TcpListener` in the test, with no new dependencies: three chunks and `[DONE]` give three growing replies and a result with the full text; 401 gives the key message; a closed port gives the address message; cancelling mid-stream ends without a successful result; `/models` gives the model ids.
- `messages`: two earlier phrase-reply pairs and a new phrase give `user, assistant, user, assistant, user`.
- Settings: a `settings.json` without the API fields loads with the defaults.
- History: a `sessions.json` without `reply` loads.
- The credential store: a round-trip test marked `#[ignore]`, run by hand on Windows and macOS; CI runners may have no credential store.
- By hand with LM Studio on Windows and macOS. Before and after screenshots of Settings → Agent and of an API session on the Sessions tab, with the user path for each.
