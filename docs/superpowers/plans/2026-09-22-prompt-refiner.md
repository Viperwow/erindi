# Prompt Refiner Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A local `llama-server` cleans every utterance before Claude sees it, and both the speech model and the cleanup model install from Settings with a Download button.

**Architecture:** Pure logic lives in `erindi-core`: the model catalog and downloader (`models.rs`), the refiner request and response (`refine.rs`), the server process and HTTP client (`llama.rs`), and new controller states. The desktop app wires them: a `Refiner` that owns the server, Tauri commands for model status and download, and Settings blocks. The release zip carries the official Vulkan `llama-server`; models download on demand.

**Tech Stack:** Rust 2024, Tauri 2, Preact + Tailwind, `ureq` 3 (HTTP), `sha2` (hashing), `process-wrap` (Job Object), llama.cpp `b11095` Vulkan build, Qwen2.5-3B-Instruct Q4_K_M.

**Spec:** `docs/superpowers/specs/2026-09-22-prompt-refiner-design.md`

## Global Constraints

- Nothing downloads without the user pressing Download.
- The user never runs a script. `scripts/fetch-models.ps1` is for development only.
- Refining never cuts or drops the user's words. Any failure falls back to the parser's text.
- 1.5 s per utterance is a logged target, not a timeout.
- `llama-server` listens on `127.0.0.1` only and runs in a Job Object with `CREATE_NO_WINDOW`.
- Pinned llama.cpp: `b11095`, asset `llama-b11095-bin-win-vulkan-x64.zip`, SHA-256 `45c586f50af57b7e144aa76c6fc38c544a717c4f4b7b659c489b980f3993412c`.
- Model files (all SHA-256 verified against these values):

| File | Size | SHA-256 | URL |
|------|------|---------|-----|
| `silero_vad.onnx` | 643854 | `9e2449e1087496d8d4caba907f23e0bd3f78d91fa552479bb9c23ac09cbb1fd6` | `https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/silero_vad.onnx` |
| `sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8/encoder.int8.onnx` | 652184281 | `acfc2b4456377e15d04f0243af540b7fe7c992f8d898d751cf134c3a55fd2247` | `https://huggingface.co/csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8/resolve/2bda32ec70b097a55adaa07d9a7173915b43cc78/encoder.int8.onnx` |
| `sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8/decoder.int8.onnx` | 11845275 | `179e50c43d1a9de79c8a24149a2f9bac6eb5981823f2a2ed88d655b24248db4e` | same repo and revision, `decoder.int8.onnx` |
| `sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8/joiner.int8.onnx` | 6355277 | `3164c13fc2821009440d20fcb5fdc78bff28b4db2f8d0f0b329101719c0948b3` | same repo and revision, `joiner.int8.onnx` |
| `sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8/tokens.txt` | 93939 | `d58544679ea4bc6ac563d1f545eb7d474bd6cfa467f0a6e2c1dc1c7d37e3c35d` | same repo and revision, `tokens.txt` |
| `qwen2.5-3b-instruct-q4_k_m.gguf` | 2104932768 | `626b4a6678b86442240e33df819e00132d3ba7dddfe1cdc4fbb18e0a9615c62d` | `https://huggingface.co/Qwen/Qwen2.5-3B-Instruct-GGUF/resolve/7dabda4d13d513e3e842b20f0d435c732f172cbe/qwen2.5-3b-instruct-q4_k_m.gguf` |

- UI copy is English. Comments follow the repository style: few, and only for non-obvious logic.
- Commit messages carry no AI attribution trailers.

## Review Focus

1. **Download interrupted halfway** (app closed, network drop): the next start must not treat the model as installed. Pinned by `partial_file_is_not_installed` in Task 1.
2. **Release build without a `models/` folder next to the exe**: models must go to a writable per-user folder, not the repository path baked in at compile time. Pinned by `release_uses_data_dir` in Task 3.
3. **Model answers the task instead of rewriting it** ("напиши функцию сортировки" returns code): the length check must reject it. Pinned by `answers_are_rejected` in Task 2.
4. **`llama-server` dies between utterances**: the next utterance must still reach Claude, via the parser. Pinned by `failed_refine_runs_the_parser_text` in Task 5.
5. **Hotkey pressed before the speech model exists**: Settings must open instead of silently doing nothing. Pinned by `hotkey_without_model_opens_settings` in Task 3.

---

## File Structure

| File | Responsibility |
|------|----------------|
| `crates/core/src/models.rs` (new) | Model catalog, installed check, download with `.partial` and SHA-256. |
| `crates/core/src/refine.rs` (new) | System prompt, request JSON, response parsing, output acceptance. |
| `crates/core/src/llama.rs` (new) | `llama-server` process in a Job Object, health polling, chat request. |
| `crates/core/src/state.rs` | New states `NoModel`, `Refining` and their events. |
| `crates/core/src/controller.rs` | Missing-model handling, the refine step, `raw` text on runs. |
| `crates/core/examples/refine-bench.rs` (new) | Speed and quality benchmark against a live server. |
| `crates/core/examples/refine-cases.jsonl` (new) | Labeled utterances for the benchmark. |
| `apps/desktop/src-tauri/src/history.rs` | Prompts keep their raw text. |
| `apps/desktop/src-tauri/src/runtime.rs` | Models folder, speech loading, `Refiner`, new effects. |
| `apps/desktop/src-tauri/src/settings.rs` | `cleanup` field. |
| `apps/desktop/src-tauri/src/lib.rs` | Model commands, first-launch Settings. |
| `apps/desktop/src/settings.tsx` | Blocks, model rows, cleanup checkbox. |
| `apps/desktop/src/overlay.tsx`, `sessions.tsx` | New states; raw text in history. |
| `scripts/fetch-models.ps1`, `scripts/package.ps1` | Dev download of the refiner; `llama-server` in the zip. |
| `ROADMAP.md` | Done items and deferred items. |

---

### Task 1: Model catalog and downloader

**Files:**
- Create: `crates/core/src/models.rs`
- Modify: `crates/core/src/lib.rs`, `crates/core/Cargo.toml`

**Interfaces:**
- Produces:
  - `pub struct ModelFile { pub path: &'static str, pub url: &'static str, pub size: u64, pub sha256: &'static str }`
  - `pub struct Model { pub id: &'static str, pub label: &'static str, pub files: &'static [ModelFile] }`
  - `pub const SPEECH: Model`, `pub const CLEANUP: Model`, `pub const CLEANUP_GGUF: &str`
  - `pub fn by_id(id: &str) -> Option<&'static Model>`
  - `impl Model { pub fn installed(&self, dir: &Path) -> bool; pub fn size(&self) -> u64 }`
  - `pub fn download(model: &Model, dir: &Path, progress: impl FnMut(u64, u64)) -> Result<(), String>`

- [ ] **Step 1: Add dependencies**

Run: `cargo add -p erindi-core ureq --features json && cargo add -p erindi-core sha2`
Expected: both appear in `crates/core/Cargo.toml`.

- [ ] **Step 2: Write the failing tests**

Create `crates/core/src/models.rs` with only the test module and `pub mod models;` in `lib.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    const HELLO_SHA: &str = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";

    #[test]
    fn saves_a_file_whose_hash_matches() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a/hello.bin");
        let mut seen = 0;
        save(Cursor::new(b"hello"), &path, HELLO_SHA, |n| seen += n).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"hello");
        assert_eq!(seen, 5);
        assert!(!dir.path().join("a/hello.bin.partial").exists());
    }

    #[test]
    fn hash_mismatch_leaves_nothing_behind() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hello.bin");
        let err = save(Cursor::new(b"hellO"), &path, HELLO_SHA, |_| {}).unwrap_err();
        assert!(err.contains("SHA-256"), "{err}");
        assert!(!path.exists());
        assert!(!dir.path().join("hello.bin.partial").exists());
    }

    #[test]
    fn partial_file_is_not_installed() {
        let dir = tempfile::tempdir().unwrap();
        let model = &SPEECH;
        for f in model.files {
            let path = dir.path().join(f.path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::File::create(&path).unwrap().set_len(f.size).unwrap();
        }
        assert!(model.installed(dir.path()));
        let last = model.files.last().unwrap();
        std::fs::File::create(dir.path().join(last.path))
            .unwrap()
            .set_len(last.size - 1)
            .unwrap();
        assert!(!model.installed(dir.path()));
    }

    #[test]
    fn catalog_lookup() {
        assert_eq!(by_id("speech").unwrap().id, "speech");
        assert_eq!(by_id("cleanup").unwrap().files[0].path, CLEANUP_GGUF);
        assert!(by_id("other").is_none());
        assert!(SPEECH.size() > 600_000_000);
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test -p erindi-core models`
Expected: FAIL to compile, `save`, `SPEECH`, `by_id` not found.

