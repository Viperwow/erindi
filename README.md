# Erindi 0.0.1

> **Work in progress.** Proof of concept: Windows only, Claude Code only.

Hold a hotkey, speak a task, and Erindi transcribes it locally and runs it in Claude Code, without leaving the app you are in.

**The name.** *Erindi* (Icelandic, from Old Norse *erendi*) means an errand, a message and a speech. It shares its root with English *errand*, Danish *ærinde* and Swedish *ärende*. Pronounced *ER-in-dee*.

| Part | Built with |
|---|---|
| Desktop shell, tray, overlay | Tauri 2, Preact, Tailwind, TypeScript |
| Audio capture and resampling | cpal, rubato |
| Speech recognition (RU/EN) | sherpa-onnx, Parakeet TDT 0.6B v3 |
| End of speech | Silero VAD |
| Agent run and cancel | `claude -p` stream-json, Windows Job Objects |

Hotkeys: `Ctrl+Alt+Space` hold to talk, `Ctrl+Alt+Shift+Space` hands-free, `Ctrl+Alt+N` hands-free in a new session.

```powershell
./scripts/fetch-models.ps1
cd apps/desktop; pnpm install; pnpm tauri dev
```

## Status

- [x] Speak a task with a hold or hands-free hotkey
- [x] Local speech recognition with a live transcript
- [x] Run Claude Code headless, cancel it, reopen the session
- [x] Continue the same session by voice
- [ ] Session history sidebar
- [ ] Prompt cleanup with a fast LLM
- [ ] Provider and model picker
- [x] Hotkey recorder (mouse buttons next)
- [ ] macOS build

Full plan: [ROADMAP.md](ROADMAP.md). Building and contributing: [CONTRIBUTING.md](CONTRIBUTING.md). Tests: `cargo test --workspace`.
