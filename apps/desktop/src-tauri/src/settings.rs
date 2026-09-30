use std::collections::BTreeMap;
use std::path::Path;

use erindi_core::agent::{Agent, claude_models, resume_in_terminal, valid_model};
use erindi_core::commands::{Parser, Patterns};
use erindi_core::controller::{Action, Gesture, Msg};
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
    pub talk_gesture: Gesture,
    pub cancel_hotkey: String,
    pub cancel_gesture: Gesture,
    pub hands_free_hotkey: String,
    pub hands_free_gesture: Gesture,
    pub new_session_hands_free_hotkey: String,
    pub new_session_hands_free_gesture: Gesture,
    pub new_session_hotkey: String,
    pub new_session_gesture: Gesture,
    pub terminal_hotkey: String,
    pub terminal_gesture: Gesture,
    pub patterns: Patterns,
    pub cwd: String,
    /// The agent of new sessions nobody named an agent for.
    pub agent: Agent,
    pub agents: BTreeMap<Agent, AgentSettings>,
    /// Empty means the system default microphone.
    pub microphone: String,
    pub silence_secs: f32,
    /// How long an idle bubble stays up before it hides.
    pub hide_secs: f32,
    /// How soon a second press must follow to make a double-press.
    pub double_secs: f32,
    /// Where the debug log goes; empty turns it off.
    pub log_path: String,
    pub session_policy: SessionPolicy,
    /// Used by `SessionPolicy::ContinueIfRecent`.
    pub recent_minutes: u32,
    pub dictionary: Vec<(String, String)>,
    /// Ask the local model for commands the patterns miss.
    #[serde(alias = "cleanup")]
    pub model_commands: bool,
    /// Show the window on launch instead of staying in the tray.
    pub open_on_launch: bool,
    pub launch_at_login: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            talk_hotkey: "Ctrl+Space".into(),
            talk_gesture: Gesture::Hold,
            cancel_hotkey: "Ctrl+Space".into(),
            cancel_gesture: Gesture::Tap,
            hands_free_hotkey: "Ctrl+Alt+Space".into(),
            hands_free_gesture: Gesture::DoubleTap,
            new_session_hands_free_hotkey: "Ctrl+Alt+Shift+Space".into(),
            new_session_hands_free_gesture: Gesture::DoubleTap,
            new_session_hotkey: "Ctrl+Shift+Space".into(),
            new_session_gesture: Gesture::Hold,
            terminal_hotkey: "Ctrl+Alt+T".into(),
            terminal_gesture: Gesture::Tap,
            patterns: Patterns::default(),
            cwd: crate::runtime::home()
                .map(|h| h.display().to_string())
                .unwrap_or_default(),
            agent: Agent::Claude,
            agents: BTreeMap::new(),
            microphone: String::new(),
            silence_secs: 2.0,
            hide_secs: 5.0,
            double_secs: 0.4,
            log_path: std::env::temp_dir()
                .join("erindi-trace.log")
                .to_string_lossy()
                .into_owned(),
            session_policy: SessionPolicy::Continue,
            recent_minutes: 30,
            dictionary: vec![],
            model_commands: false,
            open_on_launch: false,
            launch_at_login: false,
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
            hide_after: std::time::Duration::from_secs_f32(self.hide_secs),
            double: std::time::Duration::from_millis((self.double_secs * 1000.0).round() as u64),
            bindings: self.bindings().1,
        }
    }

    fn shortcuts(&self) -> [(&str, Action, Gesture); 6] {
        [
            (&self.talk_hotkey, Action::PushToTalk, self.talk_gesture),
            (
                &self.new_session_hotkey,
                Action::NewSession,
                self.new_session_gesture,
            ),
            (
                &self.terminal_hotkey,
                Action::Terminal,
                self.terminal_gesture,
            ),
            (&self.cancel_hotkey, Action::Cancel, self.cancel_gesture),
            (
                &self.hands_free_hotkey,
                Action::HandsFree,
                self.hands_free_gesture,
            ),
            (
                &self.new_session_hands_free_hotkey,
                Action::NewSessionHandsFree,
                self.new_session_hands_free_gesture,
            ),
        ]
    }

    /// The distinct key combinations to register, and what each one does.
    pub fn bindings(&self) -> (Vec<String>, Vec<Vec<(Action, Gesture)>>) {
        let mut combos: Vec<String> = vec![];
        let mut bindings: Vec<Vec<(Action, Gesture)>> = vec![];
        for (combo, action, gesture) in self.shortcuts() {
            match combos.iter().position(|c| c.eq_ignore_ascii_case(combo)) {
                Some(i) => bindings[i].push((action, gesture)),
                None => {
                    combos.push(combo.to_string());
                    bindings.push(vec![(action, gesture)]);
                }
            }
        }
        (combos, bindings)
    }

    /// Two actions may share a combination only with different gestures.
    fn check_bindings(&self) -> Result<(), String> {
        let shortcuts = self.shortcuts();
        for (i, (combo, action, gesture)) in shortcuts.iter().enumerate() {
            combo
                .parse::<Shortcut>()
                .map_err(|e| format!("Invalid hotkey {combo:?}: {e}"))?;
            let clash = shortcuts[i + 1..]
                .iter()
                .find(|(c, _, g)| c.eq_ignore_ascii_case(combo) && g == gesture);
            if let Some((_, other, _)) = clash {
                return Err(format!(
                    "{} and {} both use {combo} with {}; change one shortcut or mode",
                    action_name(*action),
                    action_name(*other),
                    gesture_name(*gesture)
                ));
            }
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<(), String> {
        self.check_bindings()?;
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
        if !(2.0..=120.0).contains(&self.hide_secs) {
            return Err("Hide delay must be between 2 and 120 seconds".into());
        }
        if !(0.2..=2.0).contains(&self.double_secs) {
            return Err("Double-press window must be between 0.2 and 2 seconds".into());
        }
        if self
            .dictionary
            .iter()
            .any(|(from, to)| from.trim().is_empty() || to.trim().is_empty())
        {
            return Err("Fill in both words of every dictionary entry".into());
        }
        Ok(())
    }
}

fn action_name(action: Action) -> &'static str {
    match action {
        Action::PushToTalk => "Push to talk",
        Action::NewSession => "New session",
        Action::HandsFree => "Hands-free",
        Action::NewSessionHandsFree => "New session hands-free",
        Action::Cancel => "Cancel",
        Action::Terminal => "Open in terminal",
    }
}

