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

## Run

Requirements: Rust, Node with pnpm, the MSVC build tools, Windows Terminal, and a logged-in `claude` on `PATH`.

```powershell
./scripts/fetch-models.ps1        # ~490 MB into models/, SHA-256 verified
cd apps/desktop
pnpm install
pnpm tauri dev
```

The app lives in the tray. Open **Settings** there to pick the project folder and permission mode.

- `Ctrl+Alt+Space` — hold, speak, release to send.
- `Ctrl+Alt+Shift+Space` — press, speak, and the prompt goes out after the configured silence (or press again).
- Either hotkey during a run cancels it. Click the finished bubble to open the session in Windows Terminal.

Models are read from `models/` in the repository; set `WHISPIO_MODELS` to use another folder.

## Test

```powershell
cargo test --workspace                     # unit and fake-agent tests
cargo test --workspace -- --include-ignored  # also microphone and model tests
```

## Layout

- `crates/core` — app state machine, prompt pipeline, Claude adapter, process runner.
- `crates/audio-asr` — audio capture, resampling, VAD, ASR.
- `apps/desktop` — Tauri 2 shell with Preact, Tailwind and TypeScript.
