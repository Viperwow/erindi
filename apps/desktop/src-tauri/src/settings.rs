use std::collections::BTreeMap;
use std::path::Path;

use erindi_core::agent::{Agent, claude_models, resume_in_terminal, valid_model};
use erindi_core::commands::{Parser, Patterns};
use erindi_core::controller::Msg;
use erindi_core::session::SessionPolicy;
use serde::{Deserialize, Serialize};
use tauri_plugin_global_shortcut::Shortcut;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ModelChoice {
    Listed(String),
    Custom(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AgentSettings {
    /// `None` passes no model flag.
    pub model: Option<ModelChoice>,
    /// `"default"` passes no permission flag.
    pub permission: String,
}

impl Default for AgentSettings {
    fn default() -> Self {
        Self {
            model: None,
            permission: "default".into(),
        }
    }
}

impl AgentSettings {
    pub fn model_id(&self) -> Option<&str> {
        match &self.model {
            Some(ModelChoice::Listed(id) | ModelChoice::Custom(id)) => Some(id),
            None => None,
        }
    }

    pub fn permission_flag(&self) -> Option<&str> {
        (self.permission != "default").then_some(self.permission.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    #[serde(alias = "holdHotkey")]
    pub talk_hotkey: String,
    pub new_session_hotkey: String,
    pub terminal_hotkey: String,
    pub patterns: Patterns,
    pub cwd: String,
    /// The agent of new sessions nobody named an agent for.
    pub agent: Agent,
    pub agents: BTreeMap<Agent, AgentSettings>,
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
            agent: Agent::Claude,
            agents: BTreeMap::new(),
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
            .and_then(|json| Self::from_json(&json).ok())
            .unwrap_or_default()
    }

    pub fn agent_settings(&self, agent: Agent) -> AgentSettings {
        self.agents.get(&agent).cloned().unwrap_or_default()
    }

    /// Reads a settings file, moving the old Claude-only `mode` and `model` into `agents.claude`.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        let mut value: serde_json::Value = serde_json::from_str(json)?;
        if let Some(obj) = value.as_object_mut() {
            let mode = obj.remove("mode");
            let model = obj.remove("model");
            if !obj.contains_key("agents") && (mode.is_some() || model.is_some()) {
                let aliases = claude_models();
                let model = model
                    .and_then(|m| m.as_str().map(String::from))
                    .filter(|m| !m.is_empty())
                    .map(|m| {
                        if aliases.iter().any(|a| a.id == m) {
                            ModelChoice::Listed(m)
                        } else {
                            ModelChoice::Custom(m)
                        }
                    });
                let permission = mode
                    .and_then(|m| m.as_str().map(String::from))
                    .unwrap_or_else(|| "default".into());
                let claude = AgentSettings { model, permission };
                obj.insert("agents".into(), serde_json::json!({ "claude": claude }));
            }
        }
        serde_json::from_value(value)
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
            agent: self.agent,
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
        resume_in_terminal("claude", &self.cwd, Agent::Claude, &Uuid::nil().to_string())
            .map_err(|_| "The folder path cannot contain ';' or start with '-'")?;
        for (agent, s) in &self.agents {
            let name = agent.label();
            if let Some(id) = s.model_id() {
                if id.is_empty() {
                    return Err(format!("Enter a model ID for {name}"));
                }
                if !valid_model(id) {
                    return Err(format!("Invalid model ID for {name}: {id}"));
                }
            }
            if s.permission != "default" && !agent.permissions().contains(&s.permission.as_str()) {
                return Err(format!("Unknown permission for {name}: {}", s.permission));
            }
        }
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
        let mut settings = Settings {
            agent: Agent::Codex,
            dictionary: vec![("клод".into(), "Claude".into())],
            ..Settings::default()
        };
        settings.agents.insert(
            Agent::Codex,
            AgentSettings {
                model: Some(ModelChoice::Custom("gpt-5.5".into())),
                permission: "workspace-write".into(),
            },
        );
        settings.save(&path).unwrap();
        assert_eq!(Settings::load(&path), settings);
    }

    #[test]
    fn old_mode_and_model_move_to_claude() {
        let json = r#"{"mode":"plan","model":"opus","cwd":"C:/p"}"#;
        let s = Settings::from_json(json).unwrap();
        assert_eq!(s.agent, Agent::Claude);
        assert_eq!(
            s.agent_settings(Agent::Claude),
            AgentSettings {
                model: Some(ModelChoice::Listed("opus".into())),
                permission: "plan".into()
            }
        );
        let s = Settings::from_json(r#"{"model":"claude-opus-4-8"}"#).unwrap();
        assert_eq!(
            s.agent_settings(Agent::Claude).model,
            Some(ModelChoice::Custom("claude-opus-4-8".into()))
        );
        let s = Settings::from_json(r#"{"mode":"default","model":""}"#).unwrap();
        assert_eq!(s.agent_settings(Agent::Claude), AgentSettings::default());
    }

    #[test]
    fn missing_agents_get_defaults() {
        let s = Settings::from_json("{}").unwrap();
        assert_eq!(s.agent_settings(Agent::Codex), AgentSettings::default());
        assert_eq!(s.agent_settings(Agent::Codex).permission_flag(), None);
        assert_eq!(s.agent_settings(Agent::Codex).model_id(), None);
    }

    #[test]
    fn model_ids_are_checked_on_save() {
        let dir = tempfile::tempdir().unwrap();
        let ok = Settings {
            cwd: dir.path().to_string_lossy().into(),
            ..Settings::default()
        };
        let with = |model: ModelChoice, permission: &str| {
            let mut s = ok.clone();
            s.agents.insert(
                Agent::Claude,
                AgentSettings {
                    model: Some(model),
                    permission: permission.into(),
                },
            );
            s.validate()
        };
        assert_eq!(
            with(ModelChoice::Custom(String::new()), "default"),
            Err("Enter a model ID for Claude".into())
        );
        assert_eq!(
            with(ModelChoice::Custom("--x".into()), "default"),
            Err("Invalid model ID for Claude: --x".into())
        );
        assert!(with(ModelChoice::Listed("a b".into()), "default").is_err());
        assert!(with(ModelChoice::Custom("claude-opus-4-8".into()), "plan").is_ok());
        assert_eq!(
            with(ModelChoice::Listed("opus".into()), "workspace-write"),
            Err("Unknown permission for Claude: workspace-write".into())
        );
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
        assert_eq!(loaded.agent_settings(Agent::Claude).permission, "plan");
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