- [ ] **Step 4: Implement**

Above the test module in `crates/core/src/models.rs`:

```rust
use std::io::{Read, Write};
use std::path::Path;

use sha2::{Digest, Sha256};

pub struct ModelFile {
    /// Relative to the models folder.
    pub path: &'static str,
    pub url: &'static str,
    pub size: u64,
    pub sha256: &'static str,
}

pub struct Model {
    pub id: &'static str,
    pub label: &'static str,
    pub files: &'static [ModelFile],
}

/// Smallest files first, so a broken network fails fast.
pub const SPEECH: Model = Model {
    id: "speech",
    label: "Parakeet TDT 0.6B v3 (~670 MB)",
    files: &[
        ModelFile {
            path: "silero_vad.onnx",
            url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/silero_vad.onnx",
            size: 643_854,
            sha256: "9e2449e1087496d8d4caba907f23e0bd3f78d91fa552479bb9c23ac09cbb1fd6",
        },
        ModelFile {
            path: "sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8/tokens.txt",
            url: "https://huggingface.co/csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8/resolve/2bda32ec70b097a55adaa07d9a7173915b43cc78/tokens.txt",
            size: 93_939,
            sha256: "d58544679ea4bc6ac563d1f545eb7d474bd6cfa467f0a6e2c1dc1c7d37e3c35d",
        },
        ModelFile {
            path: "sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8/joiner.int8.onnx",
            url: "https://huggingface.co/csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8/resolve/2bda32ec70b097a55adaa07d9a7173915b43cc78/joiner.int8.onnx",
            size: 6_355_277,
            sha256: "3164c13fc2821009440d20fcb5fdc78bff28b4db2f8d0f0b329101719c0948b3",
        },
        ModelFile {
            path: "sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8/decoder.int8.onnx",
            url: "https://huggingface.co/csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8/resolve/2bda32ec70b097a55adaa07d9a7173915b43cc78/decoder.int8.onnx",
            size: 11_845_275,
            sha256: "179e50c43d1a9de79c8a24149a2f9bac6eb5981823f2a2ed88d655b24248db4e",
        },
        ModelFile {
            path: "sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8/encoder.int8.onnx",
            url: "https://huggingface.co/csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8/resolve/2bda32ec70b097a55adaa07d9a7173915b43cc78/encoder.int8.onnx",
            size: 652_184_281,
            sha256: "acfc2b4456377e15d04f0243af540b7fe7c992f8d898d751cf134c3a55fd2247",
        },
    ],
};
```

```rust
pub const CLEANUP_GGUF: &str = "qwen2.5-3b-instruct-q4_k_m.gguf";

pub const CLEANUP: Model = Model {
    id: "cleanup",
    label: "Qwen2.5-3B-Instruct Q4 (~2.1 GB)",
    files: &[ModelFile {
        path: CLEANUP_GGUF,
        url: "https://huggingface.co/Qwen/Qwen2.5-3B-Instruct-GGUF/resolve/7dabda4d13d513e3e842b20f0d435c732f172cbe/qwen2.5-3b-instruct-q4_k_m.gguf",
        size: 2_104_932_768,
        sha256: "626b4a6678b86442240e33df819e00132d3ba7dddfe1cdc4fbb18e0a9615c62d",
    }],
};

pub fn by_id(id: &str) -> Option<&'static Model> {
    [&SPEECH, &CLEANUP].into_iter().find(|m| m.id == id)
}

impl Model {
    /// Size is checked instead of the hash so the check is instant; downloads verify the hash
    /// before a file gets its final name.
    pub fn installed(&self, dir: &Path) -> bool {
        self.files.iter().all(|f| {
            std::fs::metadata(dir.join(f.path)).is_ok_and(|m| m.len() == f.size)
        })
    }

    pub fn size(&self) -> u64 {
        self.files.iter().map(|f| f.size).sum()
    }
}

/// Downloads the missing files of `model` into `dir`, reporting `(done, total)` bytes.
pub fn download(model: &Model, dir: &Path, mut progress: impl FnMut(u64, u64)) -> Result<(), String> {
    let total = model.size();
    let mut done = 0;
    for f in model.files {
        let path = dir.join(f.path);
        if std::fs::metadata(&path).is_ok_and(|m| m.len() == f.size) {
            done += f.size;
            progress(done, total);
            continue;
        }
        let mut response = ureq::get(f.url)
            .call()
            .map_err(|e| format!("Cannot download {}: {e}", f.path))?;
        let reader = response.body_mut().as_reader();
        save(reader, &path, f.sha256, |n| {
            done += n;
            progress(done, total);
        })?;
    }
    Ok(())
}

/// Writes `reader` to `<path>.partial` and renames it to `path` once the hash matches.
fn save(mut reader: impl Read, path: &Path, sha256: &str, mut progress: impl FnMut(u64)) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let partial = std::path::PathBuf::from(format!("{}.partial", path.display()));
    let mut file = std::fs::File::create(&partial).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0; 1 << 20];
    loop {
        let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        progress(n as u64);
    }
    drop(file);
    let actual = format!("{:x}", hasher.finalize());
    if actual != sha256 {
        let _ = std::fs::remove_file(&partial);
        return Err(format!("{}: SHA-256 mismatch", path.display()));
    }
    std::fs::rename(&partial, path).map_err(|e| e.to_string())
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p erindi-core models`
Expected: 4 passed.

- [ ] **Step 6: Commit**

```bash
git add crates/core
git commit -m "feat(core): model catalog and verified downloads"
```

---

### Task 2: Refiner request and response

**Files:**
- Create: `crates/core/src/refine.rs`
- Modify: `crates/core/src/lib.rs`

**Interfaces:**
- Consumes: `crate::session::Intent`.
- Produces:
  - `#[derive(Debug, Clone, PartialEq)] pub struct Refined { pub intent: Intent, pub text: String }`
  - `pub fn request(text: &str) -> serde_json::Value`
  - `pub fn parse_response(body: &serde_json::Value) -> Option<Refined>`
  - `pub fn accept(input: &str, refined: Option<Refined>) -> Option<Refined>`
  - `pub const WARM_UP: &str`

- [ ] **Step 1: Write the failing tests**

`crates/core/src/refine.rs`, test module only, plus `pub mod refine;` in `lib.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn reply(content: &str) -> serde_json::Value {
        json!({"choices": [{"message": {"role": "assistant", "content": content}}]})
    }

    #[test]
    fn request_is_deterministic_and_typed() {
        let body = request("эээ проверь diff");
        assert_eq!(body["temperature"], 0);
        assert_eq!(body["cache_prompt"], true);
        assert_eq!(body["messages"][0]["content"], SYSTEM_PROMPT);
        assert_eq!(body["messages"][1]["content"], "эээ проверь diff");
        let schema = &body["response_format"]["json_schema"]["schema"];
        assert_eq!(schema["properties"]["intent"]["enum"], json!(["new", "continue", "none"]));
        assert!(body["max_tokens"].as_u64().unwrap() >= 64);
    }

    #[test]
    fn parses_intent_and_text() {
        let r = parse_response(&reply(r#"{"intent":"new","text":" Проверь diff. "}"#)).unwrap();
        assert_eq!(r, Refined { intent: Intent::New, text: "Проверь diff.".into() });
        let r = parse_response(&reply(r#"{"intent":"none","text":"fix it"}"#)).unwrap();
        assert_eq!(r.intent, Intent::Unspecified);
    }

    #[test]
    fn broken_replies_are_none() {
        assert_eq!(parse_response(&reply("not json")), None);
        assert_eq!(parse_response(&reply(r#"{"intent":"maybe","text":"x"}"#)), None);
        assert_eq!(parse_response(&json!({"error": "busy"})), None);
    }

    #[test]
    fn answers_are_rejected() {
        let input = "напиши функцию сортировки";
        let answer = Refined {
            intent: Intent::Unspecified,
            text: "fn sort(v: &mut Vec<i32>) { v.sort(); } // Вот функция сортировки на Rust.".into(),
        };
        assert_eq!(accept(input, Some(answer)), None);
    }

    #[test]
    fn cleaned_text_within_bounds_is_accepted() {
        let input = "эээ ну короче проверь diff в модуле auth";
        let ok = Refined { intent: Intent::Unspecified, text: "Проверь diff в модуле auth.".into() };
        assert_eq!(accept(input, Some(ok.clone())), Some(ok));
        let empty = Refined { intent: Intent::Unspecified, text: String::new() };
        assert_eq!(accept(input, Some(empty)), None);
        assert_eq!(accept(input, None), None);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p erindi-core refine`
