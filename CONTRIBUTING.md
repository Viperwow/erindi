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

The app starts in the tray. Open **Settings** from the tray icon to choose the project folder.

## Test and lint

CI runs the same commands on every push and pull request:

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

Pushing a tag `vX.Y.Z` makes CI build the same archive and attach it to a GitHub pre-release. Bump the version in `Cargo.toml`, `apps/desktop/package.json` and `apps/desktop/src-tauri/tauri.conf.json` first.

## Project layout

| Path | Contents |
|---|---|
| `crates/core` | State machine, controller, prompt dictionary, Claude arguments, process runner |
| `crates/audio-asr` | Microphone capture, resampling, Silero VAD, Parakeet ASR |
| `apps/desktop/src-tauri` | Tauri shell: tray, hotkeys, overlay, settings, runtime |
| `apps/desktop/src` | Preact + Tailwind UI for the overlay and settings |
| `scripts` | Model download and release packaging |

## How we work

- Test first: write a failing test, make it pass, then commit.
- Keep commits small and use [Conventional Commits](https://www.conventionalcommits.org/) (`feat(core): ...`, `fix(desktop): ...`).
- Rust owns runtime state. The UI only renders events and calls narrowly scoped commands.
- Never build a shell command from user text. Pass the executable and arguments separately, and send prompts through stdin.
- Give each window only the Tauri permissions it needs; `capabilities/*.json` is covered by tests.
- Write code, comments and user-facing strings in English.
