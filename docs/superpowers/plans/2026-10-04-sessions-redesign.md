# Sessions Redesign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the expanding Sessions cards with a searchable session list and a read-only conversation pane, with stored answers, per-answer models and live status for every agent.

**Architecture:** History stores every run's answer, its model and whether it failed. The runtime also emits the controller's `view` to the settings window. The Sessions tab is rebuilt from small Preact components fed by three pure modules (search, status, copy) that carry the logic and the tests.

**Tech Stack:** Rust (erindi-core, Tauri 2), Preact + Tailwind 4 + TypeScript, `node --test`.

**Spec:** `docs/superpowers/specs/2026-10-04-sessions-redesign-design.md`, with the screens in `docs/superpowers/specs/2026-10-04-sessions-states.html`.

## Global Constraints

- Commit subjects start lowercase (commitlint). No AI attribution in commits.
- No element moves when state changes: reserve slots, change opacity, keep fixed sizes.
- Rails: 2.5 px, drawn as `border-left`; colours as the bubble: speaking `#38bdf8`, transcribing `#a855f7`, running `#f59e0b`, done `rgba(74,222,128,.55)`, failed `#f87171`, cancelled `rgba(255,255,255,.22)`, queued dashed 2 px / 2 px `rgba(255,255,255,.35)`, waiting `#22508a`.
- Session marks: waiting hollow circle `#22508a`; listening dot `#38bdf8` pulsing; transcribing triangle pointing down `#a855f7` pulsing; answering pentagon `#f59e0b` turning clockwise; idle dot `#737373`. 12 px SVG, 0.6 px stroke in the fill colour, round joins; motion off under `prefers-reduced-motion`.
- Window: opens at 1120 × 720, minimum 640 × 480; below 900 px one pane at a time.
- Comments only where the logic is not obvious; say what the code means.
- Old history files load unchanged: every new field is optional.

## Review Focus

1. A 0.14 history file with Plain, Refined and Answered prompts loads and saves back unchanged: Task 1, `old_history_round_trips`.
2. An answered phrase that Erindi cleaned up keeps its "Said:" text: Task 1, `an_answer_keeps_what_was_said`.
3. A run that ends after its session was deleted saves nothing and does not panic: Task 1, `an_answer_for_a_deleted_session_is_dropped`.
4. Substring search treats `.`, `(`, `*` literally; only `.*` mode reads them as regex: Task 5, `substring_search_escapes_regex_characters`.
5. A cancelled CLI run stores no answer, and a failed one stores its error: Task 3, `answer_of` tests.

---

### Task 1: Answers, models and failures in history

**Files:**
- Modify: `apps/desktop/src-tauri/src/history.rs`
- Modify: `apps/desktop/src-tauri/src/runtime.rs` (callers of `set_reply`)

**Interfaces:**
- Produces:
  - `Prompt::Answered { text: String, reply: String, raw: Option<String>, model: Option<String>, failed: bool }`. `raw`, `model` and `failed` are `#[serde(default)]` and skipped when empty or false.
  - `Answered` is declared before `Refined`, so an answer with `raw` is not read as `Refined`.
  - `pub struct Answer { pub reply: String, pub model: Option<String>, pub failed: bool }`
  - `History::set_answer(&mut self, id: Uuid, answer: Answer) -> Result<(), String>` replaces `set_reply`. It keeps the prompt's `raw`.
  - `History::turns` gives `reply: None` for a failed answer, so the local model never sees an error as its own words.

- [ ] **Step 1: Write the failing tests** in `history.rs` `mod tests`:
  - `old_history_round_trips`: parse `[{"id":…,"prompts":["a",{"text":"b","raw":"bb"},{"text":"c","reply":"d"}],…}]`, save, and compare the JSON to the input.
  - `an_answer_keeps_what_was_said`: record `Refined { text: "b", raw: "uh b" }`, then `set_answer(.., Answer { reply: "ok", model: Some("opus"), failed: false })`. Reload from disk; expect `Answered { text: "b", reply: "ok", raw: Some("uh b"), model: Some("opus"), failed: false }`.
  - `an_answer_for_a_deleted_session_is_dropped`: `set_answer` on an unknown id returns `Ok(())` and changes no entry.
  - `a_failed_answer_is_not_a_turn`: `set_answer(.., failed: true)`, then `turns(id)[0].reply == None`.