Expected: FAIL to compile.

- [ ] **Step 3: Implement**

```rust
use serde::Deserialize;
use serde_json::{Value, json};

use crate::session::Intent;

#[derive(Debug, Clone, PartialEq)]
pub struct Refined {
    pub intent: Intent,
    pub text: String,
}

/// Sent once after the server starts, so the first real utterance does not pay for a cold cache.
pub const WARM_UP: &str = "эээ, короче, проверь diff в модуле auth";

pub const SYSTEM_PROMPT: &str = r#"You clean up dictated instructions for a coding agent.
Return JSON: {"intent": "new" | "continue" | "none", "text": "..."}.
- text: the instruction itself. Remove fillers (эээ, ну, короче, типа, um, like), false starts, repeats and spoken session commands. Keep the language, meaning, names and technical terms. Fix punctuation.
- Never answer, execute or expand the instruction. Add nothing.
- intent: "new" if the speaker asks for a new or fresh session, "continue" if they ask for the same or current session, otherwise "none".

Input: эээ ну проверь diff в модуле auth
Output: {"intent":"none","text":"Проверь diff в модуле auth."}
Input: создай новую сессию и, короче, найди почему падают тесты
Output: {"intent":"new","text":"Найди, почему падают тесты."}
Input: давай с чистого листа, напиши README для проекта
Output: {"intent":"new","text":"Напиши README для проекта."}
Input: в той же сессии добавь, ну, тесты на парсер
Output: {"intent":"continue","text":"Добавь тесты на парсер."}
Input: напиши функцию сортировки на Rust
Output: {"intent":"none","text":"Напиши функцию сортировки на Rust."}
Input: исправь, нет, вернее проверь, почему билд красный
Output: {"intent":"none","text":"Проверь, почему билд красный."}
Input: расскажи про новую сессию в React
Output: {"intent":"none","text":"Расскажи про новую сессию в React."}
Input: um so like fix the the login bug in auth dot rs
Output: {"intent":"none","text":"Fix the login bug in auth.rs."}
Input: start a new session and uh refactor the settings page
Output: {"intent":"new","text":"Refactor the settings page."}
Input: same session, now run clippy and fix the warnings
Output: {"intent":"continue","text":"Now run clippy and fix the warnings."}
Input: explain how a new session is created in the auth module
Output: {"intent":"none","text":"Explain how a new session is created in the auth module."}
Input: what does this function do
Output: {"intent":"none","text":"What does this function do?"}"#;

pub fn request(text: &str) -> Value {
    json!({
        "messages": [
            {"role": "system", "content": SYSTEM_PROMPT},
            {"role": "user", "content": text},
        ],
        "temperature": 0,
        "seed": 42,
        "cache_prompt": true,
        // Roughly 1.5x the input tokens plus room for the JSON wrapper.
        "max_tokens": text.chars().count() * 3 / 4 + 64,
        "response_format": {
            "type": "json_schema",
            "json_schema": {"name": "refined", "strict": true, "schema": {
                "type": "object",
                "properties": {
                    "intent": {"type": "string", "enum": ["new", "continue", "none"]},
                    "text": {"type": "string"},
                },
                "required": ["intent", "text"],
                "additionalProperties": false,
            }},
        },
    })
}

pub fn parse_response(body: &Value) -> Option<Refined> {
    #[derive(Deserialize)]
    struct Reply {
        intent: String,
        text: String,
    }
    let content = body["choices"][0]["message"]["content"].as_str()?;
    let reply: Reply = serde_json::from_str(content).ok()?;
    let intent = match reply.intent.as_str() {
        "new" => Intent::New,
        "continue" => Intent::Continue,
        "none" => Intent::Unspecified,
        _ => return None,
    };
    Some(Refined {
        intent,
        text: reply.text.trim().to_string(),
    })
}

/// Rejects output that is empty or far from the input's length, which is how an answer
/// or a lost sentence usually looks.
pub fn accept(input: &str, refined: Option<Refined>) -> Option<Refined> {
    let refined = refined?;
    let (inp, out) = (input.chars().count() as f64, refined.text.chars().count() as f64);
    (out > 0.0 && out >= inp * 0.3 && out <= inp * 1.5).then_some(refined)
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p erindi-core refine`
Expected: 5 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/core
git commit -m "feat(core): refiner request, response parsing and output check"
```

---

### Task 3: Speech model can be missing

**Files:**
- Modify: `crates/core/src/state.rs`, `crates/core/src/controller.rs`, `apps/desktop/src-tauri/src/runtime.rs`, `apps/desktop/src-tauri/src/lib.rs`, `apps/desktop/src/overlay.tsx`

**Interfaces:**
- Produces:
  - `AppState::NoModel`, `Event::ModelMissing`
  - `Msg::ModelMissing`, `Effect::OpenSettings`
  - `runtime::models_dir()` resolves to a writable folder in release builds
  - `Runtime::load_speech(&self)`
  - `pub fn show_settings(app: &AppHandle)` in `lib.rs`, opening `index.html#settings`

- [ ] **Step 1: Write the failing tests**

In `state.rs` tests:

```rust
#[test]
fn missing_model_waits_for_download() {
    let mut m = Machine::new();
    assert_eq!(m.apply(ModelMissing), Ok(Outcome::Changed(NoModel)));
    assert!(m.apply(StartListening).is_err());
    assert_eq!(m.apply(ModelReady), Ok(Outcome::Changed(Idle)));
}
```

In `controller.rs` tests:

```rust
#[test]
fn hotkey_without_model_opens_settings() {
    let mut c = Controller::new(Box::new(Dictionary::default()));
    let now = Instant::now();
    c.handle(Msg::ModelMissing, now);
    assert_eq!(c.state(), AppState::NoModel);
    assert_eq!(c.handle(Msg::KeyDown(Key::Hold), now), [Effect::OpenSettings]);
    c.handle(Msg::ModelReady, now);
    assert_eq!(c.state(), AppState::Idle);
}
```

In `runtime.rs` tests, replace `falls_back_to_repository_models` with:

```rust
#[test]
fn debug_falls_back_to_repository_models() {
    let dir = tempfile::tempdir().unwrap();
    let picked = pick_models_dir(None, Some(dir.path().into()), Some("D:\\data".into()), true);
    assert!(picked.ends_with("models") && picked.starts_with(env!("CARGO_MANIFEST_DIR")));
}

#[test]
fn release_uses_data_dir() {
    let dir = tempfile::tempdir().unwrap();
    let picked = pick_models_dir(None, Some(dir.path().into()), Some("D:\\data".into()), false);
    assert_eq!(picked, PathBuf::from("D:\\data\\models"));
}
```

and add the two new arguments `None, false` to the calls in `env_var_wins` and `models_next_to_exe_beat_repository` (their expectations do not change).

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p erindi-core && cargo test -p erindi-desktop`
Expected: FAIL to compile, `NoModel`, `ModelMissing`, `OpenSettings` unknown, `pick_models_dir` takes 2 arguments.

- [ ] **Step 3: Implement the core**

`state.rs`: add `NoModel` after `LoadingModel` in `AppState`, `ModelMissing` after `ModelFailed` in `Event`, and two transitions:

```rust
(S::LoadingModel, E::ModelMissing) => S::NoModel,
(S::NoModel, E::ModelReady) => S::Idle,
```

`controller.rs`: add `ModelMissing` to `Msg` after `ModelFailed`, `OpenSettings` to `Effect`, and in `handle`:

```rust
Msg::ModelMissing => self.apply(Event::ModelMissing),
Msg::KeyDown(_) if state == S::NoModel => vec![Effect::OpenSettings],
```

The `KeyDown` arm goes before the existing `Msg::KeyDown(key) => match state` arm.

- [ ] **Step 4: Implement the desktop side**

`runtime.rs`:

```rust
pub fn models_dir() -> PathBuf {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(PathBuf::from));
    let data_dir = std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("Erindi"));
    pick_models_dir(std::env::var_os("ERINDI_MODELS"), exe_dir, data_dir, cfg!(debug_assertions))
}

