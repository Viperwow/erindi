# Roadmap

Erindi is a work in progress. This list tracks what exists and what comes next.

## Done in 0.0.1

- Tray app, single instance, aurora overlay with a transcript bubble.
- Two hotkeys: hold to talk, and toggle with a silence endpoint.
- Local speech recognition: Parakeet TDT via sherpa-onnx, Silero VAD, live transcript.
- Dictionary of replacements applied to the transcript.
- Headless Claude Code runs with a permission mode and whole-process-tree cancel.
- Click the finished bubble to open the session with `claude --resume` in Windows Terminal.
- Settings: project folder, model, hotkeys, microphone, silence length.
- CI checks and a Windows release archive.

## Next

### Working with agents

- **Continue the session.** A separate hotkey or checkbox sends the next utterance into the same Claude session, to add to a task or answer the agent.
- **Session history.** A "Recent" sidebar lists sessions started from Erindi with their transcript and session ID; clicking one opens it.
- **Prompt cleanup model.** A fast intermediate LLM turns the raw transcript into clear, logical text before it reaches the agent.
- **Provider, model, permission mode.** Pick a provider, then choose from its available models and permission modes instead of typing a model name.
- **Orca and other orchestrators.** Hand tasks to the locally installed Orca and similar agent orchestrators.
- **Computer control (later).** An "Allow computer control" checkbox lets agents click and type in other apps. A short warning hint appears under it; the same hint appears when `bypassPermissions` is selected.

### Voice and models

- **Speech model picker.** A small catalog of speech-to-text models, downloaded from Hugging Face only after the user confirms. Downloaded models look bright, others dimmed but still selectable.

### Controls

- **Hotkey recorder.** Click the field and press the combination, as in games. Mouse buttons are supported.
- **Startup behavior.** Choose between opening Settings on launch and starting minimized to the tray.

### UI and UX

- **Redesign.** Modern look, dark by default, with accent colors.
- **Themes.** Light, dark and system modes, with main colors set as hex codes.
- **Dictionary tab.** Move the dictionary to its own tab; the main page keeps only what matters most.
- **Branding.** Show the name once, following how strong brands handle window titles and headers.
- **Form saving.** Keep one Save button for the whole form.
- **Localization.** English interface by default, with infrastructure for more interface and speech languages.

### Platforms

- **macOS build** for testing, as a primary platform.

## From the original plan

- Adapters for Codex, Pi and Gemini, then Copilot, Qwen and Kimi.
- HTTP providers: OpenAI-compatible APIs, Ollama, OpenRouter.
- API keys stored in the OS keychain.
- Wake word activation.
- Signed installers and macOS notarization.
- Auto-update channel.
- Modifier-only hotkeys.
- Update GitHub Actions versions that still target Node.js 20.
