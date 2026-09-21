use std::path::Path;

use erindi_core::claude::{ClaudeMode, ClaudeRequest, claude_args, resume_in_terminal};
use serde::{Deserialize, Serialize};
use tauri_plugin_global_shortcut::Shortcut;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub hold_hotkey: String,
    pub toggle_hotkey: String,
    pub cwd: String,
    pub mode: ClaudeMode,
    /// Empty means the default model of Claude Code.
    pub model: String,
    /// Empty means the system default microphone.
    pub microphone: String,
    pub silence_secs: f32,
    pub dictionary: Vec<(String, String)>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            hold_hotkey: "Ctrl+Alt+Space".into(),
            toggle_hotkey: "Ctrl+Alt+Shift+Space".into(),
            cwd: std::env::var("USERPROFILE").unwrap_or_default(),
            mode: ClaudeMode::Default,
            model: String::new(),
            microphone: String::new(),
            silence_secs: 2.0,
            dictionary: vec![],
        }
    }
}

impl Settings {
    /// Missing or unreadable files fall back to defaults.
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(path, json).map_err(|e| e.to_string())
    }

    pub fn validate(&self) -> Result<(), String> {
        for combo in [&self.hold_hotkey, &self.toggle_hotkey] {
            combo
                .parse::<Shortcut>()
                .map_err(|e| format!("Invalid hotkey {combo:?}: {e}"))?;
        }
        if self.hold_hotkey.eq_ignore_ascii_case(&self.toggle_hotkey) {
            return Err("Hold and toggle hotkeys must differ".into());
        }
        if !Path::new(&self.cwd).is_dir() {
            return Err(format!("Folder does not exist: {}", self.cwd));
        }
        resume_in_terminal(&self.cwd, Uuid::nil())
            .map_err(|_| "The folder path cannot contain ';' or start with '-'")?;
        let request = ClaudeRequest {
            mode: self.mode,
            model: (!self.model.is_empty()).then(|| self.model.clone()),
            session_id: Uuid::nil(),
        };
        claude_args(&request).map_err(|_| format!("Invalid model name: {}", self.model))?;
        if !(0.5..=10.0).contains(&self.silence_secs) {
            return Err("Silence must be between 0.5 and 10 seconds".into());
        }
        if self
            .dictionary
            .iter()
            .any(|(from, _)| from.trim().is_empty())
        {
            return Err("Dictionary entries need a spoken form".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/settings.json");
        let settings = Settings {
            mode: ClaudeMode::AcceptEdits,
            dictionary: vec![("клод".into(), "Claude".into())],
            ..Settings::default()
        };
        settings.save(&path).unwrap();
        assert_eq!(Settings::load(&path), settings);
    }

    #[test]
    fn missing_or_corrupt_file_gives_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        assert_eq!(Settings::load(&path), Settings::default());
        std::fs::write(&path, "{not json").unwrap();
        assert_eq!(Settings::load(&path), Settings::default());
    }

    #[test]
    fn partial_file_keeps_defaults_for_missing_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, r#"{"mode":"plan"}"#).unwrap();
        let loaded = Settings::load(&path);
        assert_eq!(loaded.mode, ClaudeMode::Plan);
        assert_eq!(loaded.hold_hotkey, Settings::default().hold_hotkey);
    }

    #[test]
    fn validation() {
        let dir = tempfile::tempdir().unwrap();
        let ok = Settings {
            cwd: dir.path().to_string_lossy().into(),
            ..Settings::default()
        };
        assert_eq!(ok.validate(), Ok(()));

        let bad = [
            Settings {
                cwd: dir.path().join("missing").to_string_lossy().into(),
                ..ok.clone()
            },
            Settings {
                cwd: format!("{};calc", dir.path().display()),
                ..ok.clone()
            },
            Settings {
                hold_hotkey: ok.toggle_hotkey.clone(),
                ..ok.clone()
            },
            Settings {
                toggle_hotkey: String::new(),
                ..ok.clone()
            },
            Settings {
                model: "--dangerously-skip-permissions".into(),
                ..ok.clone()
            },
            Settings {
                silence_secs: 0.1,
                ..ok.clone()
            },
            Settings {
                dictionary: vec![("".into(), "x".into())],
                ..ok.clone()
            },
        ];
        for s in bad {
            assert!(s.validate().is_err(), "{s:?}");
        }
    }
}