/// `ERINDI_MODELS` wins, then `models/` next to the executable. Development builds fall back to
/// `models/` in the repository; release builds to a per-user folder, since the exe folder may be read-only.
fn pick_models_dir(
    env: Option<std::ffi::OsString>,
    exe_dir: Option<PathBuf>,
    data_dir: Option<PathBuf>,
    debug: bool,
) -> PathBuf {
    if let Some(env) = env {
        return env.into();
    }
    if let Some(dir) = exe_dir.map(|d| d.join("models")).filter(|d| d.is_dir()) {
        return dir;
    }
    match data_dir {
        Some(data) if !debug => data.join("models"),
        _ => PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../models")),
    }
}
```

Move the ASR loading out of `Runtime::start` into a method, keeping `asr` on `Runtime`:

```rust
/// Loads the speech model in the background, or reports that it is not downloaded yet.
pub fn load_speech(&self) {
    let (tx, asr) = (self.tx.clone(), self.asr.clone());
    std::thread::spawn(move || {
        let dir = models_dir();
        if !erindi_core::models::SPEECH.installed(&dir) {
            let _ = tx.send(Msg::ModelMissing);
            return;
        }
        match Asr::load(&dir) {
            Ok(model) => {
                let _ = asr.set(model);
                let _ = tx.send(Msg::ModelReady);
            }
            Err(e) => {
                let _ = tx.send(Msg::ModelFailed(e));
            }
        }
    });
}
```

Add `asr: Arc<OnceLock<Asr>>` to the `Runtime` struct, set it in `start`, and call `runtime.load_speech()` at the end of `start` before returning (build `let runtime = Self { … }; runtime.load_speech(); runtime`).

In `Executor::execute`:

```rust
Effect::OpenSettings => crate::show_settings(&self.app),
```

and extend the hide arm to `AppState::Idle | AppState::LoadingModel | AppState::NoModel`.

`lib.rs`: make `show_settings` `pub(crate)` and open the Settings tab:

```rust
pub(crate) fn show_settings(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("settings") {
        let _ = window.eval("location.hash = 'settings'");
        let _ = window.show();
        let _ = window.set_focus();
        return;
    }
    let _ = WebviewWindowBuilder::new(app, "settings", WebviewUrl::App("index.html#settings".into()))
        .title("Erindi")
        .inner_size(880.0, 680.0)
        .build();
}
```

In `setup`, after `app.manage(runtime)`:

```rust
if !erindi_core::models::SPEECH.installed(&runtime::models_dir()) {
    show_settings(app.handle());
}
```

The tray "Settings" item keeps calling `show_settings`, which now lands on the Settings tab; that matches its name.

`overlay.tsx`: add `"NoModel"` to `AppState` and `NoModel: ["#64748b", "#94a3b8"]` to `palette`.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p erindi-core && cargo test -p erindi-desktop`
Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add crates apps
git commit -m "feat: run without a speech model and open Settings to install it"
```

---

### Task 4: llama-server process

**Files:**
- Create: `crates/core/src/llama.rs`
- Modify: `crates/core/src/lib.rs`, `crates/core/Cargo.toml`

**Interfaces:**
- Consumes: `refine::{request, parse_response, Refined}`.
- Produces:
  - `pub struct LlamaServer`
  - `impl LlamaServer { pub fn start(exe: &Path, model: &Path) -> Result<Self, String>; pub fn refine(&self, text: &str) -> Result<Option<Refined>, String> }`
  - Dropping a `LlamaServer` stops the process.

- [ ] **Step 1: Enable the std wrapper**

In `crates/core/Cargo.toml` add `"std"` to the `process-wrap` features.

- [ ] **Step 2: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_server_is_an_error() {
        let err = LlamaServer::start(Path::new("C:/nope/llama-server.exe"), Path::new("m.gguf"))
            .err()
            .unwrap();
        assert!(err.contains("llama-server"), "{err}");
    }

    /// `ERINDI_LLAMA=<llama-server.exe>;<model.gguf> cargo test -p erindi-core llama -- --ignored`
    #[test]
    #[ignore = "needs llama-server and the cleanup model"]
    fn refines_with_a_live_server() {
        let var = std::env::var("ERINDI_LLAMA").unwrap();
        let (exe, model) = var.split_once(';').unwrap();
        let server = LlamaServer::start(Path::new(exe), Path::new(model)).unwrap();
        let r = server.refine("эээ ну проверь diff в модуле auth").unwrap().unwrap();
        assert!(r.text.contains("diff"), "{r:?}");
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test -p erindi-core llama`
Expected: FAIL to compile.

- [ ] **Step 4: Implement**

```rust
use std::net::TcpListener;
use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};

use process_wrap::std::*;
use serde_json::Value;

use crate::refine::{Refined, parse_response, request};

/// Loading a 2 GB model from a slow disk can take this long.
const START_TIMEOUT: Duration = Duration::from_secs(120);

/// A local `llama-server` that lives as long as this value.
pub struct LlamaServer {
    child: Box<dyn ChildWrapper>,
    url: String,
    agent: ureq::Agent,
}

impl LlamaServer {
    pub fn start(exe: &Path, model: &Path) -> Result<Self, String> {
        let port = TcpListener::bind("127.0.0.1:0")
            .and_then(|l| l.local_addr())
            .map_err(|e| format!("No free port for llama-server: {e}"))?
            .port();
        let mut command = std::process::Command::new(exe);
        command
            .arg("-m")
            .arg(model)
            .args(["--host", "127.0.0.1", "--port", &port.to_string()])
            .args(["-ngl", "99", "-c", "4096", "-np", "1", "--no-webui"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut wrap = CommandWrap::from(command);
        #[cfg(windows)]
        {
            use windows::Win32::System::Threading::CREATE_NO_WINDOW;
            wrap.wrap(CreationFlags(CREATE_NO_WINDOW)).wrap(JobObject);
        }
        let child = wrap
            .spawn()
            .map_err(|e| format!("Cannot start llama-server: {e}"))?;
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(300)))
            .build()
            .into();
        let mut server = Self {
            child,
            url: format!("http://127.0.0.1:{port}"),
            agent,
        };
        server.wait_ready()?;
        Ok(server)
    }

    fn wait_ready(&mut self) -> Result<(), String> {
        let deadline = Instant::now() + START_TIMEOUT;
        while Instant::now() < deadline {
            if let Ok(Some(status)) = self.child.try_wait() {
                return Err(format!("llama-server exited: {status}"));
            }
            if self.agent.get(format!("{}/health", self.url)).call().is_ok() {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        Err("llama-server did not start in time".into())
    }

    /// `Ok(None)` means the server answered with something that is not a refined prompt.
    pub fn refine(&self, text: &str) -> Result<Option<Refined>, String> {
        let body: Value = self
            .agent
            .post(format!("{}/v1/chat/completions", self.url))
            .send_json(request(text))
            .map_err(|e| format!("llama-server request failed: {e}"))?
            .body_mut()
            .read_json()
            .map_err(|e| format!("llama-server reply is not JSON: {e}"))?;
        Ok(parse_response(&body))
    }
}

impl Drop for LlamaServer {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
        let _ = self.child.wait();
    }
}
```

