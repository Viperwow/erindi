# Prompt refiner and in-app model downloads

Date: 2026-09-22. Branch: `feat/prompt-refiner`.

## Goal

A local language model cleans each spoken utterance before it reaches the agent: it removes fillers, repeats, slips and spoken session commands, and keeps the meaning and wording of the user. The user installs every model from Settings; no script is ever needed.

Erindi competes on design and performance. The refiner must feel instant.

## Scope

In scope:

- The prompt refiner: a local `llama-server` with one fixed model, on or off in Settings.
- In-app download of the speech model and of the cleanup model.
- Settings grouped into titled blocks with a one-line description each.

Out of scope, added to `ROADMAP.md`:

- Choosing models, and searching Hugging Face.
- Unloading the cleanup model after idle time.
- A device setting (Auto / GPU / CPU).
- A CUDA build of `llama-server`.
- A first-run wizard that detects the hardware, caches the result and suggests settings; it re-runs when the hardware changes.
- A warning such as "cleanup takes ~4 s per phrase on this machine".
- Cloud, Ollama and LM Studio endpoints with an address and an API key.
- Streaming the cleaned text into the bubble.

## Decisions

| Topic | Decision |
|-------|----------|
| Engine | Official prebuilt `llama-server`, Vulkan, Windows x64, pinned release, shipped in the release zip. |
| Model | Qwen2.5-3B-Instruct, Q4_K_M GGUF (~2 GB). The benchmark compares it with a 1.5B and a 7B model. |
| Memory | While cleanup is on, the model stays loaded. No unloading. |
| Budget | 1.5 s per utterance is a target for measurements, not a timeout. Long speech takes longer; the user's work is never cut. |
| Protocol | OpenAI-compatible HTTP. The client takes a base URL, so a cloud endpoint later is a settings change. |
| Downloads | Only when the user presses Download. Nothing downloads on its own. |

## User interface

### Settings blocks

The Settings page is split into blocks. Each block has a title and a one-line description.

- **Agent**: project folder, model, permission mode, session policy.
- **Voice**: speech model, microphone, silence length.
- **Hotkeys**: the three hotkeys.
- **Prompt cleanup**: "A local model removes slips and voice commands before the agent sees your words."
- **Dictionary**: replacements.

### Speech model (Voice block)

- A disabled select with one value: "Parakeet TDT 0.6B v3 (~640 MB)".
- Next to it: "Downloaded", or a Download button. While downloading, the button becomes a progress bar with a percentage.
- A failed download shows the reason and the Download button again.

### Cleanup model (Prompt cleanup block)

- A "Clean up prompt" checkbox, off by default.
- A disabled select with one value: "Qwen2.5-3B-Instruct Q4 (~2 GB)", with the same Download control as the speech model.
- The checkbox is disabled until the model is downloaded, so "on but not working" cannot happen.

### First launch

When the speech model is missing, Erindi opens Settings at launch instead of starting hidden in the tray, and the Voice block is highlighted.

### Bubble

- Hotkey without a speech model: the bubble shows "Speech model is not installed. Open Settings to download it." Clicking the bubble opens Settings.
- While the refiner works: the bubble shows the raw text with the status "Refining". The cleaned text replaces it when the agent starts.

### Sessions page

Each utterance keeps the cleaned text, which the agent received, and the raw text. The expanded card shows the raw text when it differs.

## Behavior

### Utterance path

1. Parakeet transcribes the speech.
2. The dictionary applies its replacements.
3. The spoken-command parser looks for an explicit session command.
4. If cleanup is on, the refiner returns `{intent, text}`.
5. `choose()` picks the session.
6. The agent receives `text`.

`intent` is what the user wants for the session: `new`, `continue` or `none`. `text` is the task for the agent, cleaned.

### Merging the parser and the refiner

- When the parser finds an explicit command, the parser's intent wins. It is deterministic and costs nothing.
- The refiner's intent is used only when the benchmark shows that it beats the parser on the labeled set. Until then the refiner only cleans the text.
- The refiner's output is rejected, and the parser's text is used, when the JSON does not parse or when `text` is shorter than 0.3× or longer than 1.5× the input in characters.
- If `llama-server` fails, the utterance goes through the parser, the failure is logged, and the server restarts before the next utterance.
- An utterance that takes longer than 1.5 s to refine is logged with its length and time.

### Model request

- Response format: JSON Schema with `intent` as an enum of `new`, `continue`, `none`, and `text` as a string.
- `temperature: 0`, a fixed `seed`, `cache_prompt: true`, and `max_tokens` set to 1.5× the input tokens plus a small constant.
- A fixed system prompt with 10–15 few-shot examples in Russian and English. It says: rewrite, never answer; add nothing; remove fillers and session commands.

### Server lifecycle

- Cleanup on at launch, or turned on in Settings: start `llama-server`, then run one warm-up request with a fixed phrase. The warm-up time goes to the log.
- Cleanup turned off: stop the server.
- Arguments: the GGUF path, `--host 127.0.0.1`, a free port chosen at start, `-ngl 99` and a small context. Exact flags follow the pinned llama.cpp release.
- Readiness: poll `GET /health` until it returns 200.
- The server runs in a Windows Job Object with `CREATE_NO_WINDOW`, as the Claude runner does, so it dies with Erindi.
- An utterance that ends while the server is still starting waits for it.

### Downloads

- Files come from Hugging Face at a pinned revision and are checked against a SHA-256 recorded in the code.
- The speech model is downloaded as separate files: encoder, decoder, joiner, `tokens.txt`, plus `silero_vad.onnx`. No archive is unpacked.
- Each file downloads to `<name>.partial` and is renamed after the hash matches. An interrupted download never leaves a broken model.
- Progress reaches Settings as events.
- A model becomes usable right after download, without restarting Erindi.
- `scripts/fetch-models.ps1` stays for development.

## Code layout

- `crates/core/src/refine.rs`: the system prompt, the JSON Schema, parsing the response, merging with the parser, rejecting suspicious output. Pure functions with unit tests and no network.
- `crates/core/src/llama.rs`: starting and stopping `llama-server`, the health check, the HTTP request. HTTP client: `ureq`, blocking and small, fitting the existing threads.
- `crates/core/src/download.rs`: download with progress, `.partial` files, SHA-256 check.
- `crates/core/src/controller.rs`: `Transcribed` emits `Effect::Refine` when cleanup is on; `Msg::Refined` starts the run.
- `apps/desktop/src-tauri`: settings field `cleanup: bool`, commands for model status and download, progress events, opening Settings on first launch.
- `apps/desktop/src/settings.tsx`: the blocks, the model rows, the Download control.
- `scripts/package.ps1` and `.github/workflows/release.yml`: fetch the pinned `llama-server` Vulkan build, check its SHA-256, and put it in the zip.

## Testing

Unit tests without network:

- Prompt building and response parsing.
- Merging the parser and refiner intents.
- Rejecting suspicious output.
- Download: `.partial` handling and hash mismatch, against a local file server or a fake reader.
- Controller: `Transcribed` → `Refine` → `Refined` → run, and the fallback when refining fails.

Benchmark `crates/core/examples/refine-bench.rs`, run against a live server:

- A labeled set of 50–100 utterances: short, medium and long; Russian and English; with and without commands. Each has the spoken text, the expected intent and an acceptable text.
- Output: time per utterance and percentiles, refiner intent accuracy against the parser, texts that lost meaning.
- Utterances over 1.5 s are marked `WARN`.
- The benchmark chooses the model size and decides whether the refiner's intent is used.
