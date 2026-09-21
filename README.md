# Whispio

Voice launcher for AI coding agents: a global hotkey records speech, local ASR turns it into a prompt, and the prompt runs in a headless agent.

## PoC scope

- Windows x64 only.
- Claude Code only (`claude -p --output-format stream-json`).
- Two hotkeys: hold-to-talk and toggle with silence endpoint.
- Local ASR: sherpa-onnx with Parakeet TDT 0.6B v3, Silero VAD.
- Deterministic prompt normalization with a user dictionary.
- Aurora overlay with a transcript bubble; the result opens via `claude --resume <session_id>`.

Out of scope: other agents, LLM cleanup, wake word, macOS, CI, installers.

## Layout

- `crates/core` — app state machine, prompt pipeline, Claude adapter, process runner.
- `crates/audio-asr` — audio capture, resampling, VAD, ASR.
- `apps/desktop` — Tauri 2 shell with Preact, Tailwind and TypeScript.