`process-wrap` 10 names: check `cargo doc -p process-wrap --open` or docs.rs for the exact std trait (`ChildWrapper`) and kill method (`start_kill` or `kill`); the tokio module in `run.rs` shows the same shape. Check that `--no-webui` and `-np` exist in `llama-server --help` for `b11095` (run it from `models/llama/` after Task 9's dev script, or skip the flag if absent).

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p erindi-core llama`
Expected: 1 passed, 1 ignored.

- [ ] **Step 6: Commit**

```bash
git add crates/core
git commit -m "feat(core): llama-server process with health check and refine request"
```

---

### Task 5: Refine step in the controller

**Files:**
- Modify: `crates/core/src/state.rs`, `crates/core/src/controller.rs`, `apps/desktop/src/overlay.tsx`

**Interfaces:**
- Consumes: `refine::{Refined, accept}`.
- Produces:
  - `AppState::Refining`, `Event::Refine { op }`, `Event::Refined { op }`
  - `Msg::Settings { policy, recent, cwd, refine: bool }`
  - `Msg::Refined { op: OpId, refined: Option<Refined> }`
  - `Effect::Refine { op: OpId, text: String }`
  - `Effect::StartRun { op, prompt, raw: Option<String>, session, cwd }`: `raw` is the parser's text when the refiner changed it.

- [ ] **Step 1: Write the failing tests**

`state.rs`:

```rust
#[test]
fn refining_sits_between_transcribing_and_running() {
    let mut m = at(Transcribing);
    let op = m.op();
    assert_eq!(m.apply(Refine { op }), Ok(Outcome::Changed(Refining)));
    assert_eq!(m.apply(Refined { op: op + 1 }), Ok(Outcome::Stale));
    assert_eq!(m.apply(Refined { op }), Ok(Outcome::Changed(Running)));
}
```

`controller.rs`: extend the harness `settings()` helper with `refine: false`, add a `refining()` helper and the tests:

```rust
fn refining() -> T {
    let mut t = T::new();
    t.send(Msg::Settings {
        policy: SessionPolicy::Continue,
        recent: Duration::from_secs(600),
        cwd: "C:/p".into(),
        refine: true,
    });
    t
}

#[test]
fn refine_runs_the_cleaned_text_and_keeps_the_raw() {
    let mut t = refining();
    let op = t.listen(Key::Hold);
    t.send(Msg::KeyUp(Key::Hold));
    let fx = t.send(Msg::Transcribed { op, text: "эээ клод проверь diff".into() });
    assert_eq!(fx[0], Effect::Refine { op, text: "эээ Claude проверь diff".into() });
    assert_eq!(shown(&fx).unwrap().state, AppState::Refining);
    let fx = t.send(Msg::Refined {
        op,
        refined: Some(Refined { intent: Intent::Unspecified, text: "Claude, проверь diff.".into() }),
    });
    let Some(Effect::StartRun { prompt, raw, .. }) = fx.first() else { panic!("{fx:?}") };
    assert_eq!(prompt, "Claude, проверь diff.");
    assert_eq!(raw.as_deref(), Some("эээ Claude проверь diff"));
}

#[test]
fn failed_refine_runs_the_parser_text() {
    let mut t = refining();
    let op = t.listen(Key::Hold);
    t.send(Msg::KeyUp(Key::Hold));
    t.send(Msg::Transcribed { op, text: "в новой сессии проверь diff".into() });
    let fx = t.send(Msg::Refined { op, refined: None });
    let Some(Effect::StartRun { prompt, raw, session, .. }) = fx.first() else { panic!("{fx:?}") };
    assert_eq!((prompt.as_str(), raw), ("проверь diff", &None));
    assert!(matches!(session, Session::New(_)));
}

#[test]
fn parser_intent_wins_over_the_model() {
    let mut t = refining();
    t.finish_saying("проверь diff");
    let op = t.listen(Key::Hold);
    t.send(Msg::KeyUp(Key::Hold));
    t.send(Msg::Transcribed { op, text: "добавь тесты".into() });
    let fx = t.send(Msg::Refined {
        op,
        refined: Some(Refined { intent: Intent::New, text: "Добавь тесты.".into() }),
    });
    let Some(Effect::StartRun { session, .. }) = fx.first() else { panic!("{fx:?}") };
    assert!(matches!(session, Session::Resume(_)));
}

#[test]
fn empty_transcript_skips_refining() {
    let mut t = refining();
    let op = t.listen(Key::Hold);
    t.send(Msg::KeyUp(Key::Hold));
    let fx = t.send(Msg::Transcribed { op, text: " ".into() });
    assert!(!fx.iter().any(|e| matches!(e, Effect::Refine { .. })));
    assert_eq!(t.c.state(), AppState::Idle);
}
```

`finish_saying` in the harness expects `StartRun` straight after `Transcribed`; in `parser_intent_wins_over_the_model` it is called while refining is on, so give it a branch: if the first effect is `Effect::Refine { op, text }`, send `Msg::Refined { op, refined: None }` and take `StartRun` from that reply.

Existing tests that match `Effect::StartRun { … }` with `..` keep compiling; tests that construct `Msg::Settings` directly get `refine: false`.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p erindi-core`
Expected: FAIL to compile.

- [ ] **Step 3: Implement the state machine**

`state.rs`: add `Refining` after `Transcribing`, events `Refine { op: OpId }` and `Refined { op: OpId }`, include both in the stale-op check, and add:

```rust
(S::Transcribing, E::Refine { .. }) => S::Refining,
(S::Refining, E::Refined { .. }) => S::Running,
```

and extend the failure arm to `(S::Listening | S::Transcribing | S::Refining | S::Running, E::StepFailed { .. })`.

- [ ] **Step 4: Implement the controller**

New fields: `refine: bool` (default `false`) and `pending: Option<(Intent, String)>` (default `None`). `Msg::Settings` gets `refine`, stored in the `Settings` arm before the `cwd` comparison.

Replace the `Msg::Transcribed` arm:

```rust
Msg::Transcribed { op, text } => {
    let (intent, spoken) = parse_intent(&text);
    let intent = match self.mode {
        Key::NewSession => Intent::New,
        _ => intent,
    };
    let prompt = self.transformer.transform(&spoken);
    if self.refine && !prompt.is_empty() {
        if let Ok(Outcome::Changed(_)) = self.machine.apply(Event::Refine { op }) {
            self.pending = Some((intent, prompt.clone()));
            self.view.text = prompt.clone();
            return vec![Effect::Refine { op, text: prompt }, self.show()];
        }
        return vec![];
    }
    let empty = prompt.is_empty();
    match self.machine.apply(Event::Transcribed { op, empty }) {
        Ok(Outcome::Changed(S::Running)) => self.start_run(op, intent, prompt, None, now),
        Ok(Outcome::Changed(_)) => vec![self.show()],
        _ => vec![],
    }
}
Msg::Refined { op, refined } if current(op) && state == S::Refining => {
    let Some((intent, input)) = self.pending.take() else {
        return vec![];
    };
    // ponytail: the model's intent is ignored until the benchmark shows it beats the parser.
    let text = accept(&input, refined).map_or_else(|| input.clone(), |r| r.text);
    let raw = (text != input).then_some(input);
    match self.machine.apply(Event::Refined { op }) {
        Ok(Outcome::Changed(S::Running)) => self.start_run(op, intent, text, raw, now),
        _ => vec![],
    }
}
```

Move the body of the old `Ok(Outcome::Changed(S::Running))` branch into a method, adding `raw` to the effect:

```rust
fn start_run(
    &mut self,
    op: OpId,
    intent: Intent,
    prompt: String,
    raw: Option<String>,
    now: Instant,
) -> Vec<Effect> {
    let resume = choose(self.policy, self.recent, intent, self.active.as_ref(), now);
    let (session, cwd) = match (resume, &self.active) {
        (Some(id), Some(active)) => (Session::Resume(id), active.cwd.clone()),
        _ => (Session::New(Uuid::new_v4()), self.cwd.clone()),
    };
    self.running = Some((session, cwd.clone()));
    self.result = None;
    self.view.text = prompt.clone();
    self.view.session_id = Some(session_id(session));
    self.view.continued = resume.is_some();
    self.view.detail.clear();
    vec![
        Effect::StartRun { op, prompt, raw, session, cwd },
        self.show(),
    ]
}
```

- [ ] **Step 5: Overlay label**

`overlay.tsx`: add `"Refining"` to `AppState`, `Refining: ["#a855f7", "#f59e0b"]` to `palette` and `Refining: "Refining…"` to `labels`.

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test -p erindi-core`
Expected: all pass.

Keep `erindi-desktop` compiling: in `settings.rs` `session_msg()` add `refine: false` (Task 7 wires the real value), and in `runtime.rs` match `Effect::StartRun { op, prompt, session, cwd, .. }` and add an empty `Effect::Refine { .. } => {}` arm (Task 6 and Task 7 replace both). Run `cargo test -p erindi-desktop`: all pass.

- [ ] **Step 7: Commit**

```bash
git add crates apps/desktop
git commit -m "feat(core): refine step between transcription and the run"
```

---

### Task 6: History keeps the raw text

**Files:**
- Modify: `apps/desktop/src-tauri/src/history.rs`, `apps/desktop/src-tauri/src/runtime.rs`, `apps/desktop/src/sessions.tsx`

**Interfaces:**
- Produces: `pub enum Prompt { Plain(String), Refined { text: String, raw: String } }`, `History::record(&mut self, id, cwd, prompt: Prompt, now_ms)`.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn refined_prompts_keep_the_raw_text_and_old_files_still_load() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s.json");
    std::fs::write(&path, format!(r#"[{{"id":"{}","cwd":"C:/a","prompts":["old"],"createdMs":1,"updatedMs":1}}]"#, id(1))).unwrap();
    let mut h = History::load(&path);
    h.record(id(1), "C:/a", Prompt::Refined { text: "Fix it.".into(), raw: "um fix it".into() }, 2).unwrap();
    let h = History::load(&path);
    assert_eq!(
        h.get(id(1)).unwrap().prompts,
        [
            Prompt::Plain("old".into()),
            Prompt::Refined { text: "Fix it.".into(), raw: "um fix it".into() }
        ]
    );
}
```

Existing tests pass `Prompt::Plain("first task".into())` where they passed `"first task"`.

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p erindi-desktop history`
Expected: FAIL to compile.

- [ ] **Step 3: Implement**

```rust
/// One utterance. Plain prompts serialize as bare strings, as history files always had them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Prompt {
    Plain(String),
    Refined { text: String, raw: String },
}
```

`Entry.prompts` becomes `Vec<Prompt>`; `record` takes `prompt: Prompt` and pushes it.

`runtime.rs`, `Effect::StartRun` arm passes `raw` to `start_run`, which records:

```rust
let entry = match raw {
    Some(raw) => Prompt::Refined { text: prompt.clone(), raw },
    None => Prompt::Plain(prompt.clone()),
};
```

`sessions.tsx`:

```tsx
type Prompt = string | { text: string; raw: string };
const textOf = (p: Prompt) => (typeof p === "string" ? p : p.text);
```

`Entry.prompts` becomes `Prompt[]`; the title uses `textOf(entry.prompts[0])`; the expanded list renders:

```tsx
<span>
  {textOf(p)}
  {typeof p !== "string" && <span class="block text-xs text-neutral-500">Said: {p.raw}</span>}