- [ ] **Step 2: Run** `cargo test -p erindi-desktop history`. Expected: FAIL, `set_answer` not found.

- [ ] **Step 3: Implement.** Make the `Prompt` and `History` changes above. Update both `set_reply` callers in `runtime.rs` (`run_api` and its tests) to `set_answer` with `model: Some(config.model.clone())`.
  - `run_api` stores `Result { ok: false, text }` as `failed: true`.
  - A cancelled run still stores nothing.

- [ ] **Step 4: Run** `env -u CODEX_HOME cargo test --workspace`. Expected: PASS.

- [ ] **Step 5: Commit** `feat: history keeps each answer's model and failure`.

### Task 2: Agents report their model

**Files:**
- Modify: `crates/core/src/stream.rs` (Claude), `crates/core/src/cursor.rs`, `crates/core/src/pi.rs`, `crates/core/src/controller.rs`

**Interfaces:**
- Produces: `RunEvent::Model { name: String }`.
  - Claude emits it from the `system`/`init` line's `model`.
  - Cursor emits it from `system`/`init`, after `SessionStarted`.
  - Pi emits it from an assistant `message_end`'s `message.model`.
- The controller ignores `Model`, so the bubble is unchanged.

- [ ] **Step 1: Write the failing tests:**
  - `claude_init_reports_the_model` (stream.rs): line `{"type":"system","subtype":"init","model":"claude-opus-5-5","session_id":"s"}` gives `[Model { name: "claude-opus-5-5" }]`.
  - `cursor_init_reports_the_model` (cursor.rs, fixture `cursor-stream.jsonl`): the events contain `Model { name: "Auto" }`.
  - `pi_reports_the_model` (pi.rs, fixture `pi-print.jsonl`): the events contain `Model { name: "prism-ml/bonsai-27b" }`.
- [ ] **Step 2: Run** `cargo test -p erindi-core model`. Expected: FAIL, no variant `Model`.
- [ ] **Step 3: Implement** the variant and the three parsers, and add a `RunEvent::Model { .. } => vec![]` arm in `controller.rs`.
- [ ] **Step 4: Run** `env -u CODEX_HOME cargo test --workspace`. Expected: PASS.
- [ ] **Step 5: Commit** `feat: agents report the model they answer with`.

### Task 3: Every run stores its answer; settings sees the live view

**Files:**
- Modify: `apps/desktop/src-tauri/src/runtime.rs`, `apps/desktop/src-tauri/src/lib.rs`

**Interfaces:**
- Consumes: `Answer`, `History::set_answer` (Task 1); `RunEvent::Model` (Task 2).
- Produces:
  - `fn answer_of(result: Option<(bool, String)>, last_reply: Option<String>, end: &RunEnd, stderr: &str) -> Option<(String, bool)>`. It returns the answer text and whether it failed.
  - The `view` event emitted to `"settings"` as well as to `"overlay"`.

- [ ] **Step 1: Write the failing tests** in `runtime.rs` `mod tests`:
  - `answer_of_a_successful_run_is_its_result`: `(Some((true,"Done")), None, Exited{success:true}, "")` gives `Some(("Done", false))`.
  - `answer_of_an_empty_result_is_the_last_reply`: `(Some((true,"")), Some("Hi"), …)` gives `Some(("Hi", false))`. This is how Pi answers.
  - `answer_of_a_failed_run_is_its_error`: `(Some((false,"Invalid API key")), …)` gives `Some(("Invalid API key", true))`.
  - `answer_of_a_crash_is_stderr`: `(None, None, Exited{success:false}, "boom\n")` gives `Some(("boom", true))`.
  - `answer_of_a_cancelled_run_is_nothing`: `end = Cancelled` gives `None` whatever else.