fn gesture_name(gesture: Gesture) -> &'static str {
    match gesture {
        Gesture::Tap => "Tap",
        Gesture::Hold => "Hold",
        Gesture::DoubleTap => "Double-tap",
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
                cancel_gesture: Gesture::Hold,
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
                hide_secs: 1.0,
                ..ok.clone()
            },
            Settings {
                double_secs: 5.0,
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
            Settings {
                dictionary: vec![("x".into(), " ".into())],
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
    fn talk_and_cancel_share_a_shortcut_by_default() {
        let (combos, bindings) = Settings::default().bindings();
        assert_eq!(
            combos,
            [
                "Ctrl+Space",
                "Ctrl+Shift+Space",
                "Ctrl+Alt+T",
                "Ctrl+Alt+Space",
                "Ctrl+Alt+Shift+Space"
            ]
        );
        assert_eq!(
            bindings[4],
            [(Action::NewSessionHandsFree, Gesture::DoubleTap)]
        );
        assert_eq!(
            bindings[0],
            [
                (Action::PushToTalk, Gesture::Hold),
                (Action::Cancel, Gesture::Tap)
            ]
        );
        assert_eq!(bindings[3], [(Action::HandsFree, Gesture::DoubleTap)]);
    }

    #[test]
    fn the_same_shortcut_and_mode_twice_is_rejected() {
        let s = Settings {
            cancel_gesture: Gesture::Hold,
            ..Settings::default()
        };
        let err = s.check_bindings().unwrap_err();
        assert!(
            err.contains("Push to talk") && err.contains("Cancel"),
            "{err}"
        );
    }

    #[test]
    fn double_press_window_reaches_the_controller() {
        let s = Settings {
            double_secs: 0.8,
            ..Settings::default()
        };
        assert!(matches!(
            s.session_msg(),
            Msg::Settings { double, .. } if double == std::time::Duration::from_millis(800)
        ));
        assert_eq!(Settings::default().double_secs, 0.4);
    }

    #[test]
    fn the_debug_log_is_on_by_default_in_temp() {
        let s = Settings::default();
        assert_eq!(
            std::path::PathBuf::from(&s.log_path),
            std::env::temp_dir().join("erindi-trace.log")
        );
    }

    #[test]
    fn hide_delay_reaches_the_controller() {
        let s = Settings {
            hide_secs: 12.0,
            ..Settings::default()
        };
        assert!(matches!(
            s.session_msg(),
            Msg::Settings { hide_after, .. } if hide_after == std::time::Duration::from_secs(12)
        ));
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
