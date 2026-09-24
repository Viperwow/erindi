# Roadmap

Erindi is a work in progress. This list tracks what exists and what comes next.

## Done in 0.0.1

- [x] Tray app, single instance, aurora overlay with a transcript bubble.
- [x] Two hotkeys: hold to talk, and toggle with a silence endpoint.
- [x] Local speech recognition: Parakeet TDT via sherpa-onnx, Silero VAD, live transcript.
- [x] Dictionary of replacements applied to the transcript.
- [x] Headless Claude Code runs with a permission mode and whole-process-tree cancel.
- [x] Click the finished bubble to open the session with `claude --resume` in Windows Terminal.
- [x] Settings: project folder, model, hotkeys, microphone, silence length.
- [x] CI checks on pull requests and a Windows build published as the `latest` pre-release on every merge into `main`.

## Next

### Working with agents

- [x] **Continue the session.** Utterances continue the active Claude session by default. A session setting (continue, continue if recent, always new), a "new session" hotkey or the spoken "new session" command choose otherwise.
- [x] **Session history.** A Sessions page lists sessions started from Erindi with what was said, the folder and the session ID. Each one opens in a terminal, becomes the active session for the next utterance, or is deleted from the list.
- [x] **Voice commands.** New session, open in terminal and cancel, each with a hotkey and editable regex patterns on a Commands tab; commands count only at the start or end of a phrase.
- [x] **Commands in your own words.** A local model (Qwen2.5-3B on llama.cpp Vulkan) recognises commands the patterns miss; it never changes the text that reaches the agent.
- [ ] **More voice commands.** Switch the project folder by name.
- [ ] **Unload the command model** after idle time, for machines short on memory.
- [ ] **Command model device.** Auto, GPU or CPU; a CUDA build if Vulkan is not fast enough.
- [ ] **Command model endpoints.** Cloud, Ollama and LM Studio by address and API key.
- [ ] **Prompt cleanup, revisited.** Rewriting dictation with a 3B model translated and dropped sentences; retry with a larger model or drop the idea.
- [ ] **Provider, model, permission mode.** Pick a provider, then choose from its available models and permission modes instead of typing a model name.
- [ ] **Orca and other orchestrators.** Hand tasks to the locally installed Orca and similar agent orchestrators.
- [ ] **Plugin system.** Users add their own agents and customize Erindi through plugins, beyond the agents built into the app.
- [ ] **Agent install guide.** When an agent's CLI is missing, show the minimal install steps for that agent on the user's system.
- [ ] **Agent Client Protocol.** Talk to every agent through ACP, the shared protocol Zed and other editors use, instead of parsing each CLI's own output. Weigh it against Codex app-server, the JSON-RPC interface behind the Codex VS Code extension.
- [ ] **Computer control (later).** An "Allow computer control" checkbox lets agents click and type in other apps. A short warning hint appears under it; the same hint appears when `bypassPermissions` is selected.

### Voice and models

- [x] **Model downloads.** Speech and command models install from Settings with progress and hash checks.
- [ ] **Model picker.** Choose among several speech and command models, with Hugging Face search.

### Controls

- [x] **Hotkey recorder.** Click the field and press the combination, as in games.
- [x] **Hotkey gestures.** One talk key: hold to talk, double-press for hands-free, press once to cancel.
- [ ] **Mouse buttons as hotkeys.** Needs a low-level input hook; the current global shortcut plugin handles keyboard keys only.
- [ ] **Startup behavior.** Choose between opening Settings on launch and starting minimized to the tray.
- [ ] **Launch at login.** A Settings toggle starts Erindi with the system.
- [ ] **First-run setup.** Detect GPU, VRAM and CPU, pick models and device, warn when cleanup would be slow; re-run when the hardware changes.
- [ ] **Tray click.** Clicking or double-clicking the tray icon opens the window.

### UI and UX

- [ ] **Redesign.** Modern look, dark by default, with accent colors.
- [ ] **Themes.** Light, dark and system modes, with main colors set as hex codes.
- [x] **Dictionary tab.** Move the dictionary to its own tab; the main page keeps only what matters most.
- [x] **Branding.** The name shows once, in the window title and the sidebar header.
- [x] **Logo.** Three normal-distribution peaks in the aurora stage colors: green, orange and blue with lilac joins.
- [ ] **Logo order.** Blue on the left as the start, orange in the middle, green on the right as the result.
- [x] **Form saving.** One Save button for the whole form (keep it).
- [ ] **Localization.** English interface by default, with infrastructure for more interface and speech languages.

### Platforms

- [ ] **macOS build** for testing, as a primary platform.

### Website and videos

- [ ] **README demo.** A short GIF under the README header, recorded from the real app: the tabs and the overlay while someone talks.
- [ ] **Landing page.** A site that presents Erindi, with a download link to the releases.
- [ ] **Feature videos.** Automatically recorded short videos for the main features only, such as talking to Claude, voice commands and sessions, shown on the landing page.
- [ ] **Performance benchmark.** Measure Erindi against other voice tools on key tasks: speed first, such as the time from the end of speech to the transcript and to the agent start, then recognition accuracy, shown honestly even where Erindi is slightly behind. Keep the numbers over releases and show them in the README and on the landing page, together with the methodology: hardware, audio set, tool versions, how each number is measured and how to reproduce it.

## From the original plan

- [ ] Adapters for Codex, Pi and Gemini, then Copilot, Qwen and Kimi.
- [ ] HTTP providers: OpenAI-compatible APIs, Ollama, OpenRouter.
- [ ] API keys stored in the OS keychain.
- [ ] Wake word activation, as a second way to start a recording next to the hotkeys.
- [ ] MSI installer for Windows through the Tauri bundler, with `llama/` as a bundled resource.
- [ ] Signed installers and macOS notarization.
- [ ] **Auto-update.** Erindi checks the GitHub releases for a newer signed version, downloads it and installs it on restart, so nobody follows the install guide again.
- [ ] Modifier-only hotkeys.
- [ ] Update GitHub Actions versions that still target Node.js 20.