- [ ] **Step 2: Run** `cargo test -p erindi-desktop answer_of`. Expected: FAIL.
- [ ] **Step 3: Implement.** Write `answer_of`. Then change the `start_run` task:
  - In the `run` callback, keep the last `Result`, the last `Reply` text and the last `Model` name.
  - After the run, call `set_answer` with `model = reported.or(request.model)`. When nothing is reported or requested, use `None`.
  - Emit `sessions-changed`.
  - Also emit `view` to `"settings"` at `runtime.rs:448`.
  - In `lib.rs`, open the settings window with `.inner_size(1120.0, 720.0)`.
- [ ] **Step 4: Run** `env -u CODEX_HOME cargo test --workspace`. Expected: PASS, including the overlay-capability test.
- [ ] **Step 5: Commit** `feat: every agent's answer is saved with its session`.

### Task 4: Frontend types

**Files:**
- Modify: `apps/desktop/src/model.ts`, `apps/desktop/src/model.test.ts`

**Interfaces:**
- Produces:
  - `type Prompt = string | { text: string; raw: string } | { text: string; reply: string; raw?: string; model?: string; failed?: boolean }`.
  - `rawOf(p: Prompt): string | null`, `modelOf(p: Prompt): string | null`, `failedOf(p: Prompt): boolean`.
  - `textOf` and `replyOf` keep their signatures.

- [ ] **Step 1: Write the failing tests:**
  - `an_answer_keeps_raw_model_and_failure`: `rawOf({text:"b",reply:"r",raw:"uh b"}) === "uh b"`, `modelOf(…model:"opus") === "opus"`, `failedOf(…failed:true) === true`, `failedOf("a") === false`.
- [ ] **Step 2: Run** `pnpm -C apps/desktop test`. Expected: FAIL.
- [ ] **Step 3: Implement** in `model.ts`.
- [ ] **Step 4: Run** `pnpm -C apps/desktop test`. Expected: PASS.
- [ ] **Step 5: Commit** `feat: prompts expose what was said, the model and failure`.

### Task 5: Search

**Files:**
- Create: `apps/desktop/src/sessions/search.ts`, `apps/desktop/src/sessions/search.test.ts`
- Modify: `apps/desktop/package.json` test script to `node --test src/*.test.ts src/sessions/*.test.ts`

**Interfaces:**
- Consumes: `Prompt`, `textOf`, `replyOf` (Task 4).
- Produces:
  - `type Options = { query: string; matchCase: boolean; word: boolean; regex: boolean; questions: boolean; answers: boolean; names: boolean }`
  - `type Hit = { turn: number; kind: "q" | "a"; before: string; match: string; after: string }`. `turn` is 1-based.
  - `type Group = { id: string; title: string; titleHit: boolean; hits: Hit[] }`
  - `type Found = { groups: Group[]; total: number } | { error: "regex" } | { error: "nothing" }`
  - `search(sessions: { id: string; prompts: Prompt[] }[], o: Options): Found`
  - `pattern(o: Options): RegExp | null`. It returns `null` for an invalid regex.
- Snippet: 40 characters before and 80 after the match, cut at word boundaries, with `…` where text was cut. The title is the first prompt's text.

- [ ] **Step 1: Write the failing tests:**
  - `substring_search_escapes_regex_characters`: the query `a.b` matches `a.b` and not `axb`.
  - `regex_mode_reads_patterns`: `re?nt(al)?` with `regex` matches `rent` and `rental`.
  - `case_and_word_toggles`: `Rent` with `matchCase` skips `rent`. `rent` with `word` skips `parent`.
  - `invalid_regex_is_reported`: `(` with `regex` gives `{ error: "regex" }`.
  - `no_filter_is_reported`: all three filters false gives `{ error: "nothing" }`.
  - `answers_and_questions_are_told_apart`: a session whose question and answer both match gives hits `[{turn:1,kind:"q"},{turn:1,kind:"a"}]`, and `total === 2`.
  - `names_only_marks_the_title`: `names` only gives `titleHit: true` and `hits: []`.
  - `an_empty_query_finds_nothing`: `query: ""` gives `{ groups: [], total: 0 }`.
- [ ] **Step 2: Run** `pnpm -C apps/desktop test`. Expected: FAIL.
- [ ] **Step 3: Implement** `search.ts`.
- [ ] **Step 4: Run** `pnpm -C apps/desktop test`. Expected: PASS.
- [ ] **Step 5: Commit** `feat: search across every session`.

