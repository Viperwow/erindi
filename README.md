<p align="center">
  <img src="apps/desktop/src/logo.svg" width="112" alt="Erindi logo">
</p>

<h1 align="center">Erindi</h1>

<p align="center">
  Speak a task, and Erindi runs it in your coding agent. Speech recognition stays on your computer.
</p>

<p align="center">
  <a href="https://github.com/Viperwow/erindi/releases">Download</a> ·
  <a href="ROADMAP.md">Roadmap</a> ·
  <a href="CONTRIBUTING.md">Contributing</a>
</p>

> [!NOTE]
> Erindi is an early-stage project: Windows and macOS 15 or later, with Claude Code, Codex and Pi.

## Features

- **Talk from any app.** Hold a hotkey to talk, or double-tap another for hands-free listening: each pause sends a phrase, and phrases said while the agent works wait in a queue.
- **Local speech recognition.** Many languages, even mixed in one phrase, with a live transcript in the overlay bubble.
- **Claude Code, Codex or Pi.** Pick the default agent in Settings, or say "claude", "codex" or "pi" to start a session with one. Each session keeps its agent.
- **Runs in the background.** Cancel a run with one press, or open the session in a terminal.
- **Sessions.** New utterances continue the active session; the Sessions tab lists past ones and reopens them.
- **Voice commands.** Say "new session", "open in terminal" or "cancel" at the start or end of a phrase. A local model understands commands in your own words.
- **Dictionary.** Replaces what you say with how it should be written.

## Install

Download from [Releases](https://github.com/Viperwow/erindi/releases):

| File | For |
|---|---|
| `erindi-X.Y.Z-windows-x64.zip` | Windows 10 and 11 |
| `erindi-X.Y.Z-macos-arm64.dmg` | Macs with Apple silicon (M1 and later), macOS 15 or later |
| `erindi-X.Y.Z-macos-x64.dmg` | Macs with an Intel processor, macOS 15 or later |

**Windows:** unpack the archive and start `erindi.exe`.

**macOS:** open the dmg and drag Erindi to Applications. Erindi is not notarized by Apple, so macOS blocks its first launch. To allow it, open System Settings → Privacy & Security, scroll to Security and click **Open Anyway**, then enter your password. The button stays for about an hour after the blocked launch.

macOS asks for the microphone the first time you talk. Erindi lives in the menu bar, not in the Dock.

Settings opens on first launch. Press **Download** there to install the speech model.

The `latest` pre-release is rebuilt on every merge into `main`.

## Usage

| Windows | macOS | Action |
|---|---|---|
| `Ctrl+Space` | `⌃⇧Space` | Hold to talk; tap to cancel |
| `Ctrl+Alt+Space` | `⌃⇧⌘Space` | Double-tap to turn hands-free listening on or off |
| `Ctrl+Shift+Space` | `⌃⌥⇧Space` | Hold to talk into a new session |
| `Ctrl+Alt+Shift+Space` | `⌃⌥⇧⌘Space` | Double-tap for hands-free listening into a new session |
| `Ctrl+Alt+T` | `⌃⌥T` | Tap to open the active session in a terminal |

Each shortcut and its mode (Tap, Hold or Double-tap) can be changed in Settings.

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
