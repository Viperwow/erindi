use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

use crate::agent::Agent;

/// How much of the end of a log is read; logs grow to many megabytes.
const TAIL: u64 = 512 << 10;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Details {
    pub model: Option<String>,
    pub permission: Option<String>,
}

/// The newest model and permission in `path`, or `None` when neither is found.
pub fn read(agent: Agent, path: &Path) -> Option<Details> {
    let text = tail(path).ok()?;
    let mut details = Details {
        model: None,
        permission: None,
    };
    for line in text.lines().rev() {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let (model, permission) = match agent {
            Agent::Claude => (
                v["message"]["model"]
                    .as_str()
                    .filter(|m| !m.starts_with('<')),
                v["permissionMode"].as_str(),
            ),
            Agent::Codex if v["type"] == "turn_context" => (
                v["payload"]["model"].as_str(),
                v["payload"]["sandbox_policy"]["type"].as_str(),
            ),
            Agent::Codex => (None, None),
        };
        details.model = details.model.or(model.map(String::from));
        details.permission = details.permission.or(permission.map(String::from));
        if details.model.is_some() && details.permission.is_some() {
            break;
        }
    }
    (details.model.is_some() || details.permission.is_some()).then_some(details)
}

/// The last `TAIL` bytes, starting at a line boundary.
fn tail(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let len = file.metadata()?.len();
    let start = len.saturating_sub(TAIL);
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    let text = String::from_utf8_lossy(&bytes).into_owned();
    Ok(if start > 0 {
        text.split_once('\n')
            .map_or(String::new(), |(_, rest)| rest.to_string())
    } else {
        text
    })
}

/// Log files by native ID under the user's home folder.
pub fn find_logs(agent: Agent, home: &Path) -> HashMap<String, PathBuf> {
    let mut found = HashMap::new();
    match agent {
        // One folder per project, one `<id>.jsonl` per session.
        Agent::Claude => {
            for dir in read_dir(&home.join(".claude/projects")) {
                for file in read_dir(&dir) {
                    let id = file
                        .file_name()
                        .and_then(|n| n.to_str())
                        .and_then(|n| n.strip_suffix(".jsonl"))
                        .map(String::from);
                    if let Some(id) = id {
                        found.insert(id, file);
                    }
                }
            }
        }
        // `sessions/YYYY/MM/DD/rollout-<time>-<id>.jsonl`.
        Agent::Codex => {
            let root = std::env::var_os("CODEX_HOME").map_or(home.join(".codex"), PathBuf::from);
            let mut stack = vec![root.join("sessions")];
            while let Some(dir) = stack.pop() {
                for entry in read_dir(&dir) {
                    if entry.is_dir() {
                        stack.push(entry);
                        continue;
                    }
                    let id = entry
                        .file_name()
                        .and_then(|n| n.to_str())
                        .and_then(|n| n.strip_suffix(".jsonl"))
                        .and_then(uuid_suffix)
                        .map(String::from);
                    if let Some(id) = id {
                        found.insert(id, entry);
                    }
                }
            }
        }
    }
    found
}

/// `rollout-2026-09-24T13-29-56-01a0d2c0-0c6d-7dc0-90c7-da4ffbaf65a2` ends with the 36-character UUID.
fn uuid_suffix(stem: &str) -> Option<&str> {
    stem.strip_prefix("rollout-")?;
    let at = stem.len().checked_sub(36)?;
    let id = stem.get(at..)?;
    (stem[..at].ends_with('-') && uuid::Uuid::parse_str(id).is_ok()).then_some(id)
}

fn read_dir(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .map(|it| it.flatten().map(|e| e.path()).collect())
        .unwrap_or_default()
}

