# Whispio 0.0.1

> **Work in progress.** Proof of concept: Windows only, Claude Code only.

Hold a hotkey, speak a task, and Whispio transcribes it locally and runs it in Claude Code, without leaving the app you are in.

| Part | Built with |
|---|---|
| Desktop shell, tray, overlay | Tauri 2, Preact, Tailwind, TypeScript |
| Audio capture and resampling | cpal, rubato |
| Speech recognition (RU/EN) | sherpa-onnx, Parakeet TDT 0.6B v3 |
| End of speech | Silero VAD |
| Agent run and cancel | `claude -p` stream-json, Windows Job Objects |

Hotkeys: `Ctrl+Alt+Space` hold to talk, `Ctrl+Alt+Shift+Space` hands-free.

```powershell
./scripts/fetch-models.ps1
cd apps/desktop; pnpm install; pnpm tauri dev
```

Tests: `cargo test --workspace`.
