# Overlay Bubble Implementation Plan (PR 1)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the full-screen aurora overlay with the approved bubble — microphone strip, phrase rails, bottom status row, tooltips — driven by today's controller states.

**Architecture:** A pure TypeScript function turns the `View` Rust already emits (plus the audio level) into a small bubble model; Preact components render that model. Rust changes only where the window must let the mouse reach the bubble: hover tracking runs whenever the overlay is shown, and the frontend reports the bubble's rectangle instead of Rust guessing it.

**Tech Stack:** Preact + TypeScript + Tailwind v4 (`apps/desktop/src`), `node --test` for pure TS, Tauri 2 + Rust (`apps/desktop/src-tauri`).

**Spec:** `docs/superpowers/specs/2026-09-29-continuous-listening-queue-design.md` (section "Overlay bubble (PR 1)"), reference `docs/superpowers/specs/2026-09-29-overlay-states.html` — open it in a browser; every class and colour below comes from it.

## Global Constraints

- Phrase text is always neutral: live phrases (the agent's task, the phrase being spoken or transcribed) white; queued and every finished phrase muted (`rgba(255,255,255,.62)`, 12px). No coloured text, no icons in phrase rows.
- Rails (2px, fixed 11px left gutter): amber `#f59e0b` agent works on it · blue `#38bdf8` being spoken · violet `#a855f7` being transcribed · dashed grey queued · muted green `rgba(74,222,128,.55)` done · red `#f87171` error · light grey `rgba(255,255,255,.22)` cancelled (text struck through).
- Strip (2px, inside the bubble, clipped to its bottom corners) shows the microphone only: waiting `#06b6d4→#3b82f6` at `brightness(.55)`, 6s · hearing speech same colours, 1.6s · transcribing `#a855f7→#6366f1` · off `rgba(255,255,255,.08)`, still · error `#ef4444→#f43f5e`, still.
- Bottom row, all icons and labels grey `rgba(255,255,255,.62)`: the microphone always on the left — "Waiting", "Listening" (hearing speech), "Mic off" (crossed-out mic), "Mic unavailable" (crossed-out mic, light text `rgba(255,255,255,.9)`); running work on the right with an action icon, no dot — pencil + "Transcribing", terminal + "{Agent} is working · {detail}" / "Cancelling". "Listening" and "Transcribing" end with three dots appearing one by one in a fixed-width slot (`.ell`, `@keyframes ell` over `content`), static "..." with reduced motion.
- A global error (failure with no phrase) replaces the microphone label: crossed-out mic on the left, one white line centred in the row (grid `15px 1fr 15px`); the strip is red. No red dot.
- Tooltips: a finished or failed phrase always has one (still dot + outcome or reason, a divider, the full phrase, and "Click to open in terminal" when clickable). Any other line gets one only when it really overflows. A line without a tooltip does not react to the mouse.
- Icons centre on the capital letters of their own line: `align-self: baseline; transform: translateY(calc((var(--s) - 1cap) / 2))`, with the row's own font size (`--s: 15px` for mic/pencil/terminal, `13px` for the still dots in tooltips). The pencil's `viewBox` is `-3 -3 30 30` so it reads the same size as the microphone.
- The bubble is clickable only for a finished run with a session (today: `Succeeded` or `Failed` with `sessionId`).
- No marker next to the active phrase, no shimmer, no level bars, no pulsing dots.

## Review Focus

- The cursor hovers the bubble while a result auto-dismisses (8 s): the tooltip must disappear with the bubble, and the window must go back to ignoring the cursor. → Task 3 test `hover_stops_when_the_op_changes`.
- A very long phrase with no spaces (a path or URL): it must truncate with an ellipsis, not widen the bubble. → Task 1 test `long unbroken text stays one phrase` and the `min-width:0` / `overflow-wrap:anywhere` rules in Task 2.
- A failure before any text exists (microphone can't open): no empty phrase row, a global error line next to the crossed-out mic and a red strip. → Task 1 test `failure without a phrase is a global error`.
- Codex in limited mode: the working status says so, and a finished limited run's click hint says "Click to open in Codex and trust". → Task 1 tests `limited mode shows in the status` and `limited click hint`.
- Two monitors with different DPI: the hover rectangle must use the overlay window's own scale factor. → Task 3 test `inside_uses_the_window_scale`.

---

### Task 1: Bubble model

**Files:**
- Create: `apps/desktop/src/bubble.ts`
- Test: `apps/desktop/src/bubble.test.ts`

**Interfaces:**
- Consumes: `View` (move the `View` type from `overlay.tsx` into `bubble.ts` and export it), `agentLabels` from `controls.tsx`.
- Produces (exported from `bubble.ts`):
  ```ts
  export type Rail = "run" | "speak" | "decode" | "wait" | "ok" | "err" | "gone";
  export type Phrase = { text: string; rail: Rail; outcome: string | null; clickHint: string | null; newest: boolean };
  export type Running = { icon: "terminal" | "pencil"; text: string; dots: boolean } | null;
  export type Mic = "waiting" | "listening" | "off" | "error";
  export type Strip = "idle" | "speak" | "decode" | "off" | "error";
  export type Bubble = { phrases: Phrase[]; running: Running; globalError: string | null; mic: Mic; strip: Strip; clickable: boolean };
  // `Phrase.newest` is kept for PR 2; on today's logic every phrase is the newest.
  export const SPEECH_LEVEL = 0.02;
  export function bubble(view: View, level: number): Bubble;
  ```

- [ ] **Step 1: Write the failing tests** in `bubble.test.ts` (style of `time.test.ts`; build views with a small `view(overrides)` helper defaulting to `{ op: 1, state: "Idle", text: "", detail: "", sessionId: null, continued: false, agent: "claude", limited: false }`):

  ```ts
  test("listening in silence waits", () => {
    const b = bubble(view({ state: "Listening" }), 0);
    assert.deepEqual([b.strip, b.mic, b.running, b.phrases.length], ["idle", "waiting", null, 0]);
  });
  test("speech above the level speaks", () => {
    const b = bubble(view({ state: "Listening", text: "проверь diff" }), SPEECH_LEVEL);
    assert.deepEqual([b.strip, b.mic, b.phrases[0].rail], ["speak", "listening", "speak"]);
  });
  test("transcribing shows the pencil", () => {
    const b = bubble(view({ state: "Transcribing", text: "проверь diff" }), 0);
    assert.deepEqual([b.strip, b.mic, b.running, b.phrases[0].rail], ["decode", "off", { icon: "pencil", text: "Transcribing", dots: true }, "decode"]);
  });
  test("classifying reads as transcribing", () => {
    assert.deepEqual(bubble(view({ state: "Classifying", text: "x" }), 0).running, { icon: "pencil", text: "Checking command", dots: true });
  });
  test("running shows the agent and the tool", () => {
    const b = bubble(view({ state: "Running", text: "x", detail: "Bash cargo test", agent: "codex" }), 0);
    assert.deepEqual([b.running, b.mic, b.strip, b.phrases[0].rail], [{ icon: "terminal", text: "Codex is working · Bash cargo test", dots: false }, "off", "off", "run"]);
  });
  test("limited mode shows in the status", () => {
    assert.equal(bubble(view({ state: "Running", text: "x", agent: "codex", limited: true }), 0).running?.text, "Codex is working · limited mode");
  });
  test("cancelling strikes the phrase", () => {
    const b = bubble(view({ state: "Cancelling", text: "x" }), 0);
    assert.deepEqual([b.phrases[0].rail, b.running], ["gone", { icon: "terminal", text: "Cancelling", dots: false }]);
  });
  test("done goes on the phrase", () => {
    const b = bubble(view({ state: "Succeeded", text: "x", detail: "no lint errors", sessionId: "s" }), 0);
    assert.deepEqual([b.phrases[0].rail, b.phrases[0].outcome, b.phrases[0].clickHint, b.running, b.mic, b.clickable], ["ok", "Done · no lint errors", "Click to open in terminal", null, "off", true]);
  });
  test("failure goes on the phrase", () => {
    const b = bubble(view({ state: "Failed", text: "x", detail: "rate limit reached", sessionId: "s" }), 0);
    assert.deepEqual([b.phrases[0].rail, b.phrases[0].outcome, b.globalError], ["err", "Claude failed · rate limit reached", null]);
  });
  test("limited click hint", () => {
    const b = bubble(view({ state: "Failed", text: "x", agent: "codex", limited: true, sessionId: "s" }), 0);
    assert.equal(b.phrases[0].clickHint, "Click to open in Codex and trust");
  });
  test("failure without a phrase is a global error", () => {
    const b = bubble(view({ state: "Failed", detail: "Microphone unavailable" }), 0);
    assert.deepEqual([b.phrases.length, b.globalError, b.strip, b.mic, b.clickable], [0, "Microphone unavailable", "error", "error", false]);
  });
  test("no session, no click", () => {
    assert.equal(bubble(view({ state: "Succeeded", text: "x" }), 0).clickable, false);
  });
  test("long unbroken text stays one phrase", () => {
    const b = bubble(view({ state: "Running", text: "C:/a/".repeat(80) }), 0);
    assert.equal(b.phrases.length, 1);
  });
  ```

- [ ] **Step 2: Run to see them fail**

  Run: `cd apps/desktop && node --test src/bubble.test.ts`
  Expected: FAIL, cannot find `./bubble.ts`.

- [ ] **Step 3: Implement `bubble(view, level)` in `bubble.ts`**

  One `switch (view.state)`. Hidden states (`Idle`, `LoadingModel`, `NoModel`) return an empty bubble with `strip: "off"`, `mic: "off"`. Every phrase has `newest: true` (only one phrase exists on today's logic). Outcome and hint strings exactly as in the tests; a missing `detail` drops the `" · …"` part.

- [ ] **Step 4: Run to see them pass**

  Run: `cd apps/desktop && node --test src/bubble.test.ts`
  Expected: 13 tests pass.

- [ ] **Step 5: Commit**

  ```bash
  git add apps/desktop/src/bubble.ts apps/desktop/src/bubble.test.ts
  git commit -m "feat(desktop): bubble model for the overlay"
  ```

### Task 2: Bubble components and styles

**Files:**
- Modify: `apps/desktop/src/overlay.tsx` (replace the render; keep the `view`/`level` listeners and `open_session`)
- Modify: `apps/desktop/src/style.css` (drop `.aurora`, `.aurora-sweep`, `@keyframes drift`; add the bubble styles)

**Interfaces:**
- Consumes: `bubble`, `Bubble`, `Phrase`, `Rail` from Task 1; `useBusy` from `controls.tsx`.
- Produces: the DOM element with `id="bubble"` wrapping the bubble; Task 3 measures it.

- [ ] **Step 1: Render the model** in `overlay.tsx` as four small components in the same file: `PhraseRow`, `BottomRow`, `Strip`, `Tip`.
  - `PhraseRow`: `div.rail.r-{rail}` with the text in `span.t`; `run`, `speak` and `decode` phrases white, all others muted (12px); `r-gone` also strikes the text; `r-run` clamps to two lines (`-webkit-line-clamp:2`).
  - `BottomRow`: left = the microphone (icon `mic` or `micoff`, grey label "Waiting" / "Listening" with `.ell` / "Mic off" / "Mic unavailable" in light text); right = running work (icon `terminal` or `pencil`, grey text, `.ell` when `dots`). With a `globalError`, the row is one grid `15px 1fr 15px`: crossed-out mic, the white error text centred, an empty cell.
  - `Strip`: `div.stripwrap > div.strip.{strip}`.
  - `Tip`: wraps a row; it renders its tooltip when `outcome` is set, or when a `useLayoutEffect` measures `scrollWidth > clientWidth + 1 || scrollHeight > clientHeight + 1` on the text element. Tooltip content: still dot + outcome (only when set), divider, full text, then `clickHint` (only when set).
  - The bubble root gets `onClick` only when `clickable`.

- [ ] **Step 2: Add the styles** to `style.css`, copying values from the reference file: `.rail`/`.r-*` (the `::before` rail, `left:0; top:.28em; bottom:.28em; width:2px`), `.stripwrap`/`.strip.*` with `@keyframes slide`, the cap-alignment rule for the icons (`--s: 15px` for mic, pencil and terminal, `13px` for the still tooltip dots), `.ell` with `@keyframes ell{0%{content:""}25%{content:"."}50%{content:".."}75%{content:"..."}}` (`width:1.1em; display:inline-block`), the tooltip (`.tip`, `.why` with the divider `border-bottom:1px solid rgba(255,255,255,.12)`), `min-width:0` and `overflow-wrap:anywhere` on phrase text, and `prefers-reduced-motion` turning off `slide` and showing a static "..." in `.ell`.

- [ ] **Step 3: Type-check and test**

  Run: `cd apps/desktop && pnpm exec tsc --noEmit && pnpm test`
  Expected: no type errors; all tests pass.

- [ ] **Step 4: Commit**

  ```bash
  git add apps/desktop/src/overlay.tsx apps/desktop/src/style.css
  git commit -m "feat(desktop): overlay bubble with microphone strip and phrase rails"
  ```

### Task 3: Let the mouse reach the bubble

**Files:**
- Modify: `apps/desktop/src-tauri/src/overlay.rs` (hover tracking, rectangle, height)
- Modify: `apps/desktop/src-tauri/src/runtime.rs:404-425` (start tracking for every shown view)
- Modify: `apps/desktop/src-tauri/src/lib.rs` (register the command), `apps/desktop/src-tauri/build.rs` (command list), `apps/desktop/src-tauri/capabilities/overlay.json` (`allow-set-bubble-rect`)
- Modify: `apps/desktop/src/overlay.tsx` (report the rectangle)

**Interfaces:**
- Consumes: `#bubble` from Task 2.
- Produces:
  ```rust
  #[derive(Clone, Copy, Default, serde::Deserialize)]
  pub struct Rect { pub left: f64, pub top: f64, pub right: f64, pub bottom: f64 } // CSS px, relative to the overlay window
  #[derive(Clone, Default)] pub struct BubbleRect(pub Arc<Mutex<Rect>>);          // managed state
  fn inside(cursor: (f64, f64), window: (f64, f64), scale: f64, rect: Rect) -> bool;
  #[tauri::command] fn set_bubble_rect(rect: Rect, state: tauri::State<BubbleRect>);
  ```
  TS: `invoke("set_bubble_rect", { rect: { left, top, right, bottom } })` from a `ResizeObserver` on `#bubble` plus every `view` update.

- [ ] **Step 1: Write the failing tests** in `overlay.rs`:

  ```rust
  #[test]
  fn inside_the_bubble_only() {
      let r = Rect { left: 100.0, top: 50.0, right: 460.0, bottom: 190.0 };
      assert!(inside((200.0, 100.0), (0.0, 0.0), 1.0, r));
      assert!(!inside((50.0, 100.0), (0.0, 0.0), 1.0, r));
      assert!(!inside((200.0, 195.0), (0.0, 0.0), 1.0, r));
  }
  #[test]
  fn inside_uses_the_window_scale() {
      let r = Rect { left: 100.0, top: 50.0, right: 460.0, bottom: 190.0 };
      assert!(inside((1000.0 + 300.0, 500.0 + 150.0), (1000.0, 500.0), 1.5, r));
      assert!(!inside((1000.0 + 700.0, 500.0 + 150.0), (1000.0, 500.0), 1.5, r));
  }
  #[test]
  fn hover_stops_when_the_op_changes() {
      let active = Arc::new(AtomicU64::new(7));
      assert!(keep_tracking(&active, 7));
      active.store(8, Ordering::SeqCst);
      assert!(!keep_tracking(&active, 7));
  }
  ```
  (`keep_tracking(active: &AtomicU64, op: u64) -> bool` is the loop condition pulled out of `track_bubble_hover`.)

- [ ] **Step 2: Run to see them fail**

  Run: `cargo test -p erindi-desktop overlay`
  Expected: FAIL, `inside`, `Rect` and `keep_tracking` not found.

- [ ] **Step 3: Implement**
  - `inside`: cursor in physical px against `window + rect * scale`.
  - `track_bubble_hover` reads the rectangle from `BubbleRect` on every tick instead of `BUBBLE_HALF_WIDTH`/`BUBBLE_BOTTOM`/`BUBBLE_TOP` (delete those constants).
  - `HEIGHT` becomes `260` so a tooltip above the bubble fits.
  - `runtime.rs`: rename `clickable_op` to `hover_op`; store `view.op` for every view that shows the overlay, `0` when it hides; start tracking whenever it becomes non-zero for a new op. Clicks stay gated in the frontend by `clickable`.
  - Register `set_bubble_rect` (lib.rs handler list, build.rs command list, capability).

- [ ] **Step 4: Run the tests and the checks**

  Run: `cargo test -p erindi-desktop && cargo clippy -p erindi-desktop --all-targets && cargo fmt --check -p erindi-desktop && cd apps/desktop && pnpm exec tsc --noEmit`
  Expected: all pass, no warnings.

- [ ] **Step 5: Commit**

  ```bash
  git add apps/desktop/src-tauri apps/desktop/src/overlay.tsx
  git commit -m "feat(desktop): the overlay follows the bubble's own rectangle for hover"
  ```

### Task 4: Docs, build and manual check

**Files:**
- Modify: `README.md` (the feature line "with a live transcript above the aurora overlay" → "with a live transcript in the overlay bubble")
- Modify: `ROADMAP.md` (mark the "Redesign" line's overlay part done, or add "- [x] **Overlay bubble.** …" under UI and UX)

- [ ] **Step 1: Update README and ROADMAP** as above.
- [ ] **Step 2: Build the release exe for manual tests**

  Run: `cd apps/desktop && pnpm tauri build --no-bundle`
  Expected: `Built application at: …\target\release\erindi-desktop.exe`

- [ ] **Step 3: Manual check against the reference** — on the real build, reproduce and compare: 1 (hold key, silence), 2 (speak), 3 (release → transcribing), 6 (running), 7 (single press → cancelling), 8/10 (done, hover the phrase: outcome, divider, text, click hint; click opens the terminal), 12 (force a failure, e.g. an invalid model ID), 18 (unplug or block the microphone), a long phrase (tooltip only when cut off). Put this list in the PR description as a checklist.
- [ ] **Step 4: Commit**

  ```bash
  git add README.md ROADMAP.md
  git commit -m "docs: the overlay is a bubble now"
  ```
