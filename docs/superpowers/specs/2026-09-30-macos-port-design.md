# macOS port

Date: 2026-09-30. Branch: `feat/macos-build`.

## Goal

Erindi works on macOS the way it works on Windows: shortcuts, recording and recognition, the overlay, agent runs, "Open in terminal", the local command model and launch at login. GitHub Actions builds a macOS dmg and publishes it next to the Windows zip. Windows behaviour does not change, except where a decision below says so for both platforms.

Scope: macOS 15 Sequoia or later, two builds: Apple Silicon (arm64) and Intel (x86_64). In May 2026 macOS 15 and later ran on about 86% of Macs (StatCounter).

## Decisions

| # | Topic | Decision |
|---|-------|----------|
| 1 | Distribution and signing | Public open-source release, ad-hoc signed, no Apple Developer account. The first launch is blocked by Gatekeeper; README and the release notes give the two ways that work since Sequoia, which removed Control-click → Open: **Open Anyway** in System Settings → Privacy & Security (asks for the admin password, shown for about an hour after the blocked launch), or `xattr -dr com.apple.quarantine /Applications/Erindi.app`, the only fix when macOS calls the download damaged. Developer ID signing and notarization stay in ROADMAP until a paid release. |
| 2 | Packaging | Two dmgs built by Tauri, `erindi-X.Y.Z-macos-arm64.dmg` and `erindi-X.Y.Z-macos-x64.dmg`. sherpa-onnx links statically on macOS, so the bundle carries no dylibs. `MACOSX_DEPLOYMENT_TARGET=15.0` for the build and `minimumSystemVersion` 15.0 in the bundle, so an older macOS refuses the app instead of crashing it. The matching llama.cpp release (`macos-arm64` with Metal, `macos-x64` for Intel) ships inside the bundle in `Contents/Resources/llama/`, pinned by build number and SHA-256 like the Windows Vulkan build. |
| 3 | CI | `ci.yml` runs the same checks on a `windows-latest` + `macos-latest` matrix: `cargo test`, `clippy -D warnings`, `fmt`, `tsc`, `pnpm test`. `release.yml` has three build jobs (Windows, macOS arm64, macOS x64) that upload their file as a workflow artifact, and one publish job after all of them that recreates `latest` with the three files and attaches them to the version release. A failed build blocks the whole release. The x64 build runs on `macos-15-intel`, which GitHub keeps until August 2027; after that it cross-compiles with `--target x86_64-apple-darwin` on the arm64 runner (a ROADMAP item with that date). CI checks run on arm64 only. |
| 4 | Child process lifetime | On macOS, Erindi starts a copy of itself as a guard (`Erindi --guard`) connected by a pipe. Erindi tells the guard the process group of every agent run and of llama-server. When the pipe closes (quit, crash, `kill -9`), the guard kills every recorded group and exits. Windows keeps its Job Objects. |
| 5 | Open in terminal | Terminal.app only. Erindi writes a temporary executable `.command` script that changes to the folder and runs the agent command, opens it with `open -a Terminal`, and deletes it once Terminal has read it. No Automation permission. Windows keeps Windows Terminal. |
| 6 | Finding and starting agents | On macOS, Erindi reads the user's login-shell environment once at startup: `$SHELL -ilc` prints `env` between two markers, so whatever the rc files print stays outside, with a 5 s timeout. A shell that is not zsh, bash or sh (fish, Nushell), or an unset `SHELL`, uses `/bin/zsh`. The snapshot is kept in memory and refreshed by the agents recheck in Settings. Agents start directly, with that environment, and are found on its PATH, the same search Windows uses. A change to the rc files reaches Erindi after a restart or a recheck. |
| 7 | Agent environment | No allow-list on either platform. An agent gets Erindi's full environment, on macOS the login-shell snapshot, the same as a run started by hand. The per-agent filters and their tests go. |
| 8 | Paths | The home folder comes from `std::env::home_dir()` on both platforms. Models live in `%LOCALAPPDATA%\Erindi\models` on Windows and `~/Library/Application Support/Erindi/models` on macOS. Settings and history stay in Tauri's config folder. Erindi never writes inside `Erindi.app`; the models-next-to-the-executable lookup is Windows-only. |
| 9 | Shortcuts | macOS defaults below. The UI shows macOS shortcuts with symbols (⌥, ⌘, ⇧, ⌃); settings store them in the current format. A shortcut the system refuses gives "Hotkey X is taken by the OS or another app; choose another" on both platforms. |
| 10 | Overlay and menu bar | The overlay is a non-activating NSPanel made with the `tauri-nspanel` crate (non-activating style, cannot become key, joins all Spaces, full-screen auxiliary, status level): it never takes focus, shows above full-screen apps and on every Space, and lets clicks through its empty part. The tray icon becomes a monochrome menu-bar template icon with the same menu. Erindi has no Dock icon and is not in Cmd+Tab: `LSUIElement` in Info.plist for the bundle, `ActivationPolicy::Accessory` at startup for `tauri dev`. Settings opens from the menu bar. |
| 11 | Copy | Error texts say "terminal" instead of "Windows Terminal". Hints that name a shortcut show the one set in Settings. README gets a Download line with the three files and which Mac takes which, a shortcut table with Windows and macOS columns, a "First launch on macOS" block and a note on the microphone permission prompt. ROADMAP marks the macOS build done. The `latest` release notes carry the first-launch steps. |

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
- **Shell environment.** A macOS-only module reads and caches the login-shell environment. `cli::current_path` returns its PATH on macOS; `cli::find` splits PATH by the platform separator and uses PATHEXT only on Windows. Agent runs get the snapshot as their environment through `RunSpec::env`.
- **Guard.** A small module in the desktop crate: `spawn_guard()` at startup on macOS, `guard.track(pgid)` after each spawn. The guard mode is a branch at the top of `main` that never starts Tauri.
- **llama-server.** `llama.rs` keeps the Windows job; on macOS the server starts as its own process group and is tracked by the guard. The executable name is `llama-server` without `.exe`, found in the bundle's Resources on macOS and in `models/llama/` in development.
- **Overlay.** macOS-only window setup in `overlay.rs` behind `cfg(target_os = "macos")`, converting the overlay to a `tauri-nspanel` panel. Tauri's `macos-private-api` is on for the transparent window.
- **Info.plist.** `NSMicrophoneUsageDescription` and `LSUIElement` (no Dock icon).

