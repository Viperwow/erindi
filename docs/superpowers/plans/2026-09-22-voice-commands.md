# Voice Commands Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** One talk hotkey with Wispr-style gestures, three voice commands with editable regex patterns on a Commands tab, and the local model used only to recognise commands.

**Architecture:** `erindi-core` gets `commands.rs` (patterns, parser) and gesture handling in the controller with a timer effect. `refine.rs` becomes a command classifier whose answer must be an edge cut of the transcript. The desktop app stores patterns and hotkeys in settings, runs timers, opens terminals, and shows a Commands tab.

**Tech Stack:** Rust 2024, `regex` crate, Tauri 2, Preact + Tailwind.

**Spec:** `docs/superpowers/specs/2026-09-22-voice-commands-design.md`

## Global Constraints

- `HOLD` = 300 ms, `DOUBLE` = 300 ms.
- Recording starts on the first key-down.
- Cancel is a single press of the same key, in every state.
- Commands only at the start or end of a phrase; cancel only at the end.
- The model never changes the text that reaches Claude; its answer is accepted only as an edge cut.
- Defaults: talk `Ctrl+Alt+Space`, new session `Ctrl+Alt+N`, terminal `Ctrl+Alt+T`.
- Saved `holdHotkey` loads as the talk hotkey; saved `cleanup` loads as `modelCommands`; `toggleHotkey` is ignored.
- Commit messages carry no AI attribution trailers.

## Review Focus

1. **Key auto-repeat while holding** sends repeated key-downs: must not count as a double-press. Pinned by `auto_repeat_is_not_a_double_press` in Task 2.
2. **Pattern typed by the user that matches everything** (`.*`): must be refused on save, not cancel every phrase. Pinned by `patterns_matching_empty_are_rejected` in Task 1.
3. **Model returns a rewritten `rest`**: must fall back to the whole transcript. Pinned by `rewritten_rest_is_rejected` in Task 3.
4. **Single press during transcription**: must cancel, not start a run when the transcript arrives. Pinned by `press_during_transcription_cancels` in Task 2.
5. **Terminal hotkey with no active session**: nothing happens, no crash. Pinned by `terminal_without_active_session_does_nothing` in Task 2.

---

### Task 1: Command patterns and parser

**Files:** create `crates/core/src/commands.rs`; modify `crates/core/src/session.rs`, `crates/core/src/lib.rs`, `crates/core/Cargo.toml`.

**Produces:**
- `#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)] #[serde(rename_all = "camelCase")] pub enum Command { NewSession, OpenTerminal, Cancel }`
- `#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)] #[serde(default, rename_all = "camelCase")] pub struct Patterns { pub new_session: Vec<String>, pub open_terminal: Vec<String>, pub cancel: Vec<String> }` with `Default` = the spec's defaults.
- `pub struct Parser` built by `Parser::new(&Patterns) -> Result<Parser, String>` (error names the bad pattern).
- `Parser::parse(&self, text: &str) -> (Option<Command>, String)`: the command and the rest with joining words and punctuation trimmed.
- `session.rs`: `parse_intent`, `FILLERS`, `CONNECTORS`, `PHRASES` and `Intent::Continue` are removed; `choose(policy, recent, new: bool, active, now)`.

Tests (write first, watch fail):
- `new_session_at_start_and_end`: "создай новую сессию и проверь diff" → (NewSession, "проверь diff"); "Проверь diff и создай новую сессию." → (NewSession, "Проверь diff"); "New session, fix the tests" → (NewSession, "fix the tests"); "fix the tests in a new session" → (NewSession, "fix the tests"); "Открой в новой сессии: найди баг." → (NewSession, "найди баг.").
- `commands_in_the_middle_are_ignored`: "расскажи про новую сессию в React и как её закрыть" → (None, same text).
- `cancel_only_at_the_end_and_wins`: "новая сессия, проверь diff, отмена" → (Cancel, …); "отмена проверки diff" at start → (None, same).
- `open_terminal_alone`: "Открой в терминале." → (OpenTerminal, "").
- `patterns_matching_empty_are_rejected`: `Parser::new` with `.*` or `(a)?` → Err; with `(` → Err.
- `choosing_a_session` in `session.rs` rewritten for `new: bool`.

Implementation notes:
- Build per command and position one regex: start `(?i)^[\W_]*(?:p1|p2|…)\b` and end `(?i)\b(?:p1|p2|…)[\W_]*$`; each user pattern wrapped in `(?:…)`. Longest match wins because the parser tries every pattern separately at a position and keeps the longest; keep a `Vec<(Command, Regex, Regex)>` per pattern.
- Empty-match check: `Regex::new(&format!("^(?:{p})$"))?.is_match("")`.
- Joining words: after a start match, strip `^[\s\p{P}]*((и|потом|затем|and|then)\b[\s\p{P}]*)?`; before an end match, strip `[\s\p{P}]*((и|потом|затем|and|then)\s*)?$` from the rest, then trim trailing `,;:-` and spaces (keep a final `.` only when it was part of the task, as the old parser did: trim `[,;:\-\s]+$`).
- Order: cancel at end → any command at start → new session or terminal at end.

Commit: `feat(core): regex voice command patterns and parser`.

---

### Task 2: Gestures and commands in the controller

**Files:** modify `crates/core/src/controller.rs`, `crates/core/src/state.rs`.

**Consumes:** `Parser`, `Command`, `Patterns`.

