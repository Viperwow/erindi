# Listening Mode and Phrase Queue Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Hands-free listening keeps going phrase after phrase, and phrases finished while the agent works wait in a queue and run one by one.

**Architecture:** The single `Machine` in `crates/core/src/state.rs` goes away. The controller keeps a microphone track (an optional open capture with its current segment), a list of phrases (`Series`) whose `Queued` entries are the queue, and an agent track derived from the one phrase that is `Classifying`, `Running` or `Cancelling`. The runtime keeps the capture open for the whole listening mode and resets the VAD after each phrase. The overlay renders the phrase list.

**Tech Stack:** Rust (`crates/core`, `crates/audio-asr`, Tauri 2 runtime), Preact + TS overlay, `node --test` for the bubble model.

**Spec:** `docs/superpowers/specs/2026-09-29-continuous-listening-queue-design.md` (Decisions, "Listening mode and queue (PR 2)", Errors, Testing). Visual reference: `docs/superpowers/specs/2026-09-29-overlay-states.html`.

**Branch:** `feat/listening-queue`, cut from `feat/overlay-bubble` (PR #23).

## Global Constraints

- Everything in `crates/core` stays free of Tauri and is tested without the app.
- Hold mode keeps today's behaviour: capture while held, one phrase on release, then to the queue or straight to the agent.
- A double press of the talk key turns listening on and off. Every pause ends a phrase; long silence does not turn listening off.
- "Double-press while hands-free sends now" goes away.
- Single press cancels by priority: the phrase being spoken, otherwise the running agent. The queue and listening stay. With nothing running it does nothing.
- One queue for every input; queued phrases run one by one in the active session; commands in a queued phrase apply when it leaves the queue.
- After a cancelled or failed run, the next queued phrase starts at once.
- Turning listening off does not cancel the running agent or the queue.
- The last three finished phrases stay while a series goes on; they clear when a new phrase starts after the agent went idle with an empty queue.
- Final transcriptions run one at a time, in the order phrases were said. The live transcript shows only the phrase being spoken and waits while a final transcription runs.
- Copy, verbatim: "Couldn't transcribe this phrase"; "Microphone unavailable · check the microphone in Settings"; "Speech model failed to load · open Settings to download it again".
- Commits: Conventional Commits, header ≤ 100 chars, no AI attribution trailers. Code comments only for non-obvious logic.

## Review Focus

1. Hands-free with a long silence: the buffer must not grow toward `MAX_RECORDING`; audio before the first speech is trimmed to a one-second pre-roll. Pinned in Task 4.
2. A tap to cancel a run while the microphone is off: the tap first opens a hold capture of its own, and that capture must not take the cancel. Pinned in Task 3.
3. A phrase dropped by a single press while its final transcription runs: its late result is ignored, and the next waiting transcription still starts. Pinned in Task 4.
4. A run end arriving for a phrase that is no longer the agent's (stale), and a transcription result carrying a run's op: neither changes the other. Pinned in Task 3.
5. Holding the key while listening mode is on: it must not open a second capture or cut the phrase. Pinned in Task 4.

---

### Task 1: The VAD resets per phrase; "no speech" goes away

**Files:**
- Modify: `crates/audio-asr/src/vad.rs`
- Modify: `apps/desktop/src-tauri/src/runtime.rs` (capture loop, ~lines 462–495)

**Interfaces:**
- Produces: `Endpointer::reset(&mut self)`; `Endpoint` has only `Continue` and `SpeechEnded`. `Msg::NoSpeech` is removed in Task 3; this task stops sending it.

- [ ] **Step 1: Write the failing test** in `vad.rs` tests, `#[ignore = "needs models/"]` like its neighbours:

```rust
#[test]
#[ignore = "needs models/"]
fn reset_waits_for_the_next_phrase() {
    let mut e = Endpointer::new(&models_dir(), Duration::from_secs(1)).unwrap();
    let mut audio = speech();
    audio.extend(vec![0.0; 2 * TARGET_RATE as usize]);
    let mut ends = 0;
    for chunk in [audio.clone(), audio].concat().chunks(480) {
        if e.push(chunk) == Endpoint::SpeechEnded {
            ends += 1;
            e.reset();
        }
    }
    assert_eq!(ends, 2);
}
```

- [ ] **Step 2: Run it and see it fail to compile** — `cargo test -p erindi-audio-asr -- --ignored reset_waits` → error: no method `reset`.
- [ ] **Step 3: Implement.** `reset` calls `self.vad.reset()` and zeroes `heard` and `samples`. Delete `Endpoint::NoSpeech`, `NO_SPEECH_TIMEOUT` and the `no_speech` test. In the runtime capture loop drop the `ended` latch: on `SpeechEnded` send `Msg::SpeechEnded { op }` and call `reset()` on the endpointer.
- [ ] **Step 4: Run** `cargo test -p erindi-audio-asr -- --include-ignored` → PASS (models present locally), and `cargo build -p erindi-desktop` compiles (`Msg::NoSpeech` still exists, unused by the runtime).
- [ ] **Step 5: Commit** `feat(audio): the endpointer resets for the next phrase`.

---

### Task 2: `Series`, the phrase list that holds the queue

**Files:**
- Create: `crates/core/src/series.rs`; register in `crates/core/src/lib.rs`
- Test: same file

**Interfaces:**
- Produces (all `pub`, serde `rename_all = "camelCase"` on the serialized types):

```rust
pub type PhraseId = u64;
pub enum Status { Speaking, Transcribing, Queued, Classifying, Running, Cancelling, Done, Failed, Cancelled }
pub enum Kind { Speech, Terminal }            // Terminal: the terminal hotkey pressed while the agent is busy
pub struct Phrase {
    pub id: PhraseId, pub kind: Kind, pub status: Status,
    pub text: String, pub outcome: String,
    #[serde(skip)] pub new_session: bool,     // said with the new-session key
}
pub const KEEP_FINISHED: usize = 3;
pub struct Series { /* phrases in creation order, next id, series id */ }
impl Series {
    pub fn start(&mut self, kind: Kind, status: Status, new_session: bool) -> PhraseId;
    pub fn get(&self, id: PhraseId) -> Option<&Phrase>;
    pub fn get_mut(&mut self, id: PhraseId) -> Option<&mut Phrase>;
    pub fn remove(&mut self, id: PhraseId);
    pub fn next_queued(&self) -> Option<PhraseId>;
    pub fn agent(&self) -> Option<&Phrase>;   // the phrase that is Classifying, Running or Cancelling
    pub fn finish(&mut self, id: PhraseId, status: Status, outcome: String);
    pub fn active(&self) -> bool;             // any phrase not Done, Failed or Cancelled
    pub fn clear_finished(&mut self);         // bumps the series id when it removes anything
    pub fn id(&self) -> u64;
    pub fn phrases(&self) -> &[Phrase];
}
```

`Done`, `Failed` and `Cancelled` are the finished statuses.

- [ ] **Step 1: Write the failing tests:**

```rust
#[test] fn queue_is_first_queued_in_order()        // start three Queued; next_queued is the first; after finish(first, Done) it is the second
#[test] fn keeps_only_the_last_three_finished()     // finish four phrases; phrases() keeps the newest three finished, in order
#[test] fn a_new_phrase_after_an_idle_series_clears_it() // two Done, no active; start(...) leaves only the new phrase and bumps id()
#[test] fn a_new_phrase_during_a_series_keeps_it()  // one Running, one Done; start(...) keeps all three, id() unchanged
#[test] fn agent_is_the_classifying_running_or_cancelling_phrase()
```

- [ ] **Step 2: Run** `cargo test -p erindi-core series` → FAIL (module missing).
- [ ] **Step 3: Implement.** `start` calls `clear_finished` first when `!self.active()`. `finish` sets status and outcome, then removes the oldest finished phrases beyond `KEEP_FINISHED`.
- [ ] **Step 4: Run** `cargo test -p erindi-core series` → PASS.
- [ ] **Step 5: Commit** `feat(core): phrase series with the queue and the last finished phrases`.

---

### Task 3: The agent track and the queue replace the state machine

**Files:**
- Modify: `crates/core/src/controller.rs` (whole non-test part, and every test)
- Delete: `Machine`, `Event`, `Outcome`, `InvalidTransition` and `AppState` from `crates/core/src/state.rs`; keep `OpId` (move it to `series.rs` or keep `state.rs` with `OpId` only)

**Interfaces:**
- Consumes: `Series`, `Phrase`, `Status`, `Kind`, `PhraseId` from Task 2.
- Produces for Task 4 and Task 5:

```rust
pub enum Model { Loading, Missing, Ready, Failed }
#[serde(rename_all = "camelCase")] pub enum Mic { Off, Waiting, Listening, Error }
pub struct View {                 // serde rename_all = "camelCase"
    pub series: u64,              // Series::id(); the runtime keys dismiss and hover on it
    pub visible: bool,            // model not Loading/Missing, and capture open, phrases present or a global error
    pub idle: bool,               // no capture and no active phrase: the runtime may schedule Dismiss
    pub mic: Mic,
    pub transcribing: bool,       // a final transcription is in flight
    pub phrases: Vec<Phrase>,
    pub detail: String,           // the running agent's current tool
    pub agent: Agent,
    pub limited: bool,
    pub session_id: Option<Uuid>, // the active session, for the click
    pub global_error: Option<String>,
}
```

  Message and effect ops: capture messages (`Audio`, `Speaking`, `SpeechEnded`, `MicFailed`) carry the **capture op**, a counter bumped per `StartCapture`. Everything about one phrase (`Live`, `Transcribed`, `Classified`, `Run`, `RunExited`, `Failed`, and the effects `LiveDecode`, `Transcribe`, `Classify`, `StartRun`) carries its `PhraseId` as `op`. New: `Msg::MicFailed { op, error }`; `Msg::Dismiss { series: u64 }` replaces `Dismiss { op }`; `Msg::NoSpeech` is removed. `Controller::state()` is replaced by `Controller::view(&self) -> &View`.

  Rulings this task carries, from the spec's two-counter rule: phrase ids are unique for the app's lifetime, so a run keyed by its phrase id cannot be taken for a transcription, because `Transcribed` acts only on a phrase in `Transcribing` and `RunExited` only on the agent phrase.

- [ ] **Step 1: Write the failing tests** (the listening-mode tests are Task 4). Helpers on `T`: `statuses(&self) -> Vec<Status>`, `say(&mut self, text) -> PhraseId` (hold, release, `Transcribed`), `finish_run(&mut self, op, ok)`.

```rust
#[test] fn a_phrase_said_during_a_run_waits_in_the_queue()   // say A; say B while A runs: statuses [Running, Queued]; only one StartRun so far
#[test] fn queued_phrases_run_one_by_one_in_order()          // after A's RunExited ok: [Done, Running]; StartRun prompt is B's text
#[test] fn after_a_failed_run_the_next_phrase_starts()       // A fails: [Failed, Running]; A's outcome is the failure detail
#[test] fn single_press_cancels_the_run_and_the_next_starts() // tap: CancelRun, [Cancelling, Queued]; RunExited Cancelled: [Cancelled, Running]
#[test] fn a_tap_during_a_run_is_not_taken_by_its_own_capture() // mic off, A runs: tap(Talk) → CancelRun, no Transcribe, mic Off
#[test] fn single_press_with_nothing_running_does_nothing()  // tap on an idle controller: no effects except capture start/stop
#[test] fn commands_apply_when_the_phrase_leaves_the_queue() // A runs; say "новая сессия проверь diff"; SetActive changes the active session before A ends; B's StartRun uses Session::New
#[test] fn terminal_key_during_a_run_waits_in_the_queue()    // A runs: KeyDown(Terminal) → no OpenTerminal, statuses [Running, Queued]; after A ends → OpenTerminal, [Done, Done]
#[test] fn cancel_at_the_end_of_a_phrase_drops_it()          // "проверь diff отмена" (whatever Patterns::default() maps to Command::Cancel) never reaches the queue
#[test] fn a_stale_run_end_changes_nothing()                  // RunExited with A's id after A finished, while B runs: B still Running
#[test] fn a_transcription_result_with_a_run_op_is_ignored()  // Transcribed { op: running phrase id } changes nothing
#[test] fn succeeded_does_not_wait_for_dismiss()              // A ends ok: a new hold phrase starts at once and runs
#[test] fn dismiss_clears_an_idle_series()                     // after A Done: Dismiss { series } → phrases empty, visible false; a stale series id does nothing
```

- [ ] **Step 2: Run** `cargo test -p erindi-core controller` → FAIL (compile errors on the new API).
- [ ] **Step 3: Implement.** Controller fields that replace `machine`, `view.state`, `hands_free`, `mode`, `buffer`, `pending`, `running`, `result`:
  - `model: Model`, `series: Series`, `capture: Option<Capture>` with `struct Capture { op: OpId, key: Key, hands_free: bool, phrase: Option<PhraseId>, buffer: Vec<f32> }`, `capture_op: OpId`, `speaking: bool`, `global_error: Option<String>`, and per-agent-phrase `result`, `running: Option<(Session, String, Agent)>`, `pending: Option<String>`.
  - `fn pump(&mut self, now: Instant) -> Vec<Effect>`: while `series.agent()` is `None` and `series.next_queued()` is `Some(id)`: a `Terminal` phrase opens the terminal and finishes `Done`; a speech phrase is parsed and goes through today's classify-or-`act` path, which now marks it `Classifying` or `Running`, or finishes it (`Done` for a terminal run, removed when nothing is left to send). Called after every `Transcribed`, `Classified`, `RunExited` and `Failed`.
  - `RunExited`: acts only when `op == series.agent().id`. `Cancelling` finishes `Cancelled`; otherwise `Done` or `Failed` with today's detail as the outcome; `remember` runs as today; then `pump`.
  - Single press, in order: a hold capture that is still open belongs to this tap, so stop and discard it silently and go on; then the phrase in the open hands-free segment (Task 4); then the newest `Transcribing` phrase (remove it); then the agent phrase (`Classifying` finishes `Cancelled` and pumps, `Running` sends `CancelRun` and becomes `Cancelling`). Remove today's instant cancel on `KeyDown` outside listening: a press during a run now waits for `DOUBLE`, because a double press there turns listening on.
  - Hold: `KeyDown` with no capture starts one (`StartCapture { op }` with a fresh capture op) and a `Speaking` phrase for it; a long `KeyUp` of the same key ends it: `StopCapture`, the phrase becomes `Transcribing` and goes to transcription.
  - `KeyDown` while `model` is `Missing` or `Failed` sends `OpenSettings`; while `Loading` it does nothing.
  - `Transcribed`: transform the text; empty or containing `Command::Cancel` removes the phrase; otherwise the phrase becomes `Queued` with that text, then `pump`.
  - `Failed { op }` on a `Transcribing` phrase finishes it `Failed` with outcome "Couldn't transcribe this phrase"; on the agent phrase, `Failed` with the error, then `pump`.
  - `ModelFailed` sets `global_error` to "Speech model failed to load · open Settings to download it again".
  - Terminal key: with no agent phrase and nothing `Queued`, open now as today; otherwise `series.start(Kind::Terminal, Status::Queued, false)` with text "Open in terminal".
  - `Dismiss { series }`: when `series == self.series.id()` and the view is idle, `clear_finished` and clear `global_error`.
  - Every change ends with `Effect::Show(self.view())`, built fresh from the fields.
- [ ] **Step 4: Migrate every existing controller test** to the new API. Map old asserts: `Idle` → `statuses()` has no active phrase and `mic` is `Off`; `Listening` → `mic != Off`; `Transcribing`/`Running`/`Cancelling` → the newest phrase has that status; `Succeeded` → `Done`; `Failed` → `Failed`. Delete tests whose behaviour the spec removes: "double press while hands-free sends now", "press cancels at once outside listening", `NoSpeech` handling, `Succeeded`/`Failed` waiting for dismiss.
- [ ] **Step 5: Run** `cargo test -p erindi-core` → all PASS. (`erindi-desktop` does not compile until Task 5.)
- [ ] **Step 6: Commit** `feat(core): phrases queue up and run one by one`.

---

### Task 4: Listening mode keeps listening

**Files:**
- Modify: `crates/core/src/controller.rs`

**Interfaces:**
- Consumes: `Capture`, `Series`, `pump` from Task 3.
- Produces: `pub const PRE_ROLL: usize = 16_000;` (one second of 16 kHz audio kept before the first speech of a segment).

- [ ] **Step 1: Write the failing tests:**

```rust
#[test] fn double_press_turns_listening_on_and_off()           // double(Talk): capture open, hands_free; double(Talk) again: StopCapture, mic Off
#[test] fn a_pause_sends_the_phrase_and_listening_goes_on()    // hands-free; Speaking true; Audio; SpeechEnded → Transcribe for that phrase, no StopCapture; Speaking true again starts a new Speaking phrase
#[test] fn a_phrase_finished_while_the_agent_runs_joins_the_queue() // A runs; hands-free phrase transcribed → [Running, Queued]
#[test] fn transcriptions_run_one_at_a_time_in_order()         // two SpeechEnded before any Transcribed → one Transcribe; after the first Transcribed, the second Transcribe
#[test] fn a_dropped_phrase_still_lets_the_next_transcription_start() // phrase 1 transcribing, phrase 2 waiting; tap drops phrase 1; its late Transcribed is ignored and phrase 2's Transcribe goes out
#[test] fn single_press_drops_the_phrase_being_spoken()        // hands-free, Speaking true, Audio; tap → phrase removed, capture still open, a run in progress keeps running
#[test] fn silence_before_speech_keeps_only_the_pre_roll()     // hands-free, no Speaking; Audio of 3 * PRE_ROLL samples; then Speaking true + SpeechEnded → Transcribe samples.len() <= PRE_ROLL + the audio after Speaking
#[test] fn holding_the_key_while_listening_does_nothing()       // hands-free; down(Talk), release after HOLD → no StartCapture, no StopCapture, no Transcribe
#[test] fn turning_listening_off_leaves_the_run_and_queue()     // A runs, B queued; double press off → [Running, Queued], no CancelRun; a segment with speech is sent to transcription, one without is dropped
#[test] fn live_decode_is_for_the_phrase_being_spoken_and_waits_for_a_final_one() // LiveDecode op is the Speaking phrase; none while a Transcribe is in flight
#[test] fn microphone_failure_turns_listening_off_only()        // A runs; MicFailed → StopCapture, mic Error, global_error "Microphone unavailable · check the microphone in Settings", A still Running
```

- [ ] **Step 2: Run** `cargo test -p erindi-core controller` → the new tests FAIL.
- [ ] **Step 3: Implement.**
  - Double press with no hands-free capture: the capture opened by the first press becomes `hands_free`; its phrase is dropped if empty, and the segment's phrase is created later by speech. Double press with a hands-free capture: `StopCapture`; the segment's phrase goes to transcription when it heard speech, otherwise it is removed.
  - `Speaking { op, speaking: true }` in a hands-free capture with no segment phrase starts one: `series.start(Kind::Speech, Status::Speaking, key == Key::NewSession)`.
  - `SpeechEnded` in a hands-free capture with a segment phrase cuts it: the phrase becomes `Transcribing`, its buffer goes to transcription, and the segment restarts with no phrase and an empty buffer. Ignored in hold mode.
  - `Audio` with no segment phrase keeps only the last `PRE_ROLL` samples. `MAX_RECORDING` ends a hold phrase as a release does, and cuts a hands-free phrase as a pause does.
  - Transcriptions: `decoding: Option<PhraseId>` and `waiting: VecDeque<(PhraseId, Vec<f32>)>`. A phrase sent to transcription goes out at once when `decoding` is `None`, otherwise it waits. Every `Transcribed` or `Failed` for the decoding id — whether or not its phrase still exists — sends the next waiting one.
  - Live decode: only for the segment phrase, only while `speaking`, `decoding` is `None`, nothing live in flight, and `LIVE_INTERVAL` has passed.
  - `KeyDown` or a long `KeyUp` while a hands-free capture is open starts and ends nothing; the gesture logic for taps and double presses still runs.
  - `MicFailed { op }` for the current capture: drop the capture and its segment phrase, send `StopCapture`, set `global_error`. The next capture that starts clears it.
- [ ] **Step 4: Run** `cargo test -p erindi-core` → all PASS.
- [ ] **Step 5: Commit** `feat(core): listening mode keeps listening after each phrase`.

---

### Task 5: Runtime, overlay and copy

**Files:**
- Modify: `apps/desktop/src-tauri/src/runtime.rs` (`Effect::Show`, capture errors, `StartCapture`)
- Modify: `apps/desktop/src-tauri/src/overlay.rs` (`HEIGHT`)
- Modify: `apps/desktop/src/bubble.ts`, `apps/desktop/src/bubble.test.ts`, `apps/desktop/src/overlay.tsx`
- Modify: `apps/desktop/src/settings.tsx:198,219-221`, `README.md:22`

**Interfaces:**
- Consumes: `View`, `Mic`, `Phrase`, `Status`, `Kind`, `Msg::MicFailed`, `Msg::Dismiss { series }` from Task 3.
- Produces: TS `View` mirroring the Rust one (camelCase), `bubble(view: View): Bubble` with today's `Bubble` shape.

- [ ] **Step 1: Write the failing bubble tests** in `bubble.test.ts`:

```ts
test("rails follow each phrase's status")          // done, running, queued, transcribing, speaking → rails ok, run, wait, decode, speak, in that order
test("a speaking phrase without words has no row")
test("the agent's work fills the right slot")      // running phrase: { icon: "terminal", text: "Codex is working · Bash cargo test", dots: false }
test("transcribing shows when the agent is idle")  // transcribing and no agent phrase: pencil "Transcribing", strip "decode" when mic waiting
test("listening while transcribing keeps the mic label") // mic listening + transcribing: mic "listening", strip "speak", pencil "Transcribing"
test("queued terminal reads Open in terminal")
test("clickable only when the agent is idle and the queue is empty") // Done + sessionId → clickable; Done + Queued → not
test("a global error keeps the phrases")           // globalError set, one Running phrase → both shown, strip "error"
test("cancelled phrase is struck through")          // status cancelled → rail "gone"
```

- [ ] **Step 2: Run** `pnpm test` in `apps/desktop` → FAIL.
- [ ] **Step 3: Implement `bubble.ts`.** Rails: speaking→speak (only with text), transcribing→decode, queued→wait, classifying→decode, running and cancelling→run, done→ok, failed→err, cancelled→gone. Outcomes: done "Done · {outcome}" or "Done"; failed "{outcome}" or "Failed"; cancelled "Cancelled". Right slot: the agent phrase (`running` → "{label} is working" · detail or "limited mode"; `cancelling` → "Cancelling"; `classifying` → pencil "Checking command"), otherwise pencil "Transcribing" when `transcribing`. Strip: mic error→error, listening→speak, transcribing→decode, waiting→idle, off→off. Click hint only on the newest finished phrase when clickable.
- [ ] **Step 4: Run** `pnpm exec tsc --noEmit && pnpm test` → PASS.
- [ ] **Step 5: Wire the runtime.** `Effect::Show`: show the overlay when `view.visible`, else hide; key the hover tracker on `view.series`; when `view.visible && view.idle`, send `Msg::Dismiss { series: view.series }` after `DISMISS_AFTER`. Capture start errors send `Msg::MicFailed`. `overlay.tsx` renders all rows from `bubble(view)` (no other change). `overlay.rs`: `HEIGHT` 360, and update `height_scales_with_the_monitor` to 360 / 540 / 720.
- [ ] **Step 6: Copy.** `settings.tsx` hint: "A pause this long sends the phrase in hands-free mode." Help list: keep the hold line; replace the two double-press lines with "Double-press: hands-free listening on or off; each pause sends a phrase." and add "Press once: cancel the phrase you are saying, otherwise the running agent." if no such line exists. `README.md:22`: "- **Talk from any app.** Hold a hotkey to talk, or double-press it for hands-free listening: each pause sends a phrase, and phrases said while the agent works wait in a queue."
- [ ] **Step 7: Run** `cargo test -p erindi-core -p erindi-desktop -p erindi-audio-asr`, `cargo clippy --all-targets`, `cargo fmt --all --check`, `pnpm exec tsc --noEmit`, `pnpm test` → all PASS.
- [ ] **Step 8: Commit** `feat(desktop): the overlay shows the queue; listening stays open across phrases`, then build `pnpm tauri build --no-bundle` for the manual check.

**Manual checklist (goes into the PR description):** hold to talk while idle and while the agent runs (queues); double-press on, say three phrases with pauses while the first runs; single press while speaking, while transcribing, during a run; double-press off during a run; the terminal hotkey during a run; unplug the microphone while listening.
