<p align="center">
  <img src="apps/desktop/src/logo.svg" width="112" alt="Erindi logo">
</p>

<h1 align="center">Erindi</h1>

<p align="center">
  Speak a task, and Claude Code runs it. Speech recognition stays on your computer.
</p>

<p align="center">
  <a href="https://github.com/Viperwow/erindi/releases">Download</a> ·
  <a href="ROADMAP.md">Roadmap</a> ·
  <a href="CONTRIBUTING.md">Contributing</a>
</p>

> [!NOTE]
> Erindi is a proof of concept: Windows only, Claude Code and Codex.

## Features

- **Talk from any app.** Hold a hotkey to talk, or double-press it for hands-free mode that sends after a pause.
- **Local speech recognition.** Russian and English, with a live transcript above the aurora overlay.
- **Claude Code or Codex.** Pick the default agent in Settings, or say "claude" or "codex" to start a session with one. Each session keeps its agent.
- **Runs in the background.** Cancel a run with one press, or open the session in Windows Terminal.
- **Sessions.** New utterances continue the active session; the Sessions tab lists past ones and reopens them.
- **Voice commands.** Say "new session", "open in terminal" or "cancel" at the start or end of a phrase. A local model understands commands in your own words.
- **Dictionary.** Replaces what you say with how it should be written.

## Install

1. Download the archive from [Releases](https://github.com/Viperwow/erindi/releases).
2. Unpack it and start `erindi.exe`.
3. Settings opens on first launch. Press **Download** there to install the speech model.

The `latest` pre-release is rebuilt on every merge into `main`.

## Usage

| Hotkey | Action |
|---|---|
| `Ctrl+Alt+Space` | Hold to talk, double-press for hands-free, press once to cancel |
| `Ctrl+Alt+N` | Talk into a new session |
| `Ctrl+Alt+T` | Open the active session in a terminal |

The Commands tab lists the voice command patterns and lets you edit them.

## Build from source

```powershell
./scripts/fetch-models.ps1 -Refiner   # development models; drop -Refiner to skip the command model
cd apps/desktop
pnpm install
pnpm tauri dev
```

Run the tests with `cargo test --workspace`. [CONTRIBUTING.md](CONTRIBUTING.md) covers release builds, versions and commit messages.

## Built with

| Part | Built with |
|---|---|
| Desktop shell, tray, overlay | Tauri 2, Preact, Tailwind, TypeScript |
| Audio capture and resampling | cpal, rubato |
| Speech recognition | sherpa-onnx, Parakeet TDT 0.6B v3 |
| End of speech | Silero VAD |
| Voice commands in your own words | llama.cpp (Vulkan), Qwen2.5-3B-Instruct |
| Agent run and cancel | `claude -p` stream-json, `codex exec --json`, Windows Job Objects |

## The name

*Erindi* (Icelandic, from Old Norse *erendi*) means an errand, a message and a speech. It shares its root with English *errand*, Danish *ærinde* and Swedish *ärende*. Pronounced *ER-in-dee*.
