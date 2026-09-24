use std::path::{Path, PathBuf};

use erindi_core::agent::Agent;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Oldest sessions beyond this count are dropped.
pub const MAX_ENTRIES: usize = 200;

/// One utterance. Plain prompts serialize as bare strings, as history files always had them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Prompt {
    Plain(String),
    Refined { text: String, raw: String },
}

/// A session started from Erindi, with everything the user said in it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub id: Uuid,
    pub cwd: String,
    pub prompts: Vec<Prompt>,
    pub created_ms: u64,
    pub updated_ms: u64,
    #[serde(default)]
    pub agent: Agent,
    /// The agent's own session ID; `None` for a Codex run that never reported one.
    #[serde(default)]
    pub native_id: Option<String>,
    #[serde(default)]
    pub started_model: Option<String>,
    #[serde(default)]
    pub started_permission: Option<String>,
}

impl Entry {
    pub fn native(&self) -> Option<&str> {
        self.native_id.as_deref()
    }
}

/// What a new session starts with; an existing session keeps its own values.
pub struct Start {
    pub agent: Agent,
    pub native_id: Option<String>,
    pub model: Option<String>,
    pub permission: Option<String>,
}

/// Session history kept newest first in a JSON file.
pub struct History {
    path: PathBuf,
    entries: Vec<Entry>,
}

impl History {
    /// Missing or unreadable files start an empty history.
    pub fn load(path: &Path) -> Self {
        let mut entries: Vec<Entry> = std::fs::read_to_string(path)
            .ok()
            // Editors such as Notepad may add a byte order mark that serde_json rejects.
            .and_then(|json| serde_json::from_str(json.trim_start_matches('\u{feff}')).ok())
            .unwrap_or_default();
        // Claude sessions from before agents existed resume by Erindi's own ID.
        for e in &mut entries {
            if e.agent == Agent::Claude && e.native_id.is_none() {
                e.native_id = Some(e.id.to_string());
            }
        }
        Self {
            path: path.to_path_buf(),
            entries,
        }
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn get(&self, id: Uuid) -> Option<&Entry> {
        self.entries.iter().find(|e| e.id == id)
    }

    /// Removes session `id` from the list and saves the file. Claude keeps the session itself.
    pub fn remove(&mut self, id: Uuid) -> Result<(), String> {
        self.entries.retain(|e| e.id != id);
        self.save()
    }

    /// Adds a prompt to session `id`, creating it if needed, and saves the file.
    pub fn record(
        &mut self,
        id: Uuid,
        cwd: &str,
        prompt: Prompt,
        now_ms: u64,
        start: &Start,
    ) -> Result<(), String> {
        let entry = match self.entries.iter().position(|e| e.id == id) {
            Some(i) => {
                let mut entry = self.entries.remove(i);
                entry.prompts.push(prompt);
                entry.updated_ms = now_ms;
                entry
            }
            None => Entry {
                id,
                cwd: cwd.into(),
                prompts: vec![prompt],
                created_ms: now_ms,
                updated_ms: now_ms,
                agent: start.agent,
                native_id: start.native_id.clone(),
                started_model: start.model.clone(),
                started_permission: start.permission.clone(),
            },
        };
        self.entries.insert(0, entry);
        self.entries.truncate(MAX_ENTRIES);
        self.save()
    }

    /// Stores the ID the agent reported for session `id`.
    pub fn set_native(&mut self, id: Uuid, native_id: &str) -> Result<(), String> {
        if let Some(e) = self.entries.iter_mut().find(|e| e.id == id) {
            e.native_id = Some(native_id.to_string());
        }
        self.save()
    }

    fn save(&self) -> Result<(), String> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let json = serde_json::to_string(&self.entries).map_err(|e| e.to_string())?;
        std::fs::write(&self.path, json).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use erindi_core::agent::Agent;

    fn id(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }

    fn plain(text: &str) -> Prompt {
        Prompt::Plain(text.into())
    }

    fn claude_start(id: Uuid) -> Start {
        Start {
            agent: Agent::Claude,
            native_id: Some(id.to_string()),
            model: None,
            permission: None,
        }
    }

    #[test]
    fn entries_without_agent_are_claude() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.json");
        let old = format!(
            r#"[{{"id":"{}","cwd":"C:/a","prompts":["x"],"createdMs":1,"updatedMs":1}}]"#,
            id(1)
        );
        std::fs::write(&path, old).unwrap();
        let h = History::load(&path);
        let e = h.get(id(1)).unwrap();
        assert_eq!(e.agent, Agent::Claude);
        assert_eq!(e.native(), Some(id(1).to_string().as_str()));
    }

