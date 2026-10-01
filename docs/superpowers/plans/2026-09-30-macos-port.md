# macOS Port Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Erindi runs on macOS 15+ (arm64 and x86_64) with the same features as on Windows, and GitHub Actions publishes both dmgs next to the Windows zip.

**Architecture:** Platform differences stay behind `cfg(target_os = "macos")` / `cfg(windows)` at a few seams: the agent environment (login-shell snapshot), child lifetime (a guard process), the terminal launcher (`.command` script), paths, default shortcuts and the overlay window. Shared code does not branch on the platform.

**Tech Stack:** Rust (Tauri 2, process-wrap, sherpa-onnx), Preact + TS, GitHub Actions.

**Spec:** `docs/superpowers/specs/2026-09-30-macos-port-design.md`

## Global Constraints

- macOS 15 Sequoia or later: `MACOSX_DEPLOYMENT_TARGET=15.0`, bundle `minimumSystemVersion` `15.0`.
- Two macOS builds: `aarch64-apple-darwin` and `x86_64-apple-darwin`; files `erindi-X.Y.Z-macos-arm64.dmg`, `erindi-X.Y.Z-macos-x64.dmg`.
- Ad-hoc signing only (`signingIdentity: "-"`); no Apple Developer account, no notarization.
- Windows behaviour stays the same except: no env allow-list, and the hotkey error text "Hotkey X is taken by the OS or another app; choose another".
- Erindi never writes inside `Erindi.app`.
- Every task ends green on Windows (`cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all --check`, `pnpm -C apps/desktop exec tsc --noEmit`, `pnpm -C apps/desktop test`) and, from Task 1 on, the same Rust commands on the Mac over SSH.
- Mac commands: `SSH_AUTH_SOCK=~/.ssh/agent.sock ssh erindi-mac 'source ~/.cargo/env; cd ~/src/erindi && git fetch -q && git checkout -q feat/macos-build && git pull -q && <cmd>'`. If the agent is empty or the Mac is unreachable, stop and ask the user.
- Commit headers ≤ 100 characters, Conventional Commits, no AI attribution.

## Review Focus

1. A login shell whose rc files print text, prompt, or take longer than 5 s — Erindi must still start and run agents with its own environment.
2. An agent prompt with quotes, `$`, backticks, newlines and Cyrillic sent to "Open in terminal" on macOS — the command in Terminal must receive it verbatim, never evaluated.
3. Erindi killed with `kill -9` during a run and while llama-server is loaded — nothing of Erindi's may remain.
4. The overlay shown while a full-screen app is focused — the app keeps focus and typing continues.
5. A settings file written on Windows (Ctrl/Alt shortcuts) opened on macOS — shortcuts register or fail with the shared error; nothing crashes.

---

### Task 1: The workspace builds and tests clean on macOS

**Files:**
- Modify: `crates/core/tests/runner.rs:27-33`, `crates/core/src/cli.rs:1-60`, `crates/core/src/llama.rs:44`

**Interfaces:**
- Produces: `cli::find(name: &str, path: &str, pathext: &str) -> Option<PathBuf>` splits `path` with `std::env::split_paths` semantics (`;` on Windows, `:` on unix) and ignores `pathext` off Windows (tries the bare name).

- [ ] **Step 1: Write the failing test** in `cli.rs` tests, `#[cfg(unix)]`:

```rust
#[test]
fn a_bare_cli_is_found_on_a_colon_path() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let cli = touch(b.path(), "codex");
    let path = format!("{}:{}", a.path().display(), b.path().display());
    assert_eq!(find("codex", &path, ""), Some(cli));
}
```

- [ ] **Step 2: Run on the Mac** — `cargo test -p erindi-core --lib cli` — Expected: FAIL (`None`).
- [ ] **Step 3: Implement** the `find` change; move `merge` under `#[cfg(windows)]`; drop the `mut` the Mac build warns about in `llama.rs` (keep it where Windows needs it via `#[cfg_attr]` or by restructuring); in `runner.rs` make `alive(pid)` use `kill -0 <pid>` on unix and `tasklist` on Windows.
- [ ] **Step 4: Verify** — Windows and Mac: full Rust check list from Global Constraints. Expected: all pass, no warnings.
- [ ] **Step 5: Commit** — `fix: core builds and tests clean on macOS`

### Task 2: Agents get the full environment

