# macOS port

Date: 2026-09-30. Branch: `feat/macos-build`.

## Goal

Erindi works on macOS the way it works on Windows: shortcuts, recording and recognition, the overlay, agent runs, "Open in terminal", the local command model and launch at login. GitHub Actions builds a macOS dmg and publishes it next to the Windows zip. Windows behaviour does not change, except where a decision below says so for both platforms.

Scope: Apple Silicon (arm64), macOS 13 or later. Intel Macs are out.

## Decisions

| # | Topic | Decision |
|---|-------|----------|
| 1 | Distribution and signing | Public open-source release, ad-hoc signed, no Apple Developer account. The first launch shows the unidentified-developer warning; README and the release notes explain **Open Anyway** in System Settings → Privacy & Security, and `xattr -dr com.apple.quarantine /Applications/Erindi.app`. Developer ID signing and notarization stay in ROADMAP until a paid release. |
| 2 | Packaging | A dmg, `erindi-X.Y.Z-macos-arm64.dmg`, built by Tauri. sherpa-onnx links statically on macOS, so the bundle carries no dylibs. The llama.cpp `macos-arm64` release (Metal) ships inside the bundle in `Contents/Resources/llama/`, pinned by build number and SHA-256 like the Windows Vulkan build. |
| 3 | CI | `ci.yml` runs the same checks on a `windows-latest` + `macos-latest` matrix: `cargo test`, `clippy -D warnings`, `fmt`, `tsc`, `pnpm test`. `release.yml` has two build jobs that upload their file as a workflow artifact, and one publish job after both that recreates `latest` with both files and attaches them to the version release. A failed macOS build blocks the whole release. |
| 4 | Child process lifetime | On macOS, Erindi starts a copy of itself as a guard (`Erindi --guard`) connected by a pipe. Erindi tells the guard the process group of every agent run and of llama-server. When the pipe closes (quit, crash, `kill -9`), the guard kills every recorded group and exits. Windows keeps its Job Objects. |
| 5 | Open in terminal | Terminal.app only. Erindi writes a temporary executable `.command` script that changes to the folder and runs the agent command, opens it with `open -a Terminal`, and deletes it once Terminal has read it. No Automation permission. Windows keeps Windows Terminal. |
| 6 | Finding and starting agents | On macOS, background runs start through the user's login shell: `$SHELL -lic 'exec "$0" "$@"' <cli> <args…>` (`/bin/zsh` when `SHELL` is unset). The shell loads the user's rc files and finds the CLI and `node` the way Terminal would; arguments pass as separate words, never joined into a string. "Is the agent installed" in Settings asks the same shell: `command -v <cli>`. Output the rc files print before the agent starts is ignored as non-JSON. Windows keeps its PATH search. |
| 7 | Agent environment | No allow-list on either platform. An agent gets Erindi's full environment, plus on macOS whatever the login shell adds, the same as a run started by hand. The per-agent filters and their tests go. |
| 8 | Paths | The home folder comes from `std::env::home_dir()` on both platforms. Models live in `%LOCALAPPDATA%\Erindi\models` on Windows and `~/Library/Application Support/Erindi/models` on macOS. Settings and history stay in Tauri's config folder. Erindi never writes inside `Erindi.app`; the models-next-to-the-executable lookup is Windows-only. |
| 9 | Shortcuts | macOS defaults below. The UI shows macOS shortcuts with symbols (⌥, ⌘, ⇧, ⌃); settings store them in the current format. A shortcut the system refuses gives "Hotkey X is taken by the OS or another app; choose another" on both platforms. |
| 10 | Overlay and menu bar | The overlay is a non-activating panel: it never takes focus, shows above full-screen apps and on every Space, and lets clicks through its empty part. The tray icon becomes a monochrome menu-bar template icon with the same menu. Erindi has no Dock icon and is not in Cmd+Tab; Settings opens from the menu bar. |
| 11 | Copy | Error texts say "terminal" instead of "Windows Terminal". Hints that name a shortcut show the one set in Settings. README gets a Download line with both files, a shortcut table with Windows and macOS columns, a "First launch on macOS" block and a note on the microphone permission prompt. ROADMAP marks the macOS build done. The `latest` release notes carry the first-launch steps. |

### Default shortcuts

| Action | Windows | macOS |
|---|---|---|
| Push to talk [Hold] | Ctrl+Space | Option+Space |
| Cancel [Tap] | Ctrl+Space | Option+Space |
| New session, push to talk [Hold] | Ctrl+Shift+Space | Option+Shift+Space |
| Hands-free [Double-tap] | Ctrl+Alt+Space | Cmd+Shift+Space |
| New session, hands-free [Double-tap] | Ctrl+Alt+Shift+Space | Cmd+Option+Shift+Space |
| Open in terminal [Tap] | Ctrl+Alt+T | Cmd+Option+T |

macOS reserves Cmd+Space (Spotlight), Cmd+Option+Space (Finder search), Ctrl+Space and Ctrl+Option+Space (input sources). Option+Space is free in macOS but taken by Alfred and ChatGPT by default; a user with either changes it in Settings.

## Structure

- **Platform seams in `erindi-core`.** Terminal commands (`terminal_args`, `resume_in_terminal`) return a platform-neutral command (program, args, cwd). The desktop crate turns it into `wt.exe` arguments on Windows and a `.command` script on macOS. The agent-specific argument lists stay shared.
- **Agent launch.** `run::RunSpec` gains nothing; on macOS the desktop crate wraps program and args in the login-shell form before calling `run`. `cli::locate` stays the Windows path; macOS uses a `which` through the shell.
- **Guard.** A small module in the desktop crate: `spawn_guard()` at startup on macOS, `guard.track(pgid)` after each spawn. The guard mode is a branch at the top of `main` that never starts Tauri.
- **llama-server.** `llama.rs` keeps the Windows job; on macOS the server starts as its own process group and is tracked by the guard. The executable name is `llama-server` without `.exe`, found in the bundle's Resources on macOS and in `models/llama/` in development.
- **Overlay.** macOS-only window setup in `overlay.rs` behind `cfg(target_os = "macos")`: panel style, all Spaces, full-screen auxiliary. Tauri's `macos-private-api` is on for the transparent window.
- **Info.plist.** `NSMicrophoneUsageDescription` and `LSUIElement` (no Dock icon).

## Error handling

| Case | Behaviour |
|---|---|
| The login shell does not start or `command -v` finds nothing | The agent shows as not installed, as on Windows today. |
| The rc files hang | Runs keep their existing timeout; the check in Settings gives up after 5 s and shows the agent as not found. |
| `open -a Terminal` fails | The session is forgotten, the error shows, as with `wt.exe` today. |
| The guard fails to start | Erindi runs without it and logs the error; normal quit still kills children. |
| Microphone permission denied | The existing microphone error shows; README says where to allow it. |

## Testing

- Unit tests on both platforms in CI: the shell wrapper keeps every argument as one word (quotes, spaces, `$`, newlines, Cyrillic); the `.command` script quotes the folder and arguments safely; the platform shortcut defaults; the models folder per platform.
- The runner test that checks the process tree dies uses `kill -0` on macOS instead of `tasklist`.
- A guard test: start the guard, track a sleeping child's group, close the pipe, assert the child is gone.
- Manual on the Mac: first launch through Open Anyway; microphone prompt; hold to talk in another app without it losing focus; overlay over a full-screen app; a run with Claude installed in `~/.local/bin` and Codex through nvm; Open in terminal; the local command model; launch at login; quit with a run going and check nothing is left.