</span>
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p erindi-desktop history`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add apps/desktop
git commit -m "feat(desktop): keep the raw words next to refined prompts"
```

---

### Task 7: Refiner in the desktop runtime

**Files:**
- Modify: `apps/desktop/src-tauri/src/runtime.rs`, `apps/desktop/src-tauri/src/settings.rs`, `apps/desktop/src-tauri/src/lib.rs`

**Interfaces:**
- Consumes: `LlamaServer`, `refine::WARM_UP`, `models::{CLEANUP, CLEANUP_GGUF}`, `Effect::Refine`, `Msg::Refined`.
- Produces: `Settings.cleanup: bool`; `Runtime::set_cleanup(&self, on: bool)`; `runtime::llama_server_exe() -> PathBuf`.

- [ ] **Step 1: Write the failing tests**

`settings.rs`:

```rust
#[test]
fn cleanup_is_off_by_default_and_reaches_the_controller() {
    let s = Settings::default();
    assert!(!s.cleanup);
    let on = Settings { cleanup: true, ..Settings::default() };
    assert!(matches!(on.session_msg(), Msg::Settings { refine: true, .. }));
}
```

`runtime.rs`:

```rust
#[test]
fn bundled_llama_server_beats_models_folder() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("llama")).unwrap();
    std::fs::write(dir.path().join("llama/llama-server.exe"), "").unwrap();
    assert_eq!(
        pick_llama_server(Some(dir.path().into()), Path::new("M:/models")),
        dir.path().join("llama/llama-server.exe")
    );
    assert_eq!(
        pick_llama_server(None, Path::new("M:/models")),
        Path::new("M:/models").join("llama/llama-server.exe")
    );
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p erindi-desktop`
Expected: FAIL to compile.

- [ ] **Step 3: Implement settings**

`Settings` gets `pub cleanup: bool` (default `false`), and `session_msg()` adds `refine: self.cleanup`.

In `lib.rs` `save_settings`, before `settings.save`:

```rust
if settings.cleanup && !erindi_core::models::CLEANUP.installed(&runtime::models_dir()) {
    return Err("Download the cleanup model first".into());
}
```

and after `runtime.send(settings.session_msg())`: `runtime.set_cleanup(settings.cleanup);`. In `setup`, after `app.manage(runtime)` has a clone available, call `runtime.set_cleanup(settings.read().unwrap().cleanup)` (take a clone of `runtime` before `manage`).

- [ ] **Step 4: Implement the refiner**

In `runtime.rs`:

```rust
const REFINE_BUDGET: Duration = Duration::from_millis(1500);

/// The cleanup model server. It starts when cleanup is turned on and stays loaded.
#[derive(Clone, Default)]
struct Refiner(Arc<Mutex<Option<LlamaServer>>>);

impl Refiner {
    fn set_enabled(&self, on: bool) {
        if !on {
            self.0.lock().unwrap().take();
            return;
        }
        let this = self.clone();
        std::thread::spawn(move || {
            let mut server = this.0.lock().unwrap();
            if server.is_none() {
                *server = start_llama();
            }
        });
    }

    /// Waits for a starting server, so an utterance right after launch is still refined.
    fn refine(&self, text: &str) -> Option<Refined> {
        let mut server = self.0.lock().unwrap();
        if server.is_none() {
            *server = start_llama();
        }
        let started = Instant::now();
        let result = server.as_ref()?.refine(text);
        let took = started.elapsed();
        if took > REFINE_BUDGET {
            eprintln!("refine took {took:?} for {} chars", text.chars().count());
        }
        result.unwrap_or_else(|e| {
            eprintln!("{e}");
            server.take();
            None
        })
    }
}

fn start_llama() -> Option<LlamaServer> {
    let model = models_dir().join(erindi_core::models::CLEANUP_GGUF);
    let started = Instant::now();
    let server = LlamaServer::start(&llama_server_exe(), &model)
        .map_err(|e| eprintln!("{e}"))
        .ok()?;
    let _ = server.refine(erindi_core::refine::WARM_UP);
    eprintln!("llama-server ready and warm in {:?}", started.elapsed());
    Some(server)
}

pub fn llama_server_exe() -> PathBuf {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(PathBuf::from));
    pick_llama_server(exe_dir, &models_dir())
}

/// The release zip ships `llama/` next to the exe; development builds use `models/llama/`.
fn pick_llama_server(exe_dir: Option<PathBuf>, models: &Path) -> PathBuf {
    exe_dir
        .map(|d| d.join("llama/llama-server.exe"))
        .filter(|p| p.is_file())
        .unwrap_or_else(|| models.join("llama/llama-server.exe"))
}
```

Add `refiner: Refiner` to both `Runtime` and `Executor` (same clone), `pub fn set_cleanup(&self, on: bool) { self.refiner.set_enabled(on) }` on `Runtime`, and in `Executor::execute`:

```rust
Effect::Refine { op, text } => {
    let (refiner, tx) = (self.refiner.clone(), self.tx.clone());
    std::thread::spawn(move || {
        let refined = refiner.refine(&text);
        let _ = tx.send(Msg::Refined { op, refined });
    });
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p erindi-desktop && cargo clippy --workspace --all-targets`
Expected: all pass, no warnings.

- [ ] **Step 6: Commit**

```bash
git add apps/desktop
git commit -m "feat(desktop): run the refiner on llama-server while cleanup is on"
```

---

### Task 8: Model commands and Settings blocks

**Files:**
- Modify: `apps/desktop/src-tauri/src/lib.rs`, `apps/desktop/src-tauri/build.rs`, `apps/desktop/src-tauri/capabilities/settings.json`, `apps/desktop/src/settings.tsx`

**Interfaces:**
- Consumes: `models::{by_id, download, SPEECH, CLEANUP}`, `Runtime::load_speech`.
- Produces: commands `model_status() -> Vec<ModelStatus>` and `download_model(id: String)`; events `model-progress { id, done, total }` and `model-done { id, error: string | null }`.

- [ ] **Step 1: Write the failing test**

`lib.rs` tests:

```rust
#[test]
fn settings_window_can_manage_models() {
    let cap = capability(include_str!("../capabilities/settings.json"));
    let perms = cap["permissions"].as_array().unwrap();
    for p in ["allow-model-status", "allow-download-model"] {
        assert!(perms.contains(&json!(p)), "{p}");
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p erindi-desktop settings_window_can_manage_models`
Expected: FAIL.