**Files:**
- Modify: `crates/core/src/agent.rs` (remove `env`), `crates/core/src/claude.rs:34-70` and tests, `crates/core/src/codex.rs:11-18`, `crates/core/src/pi.rs:9-18`, `apps/desktop/src-tauri/src/runtime.rs:635-645`

**Interfaces:**
- Produces: `runtime::run_env(vars, path) -> Vec<(String, String)>` unchanged; `RunSpec::env` is `run_env(<base vars>, <path>)` with no filter.

- [ ] **Step 1: Write the failing test** in `runtime.rs` tests:

```rust
#[test]
fn run_env_keeps_every_variable() {
    let vars = [("GITHUB_TOKEN", "t"), ("OPENAI_API_KEY", "k")].map(|(k, v)| (k.to_string(), v.to_string()));
    let env = run_env(vars, "p".into());
    assert!(env.contains(&("GITHUB_TOKEN".into(), "t".into())));
    assert!(env.contains(&("OPENAI_API_KEY".into(), "k".into())));
}
```

- [ ] **Step 2: Run** — `cargo test -p erindi-desktop run_env` — Expected: PASS already for `run_env` alone; then assert on the `RunSpec` built for a run: extract the env construction at `runtime.rs:639` into `fn agent_env(path: String) -> Vec<(String, String)>` and test that it keeps `GITHUB_TOKEN` from a given vars list — Expected: FAIL while `agent::env` filters it.
- [ ] **Step 3: Implement** — delete `agent::env`, `claude_env`, `codex_env`, `pi_env`, `ENV_ALLOW*`, `base_env_allowed` and their tests; `agent_env` returns `run_env(vars, path)`.
- [ ] **Step 4: Verify** — full check list, Windows and Mac. Expected: pass.
- [ ] **Step 5: Commit** — `feat: agents get the full environment, as when started by hand`

### Task 3: Home and data paths per platform

**Files:**
- Modify: `apps/desktop/src-tauri/src/lib.rs:154`, `runtime.rs:222,233-249,722,750`, `settings.rs:108`

**Interfaces:**
- Produces: `runtime::home() -> Option<PathBuf>` (`std::env::home_dir()`), `pick_models_dir(env, exe_dir, data_dir, debug)` unchanged signature; `data_dir` is `%LOCALAPPDATA%\Erindi` on Windows, `~/Library/Application Support/Erindi` on macOS; `exe_dir` is consulted on Windows only.

- [ ] **Step 1: Write the failing test** `#[cfg(target_os = "macos")] fn models_next_to_the_app_are_ignored()`: with an `exe_dir` holding `models/` and a `data_dir`, `pick_models_dir(None, Some(exe), Some(data), false) == data.join("models")`.
- [ ] **Step 2: Run on the Mac** — Expected: FAIL (returns the exe-side folder).
- [ ] **Step 3: Implement** — replace every `USERPROFILE` read with `home()`; macOS `data_dir`; `cfg(windows)` around the exe-side lookup; settings default `cwd` from `home()`.
- [ ] **Step 4: Verify** — full check list, both platforms. Expected: pass; existing Windows path tests unchanged.
- [ ] **Step 5: Commit** — `feat: home and data folders on macOS`

### Task 4: Login-shell environment snapshot (macOS)

**Files:**
- Create: `apps/desktop/src-tauri/src/shell_env.rs`
- Modify: `apps/desktop/src-tauri/src/main.rs`, `lib.rs` (startup), `agents.rs:71,113-160` (recheck, locate), `runtime.rs` (`agent_env` from Task 2)

**Interfaces:**
- Consumes: `cli::find` (Task 1), `agent_env` (Task 2).
- Produces:
  - `erindi --print-env` prints `ERINDI-ENV-BEGIN\n<JSON object of the process env>\nERINDI-ENV-END\n` and exits, before Tauri starts.
  - `shell_env::login_shell(shell: Option<&str>) -> &str` — the given path when its file name is `zsh`, `bash` or `sh`, else `/bin/zsh`.
  - `shell_env::parse(output: &str) -> Option<Vec<(String, String)>>` — the JSON between the last BEGIN/END pair.
  - `shell_env::read_env(shell: &str, timeout: Duration) -> Option<Vec<(String, String)>>` — runs `<shell> -ilc '"<current_exe>" --print-env'` with stdin null, kills it at `timeout`, parses the output.
  - `shell_env::refresh()` — `read_env(login_shell(SHELL), 5 s)`; runs `<login_shell> -ilc '"<current_exe>" --print-env'` with stdin null, kills it after 5 s, stores the result; logs and keeps the previous snapshot on failure.
  - `shell_env::vars() -> Vec<(String, String)>` — the snapshot, or `std::env::vars()` when none.
  - `shell_env::path() -> String` — `PATH` from `vars()`.
  - On Windows the module compiles to `vars() = std::env::vars()` and `path() = cli::current_path()`.

