use std::path::Path;

use erindi_core::claude::{ClaudeMode, ClaudeRequest, Session, claude_args, resume_in_terminal};
use erindi_core::commands::{Parser, Patterns};
use erindi_core::controller::Msg;
use erindi_core::session::SessionPolicy;
use serde::{Deserialize, Serialize};
use tauri_plugin_global_shortcut::Shortcut;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    #[serde(alias = "holdHotkey")]
    pub talk_hotkey: String,
    pub new_session_hotkey: String,
    pub terminal_hotkey: String,
    pub patterns: Patterns,
    pub cwd: String,
    pub mode: ClaudeMode,
    /// Empty means the default model of Claude Code.
    pub model: String,
    /// Empty means the system default microphone.
    pub microphone: String,
    pub silence_secs: f32,
    pub session_policy: SessionPolicy,
    /// Used by `SessionPolicy::ContinueIfRecent`.
    pub recent_minutes: u32,
    pub dictionary: Vec<(String, String)>,
    /// Ask the local model for commands the patterns miss.
    #[serde(alias = "cleanup")]
    pub model_commands: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            talk_hotkey: "Ctrl+Alt+Space".into(),
            new_session_hotkey: "Ctrl+Alt+N".into(),
            terminal_hotkey: "Ctrl+Alt+T".into(),
            patterns: Patterns::default(),
            cwd: std::env::var("USERPROFILE").unwrap_or_default(),
            mode: ClaudeMode::Default,
            model: String::new(),
            microphone: String::new(),
            silence_secs: 2.0,
            session_policy: SessionPolicy::Continue,
            recent_minutes: 30,
            dictionary: vec![],
            model_commands: false,
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

    /// The part of the settings the controller needs to pick sessions.
    pub fn session_msg(&self) -> Msg {
        Msg::Settings {
            policy: self.session_policy,
            recent: std::time::Duration::from_secs(u64::from(self.recent_minutes) * 60),
            cwd: self.cwd.clone(),
            patterns: self.patterns.clone(),
            model_commands: self.model_commands,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        let hotkeys = [
            &self.talk_hotkey,
            &self.new_session_hotkey,
            &self.terminal_hotkey,
        ];
        for combo in hotkeys {
            combo
                .parse::<Shortcut>()
                .map_err(|e| format!("Invalid hotkey {combo:?}: {e}"))?;
        }
        for (i, a) in hotkeys.iter().enumerate() {
            if hotkeys[i + 1..].iter().any(|b| a.eq_ignore_ascii_case(b)) {
                return Err(format!("Hotkey {a} is used twice"));
            }
        }
        Parser::new(&self.patterns)?;
        if !Path::new(&self.cwd).is_dir() {
            return Err(format!("Folder does not exist: {}", self.cwd));
        }
        resume_in_terminal("claude", &self.cwd, Uuid::nil())
            .map_err(|_| "The folder path cannot contain ';' or start with '-'")?;
        let request = ClaudeRequest {
            mode: self.mode,
            model: (!self.model.is_empty()).then(|| self.model.clone()),
            session: Session::New(Uuid::nil()),
        };
        claude_args(&request).map_err(|_| format!("Invalid model name: {}", self.model))?;
        if !(1..=1440).contains(&self.recent_minutes) {
            return Err("Recent session window must be between 1 and 1440 minutes".into());
        }
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
    use erindi_core::commands::Patterns;

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
        assert_eq!(loaded.talk_hotkey, Settings::default().talk_hotkey);
        assert_eq!(loaded.session_policy, SessionPolicy::Continue);
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
                talk_hotkey: ok.terminal_hotkey.clone(),
                ..ok.clone()
            },
            Settings {
                terminal_hotkey: String::new(),
                ..ok.clone()
            },
            Settings {
                patterns: Patterns {
                    cancel: vec!["(".into()],
                    ..Patterns::default()
                },
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
                new_session_hotkey: ok.talk_hotkey.clone(),
                ..ok.clone()
            },
            Settings {
                recent_minutes: 0,
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

    #[test]
    fn hotkeys_from_the_recorder_parse() {
        for combo in [
            "Ctrl+Alt+Shift+Space",
            "Ctrl+Super+N",
            "Ctrl+5",
            "Alt+Backquote",
            "F9",
            "Shift+F13",
        ] {
            assert!(combo.parse::<Shortcut>().is_ok(), "{combo}");
        }
    }

    #[test]
    fn model_commands_are_off_by_default_and_reach_the_controller() {
        let s = Settings::default();
        assert!(!s.model_commands);
        let on = Settings {
            model_commands: true,
            ..Settings::default()
        };
        assert!(matches!(
            on.session_msg(),
            Msg::Settings {
                model_commands: true,
                ..
            }
        ));
    }

    #[test]
    fn old_settings_keys_still_load() {
        let json = r#"{"holdHotkey":"F9","toggleHotkey":"F10","cleanup":true}"#;
        let s: Settings = serde_json::from_str(json).unwrap();
        assert_eq!(s.talk_hotkey, "F9");
        assert!(s.model_commands);
        assert_eq!(s.patterns, Patterns::default());
    }
}
