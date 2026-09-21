use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Oldest sessions beyond this count are dropped.
pub const MAX_ENTRIES: usize = 200;

/// A Claude session started from Erindi, with everything the user said in it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub id: Uuid,
    pub cwd: String,
    pub prompts: Vec<String>,
    pub created_ms: u64,
    pub updated_ms: u64,
}

/// Session history kept newest first in a JSON file.
pub struct History {
    path: PathBuf,
    entries: Vec<Entry>,
}

impl History {
    /// Missing or unreadable files start an empty history.
    pub fn load(path: &Path) -> Self {
        let entries = std::fs::read_to_string(path)
            .ok()
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default();
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

    /// Adds a prompt to session `id`, creating it if needed, and saves the file.
    pub fn record(&mut self, id: Uuid, cwd: &str, prompt: &str, now_ms: u64) -> Result<(), String> {
        let entry = match self.entries.iter().position(|e| e.id == id) {
            Some(i) => {
                let mut entry = self.entries.remove(i);
                entry.prompts.push(prompt.into());
                entry.updated_ms = now_ms;
                entry
            }
            None => Entry {
                id,
                cwd: cwd.into(),
                prompts: vec![prompt.into()],
                created_ms: now_ms,
                updated_ms: now_ms,
            },
        };
        self.entries.insert(0, entry);
        self.entries.truncate(MAX_ENTRIES);

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

    fn id(n: u128) -> Uuid {
        Uuid::from_u128(n)
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
    fn records_new_sessions_newest_first_and_persists() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/sessions.json");
        let mut h = History::load(&path);
        h.record(id(1), "C:/a", "first task", 10).unwrap();
        h.record(id(2), "C:/b", "second task", 20).unwrap();

        let ids: Vec<_> = h.entries().iter().map(|e| e.id).collect();
        assert_eq!(ids, [id(2), id(1)]);
        assert_eq!(History::load(&path).entries(), h.entries());
    }

    #[test]
    fn continuing_a_session_appends_and_moves_it_to_the_top() {
        let dir = tempfile::tempdir().unwrap();
        let mut h = History::load(&dir.path().join("sessions.json"));
        h.record(id(1), "C:/a", "first task", 10).unwrap();
        h.record(id(2), "C:/b", "second task", 20).unwrap();
        h.record(id(1), "C:/a", "add tests", 30).unwrap();

        let top = &h.entries()[0];
        assert_eq!(top.id, id(1));
        assert_eq!(top.prompts, ["first task", "add tests"]);
        assert_eq!((top.created_ms, top.updated_ms), (10, 30));
        assert_eq!(h.entries().len(), 2);
    }

    #[test]
    fn keeps_only_the_newest_entries() {
        let dir = tempfile::tempdir().unwrap();
        let mut h = History::load(&dir.path().join("sessions.json"));
        for n in 0..=MAX_ENTRIES as u128 {
            h.record(id(n), "C:/a", "task", n as u64).unwrap();
        }
        assert_eq!(h.entries().len(), MAX_ENTRIES);
        assert!(h.get(id(0)).is_none());
        assert!(h.get(id(MAX_ENTRIES as u128)).is_some());
    }
}