- [ ] **Step 1: Write the failing tests** in `shell_env.rs`:

```rust
#[test]
fn rc_noise_around_the_block_is_ignored() {
    let out = "Welcome!\nERINDI-ENV-BEGIN\n{\"PATH\":\"/a:/b\",\"X\":\"1=2\\nline\"}\nERINDI-ENV-END\nbye\n";
    let vars = parse(out).unwrap();
    assert!(vars.contains(&("PATH".into(), "/a:/b".into())));
    assert!(vars.contains(&("X".into(), "1=2\nline".into())));
}
#[test]
fn no_block_is_none() { assert_eq!(parse("oops"), None); }
#[cfg(unix)]
#[test]
fn a_hanging_shell_gives_up_after_the_timeout() {
    let dir = tempfile::tempdir().unwrap();
    let sh = dir.path().join("zsh");
    std::fs::write(&sh, "#!/bin/sh
sleep 30
").unwrap();
    std::fs::set_permissions(&sh, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let started = std::time::Instant::now();
    assert_eq!(read_env(sh.to_str().unwrap(), std::time::Duration::from_secs(1)), None);
    assert!(started.elapsed() < std::time::Duration::from_secs(3));
}
#[test]
fn only_posix_shells_are_trusted() {
    assert_eq!(login_shell(Some("/opt/homebrew/bin/zsh")), "/opt/homebrew/bin/zsh");
    assert_eq!(login_shell(Some("/bin/bash")), "/bin/bash");
    assert_eq!(login_shell(Some("/opt/homebrew/bin/fish")), "/bin/zsh");
    assert_eq!(login_shell(None), "/bin/zsh");
}
```

- [ ] **Step 2: Run** — `cargo test -p erindi-desktop shell_env` — Expected: FAIL (module missing).
- [ ] **Step 3: Implement** the module and `--print-env` in `main.rs`; call `refresh()` on a background thread at startup (macOS) and inside `Agents::recheck` before locating; `agents::locate` uses `cli::find(agent.cli(), &shell_env::path(), <PATHEXT on Windows>)`; `agent_env` uses `shell_env::vars()` and `shell_env::path()`.
- [ ] **Step 4: Verify** — full check list, both platforms; on the Mac also `cargo run -p erindi-desktop -- --print-env | head -3` shows the BEGIN marker. Expected: pass.
- [ ] **Step 5: Commit** — `feat: agents on macOS see the login-shell environment`

### Task 5: Guard process kills children when Erindi dies (macOS)

**Files:**
- Create: `apps/desktop/src-tauri/src/guard.rs`
- Modify: `main.rs`, `lib.rs` (startup), `crates/core/src/run.rs:39-62` (report the spawned pid), `crates/core/src/llama.rs` (process group, `pid()`), `runtime.rs` (track runs and the server)

**Interfaces:**
- Produces:
  - `erindi_core::run::run(spec, cancel, on_line, on_spawn: impl FnOnce(u32))` — `on_spawn` gets the child pid, which is its process group id on unix.
  - `LlamaServer::pid(&self) -> u32`; on unix the server starts with `process_group(0)`.
  - `erindi --guard` reads decimal process group ids, one per line, from stdin; on EOF sends SIGTERM to each group, waits 1 s, sends SIGKILL, exits.
  - `guard::start()` spawns `current_exe --guard` with piped stdin and keeps the `ChildStdin` in a static; `guard::track(pgid: u32)` writes one line; both are no-ops on Windows and when the guard failed to start (logged).

