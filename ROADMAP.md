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
- [x] CI checks and a Windows release archive.

## Next

### Working with agents

- [x] **Continue the session.** Utterances continue the active Claude session by default. A session setting (continue, continue if recent, always new), a "new session" hotkey and spoken commands such as "new session" or "same session" choose otherwise.
- [x] **Session history.** A Sessions page lists sessions started from Erindi with what was said, the folder and the session ID. Each one opens in a terminal or becomes the active session for the next utterance.
- [ ] **Prompt cleanup model.** A fast intermediate LLM turns the raw transcript into clear, logical text before it reaches the agent.
- [ ] **Provider, model, permission mode.** Pick a provider, then choose from its available models and permission modes instead of typing a model name.
- [ ] **Orca and other orchestrators.** Hand tasks to the locally installed Orca and similar agent orchestrators.
- [ ] **Computer control (later).** An "Allow computer control" checkbox lets agents click and type in other apps. A short warning hint appears under it; the same hint appears when `bypassPermissions` is selected.

### Voice and models

- [ ] **Speech model picker.** A small catalog of speech-to-text models, downloaded from Hugging Face only after the user confirms. Downloaded models look bright, others dimmed but still selectable.

### Controls

- [x] **Hotkey recorder.** Click the field and press the combination, as in games.
- [ ] **Mouse buttons as hotkeys.** Needs a low-level input hook; the current global shortcut plugin handles keyboard keys only.
- [ ] **Startup behavior.** Choose between opening Settings on launch and starting minimized to the tray.

### UI and UX

- [ ] **Redesign.** Modern look, dark by default, with accent colors.
- [ ] **Themes.** Light, dark and system modes, with main colors set as hex codes.
- [ ] **Dictionary tab.** Move the dictionary to its own tab; the main page keeps only what matters most.
- [ ] **Branding.** Show the name once, following how strong brands handle window titles and headers.
- [x] **Form saving.** One Save button for the whole form (keep it).
- [ ] **Localization.** English interface by default, with infrastructure for more interface and speech languages.

### Platforms

- [ ] **macOS build** for testing, as a primary platform.

## From the original plan

- [ ] Adapters for Codex, Pi and Gemini, then Copilot, Qwen and Kimi.
- [ ] HTTP providers: OpenAI-compatible APIs, Ollama, OpenRouter.
- [ ] API keys stored in the OS keychain.
- [ ] Wake word activation.
- [ ] Signed installers and macOS notarization.
- [ ] Auto-update channel.
- [ ] Modifier-only hotkeys.
- [ ] Update GitHub Actions versions that still target Node.js 20.