## Error handling

| Case | Behaviour |
|---|---|
| The login shell fails, hangs past 5 s or prints no markers | Erindi logs it and uses its own environment; agents outside the short PATH show as not installed until a recheck succeeds. |
| `open -a Terminal` fails | The session is forgotten, the error shows, as with `wt.exe` today. |
| The guard fails to start | Erindi runs without it and logs the error; normal quit still kills children. |
| Microphone permission denied | The existing microphone error shows; README says where to allow it. |

## Testing

- Unit tests on both platforms in CI: parsing the marked `env` block with rc-file noise around it and values holding newlines and `=`; the shell choice for zsh, bash, fish and an unset `SHELL`; PATH search with `:` on macOS; the `.command` script quotes the folder and arguments safely; the platform shortcut defaults; the models folder per platform.
- The runner test that checks the process tree dies uses `kill -0` on macOS instead of `tasklist`.
- A guard test: start the guard, track a sleeping child's group, close the pipe, assert the child is gone.
- The x64 build is smoke-tested on the Apple Silicon Mac under Rosetta: it launches, records and runs an agent.
- Manual on the Mac: first launch through Open Anyway; microphone prompt; hold to talk in another app without it losing focus; overlay over a full-screen app; a run with Claude installed in `~/.local/bin` and Codex through nvm; Open in terminal; the local command model; launch at login; quit with a run going and check nothing is left.
