# Continuous listening, a phrase queue and a new overlay bubble

Date: 2026-09-29. Branch for this spec: `docs/continuous-listening-spec`.

## Goal

Hands-free mode keeps listening phrase after phrase instead of stopping after one. A phrase said while the agent works waits in a queue and runs when the agent is free. The overlay becomes a bubble that shows the microphone and the agent's work at the same time, since both can now happen at once.

The approved look of every state is in [`2026-09-29-overlay-states.html`](2026-09-29-overlay-states.html). Open it in a browser. It is the reference for the overlay work; this spec describes behaviour and structure.

Order of work, two PRs:

1. **Overlay bubble** on today's logic: aurora strip inside the bubble, bottom status row, outcome colours, tooltips.
2. **Listening mode and queue**: the controller split into a microphone track and an agent track joined by a queue, and the bubble states that need them.

## Decisions

| Topic | Decision |
|-------|----------|
| Listening mode | A double press of the talk key turns it on and off. While it is on, every pause ends a phrase, and listening goes on. Long silence does not turn it off. Once listening is off and nothing is transcribed, queued or run, the bubble counts down ("Hides in 5s", 5 s by default, set in Settings), then hides; a new phrase during the countdown stops it, and the next idle stretch starts a new one. |
| Single press | Cancels by priority: the phrase being spoken, otherwise the running agent. The queue and listening stay. With nothing running it does nothing. |
| Old double-press gesture | "Double-press while hands-free sends now" goes away; the pause sends. |
| Queue | One queue for every input. A phrase finished while the agent works, by listening mode or by holding the key, joins the queue. |
| Order | Queued phrases run one by one, each as its own run in the active session. |
| After a cancelled run | The next queued phrase starts at once. |
| After a failed run | The next queued phrase starts at once, the same as after a cancel. |
| Commands in a queued phrase | "New session", an agent name, "open in terminal" and model-recognised commands apply when the phrase leaves the queue, not when it was said. |
| Open in terminal while the agent works | The hotkey and the voice command join the queue and run in their turn. The bubble is clickable only when the agent is idle and the queue is empty. |
| Turning listening off | Does not cancel the running agent or the queue; they finish. |
| Finished phrases in the bubble | The last three stay while a series goes on. They clear when a new phrase starts after the agent went idle with an empty queue. |
| Transcribing while speaking | The bottom row shows "Listening" with the microphone on the left and "Transcribing" on the right; the strip is bright blue. |

## Overlay bubble (PR 1)

The aurora no longer fills the bottom of the screen. It is a 2px strip along the bottom edge of the bubble, clipped to the bubble's rounded corners.

### The strip shows the microphone only

| Microphone | Strip |
|------------|-------|
| Waiting for a phrase | Blue at about half brightness, slow |
| Hearing speech | Bright blue |
| Transcribing, nobody speaking | Violet |
| Off | Grey |
| Error | Red, still |

### The bottom row

- Left, always: the microphone icon with a grey label — "Waiting" (waiting for a phrase), "Listening" (hearing speech), "Mic off" with a crossed-out microphone, "Mic unavailable" with a crossed-out microphone in light text. Every icon in the row is the same grey.
- Right: what is running now, with a grey action icon and no dot — a pencil for "Transcribing", a terminal for the agent ("Claude is working · Bash cargo test", "Cancelling").
- "Listening" and "Transcribing" end with three dots that appear one by one ("." → ".." → "..."), in a fixed-width slot so the label does not move; with reduced motion the dots stay as "...".
- A global error that belongs to no phrase replaces the label: one white line next to the crossed-out microphone, centred in the row ("Microphone unavailable · check the microphone in Settings", "Speech model failed to load · open Settings to download it again"). Red lives only in the strip.

### Phrases on top