- [ ] **Step 1: Write the failing test** `#[cfg(unix)] #[test] fn closing_the_pipe_kills_tracked_groups()` in `guard.rs`: spawn `sleep 30` with `process_group(0)`; run the guard loop on a pipe in a thread (`guard::watch(reader)`), write the pid, drop the writer, join; assert `kill -0 <pid>` fails within 3 s.
- [ ] **Step 2: Run on the Mac** — Expected: FAIL (module missing).
- [ ] **Step 3: Implement** — `guard::watch(impl BufRead)` holds the loop so the test and `--guard` share it; update `run` callers (runtime and `crates/core/tests/runner.rs` with `|_| {}`); runtime calls `guard::track` from `on_spawn` and after `LlamaServer::start`; `guard::start()` first thing in `run()` on macOS.
- [ ] **Step 4: Verify** — full check list, both platforms. Expected: pass.
- [ ] **Step 5: Commit** — `feat: agents and llama-server die with Erindi on macOS`

### Task 6: Open in terminal on macOS

**Files:**
- Modify: `crates/core/src/agent.rs:197-279`, `claude.rs:135-171`, `codex.rs:60-85`, `pi.rs:33-61` and their tests; `runtime.rs:193-215,555-585,730-746`
- Create: `apps/desktop/src-tauri/src/terminal.rs`

**Interfaces:**
- Produces:
  - `erindi_core::agent::TerminalCommand { cwd: String, program: String, args: Vec<String> }`; `terminal_args` and `resume_in_terminal` return `Result<TerminalCommand, InvalidRequest>` with the prompt unescaped.
  - `terminal::wt_args(&TerminalCommand) -> Vec<String>` — today's Windows Terminal argument list (`-d`, cwd, program, args with `;` as `\;`).
  - `terminal::command_script(&TerminalCommand) -> String` — `#!/bin/zsh -il\ncd <q(cwd)> && exec <q(program)> <q(arg)>…\n`, where `q` single-quotes and writes `'` as `'\''`.
  - `terminal::open(&TerminalCommand) -> Result<(), String>` — Windows: `wt.exe` with `wt_args`; macOS: writes the script to `$TMPDIR/erindi-<uuid>.command` (mode 0700), runs `open -a Terminal <file>`, deletes the file after 10 s on a thread. Errors read "Cannot open a terminal: {e}".

- [ ] **Step 1: Write the failing tests** in `terminal.rs`: `wt_args` reproduces each current `["-d", "C:/p", …]` expectation moved from the core tests; `command_script` for cwd `/Users/me/it's here` and args `["--", "a 'b' $HOME `x`\nпривет"]` equals

```text
#!/bin/zsh -il
cd '/Users/me/it'\''s here' && exec 'claude' '--' 'a '\''b'\'' $HOME `x`
привет'
```

- [ ] **Step 2: Run** — `cargo test -p erindi-desktop terminal` — Expected: FAIL (module missing).
- [ ] **Step 3: Implement** — core returns `TerminalCommand`; the three runtime call sites use `terminal::open`; doc comments stop saying "Windows Terminal".
- [ ] **Step 4: Verify** — full check list, both platforms; on the Mac write a script for `cwd = ~/src/erindi`, `program = echo`, `args = ["ok"]` through a scratch test binary or `cargo test -- --ignored` and confirm Terminal opens and prints `ok` (ask the user to look). Expected: pass.
- [ ] **Step 5: Commit** — `feat: open in terminal on macOS through Terminal.app`

### Task 7: Shortcuts on macOS

**Files:**
- Modify: `apps/desktop/src-tauri/src/settings.rs:92-110` and tests, `lib.rs:393`, `apps/desktop/src/hotkey.ts`, `apps/desktop/src/hotkey.test.ts`, `apps/desktop/src/controls.tsx:245`, `apps/desktop/src/sessions.tsx:96`

**Interfaces:**
- Produces:
  - macOS `Settings::default()` shortcuts: talk `Alt+Space` Hold, cancel `Alt+Space` Tap, new session `Alt+Shift+Space` Hold, hands-free `Super+Shift+Space` DoubleTap, new-session hands-free `Super+Alt+Shift+Space` DoubleTap, terminal `Super+Alt+T` Tap. Windows unchanged.
  - Error text: `"Hotkey {combo} is taken by the OS or another app; choose another: {e}"`.
  - `displayHotkey(combo: string, mac: boolean): string` in `hotkey.ts` — on macOS maps `Ctrl`→`⌃`, `Alt`→`⌥`, `Shift`→`⇧`, `Super`→`⌘`, joined without `+`, key last (`"Super+Shift+Space"` → `"⌘⇧Space"`); elsewhere returns the combo unchanged.
  - `isMac = navigator.userAgent.includes("Mac")` exported from `hotkey.ts`.

