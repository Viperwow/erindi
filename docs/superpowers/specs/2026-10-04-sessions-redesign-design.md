# Sessions redesign

The Sessions tab today lists sessions as cards that expand in place. A long conversation with the local model makes the list hard to read, a question and its answer are hard to copy, and there is no way to find an old answer. This redesign turns the tab into a searchable list with the conversation open beside it.

Reference screens: [2026-10-04-sessions-states.html](2026-10-04-sessions-states.html). Screen numbers below refer to it.

## Layout

The Sessions tab has two panes.

- **List (left).** A search field, a count line, and the sessions, newest first.
- **Conversation (right).** The open session, read-only, every turn stacked top to bottom.

The settings window opens at 1120 × 720; its minimum stays 640 × 480. Below 900 px of window width the conversation slides over the list from the right instead of sitting beside it; Esc or ✕ closes it.

With no sessions at all the tab shows only "No sessions yet" and "Ask an agent something by voice. Its sessions appear here." (screen 3).

## Session list

Each row has two lines (screen 2):

1. A status mark, the title, the Active chip, then on the right the agent icon, agent name and turn count, then the ⋯ button.
2. The rail and the session's last line (see Rails).

- The title is the session's first prompt, cut to one line.
- **Selected** (the session open on the right) has a grey fill, `#262626`.
- **Active** (the session the next utterance continues) has a chip right after the title: the word Active, blue text, 1 px blue outline, fully rounded. Selected and active are independent.
- The status mark sits on the rail's vertical line, left of the title, centred on the title's capital letters. Its slot is always reserved, so the title never moves.

### Status marks

One per session. Only the active session shows a live mark; every other session shows none.

| State | Mark |
|---|---|
| Waiting | hollow circle, `#22508a`, still |
| Listening | dot, `#38bdf8`, pulsing |
| Transcribing | equilateral triangle pointing down, `#a855f7`, pulsing |
| Answering | regular pentagon, `#f59e0b`, turning clockwise around its own centre |
| Idle | no mark |

Shapes are regular polygons drawn as SVG with a 0.6 px stroke in the fill colour and round joins, 12 px box. Motion stops under `prefers-reduced-motion`.

Waiting means the microphone is on for the active session and nothing has been said yet. Queued, done, failed and cancelled have no session mark; they show on the phrase rail.

### ⋯ menu

The ⋯ slot is always reserved. The button shows on row hover, keyboard focus inside the row, on the selected row, and while its menu is open.

| State | Look |
|---|---|
| Row hovered | grey dots, no fill |
| Button hovered | fill `#333` |
| Menu open | fill `#4a4a4a`, white dots |
| Keyboard focus | 2 px blue ring |

Row menu: Make active, Open in terminal, separator, Delete.

- **Make active** is dimmed with "Active" on the right when the session already is.
- **Open in terminal** is absent for the local model.
- **Delete** turns into "Confirm delete · 3 s" on a red fill; a second click within 3 seconds deletes, as today.

## Search

One field searches every session, always. Search runs in the settings window over the loaded history, 200 ms after the last keystroke.

- **✕** clears the text.
- **Aa** matches case, **ab** whole words, **.\*** treats the text as a regular expression. An invalid expression shows "Invalid regular expression" under the field and keeps the previous results.
- **Filter ▾** opens checkboxes: Questions, Answers, Session names. Questions and Answers are on by default. A changed filter shows its count on the button, "Filter · 1 ▾". **Reset** restores the defaults.

Below the field: "9 matches in 2 sessions". Results group by session, each group headed like a list row with its own ⋯. Each result is one line on the grid rail · label · text (screen 1):

- the label is "You · 97" or "Answer · 97", 68 px wide, 4 px gap;
- the text is a one-line snippet around the match, the match highlighted `#713f12` on `#fef9c3`.

A click opens the session at that turn. Clearing the search returns to the list.

## Conversation pane

Header: the session title, then "‹ 1 of 9 ›" while searching or "‹ 98 of 214 ›" otherwise, then ⋯, then ✕. The title starts on the text line of the turns below it.

- **‹ ›** and **↑ ↓** step between matches while searching, otherwise between questions.
- Opening from the list scrolls to the last turn; opening from a result scrolls to that turn.
- The current turn has a 1 px blue inset outline on `#0b1222`; matches inside it are highlighted.

Each turn:

- **Question.** Label "You · 97", text in white, blue rail.
- **Answer.** Label with agent icon, agent name and model, "Claude · opus"; the model is grey. The answer renders as Markdown when Preview is on.
- On hover, top right: **⧉ Answer** and **⧉ Q&A** copy Markdown; the button reads "✓ Copied" for 1.5 s.

A strip on the right edge maps every question of the session: one tick per question, white for the current one, red for a failed answer, a light band for the visible part. A click jumps to that question (screen 7). Turns use `content-visibility: auto`, so sessions with hundreds of turns stay fast.

Header menu: Make active, Open in terminal, Copy session (Markdown), separator, **Preview** with a switch on the right, separator, Delete. The switch is 26 × 14 px; only its knob moves.

## Rails

A rail belongs to one phrase, exactly as in the bubble: same colours, same meaning. Rails are 2.5 px, drawn as a left border so every rail renders at the same whole number of device pixels at any display scale.

| State | Rail | Text |
|---|---|---|
| Waiting | `#22508a` | "Waiting", grey |
| Speaking | `#38bdf8` | white, grows as you speak |
| Transcribing | `#a855f7` | white |
| Queued | dashed 2 px / 2 px, white 35 % | grey |
| Running | `#f59e0b` | white; tool steps under it while the agent works |
| Done | green 55 % | grey |
| Failed | `#f87171` | the error, red |
| Cancelled | white 22 % | grey, struck through |

The list row's second line shows the active session's live phrase while there is one (screens 11–13); otherwise the last question.

## Data

Today only local-model sessions keep answers, and no prompt keeps its model. The redesign needs both for every agent.

- **Answers for every agent.** The run's final `Result` text is stored on its prompt for Claude, Codex, Cursor and Pi too, through the same path as the local model's reply.
- **Failed answers.** A prompt keeps whether its run failed, with the error text as its answer.
- **Model per answer.** The model comes from the run's own output where the agent reports it: Claude's and Cursor's init line, Pi's messages. Codex `exec` does not report it, so Erindi uses the model it asked for, or the session's model read from Codex's log. The prompt stores it.
- **Live state.** The settings window receives the controller's view, the same one the overlay gets, so it can draw marks, live rails and streamed text for the active session.

Older history files keep loading: every new field is optional.

## Out of scope

- An error mark on the session and a done mark: rails carry both.
- Editing or re-asking a turn.
- Search inside agents' own logs; only what Erindi stored is searched.