- Phrase text is always neutral. Live phrases (the agent's task, the phrase being spoken) are white; queued and every finished phrase, the newest included, are muted. No coloured text and no icons in phrase rows, so every row's text starts on the same line.
- A thin 2px rail in a fixed gutter on the left of each phrase shows its state:

  | Rail | Phrase |
  |------|--------|
  | Amber | The agent works on it |
  | Blue | Being spoken now |
  | Violet | Being transcribed |
  | Dashed grey | Waiting in the queue |
  | Green, muted | Done |
  | Red | Error |
  | Light grey | Cancelled; the text is struck through |

- The phrase the agent works on takes up to two lines. Queued and earlier finished phrases take one line each.
- Outcomes belong to phrases, never to the bottom row. "CLI not found" and "Couldn't transcribe" are phrase errors too.

### Tooltips

- A finished or failed phrase always has a tooltip: the outcome or the error reason after a still green or red dot, a divider, then the full phrase. The last one adds "Click to open in terminal" when the agent is idle.
- Any other line gets a tooltip only when it is cut off, measured by real overflow, not text length.
- A line without a tooltip does not react to the mouse.

### Icon alignment

Microphone and action icons, and the still dots in tooltips, centre on the capital letters of their own line, not on the line box: each sits on the text baseline and moves down by `(icon size − 1cap) / 2`, where `1cap` comes from the row's own font size. Dots are svg like the other icons, so they align by the same rule. Any future animation on an icon goes on an inner element, never on the svg that carries the shift.

### Click

Clicking the bubble opens the active session in a terminal only when the agent is idle and the queue is empty. Otherwise the bubble is not clickable.

### Changes

- `apps/desktop/src/overlay.tsx` and `style.css`: the bubble, the strip, the bottom row and tooltips. The full-screen aurora goes away.
- `crates/core/src/controller.rs`: `View` carries what the bubble needs on today's logic: microphone state, agent state, phrase text, detail, outcome. PR 2 extends it with the queue and finished phrases.
- Overlay window size: it stays a transparent window at the bottom of the screen; only its content changes.

## Listening mode and queue (PR 2)

### Controller

Today one state machine walks Idle → Listening → Transcribing → (Classifying) → Running → Succeeded or Failed → Idle. It cannot listen while the agent runs. It becomes two tracks joined by a queue.

1. **Microphone track**: `Off → Listening → Transcribing → Off or Listening`.
   - With listening mode on, the track goes back to Listening after a phrase; otherwise it turns off.
   - Holding the key records one phrase, then Off.
   - Its output is the text of a finished phrase, which goes to the queue.
   - "Cancel" at the end of a phrase drops the phrase at once; it never reaches the queue.
2. **Queue**: an ordered list of `{id, text}` in the controller.
3. **Agent track**: `Idle → (Classifying) → Running → Cancelling → Idle`.
   - When it is idle and the queue is not empty, it takes the first phrase.
   - Commands in that phrase are worked out now: session choice, agent name, "open in terminal", the local command model.
   - Then the agent runs in the active session.
   - When the run ends, succeeds, fails or is cancelled, it takes the next phrase.
4. **Operation IDs**: two counters instead of one. A phrase op guards against stale transcription results. A run op guards against a stale run end. A run ending must not be mistaken for the phrase being spoken.
5. **Succeeded and Failed** stop being screens that wait for Dismiss. The outcome stays on its phrase in the bubble and does not hold the agent track.

Everything stays in `crates/core`, free of Tauri, and is tested like today.

### Microphone and transcription while the agent runs

- In listening mode the capture stays open for the whole mode instead of restarting per phrase, so the start of the next phrase is not lost.
- The VAD endpointer resets after each "speech ended" and waits for the next speech. The controller takes the phrase's audio, sends it to transcription and starts collecting again. "No speech" is ignored in listening mode.
- There is one speech model, so final transcriptions run one at a time, and phrases reach the queue in the order they were said. The live transcript shows only the phrase being spoken; it waits while a final transcription runs.
- The agent is an external process; it never touches the microphone. Erindi has no spoken replies, so there is no echo.
- Holding the key works as today: capture while held, one phrase, then to the queue or straight to the agent.

### Runtime

- `apps/desktop/src-tauri/src/runtime.rs`: capture may stay open while a run is in progress; effects stay as they are, with the new op IDs.
- `apps/desktop/src/settings.tsx`: the hotkey help and the "Silence before sending" hint describe the new gestures.
- `README.md`: the hands-free sentence describes continuous listening and the queue; "above the aurora overlay" goes.

## Errors

| Error | Where it shows | What happens next |
|-------|----------------|-------------------|
| The agent failed (rate limit, network, CLI error) | Its phrase gets a red rail; reason in the tooltip | The next queued phrase starts |
| The agent's CLI is missing | Its phrase gets a red rail; "… CLI not found · install it, then press Re-check in Settings" in the tooltip | The next queued phrase starts |
| One phrase could not be transcribed | The live text gets a red rail, "Couldn't transcribe this phrase" in the tooltip | Listening goes on |
| An empty phrase (a cough, noise) | Nowhere | Ignored |
| Microphone unavailable | Bottom row: one white centred line next to the crossed-out microphone; strip red | Listening mode turns off; the agent and the queue go on |
| Speech model failed to load | Bottom row: one white centred line next to the crossed-out microphone; strip red | Listening cannot start; Settings offers the download |

## Testing

1. **Core** (`crates/core`, no app):
   - The double press turns listening on and off; after a transcribed phrase and after "no speech" listening goes on.
   - A phrase finished during a run joins the queue, by listening mode and by holding the key; phrases run one by one in order.
   - A single press drops the phrase being spoken, otherwise cancels the run and starts the next queued phrase, otherwise does nothing.
   - After a failed run the next queued phrase starts.
   - Commands in a queued phrase apply when it leaves the queue; "open in terminal" during a run waits in the queue.
   - Stale events: a run end is not taken for the current phrase, and the other way round.
   - Turning listening off leaves the run and the queue alone.
   - Finished phrases: the last three stay and clear on a new series.
2. **Runtime**: phrase cutting — the VAD resets after each phrase and capture does not restart. A test on recorded audio if it runs without a microphone, otherwise a manual check.
3. **Overlay**: `tsc`, and a manual check of every state in `2026-09-29-overlay-states.html` on a real build.
4. **Manual checklist** in each PR description.

## Out of scope

- Steering a running agent mid-turn (Cursor's "send now", Codex's Enter).
- Editing or reordering the queue.
- Semantic end-of-turn detection instead of a fixed pause.
- Spoken replies and barge-in.
- A wake word.