- [ ] **Step 3: Implement the commands**

Add `"model_status"` and `"download_model"` to `build.rs` and to `generate_handler!`, and `"allow-model-status"`, `"allow-download-model"` to `capabilities/settings.json`.

```rust
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ModelStatus {
    id: &'static str,
    label: &'static str,
    installed: bool,
}

#[tauri::command]
fn model_status() -> Vec<ModelStatus> {
    let dir = runtime::models_dir();
    [&erindi_core::models::SPEECH, &erindi_core::models::CLEANUP]
        .into_iter()
        .map(|m| ModelStatus { id: m.id, label: m.label, installed: m.installed(&dir) })
        .collect()
}

#[derive(Clone, serde::Serialize)]
struct Progress {
    id: &'static str,
    done: u64,
    total: u64,
}

#[derive(Clone, serde::Serialize)]
struct Done {
    id: &'static str,
    error: Option<String>,
}

// ponytail: no guard against two downloads of one model; the button is disabled while one runs.
#[tauri::command]
fn download_model(app: AppHandle, runtime: tauri::State<Runtime>, id: String) -> Result<(), String> {
    let model = erindi_core::models::by_id(&id).ok_or("Unknown model")?;
    let runtime = runtime.inner().clone();
    std::thread::spawn(move || {
        let mut last = std::time::Instant::now();
        let result = erindi_core::models::download(model, &runtime::models_dir(), |done, total| {
            if last.elapsed() >= std::time::Duration::from_millis(100) || done == total {
                last = std::time::Instant::now();
                let _ = app.emit_to("settings", "model-progress", Progress { id: model.id, done, total });
            }
        });
        if result.is_ok() && model.id == "speech" {
            runtime.load_speech();
        }
        let _ = app.emit_to("settings", "model-done", Done { id: model.id, error: result.err() });
    });
    Ok(())
}
```

`use tauri::Emitter;` in `lib.rs`.

- [ ] **Step 4: Implement the Settings UI**

In `settings.tsx`:

```tsx
import { listen } from "@tauri-apps/api/event";

type ModelStatus = { id: "speech" | "cleanup"; label: string; installed: boolean };

function Section(props: { title: string; description: string; highlight?: boolean; children: ComponentChildren }) {
  return (
    <section
      class={`space-y-3 rounded-lg border p-4 ${
        props.highlight ? "border-red-500" : "border-neutral-200 dark:border-neutral-800"
      }`}
    >
      <div>
        <h3 class="font-semibold">{props.title}</h3>
        <p class="text-xs text-neutral-500">{props.description}</p>
      </div>
      {props.children}
    </section>
  );
}

function ModelRow(props: { model: ModelStatus; onInstalled: () => void }) {
  const [progress, setProgress] = useState<number | null>(null);
  const [error, setError] = useState("");

  useEffect(() => {
    const offProgress = listen<{ id: string; done: number; total: number }>("model-progress", (e) => {
      if (e.payload.id === props.model.id) setProgress(e.payload.done / e.payload.total);
    });
    const offDone = listen<{ id: string; error: string | null }>("model-done", (e) => {
      if (e.payload.id !== props.model.id) return;
      setProgress(null);
      if (e.payload.error) setError(e.payload.error);
      else props.onInstalled();
    });
    return () => {
      offProgress.then((f) => f());
      offDone.then((f) => f());
    };
  }, []);

  const download = () => {
    setError("");
    setProgress(0);
    invoke("download_model", { id: props.model.id }).catch((err) => {
      setProgress(null);
      setError(String(err));
    });
  };

  return (
    <div class="space-y-1">
      <div class="flex items-center gap-3">
        <select class={input} disabled aria-label="Model">
          <option>{props.model.label}</option>
        </select>
        {props.model.installed ? (
          <span class="shrink-0 text-green-700 dark:text-green-400">Downloaded</span>
        ) : progress !== null ? (
          <progress class="w-32 shrink-0" value={progress} max={1} aria-label="Download progress" />
        ) : (
          <button type="button" class="shrink-0 rounded-md border px-3 py-1.5" onClick={download}>
            Download
          </button>
        )}
        {progress !== null && <span class="w-10 shrink-0 tabular-nums">{Math.floor(progress * 100)}%</span>}
      </div>
      {error && <p class="text-xs text-red-600">{error}</p>}
    </div>
  );
}
```

In `SettingsView`, load models next to settings:

```tsx
const [models, setModels] = useState<ModelStatus[]>([]);
const refreshModels = () => invoke<ModelStatus[]>("model_status").then(setModels);
useEffect(() => { refreshModels(); }, []);
const speech = models.find((m) => m.id === "speech");
const cleanup = models.find((m) => m.id === "cleanup");
```

Add `cleanup: boolean` to the `Settings` type. Wrap the form's fields in five `Section`s, in this order and with this copy:

- `Agent`, "Where Claude runs and how it treats your requests.": project folder, permission mode, model, session policy and recent minutes.
- `Voice`, "Speech recognition runs on this computer.", `highlight={speech && !speech.installed}`: `{speech && <ModelRow model={speech} onInstalled={refreshModels} />}`, then `{speech && !speech.installed && <p class="text-xs text-red-600">Speech model is not installed. Download it to start dictating.</p>}`, microphone, silence.
- `Hotkeys`, "Click a field, then press the combination.": the three hotkey rows (drop the old inner legend and hint).
- `Prompt cleanup`, "A local model removes slips and voice commands before the agent sees your words.":

```tsx
<label class="flex items-center gap-2">
  <input
    type="checkbox"
    checked={s.cleanup}
    disabled={!cleanup?.installed}
    onChange={(e) => set({ cleanup: e.currentTarget.checked })}
  />
  Clean up prompt
</label>
{cleanup && <ModelRow model={cleanup} onInstalled={refreshModels} />}
```

- `Dictionary`, "Replaces what you say with how it should be written.": the dictionary rows and "Add word" (drop the old legend and hint).

In `App`, start on the tab named by the hash and follow later changes:

```tsx
const initial = location.hash === "#settings" ? "settings" : "sessions";
const [tab, setTab] = useState<(typeof tabs)[number][0]>(initial);
useEffect(() => {
  const onHash = () => location.hash === "#settings" && setTab("settings");
  window.addEventListener("hashchange", onHash);
  return () => window.removeEventListener("hashchange", onHash);
}, []);
```

- [ ] **Step 5: Verify**

Run: `cargo test -p erindi-desktop && cd apps/desktop && pnpm tsc --noEmit && pnpm test`
Expected: all pass.

Manual check with `pnpm tauri dev` and `ERINDI_MODELS` pointing at an empty folder:
1. Settings opens on launch; Voice block has a red border and the message.
2. Download shows progress and then "Downloaded"; the hotkey then records.
3. "Clean up prompt" is disabled until the cleanup model is downloaded.

- [ ] **Step 6: Commit**

```bash
git add apps/desktop
git commit -m "feat(desktop): Settings blocks with model downloads and prompt cleanup"
```

---

### Task 9: Ship llama-server and document

**Files:**
- Modify: `scripts/fetch-models.ps1`, `scripts/package.ps1`, `ROADMAP.md`, `README.md` (only if it tells users to run `fetch-models.ps1`)

- [ ] **Step 1: Dev script**

Append to `scripts/fetch-models.ps1` a `-Refiner` switch (declare `param([switch]$Refiner)` at the top, before `$ErrorActionPreference`):

```powershell
if ($Refiner) {
    $llama = @{
        Url = 'https://github.com/ggml-org/llama.cpp/releases/download/b11095/llama-b11095-bin-win-vulkan-x64.zip'
        Sha = '45c586f50af57b7e144aa76c6fc38c544a717c4f4b7b659c489b980f3993412c'
    }
    $zip = Join-Path $models 'llama-vulkan.zip'
    if (-not (Test-Path $zip)) { Invoke-WebRequest $llama.Url -OutFile $zip }
    if ((Get-FileHash $zip -Algorithm SHA256).Hash.ToLower() -ne $llama.Sha) {
        Remove-Item $zip
        throw 'llama.cpp: SHA-256 mismatch, file removed'
    }
    Expand-Archive $zip (Join-Path $models 'llama') -Force

    $gguf = Join-Path $models 'qwen2.5-3b-instruct-q4_k_m.gguf'
    if (-not (Test-Path $gguf)) {
        Invoke-WebRequest 'https://huggingface.co/Qwen/Qwen2.5-3B-Instruct-GGUF/resolve/7dabda4d13d513e3e842b20f0d435c732f172cbe/qwen2.5-3b-instruct-q4_k_m.gguf' -OutFile "$gguf.partial"
        Move-Item "$gguf.partial" $gguf
    }
    if ((Get-FileHash $gguf -Algorithm SHA256).Hash.ToLower() -ne '626b4a6678b86442240e33df819e00132d3ba7dddfe1cdc4fbb18e0a9615c62d') {
        Remove-Item $gguf
        throw 'cleanup model: SHA-256 mismatch, file removed'
    }
    Write-Host 'refiner ready'
}
```