**Produces:**
- `Key { Talk, NewSession, Terminal }`.
- `Msg::Settings { policy, recent, cwd, patterns: Patterns, model_commands: bool }`.
- `Msg::GestureTimeout { seq: u64 }`; `Effect::GestureTimer { seq: u64 }` (runtime sends the timeout after `DOUBLE`).
- `Effect::OpenTerminal { id: Uuid, cwd: String }`.
- `Event::CancelTranscribing` in the state machine: `Transcribing → Idle`.
- `Effect::StartCapture { op }` always uses the endpointer; the controller ignores `SpeechEnded` unless hands-free.

Behaviour per the spec's gesture table. Tests (first, watch fail):
- `hold_talks_until_release` (down, audio, up after 400 ms → Transcribe).
- `quick_tap_from_idle_drops_the_recording` (down, up after 100 ms, timeout → StopCapture, Idle, no Transcribe).
- `double_press_goes_hands_free_and_sends_on_pause` (down/up/down/up within windows → still Listening; SpeechEnded → Transcribe).
- `double_press_while_hands_free_sends_now`.
- `single_press_while_hands_free_cancels`.
- `press_during_transcription_cancels` (tap then timeout while Transcribing → Idle; a later Transcribed is ignored).
- `single_press_while_running_cancels_the_run`, `double_press_while_running_does_nothing`.
- `auto_repeat_is_not_a_double_press` (down, down, down while held, up after 500 ms → hold send).
- `new_session_key_talks_into_a_new_session`.
- `terminal_key_opens_the_active_session`, `terminal_without_active_session_does_nothing`.
- `spoken_cancel_sends_nothing`, `spoken_terminal_alone_opens_terminal_and_sends_nothing`, `spoken_new_session_starts_a_new_session`.
- Existing tests are updated from `Key::Hold`/`Key::Toggle` to gestures through test helpers `hold(key)` and `double(key)` that advance `now`.

Commit: `feat(core): hotkey gestures and voice commands in the controller`.

---

### Task 3: Model recognises commands

**Files:** modify `crates/core/src/refine.rs` (rename to `classify.rs`), `crates/core/src/llama.rs`, `crates/core/src/controller.rs`, `crates/core/src/state.rs`, `crates/core/examples/*`.

**Produces:**
- `classify::request(text) -> Value` with schema `{command: enum[new_session, open_terminal, cancel, none], rest: string}`.
- `classify::parse_response(&Value) -> Option<(Option<Command>, String)>`.
- `classify::accept(text, answer) -> Option<(Command, String)>`: `Some` only for a real command whose `rest` equals `text` minus a leading or trailing run of words, compared on lowercase words without punctuation; the returned rest is cut from the original `text`, not taken from the model.
- `LlamaServer::classify(&self, text) -> Result<Option<(Option<Command>, String)>, String>`.
- Controller: state `Refining` renamed `Classifying`; `Effect::Classify { op, text }`, `Msg::Classified { op, answer }`; used only when the parser found nothing and `model_commands` is on.
- `Effect::StartRun` loses `raw`.

Tests: `edge_cut_is_accepted` (start and end), `rewritten_rest_is_rejected`, `middle_cut_is_rejected`, `none_is_rejected`, `request_is_typed`, controller `model_command_applies`, `model_failure_sends_whole_text`.

Commit: `feat(core): local model recognises commands, never rewrites text`.

---

### Task 4: Desktop wiring

**Files:** modify `apps/desktop/src-tauri/src/settings.rs`, `runtime.rs`, `lib.rs`, `history.rs` (only writes `Prompt::Plain`), `build.rs`, `capabilities/settings.json`.

**Produces:**
- `Settings { talk_hotkey (alias "holdHotkey"), new_session_hotkey, terminal_hotkey, patterns: Patterns, model_commands (alias "cleanup"), … }` without `toggle_hotkey`.
- `Settings::validate` checks `Parser::new(&self.patterns)`.
- Runtime: `Effect::GestureTimer` → thread sleeps `DOUBLE`, sends `GestureTimeout`; `Effect::OpenTerminal` → `open_terminal`; `Effect::Classify` → `Refiner::classify`.
- Hotkeys registered for `Key::Talk`, `Key::NewSession`, `Key::Terminal`.
- Command `test_command(patterns: Patterns, text: String) -> Result<TestResult, String>` where `TestResult { command: Option<Command>, rest: String }`.

Tests: `old_settings_keys_still_load` (holdHotkey, cleanup, toggleHotkey present), `invalid_pattern_fails_validation`, capability test includes `allow-test-command`.

Commit: `feat(desktop): gestures, command hotkeys and patterns in settings`.

---

### Task 5: Commands tab and Settings

**Files:** create `apps/desktop/src/commands.tsx`, `apps/desktop/src/controls.tsx` (move `Section`, `Field`, `HotkeyInput`, `ModelRow`, `input`); modify `settings.tsx`, `overlay.tsx`.

- Tabs: Sessions, Commands, Settings; hash `#commands` supported.
- Commands tab per the spec: header and regex hint, Try a phrase (calls `test_command` on each input), rows with hotkey field and chips, add on Enter, remove, Reset to defaults, model checkbox and model row, Save.
- Settings: Hotkeys block with Talk and the gesture hint; Prompt cleanup block removed; no new-session hotkey here.
- Overlay: `Classifying` label "Checking command…".

Verify: `pnpm tsc --noEmit`, `pnpm build`.

Commit: `feat(desktop): Commands tab with editable patterns`.

---

### Task 6: Benchmark, roadmap, readme

- `refine-bench` becomes `command-bench` with cases `{say, command, rest}`: parser accuracy, model accuracy on phrases the parser misses, false commands on plain tasks, latency.
- `ROADMAP.md`: prompt cleanup item replaced by "Voice commands with editable patterns" (done) and "Hotkey gestures" (done); wake word stays; add "Project switching by voice".
- `README.md`: hotkeys line updated.

Commit: `docs: voice commands roadmap; command benchmark`.
