use serde::{Deserialize, Serialize};
use whispio_core::claude::ClaudeMode;

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