    #[test]
    fn codex_sessions_get_their_native_id_later() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.json");
        let mut h = History::load(&path);
        let start = Start {
            agent: Agent::Codex,
            native_id: None,
            model: Some("gpt-5.5".into()),
            permission: Some("read-only".into()),
        };
        h.record(id(1), "C:/a", plain("fix"), 10, &start).unwrap();
        assert_eq!(h.get(id(1)).unwrap().native(), None);
        h.set_native(id(1), "01a0").unwrap();
        let h = History::load(&path);
        let e = h.get(id(1)).unwrap();
        assert_eq!((e.agent, e.native()), (Agent::Codex, Some("01a0")));
        assert_eq!(e.started_model.as_deref(), Some("gpt-5.5"));
    }

    #[test]
    fn continuing_keeps_the_start_values() {
        let dir = tempfile::tempdir().unwrap();
        let mut h = History::load(&dir.path().join("s.json"));
        let start = Start {
            agent: Agent::Codex,
            native_id: Some("n".into()),
            model: Some("a".into()),
            permission: None,
        };
        h.record(id(1), "C:/a", plain("one"), 1, &start).unwrap();
        let later = Start {
            agent: Agent::Codex,
            native_id: Some("n".into()),
            model: None,
            permission: None,
        };
        h.record(id(1), "C:/a", plain("two"), 2, &later).unwrap();
        assert_eq!(h.get(id(1)).unwrap().started_model.as_deref(), Some("a"));
    }

    #[test]
    fn refined_prompts_keep_the_raw_text_and_old_files_still_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.json");
        let old = format!(
            r#"[{{"id":"{}","cwd":"C:/a","prompts":["old"],"createdMs":1,"updatedMs":1}}]"#,
            id(1)
        );
        std::fs::write(&path, old).unwrap();
        let mut h = History::load(&path);
        let refined = Prompt::Refined {
            text: "Fix it.".into(),
            raw: "um fix it".into(),
        };
        h.record(id(1), "C:/a", refined.clone(), 2, &claude_start(id(1)))
            .unwrap();
        let h = History::load(&path);
        assert_eq!(h.get(id(1)).unwrap().prompts, [plain("old"), refined]);
    }

    #[test]
    fn missing_or_corrupt_file_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sessions.json");
        assert!(History::load(&path).entries().is_empty());
        std::fs::write(&path, "[{").unwrap();
        assert!(History::load(&path).entries().is_empty());
    }

    #[test]
    fn file_saved_with_a_byte_order_mark_loads() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sessions.json");
        let json = format!(
            "\u{feff}[{{\"id\":\"{}\",\"cwd\":\"C:/a\",\"prompts\":[\"x\"],\"createdMs\":1,\"updatedMs\":1}}]",
            id(1)
        );
        std::fs::write(&path, json).unwrap();
        assert_eq!(History::load(&path).entries().len(), 1);
    }

    #[test]
    fn records_new_sessions_newest_first_and_persists() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/sessions.json");
        let mut h = History::load(&path);
        h.record(id(1), "C:/a", plain("first task"), 10, &claude_start(id(1)))
            .unwrap();
        h.record(
            id(2),
            "C:/b",
            plain("second task"),
            20,
            &claude_start(id(2)),
        )
        .unwrap();

        let ids: Vec<_> = h.entries().iter().map(|e| e.id).collect();
        assert_eq!(ids, [id(2), id(1)]);
        assert_eq!(History::load(&path).entries(), h.entries());
    }

    #[test]
    fn continuing_a_session_appends_and_moves_it_to_the_top() {
        let dir = tempfile::tempdir().unwrap();
        let mut h = History::load(&dir.path().join("sessions.json"));
        h.record(id(1), "C:/a", plain("first task"), 10, &claude_start(id(1)))
            .unwrap();
        h.record(
            id(2),
            "C:/b",
            plain("second task"),
            20,
            &claude_start(id(2)),
        )
        .unwrap();
        h.record(id(1), "C:/a", plain("add tests"), 30, &claude_start(id(1)))
            .unwrap();

        let top = &h.entries()[0];
        assert_eq!(top.id, id(1));
        assert_eq!(top.prompts, [plain("first task"), plain("add tests")]);
        assert_eq!((top.created_ms, top.updated_ms), (10, 30));
        assert_eq!(h.entries().len(), 2);
    }

    #[test]
    fn removed_sessions_are_gone_after_reload() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sessions.json");
        let mut h = History::load(&path);
        h.record(id(1), "C:/a", plain("first task"), 10, &claude_start(id(1)))
            .unwrap();
        h.record(
            id(2),
            "C:/b",
            plain("second task"),
            20,
            &claude_start(id(2)),
        )
        .unwrap();

        h.remove(id(1)).unwrap();
        h.remove(id(99)).unwrap();

        let ids: Vec<_> = History::load(&path)
            .entries()
            .iter()
            .map(|e| e.id)
            .collect();
        assert_eq!(ids, [id(2)]);
    }

    #[test]
    fn keeps_only_the_newest_entries() {
        let dir = tempfile::tempdir().unwrap();
        let mut h = History::load(&dir.path().join("sessions.json"));
        for n in 0..=MAX_ENTRIES as u128 {
            h.record(id(n), "C:/a", plain("task"), n as u64, &claude_start(id(n)))
                .unwrap();
        }
        assert_eq!(h.entries().len(), MAX_ENTRIES);
        assert!(h.get(id(0)).is_none());
        assert!(h.get(id(MAX_ENTRIES as u128)).is_some());
    }
}
