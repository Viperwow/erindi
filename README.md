<p align="center"><img src="apps/desktop/src/logo.svg" width="112" alt="Erindi logo"></p>

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
| Voice commands in your own words | llama.cpp (Vulkan), Qwen2.5-3B-Instruct |
| Agent run and cancel | `claude -p` stream-json, Windows Job Objects |

Hotkeys: `Ctrl+Alt+Space` hold to talk, double-press for hands-free, press once to cancel; `Ctrl+Alt+N` talks into a new session; `Ctrl+Alt+T` opens the active session in a terminal. Say "new session", "open in terminal" or "cancel" at the start or end of a phrase; the Commands tab lists and edits the patterns.

**Download:** the [latest build](https://github.com/Viperwow/erindi/releases/tag/latest) is rebuilt on every merge into `main`. Unpack it and start `erindi.exe`; Settings opens on first launch; press **Download** there to install the speech model.

**From source:**

```powershell
./scripts/fetch-models.ps1 -Refiner   # development models; drop -Refiner to skip the command model
cd apps/desktop; pnpm install; pnpm tauri dev
```

## Status

- [x] Speak a task with a hold or hands-free hotkey
- [x] Local speech recognition with a live transcript
- [x] Run Claude Code headless, cancel it, reopen the session
- [x] Continue the same session by voice
- [x] Session history sidebar
- [x] Voice commands with editable patterns and a local model
- [ ] Provider and model picker
- [x] Hotkey recorder (mouse buttons next)
- [ ] macOS build

Full plan: [ROADMAP.md](ROADMAP.md). Building and contributing: [CONTRIBUTING.md](CONTRIBUTING.md). Tests: `cargo test --workspace`.