- [ ] **Step 1: Write the failing tests** — Rust `#[cfg(target_os = "macos")] fn mac_defaults()` asserts the combos list `["Alt+Space", "Alt+Shift+Space", "Super+Alt+T", "Super+Shift+Space", "Super+Alt+Shift+Space"]`; TS:

```ts
test("mac shows symbols", () => {
  assert.equal(displayHotkey("Super+Shift+Space", true), "⌘⇧Space");
  assert.equal(displayHotkey("Ctrl+Alt+T", true), "⌃⌥T");
  assert.equal(displayHotkey("Ctrl+Alt+T", false), "Ctrl+Alt+T");
});
```

- [ ] **Step 2: Run** — `pnpm -C apps/desktop test`, Mac `cargo test -p erindi-desktop mac_defaults` — Expected: FAIL.
- [ ] **Step 3: Implement** — cfg defaults; error text; `HotkeyInput` shows `displayHotkey(props.value, isMac)`; the Sessions empty state reads the talk shortcut from settings and shows it with `displayHotkey`.
- [ ] **Step 4: Verify** — full check list, both platforms. Expected: pass.
- [ ] **Step 5: Commit** — `feat: macOS default shortcuts shown with key symbols`

### Task 8: Overlay panel, menu bar and no Dock icon

**Files:**
- Modify: `apps/desktop/src-tauri/Cargo.toml` (macOS-only `tauri-nspanel`, v2 branch from `ahkohd/tauri-nspanel`), `overlay.rs:40-60`, `lib.rs:28-35,85-100`, `apps/desktop/src-tauri/Info.plist`
- Create: `apps/desktop/src-tauri/icons/tray-template.png` (black glyph of the logo on transparent, 44×44)

**Interfaces:**
- Produces: on macOS the overlay window is converted to a panel with non-activating style, `can_become_key_window = false`, collection behaviour can-join-all-spaces + full-screen-auxiliary + stationary, level status; the tray uses `tray-template.png` with `icon_as_template(true)`; `set_activation_policy(ActivationPolicy::Accessory)` in `setup`; `Info.plist` has `LSUIElement` true.

- [ ] **Step 1: Implement** the above behind `cfg(target_os = "macos")`; register the nspanel plugin on macOS only.
- [ ] **Step 2: Verify** — Mac `cargo build -p erindi-desktop`; Windows full check list. Then on the Mac run the debug build and ask the user to check: full-screen Safari keeps focus while holding ⌥Space; overlay visible over it and on another Space; menu-bar icon adapts to light/dark; no Dock icon; Settings opens from the menu bar. Expected: all yes.
- [ ] **Step 3: Commit** — `feat: macOS overlay panel and menu-bar app`

### Task 9: macOS bundle

**Files:**
- Create: `apps/desktop/src-tauri/tauri.macos.conf.json`, `scripts/fetch-llama-macos.sh`
- Modify: `crates/audio-asr/Cargo.toml` (static sherpa-onnx on macOS), `runtime.rs:311-321` (`pick_llama_server`), `.gitignore`

**Interfaces:**
- Produces:
  - `tauri.macos.conf.json`: `bundle.targets ["app", "dmg"]`, `bundle.macOS.minimumSystemVersion "15.0"`, `bundle.macOS.signingIdentity "-"`, `bundle.resources { "llama/": "llama/" }`.
  - `scripts/fetch-llama-macos.sh <arm64|x64>` downloads the llama.cpp build `b11095` for that arch, checks its SHA-256 (pinned in the script; record the hashes from the release when writing it), unpacks `llama-server` and its libraries into `apps/desktop/src-tauri/llama/` (git-ignored), and ad-hoc signs each binary with `codesign -s - -f`.
  - `pick_llama_server(exe_dir, models)` on macOS checks `<exe_dir>/../Resources/llama/llama-server`, then `models/llama/llama-server`; the Windows `.exe` names stay.
  - sherpa-onnx: `[target.'cfg(target_os = "macos")'.dependencies] sherpa-onnx = { version = "1.13.8" }` (static default) with the existing `shared` entry moved under `cfg(windows)`.

