# Hotkey gestures and voice commands

Date: 2026-09-22. Branch: `feat/prompt-refiner`. Supersedes the text cleanup part of `2026-09-22-prompt-refiner-design.md`.

## Goal

Speaking to Erindi has three layers, each with one job:

1. **Invocation**: how a recording starts. Hotkeys with gestures, as in Wispr Flow. A wake word comes later.
2. **Commands**: what Erindi does with the recording before sending it. Each command has a hotkey and a spoken form.
3. **Hand-off**: the rest of the phrase goes to Claude unchanged.

The local model no longer rewrites dictation. The benchmark showed it translates and drops sentences; the transcript is good enough as it is. The model stays, with one job: recognising a spoken command said in the user's own words.

## Decisions

| Topic | Decision |
|-------|----------|
| Talk hotkey | One combination, `Ctrl+Alt+Space`. Hold to talk, double-press for hands-free. The separate hands-free hotkey goes away. |
| New-session hotkey | `Ctrl+Alt+N`, same gestures; the phrase goes to a new session. |
| Default target | The active session. There is no "same session" command. |
| Cancel | A single press of the same key, everywhere. |
| Spoken commands | Recognised only at the start or end of a phrase. The middle is always part of the task. Settings says so. |
| Command list | New session, open in terminal, cancel. Project switching is deferred. |
| Model role | Maps a phrase edge to one command from the list, or to none. It never rewrites text. |
| Wake word | Deferred; stays in `ROADMAP.md`. |

## Gestures

Recording starts on the first key-down, so no speech is lost while Erindi tells a hold from a tap. A press shorter than `HOLD` (300 ms) is a tap; a second tap within `DOUBLE` (300 ms) is a double-press.

| State | Hold | Single press | Double-press |
|-------|------|--------------|--------------|
| Idle | talk while held, send on release | nothing is sent (the recording started on key-down is dropped) | hands-free recording |
| Recording hands-free | — | cancel, nothing is sent | send now, without waiting for the pause |
| Transcribing, running (orange) | — | cancel | nothing |
| Result shown | same as Idle | same as Idle | same as Idle |

Hands-free recordings also send after the silence set in Settings, as today.

A single press is only known once `DOUBLE` has passed without a second press, so a cancel takes effect 300 ms after the press.

## Voice commands

| Command | Spoken (start or end of a phrase) | Hotkey | Effect |
|---------|-----------------------------------|--------|--------|
| New session | "new session", "в новой сессии", "новая сессия", "создай новую сессию" and the other phrases the parser knows | `Ctrl+Alt+N` with gestures | The rest of the phrase starts a new session. |
| Open in terminal | "open in terminal", "открой в терминале" | `Ctrl+Alt+T`, single press | Opens the active session with `claude --resume` in Windows Terminal. Said alone, nothing goes to Claude. With a task, the task runs as usual and the bubble offers the terminal when it finishes. |
| Cancel | "cancel", "scratch that", "отмена" at the end of a phrase | the talk key, single press | Nothing is sent. |

Cancel is only spoken inside the phrase it cancels. Work that is already running is cancelled by the key.

### Recognition

1. The parser looks for a known command phrase at the start or end of the transcript, after the dictionary. This is today's `parse_intent`, extended with the new commands.
2. If the parser finds nothing and "Understand commands in my own words" is on, the model gets the transcript and returns `{"command": "new_session" | "open_terminal" | "cancel" | "none", "rest": "..."}` under a JSON Schema.
3. The model's answer is accepted only when `rest` equals the transcript with a leading or trailing run of words removed, compared on lowercase words without punctuation. Anything else is treated as `none`, and the whole transcript goes to Claude. This enforces "start or end only" and forbids rewriting.

## User interface

### Settings, Hotkeys block

"How you start, send and cancel a recording."

- **Talk**: `Ctrl+Alt+Space`.
- **Talk into a new session**: `Ctrl+Alt+N`.
- **Open active session in terminal**: `Ctrl+Alt+T`.

Hint under the fields:

- Hold: talk while holding, release to send.
- Double-press: hands-free; sends after a pause.
- Double-press while recording hands-free: send now.
- Press once while recording or while Claude works: cancel.

### Settings, Voice commands block

Replaces the "Prompt cleanup" block.

"Say a command at the start or end of a phrase. In the middle it counts as part of the task."

- A list of the three commands with one example each, such as *"new session, check the diff"*.
- Checkbox **Understand commands in my own words**, off by default, with the cleanup model's row (label, Download, progress) under it. The checkbox is disabled until the model is downloaded. Hint: "A local model recognises commands such as "let's start fresh". Adds about 0.1 s."

### Bubble

- Cancel shows "Cancelled" briefly, then hides.
- A command recognised from speech shows in the status line, as "+ new session" does today.

## Removed

- Text rewriting: the `Refine` step's cleaned text, the rewrite system prompt, the script and length checks on rewritten text.
- The "Clean up prompt" checkbox. The saved `cleanup` setting is read as "Understand commands in my own words".
- The separate hands-free hotkey (`toggleHotkey`). The saved `holdHotkey` becomes the talk hotkey.

History keeps reading prompts saved with raw text during testing.

## Kept

In-app model downloads, Settings blocks, running without a speech model, `llama-server` lifecycle, first-launch Settings.

## Testing

- Gesture recognition as a pure function of key events and time: hold, tap, double-press, and each row of the gesture table, including cancel during transcription and during a run.
- Parser: the new command phrases at the start and end, and the same words in the middle left alone.
- Model acceptance: an answer whose `rest` is an edge cut is accepted; a rewritten `rest`, a cut from the middle, and an unknown command are rejected.
- Controller: open-in-terminal alone sends nothing; cancel at the end sends nothing; new session with a task starts a new session.
- Benchmark: the labeled set changes to commands. It reports parser accuracy, model accuracy on phrases the parser misses, false commands on plain tasks, and latency.

## Roadmap changes

- Invocation: hotkeys with gestures now; wake word later.
- Voice commands: project switching by name, more commands.
- Remove the prompt cleanup item from "done"; keep "Unload the cleanup model", "Refiner device" and "Refiner endpoints", renamed for the command model.