/// `claude-opus-5-5` reads as "Claude Opus 5.5"; other IDs stay as they are.
pub fn model_label(raw: &str) -> String {
    let Some(rest) = raw.strip_prefix("claude-") else {
        return raw.to_string();
    };
    let mut words = vec!["Claude".to_string()];
    let mut version = vec![];
    for part in rest.split('-') {
        let digits = part.chars().all(|c| c.is_ascii_digit());
        if digits && part.len() == 8 {
            continue;
        }
        if digits {
            version.push(part);
        } else {
            let mut chars = part.chars();
            let first: String = chars
                .next()
                .map(|c| c.to_uppercase().collect())
                .unwrap_or_default();
            words.push(first + chars.as_str());
        }
    }
    if !version.is_empty() {
        words.push(version.join("."));
    }
    words.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, name: &str, text: &str) -> PathBuf {
        let p = dir.join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, text).unwrap();
        p
    }

    #[test]
    fn newest_claude_model_and_permission() {
        let d = tempfile::tempdir().unwrap();
        let p = write(
            d.path(),
            "a.jsonl",
            include_str!("../tests/fixtures/claude-log.jsonl"),
        );
        let got = read(Agent::Claude, &p).unwrap();
        assert_eq!(got.model.as_deref(), Some("claude-opus-5-5"));
        assert_eq!(got.permission.as_deref(), Some("plan"));
    }

    #[test]
    fn newest_codex_turn_context() {
        let d = tempfile::tempdir().unwrap();
        let p = write(
            d.path(),
            "a.jsonl",
            include_str!("../tests/fixtures/codex-log.jsonl"),
        );
        let got = read(Agent::Codex, &p).unwrap();
        assert_eq!(got.model.as_deref(), Some("gpt-5.6-sol"));
        assert_eq!(got.permission.as_deref(), Some("workspace-write"));
    }

    #[test]
    fn newest_entry_ignores_a_half_written_last_line() {
        let d = tempfile::tempdir().unwrap();
        let text = format!(
            "{}{{\"type\":\"turn_con",
            include_str!("../tests/fixtures/codex-log.jsonl")
        );
        let p = write(d.path(), "a.jsonl", &text);
        assert_eq!(
            read(Agent::Codex, &p).unwrap().model.as_deref(),
            Some("gpt-5.6-sol")
        );
    }

    #[test]
    fn reads_only_the_tail_of_big_logs() {
        let d = tempfile::tempdir().unwrap();
        let filler = format!("{{\"type\":\"x\",\"pad\":\"{}\"}}\n", "a".repeat(1000)).repeat(2000);
        let text = format!(
            "{}{filler}",
            include_str!("../tests/fixtures/codex-log.jsonl")
        );
        let p = write(d.path(), "a.jsonl", &text);
        // The only turn_context is 2 MB before the end, past the tail that is read.
        assert_eq!(read(Agent::Codex, &p), None);
    }

    #[test]
    fn empty_missing_or_broken_logs_give_none() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(read(Agent::Claude, &d.path().join("none.jsonl")), None);
        let p = write(d.path(), "e.jsonl", "");
        assert_eq!(read(Agent::Claude, &p), None);
        let p = write(d.path(), "b.jsonl", "not json\n[1]\n");
        assert_eq!(read(Agent::Codex, &p), None);
    }

    #[test]
    fn logs_are_found_by_native_id() {
        let d = tempfile::tempdir().unwrap();
        let c = write(d.path(), ".claude/projects/C--p/3f2a.jsonl", "");
        let x = write(
            d.path(),
            ".codex/sessions/2026/09/24/rollout-2026-09-24T13-29-56-01a0d2c0-0c6d-7dc0-90c7-da4ffbaf65a2.jsonl",
            "",
        );
        write(d.path(), ".codex/sessions/2026/09/24/notes.jsonl", "");
        assert_eq!(find_logs(Agent::Claude, d.path()).get("3f2a"), Some(&c));
        let codex = find_logs(Agent::Codex, d.path());
        assert_eq!(codex.get("01a0d2c0-0c6d-7dc0-90c7-da4ffbaf65a2"), Some(&x));
        assert_eq!(codex.len(), 1);
    }

    #[test]
    fn readable_model_names() {
        assert_eq!(model_label("claude-opus-5-5"), "Claude Opus 5.5");
        assert_eq!(model_label("claude-sonnet-4-6"), "Claude Sonnet 4.6");
        assert_eq!(model_label("claude-haiku-4-5-20251001"), "Claude Haiku 4.5");
        assert_eq!(model_label("claude-fable-5"), "Claude Fable 5");
        assert_eq!(model_label("gpt-5.6-sol"), "gpt-5.6-sol");
    }
}