### Task 6: Status and copy

**Files:**
- Create: `apps/desktop/src/sessions/status.ts`, `apps/desktop/src/sessions/status.test.ts`, `apps/desktop/src/sessions/copy.ts`, `apps/desktop/src/sessions/copy.test.ts`

**Interfaces:**
- Consumes: the `View` type from `bubble.ts`; `Prompt` helpers (Task 4).
- Produces:
  - `type Mark = "waiting" | "speak" | "decode" | "run" | "idle"`
  - `markOf(view: View | null, sessionId: string): Mark`
  - `type Rail = "waiting" | "speak" | "decode" | "queue" | "run" | "ok" | "err" | "gone"`
  - `liveOf(view: View | null, sessionId: string): { rail: Rail; text: string }[]`. These are the live phrases of that session in order. When nothing has been said and the mic is waiting, it is `[{ rail: "waiting", text: "Waiting" }]`.
  - `qaMarkdown(p: Prompt): string`, `answerMarkdown(p: Prompt): string`, `sessionMarkdown(title: string, prompts: Prompt[]): string`

- [ ] **Step 1: Write the failing tests:**
  - `other_sessions_are_idle`: `markOf(view with sessionId "a", "b") === "idle"`.
  - Marks, in priority order:
    - a running or cancelling phrase gives `"run"`;
    - speaking gives `"speak"`;
    - transcribing or classifying gives `"decode"`;
    - `mic: "waiting"` with no live phrase gives `"waiting"`;
    - otherwise `"idle"`.
  - `live_phrases_map_to_rails`: the statuses queued, failed and cancelled map to `"queue"`, `"err"` and `"gone"`.
  - `qa_markdown`: `{text:"Q",reply:"A"}` gives `"**You:** Q\n\nA\n"`.
  - `session_markdown`: two prompts give `"# T\n\n**You:** Q1\n\nA1\n\n---\n\n**You:** Q2\n"`. A prompt without an answer has no answer part.
- [ ] **Step 2: Run** `pnpm -C apps/desktop test`. Expected: FAIL.
- [ ] **Step 3: Implement** `status.ts` and `copy.ts`.
- [ ] **Step 4: Run** `pnpm -C apps/desktop test`. Expected: PASS.
- [ ] **Step 5: Commit** `feat: session marks, live rails and markdown copies`.

### Task 7: Session list

**Files:**
- Create:
  - `apps/desktop/src/sessions/Mark.tsx`: the five SVG marks.
  - `apps/desktop/src/sessions/Menu.tsx`: the ⋯ button and menu, the Preview switch, the delete confirm.
  - `apps/desktop/src/sessions/List.tsx`: rows, the Active chip, the details tooltip, the loading bars, the empty state, the error toast.
- Modify:
  - `apps/desktop/src/sessions.tsx`: becomes the two-pane container that holds the state.
  - `apps/desktop/src/style.css`: rails, marks, animations.

**Interfaces:**
- Consumes: `markOf`, `liveOf` (Task 6); `Mark`, `Rail` types.
- Produces:
  - `<SessionMark mark={Mark} />`
  - `<MoreMenu items={MenuItem[]} label={string} />`, where `type MenuItem = { label: string; hint?: string; onSelect?: () => void; disabled?: boolean; danger?: boolean; confirm?: boolean } | "separator" | { switch: string; on: boolean; onToggle: () => void }`
  - `<SessionList entries selected active view onOpen onAction />`

Build to screens 2, 3, 8, 13, 17, 25. The rows follow spec § Session list. Menus and Delete follow the spec's ⋯ table and the existing 3 s `CONFIRM_MS` behaviour. Make active and Open in terminal call the existing `continue_session` and `open_history_session`.

- [ ] **Step 1: Implement** the components. No visible element may change size with state.
- [ ] **Step 2: Run** `pnpm -C apps/desktop build`. Expected: no type errors.
- [ ] **Step 3: Check by hand** against screens 2, 3, 8, 13, 17, 25, in both themes, at 1120 and 640 px.
- [ ] **Step 4: Commit** `feat: session list with status marks and actions`.

### Task 8: Search UI

