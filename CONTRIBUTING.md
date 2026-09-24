# Contributing

Erindi is a work in progress. Today it targets Windows x64 and Claude Code only.

## Prerequisites

| Tool | Version | Notes |
|---|---|---|
| Windows | 10 or 11, x64 | WebView2 ships with Windows 11 |
| Rust | stable, 1.88+ | `rustup default stable` |
| MSVC build tools | VS 2019 or newer | "Desktop development with C++" workload |
| Node.js | 22+ | |
| pnpm | 10 | `npm i -g pnpm` |
| Windows Terminal | any | opens finished Claude sessions |
| Claude Code | any recent | `claude` on `PATH` and logged in |

## Set up

```powershell
git clone <repo-url> erindi
cd erindi
./scripts/fetch-models.ps1          # ~490 MB into models/, SHA-256 verified
cd apps/desktop
pnpm install
```

## Run in development

```powershell
cd apps/desktop
pnpm tauri dev
```

The app starts in the tray. Choose **Settings** in the tray menu to open the window, then pick the project folder on the Settings page.

## Test and lint

CI runs the same commands on every pull request into `main`:

```powershell
cd apps/desktop; pnpm build; cd ../..   # the Rust build embeds the frontend
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cd apps/desktop; pnpm test; cd ../..   # frontend unit tests
```

Tests that need the models or a microphone are ignored by default:

```powershell
cargo test --workspace -- --include-ignored
```

## Build a release

```powershell
cd apps/desktop
pnpm tauri build --no-bundle
cd ../..
./scripts/package.ps1               # dist/erindi-<version>-windows-x64.zip
```

The archive holds `erindi.exe`, the sherpa-onnx and onnxruntime DLLs it needs, and `scripts/fetch-models.ps1`. Users unpack it, run the script once, and start `erindi.exe`.

Pull requests into `main` run the checks only. Every merge into `main` makes CI build the same archive and publish it as the `latest` pre-release, replacing the previous one.

## Versions

Commit messages follow [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/). Run `pnpm install` in the repository root once: it installs a `commit-msg` hook that checks each message with commitlint. CI checks the commits of every pull request the same way.

[semantic-release](https://github.com/semantic-release/semantic-release) sets the version from those commits on every merge into `main`. `fix` bumps the patch version and `feat` the minor one. A breaking change (`feat!` or a `BREAKING CHANGE:` footer) bumps the minor version too, because 1.0.0 is a deliberate step, not a commit type. Other types release nothing.

A release writes the version into every file that carries one (`scripts/set-version.mjs`), commits it as `chore(release): X.Y.Z`, tags `vX.Y.Z` and publishes a GitHub release that keeps its own archive. Never edit the version numbers by hand.

## Useful tools

- `cargo run -p erindi-core --example claude-smoke -- <folder> <prompt>` sends one prompt to the real `claude` through the same runner the app uses.
- `node scripts/logo.mjs` in `apps/desktop` redraws the logo into `src/logo.svg` and `app-icon.png`. Then run `pnpm tauri icon app-icon.png -o src-tauri/icons` and delete the generated `android` and `ios` folders.

## Local data

| File | Location on Windows |
|---|---|
| `settings.json` | `%APPDATA%\com.viperwow.erindi` |
| `sessions.json` (session history, newest 200) | `%APPDATA%\com.viperwow.erindi` |
| Speech models | `models/` next to the exe, or in the repository for development builds; `ERINDI_MODELS` overrides both |

## Project layout

| Path | Contents |
|---|---|
| `crates/core` | State machine, controller, prompt dictionary, Claude arguments, process runner |
| `crates/audio-asr` | Microphone capture, resampling, Silero VAD, Parakeet ASR |
| `apps/desktop/src-tauri` | Tauri shell: tray, hotkeys, overlay, settings, runtime |
| `apps/desktop/src` | Preact + Tailwind UI: overlay, Sessions and Settings pages |
| `apps/desktop/scripts` | Logo generator |
| `scripts` | Model download and release packaging |

## How we work

- Test first: write a failing test, make it pass, then commit.
- Keep commits small and use [Conventional Commits](https://www.conventionalcommits.org/) (`feat(core): ...`, `fix(desktop): ...`).
- Rust owns runtime state. The UI only renders events and calls narrowly scoped commands.
- Never build a shell command from user text. Pass the executable and arguments separately, and send prompts through stdin.
- Give each window only the Tauri permissions it needs; `capabilities/*.json` is covered by tests.
- Write code, comments and user-facing strings in English.