- [ ] **Step 1: Write the failing test** `#[cfg(target_os = "macos")] fn llama_server_is_found_in_the_bundle()` — a temp `Erindi.app/Contents/{MacOS,Resources/llama/llama-server}` layout; `pick_llama_server(Some(macos_dir), models)` returns the Resources path.
- [ ] **Step 2: Run on the Mac** — Expected: FAIL.
- [ ] **Step 3: Implement** the four pieces.
- [ ] **Step 4: Verify** — on the Mac: `MACOSX_DEPLOYMENT_TARGET=15.0 scripts/fetch-llama-macos.sh arm64 && cd apps/desktop && pnpm install && MACOSX_DEPLOYMENT_TARGET=15.0 pnpm tauri build --target aarch64-apple-darwin`; `otool -L` on the app binary lists no sherpa/onnxruntime dylib; `codesign -dv` shows an ad-hoc signature; the dmg exists. Repeat with `x64` / `x86_64-apple-darwin` (`rustup target add x86_64-apple-darwin` first). Windows full check list. Expected: both dmgs build.
- [ ] **Step 5: Commit** — `build: macOS dmg with static speech libraries and bundled llama-server`

### Task 10: CI and release workflows

**Files:**
- Modify: `.github/workflows/ci.yml`, `.github/workflows/release.yml`

**Interfaces:**
- Produces: `ci.yml` job `check` with `strategy.matrix.os: [windows-latest, macos-latest]`; the Windows-only sherpa steps run `if: runner.os == 'Windows'`. `release.yml` jobs `build-windows` (today's steps up to Package, then `actions/upload-artifact` of `dist/erindi-*.zip`), `build-macos` with matrix `{arch: arm64, runner: macos-latest, target: aarch64-apple-darwin}` and `{arch: x64, runner: macos-15-intel, target: x86_64-apple-darwin}` (fetch llama, build, copy the dmg to `dist/erindi-<version>-macos-<arch>.dmg`, upload), and `publish` with `needs: [version, build-windows, build-macos]` that downloads all artifacts and runs today's Publish and Attach steps over `dist/*`. The `latest` notes gain the macOS first-launch steps from Task 11.

- [ ] **Step 1: Implement** both workflows.
- [ ] **Step 2: Verify** — push the branch; `gh run watch` on the CI run: both matrix legs green. Trigger `release.yml` with `workflow_dispatch` only if it already supports it; otherwise verify with `act`-free review and confirm on the first merge. Expected: CI green on both OSes.
- [ ] **Step 3: Commit** — `ci: macOS checks and dmg releases for Apple Silicon and Intel`

### Task 11: Docs

**Files:**
- Modify: `README.md`, `ROADMAP.md`

- [ ] **Step 1: Write** — Download line naming the three files ("Apple Silicon (M1 and later)" / "Intel"); shortcut table with Windows and macOS columns from the spec; "First launch on macOS": Open Anyway in System Settings → Privacy & Security (admin password, about an hour after the blocked launch) or `xattr -dr com.apple.quarantine /Applications/Erindi.app`, and that macOS asks for the microphone on the first recording; requirements "macOS 15 or later". ROADMAP: tick the macOS build; add "Intel macOS builds move to cross-compilation when GitHub retires `macos-15-intel` (August 2027)"; keep notarization.
- [ ] **Step 2: Verify** — `git diff --stat`; README renders (open on GitHub after push).
- [ ] **Step 3: Commit** — `docs: macOS download, first launch and shortcuts`

### Task 12: Builds for hand testing

- [ ] **Step 1: Windows** — `pnpm -C apps/desktop tauri build --no-bundle`. Expected: `target\release\erindi-desktop.exe`.
- [ ] **Step 2: Mac over SSH** — pull the branch, `scripts/fetch-llama-macos.sh arm64`, `MACOSX_DEPLOYMENT_TARGET=15.0 pnpm -C apps/desktop tauri build --target aarch64-apple-darwin`; the same for `x64`. Expected: `~/src/erindi/target/aarch64-apple-darwin/release/bundle/dmg/Erindi_<version>_aarch64.dmg` and `…/x86_64-apple-darwin/…_x64.dmg`, plus the `.app` next to each under `bundle/macos/`.
- [ ] **Step 3: Report** the exact paths from `ls` and the manual checklist from the spec's Testing section, including the x64 app under Rosetta.