**Files:**
- Create: `apps/desktop/src/sessions/SearchBox.tsx`, `apps/desktop/src/sessions/Results.tsx`
- Modify: `apps/desktop/src/sessions.tsx`

**Interfaces:**
- Consumes: `search`, `Options`, `Found` (Task 5); `MoreMenu`, `SessionMark` (Task 7).
- Produces:
  - `<SearchBox options onChange />`: the field, ✕, Aa / ab / .\* toggles with `aria-pressed`, and Filter ▾ with its checkbox menu, count and Reset.
  - `<Results found onOpen(id, turn) />`

Build to screens 1, 18, 19, 20. Search runs 200 ms after the last keystroke. Results use the 68 px label column and the 4 px gap. Copy is exactly as in the spec: "N matches in M sessions", "No matches", "Turn off Aa, ab or .* to widen the search.", "Invalid regular expression", "Nothing to search in", "Pick Questions, Answers or Session names in Filter.".

- [ ] **Step 1: Implement.**
- [ ] **Step 2: Run** `pnpm -C apps/desktop build`. Expected: no type errors.
- [ ] **Step 3: Check by hand** against screens 1, 18, 19, 20.
- [ ] **Step 4: Commit** `feat: search field, filters and results`.

### Task 9: Conversation pane

**Files:**
- Create: `apps/desktop/src/sessions/Conversation.tsx` (header, details line, turns, copy buttons, ‹ › navigation, map strip, live phrases, the "No session selected" state)
- Modify: `apps/desktop/src/sessions.tsx`, `apps/desktop/src/style.css`

**Interfaces:**
- Consumes: `markdown`, `drawDiagrams`; `qaMarkdown`, `answerMarkdown`, `sessionMarkdown`, `liveOf` (Task 6); `rawOf`, `modelOf`, `failedOf` (Task 4); `MoreMenu` (Task 7).
- Produces: `<Conversation entry view matches focus onFocus onBack narrow />`

Build to screens 4–7, 9–12, 15, 16, 21–24, 26, 27, and follow spec § Conversation pane:
- The current turn has an inset outline. Matches are highlighted inside it.
- ‹ › and ↑ ↓ step between matches, or between questions when not searching.
- Turns use `content-visibility: auto`.
- The model shows after the agent name in grey.
- "Said: …" shows under a cleaned-up question.
- "No answer saved for this turn." shows on a turn without an answer.
- Copy buttons show "✓ Copied" for 1.5 s. Copy session shows its note for 1.5 s.
- The map strip has one tick per question, a red tick for a failed answer, and a hover hint "98 · question".
- The streamed reply comes from the existing `session-reply` event.
- Below 900 px the pane replaces the list, with "← Sessions"; Esc goes back.

- [ ] **Step 1: Implement.**
- [ ] **Step 2: Run** `pnpm -C apps/desktop build`. Expected: no type errors.
- [ ] **Step 3: Check by hand** against the listed screens in both themes. Include a session of 200+ turns and check that scrolling stays smooth.
- [ ] **Step 4: Commit** `feat: read-only conversation beside the list`.

### Task 10: Keyboard, themes and the final check

**Files:**
- Modify: `apps/desktop/src/sessions.tsx`, `apps/desktop/src/sessions/*.tsx`, `apps/desktop/src/style.css`

Implement spec § Keyboard:
- ↑ ↓ move through the list, Enter opens, Esc closes or goes back, Enter or Space opens ⋯.
- Every control has a visible focus ring.
- The menu takes focus when it opens and gives it back to ⋯ when it closes.

Also check the light-theme colours from spec § Themes, including ⋯ at `#e0e0e0` on hover and `#d0d0d0` while open.

- [ ] **Step 1: Implement** the keyboard handling and any missing light-theme classes.
- [ ] **Step 2: Run** `pnpm -C apps/desktop test && pnpm -C apps/desktop build && env -u CODEX_HOME cargo test --workspace`. Expected: all PASS.
- [ ] **Step 3: Take Before / After screenshots** of Sessions (list, search, conversation, narrow, light), and write a user path for each.
- [ ] **Step 4: Commit** `feat: sessions keyboard and light theme`.