Update the header comment to name the switch.

- [ ] **Step 2: Package llama-server**

In `scripts/package.ps1`, after copying the DLLs:

```powershell
# The official llama.cpp Vulkan build runs the prompt refiner; models download from Settings.
$llamaZip = Join-Path $root 'dist\llama-vulkan.zip'
if (-not (Test-Path $llamaZip)) {
    Invoke-WebRequest 'https://github.com/ggml-org/llama.cpp/releases/download/b11095/llama-b11095-bin-win-vulkan-x64.zip' -OutFile $llamaZip
}
if ((Get-FileHash $llamaZip -Algorithm SHA256).Hash.ToLower() -ne '45c586f50af57b7e144aa76c6fc38c544a717c4f4b7b659c489b980f3993412c') {
    Remove-Item $llamaZip
    throw 'llama.cpp: SHA-256 mismatch'
}
Expand-Archive $llamaZip (Join-Path $stage 'llama') -Force
```

Remove the line that copies `scripts\fetch-models.ps1` into the stage and the `scripts` directory creation; update the layout comment ("the exe with its DLLs and llama/; models download from Settings").

- [ ] **Step 3: Verify packaging**

Run: `./scripts/package.ps1` after `pnpm tauri build --no-bundle` in `apps/desktop`.
Expected: the zip contains `erindi.exe`, the DLLs and `llama/llama-server.exe`. `dist/` is ignored by git; confirm with `git status`.

- [ ] **Step 4: ROADMAP**

In `ROADMAP.md`:
- Mark **Prompt cleanup model** done, reworded: "A local model (Qwen2.5-3B on llama.cpp Vulkan) removes slips and spoken commands before the agent sees the text; on or off in Settings."
- Replace **Speech model picker** with two items: `[x] **Model downloads.** Speech and cleanup models install from Settings with progress and hash checks.` and `[ ] **Model picker.** Choose among several speech and cleanup models, with Hugging Face search.`
- Add under "Working with agents": `[ ] **Unload the cleanup model** after idle time, for machines short on memory.`, `[ ] **Refiner device.** Auto, GPU or CPU; CUDA build if Vulkan is not fast enough.`, `[ ] **Refiner endpoints.** Cloud, Ollama and LM Studio by address and API key.`, `[ ] **Streaming cleanup** into the bubble.`
- Add under "Controls": `[ ] **First-run setup.** Detect GPU, VRAM and CPU, pick models and device, warn when cleanup would be slow; re-run when the hardware changes.`
- Update README if it tells users to run `fetch-models.ps1`: users download models from Settings; the script is for development.

- [ ] **Step 5: Commit**

```bash
git add scripts ROADMAP.md README.md
git commit -m "build: ship llama-server in the release zip; roadmap for the refiner"
```

---

### Task 10: Refiner benchmark

**Files:**
- Create: `crates/core/examples/refine-bench.rs`, `crates/core/examples/refine-cases.jsonl`

**Interfaces:**
- Consumes: `LlamaServer`, `refine::accept`, `session::{parse_intent, Intent}`.

- [ ] **Step 1: Labeled cases**

`crates/core/examples/refine-cases.jsonl`, one JSON object per line with `say`, `intent` (`new`, `continue`, `none`) and `keep` (words the cleaned text must contain, case-insensitive). Write 50 lines covering:

- 20 short (under 12 words) with no command, Russian and English, with fillers: for example `{"say":"эээ проверь diff в модуле auth","intent":"none","keep":["diff","auth"]}`.
- 10 with explicit commands the parser knows: `{"say":"создай новую сессию и найди почему падают тесты","intent":"new","keep":["тест"]}`, `{"say":"same session, run clippy","intent":"continue","keep":["clippy"]}`.
- 8 with implicit commands the parser misses: `{"say":"давай с чистого листа, напиши README","intent":"new","keep":["README"]}`, `{"say":"forget all that and start over: build the landing page","intent":"new","keep":["landing"]}`.
- 7 that mention sessions but are not commands: `{"say":"расскажи про новую сессию в React","intent":"none","keep":["React"]}`.
- 5 long (80–200 words) dictations of a real task with slips and self-corrections, in Russian and English, `intent` `none` or `new`, with 4–6 `keep` words each (file names, function names, numbers).

- [ ] **Step 2: Benchmark**

```rust
//! Speed and quality of the prompt refiner.
//! cargo run -p erindi-core --release --example refine-bench -- <llama-server.exe> <model.gguf>

use std::path::Path;
use std::time::{Duration, Instant};

use erindi_core::llama::LlamaServer;
use erindi_core::refine::accept;
use erindi_core::session::{Intent, parse_intent};
use serde::Deserialize;

const BUDGET: Duration = Duration::from_millis(1500);

#[derive(Deserialize)]
struct Case {
    say: String,
    intent: String,
    keep: Vec<String>,
}

fn name(i: Intent) -> &'static str {
    match i {
        Intent::New => "new",
        Intent::Continue => "continue",
        Intent::Unspecified => "none",
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let server = LlamaServer::start(Path::new(&args[1]), Path::new(&args[2])).expect("server");
    let _ = server.refine(erindi_core::refine::WARM_UP);

    let cases: Vec<Case> = include_str!("refine-cases.jsonl")
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect(l))
        .collect();

    let (mut times, mut parser_ok, mut model_ok, mut kept) = (vec![], 0, 0, 0);
    for c in &cases {
        let (parsed, input) = parse_intent(&c.say);
        let started = Instant::now();
        let refined = server.refine(&input).ok().flatten();
        let took = started.elapsed();
        times.push(took);
        let model_intent = refined.as_ref().map_or("fail", |r| name(r.intent));
        let text = accept(&input, refined).map_or(input.clone(), |r| r.text);
        let lower = text.to_lowercase();
        let missing: Vec<&str> = c.keep.iter().map(String::as_str).filter(|k| !lower.contains(&k.to_lowercase())).collect();
        parser_ok += usize::from(name(parsed) == c.intent);
        model_ok += usize::from(model_intent == c.intent);
        kept += usize::from(missing.is_empty());
        let warn = if took > BUDGET { "WARN" } else { "    " };
        println!("{warn} {:>5} ms  parser={:<8} model={:<8} want={:<8} {}", took.as_millis(), name(parsed), model_intent, c.intent, text);
        if !missing.is_empty() {
            println!("      lost: {missing:?}");
        }
    }

    times.sort();
    let pct = |p: usize| times[(times.len() - 1) * p / 100].as_millis();
    let n = cases.len();
    println!("\n{n} cases  p50 {} ms  p90 {} ms  max {} ms  over budget {}", pct(50), pct(90), pct(100), times.iter().filter(|t| **t > BUDGET).count());
    println!("intent: parser {parser_ok}/{n}, model {model_ok}/{n}; meaning kept {kept}/{n}");
}
```

- [ ] **Step 3: Run**

Run: `./scripts/fetch-models.ps1 -Refiner` then `cargo run -p erindi-core --release --example refine-bench -- models/llama/llama-server.exe models/qwen2.5-3b-instruct-q4_k_m.gguf`
Expected: 50 result lines and the summary. Paste the summary into the PR description.

- [ ] **Step 4: Commit**

```bash
git add crates/core/examples
git commit -m "test(core): refiner benchmark with labeled utterances"
```

---

## After the tasks

- `cargo fmt --all && cargo clippy --workspace --all-targets && cargo test --workspace`
- Build a release locally (`pnpm tauri build --no-bundle`), run it with cleanup on, and dictate a few phrases.
- Open the PR from `feat/prompt-refiner` with the benchmark summary.
