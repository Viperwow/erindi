use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use erindi_core::agent::{Agent, ModelOption, claude_models};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

/// How old a check may be before the Settings window re-checks on focus.
const STALE: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentStatus {
    pub agent: Agent,
    pub label: &'static str,
    pub path: Option<String>,
    pub models: Vec<ModelOption>,
    pub models_error: Option<String>,
    pub permissions: Vec<&'static str>,
}

/// When the agents were last checked, and what was found.
type Checked = Option<(Instant, Vec<AgentStatus>)>;

/// What Erindi last found out about each agent CLI.
#[derive(Clone, Default)]
pub struct Agents(Arc<Mutex<Checked>>);

pub fn missing(agent: Agent) -> String {
    format!(
        "{} CLI not found. Install it, then press Re-check in Settings.",
        agent.label()
    )
}

fn status_of(
    agent: Agent,
    path: Option<String>,
    models: Result<Vec<ModelOption>, String>,
) -> AgentStatus {
    let (models, models_error) = match models {
        Ok(m) => (m, None),
        Err(e) => (
            vec![],
            Some(format!("Couldn't read {} models: {e}", agent.label())),
        ),
    };
    AgentStatus {
        agent,
        label: agent.label(),
        path,
        models,
        models_error,
        permissions: agent.permissions().to_vec(),
    }
}

fn check(agent: Agent) -> AgentStatus {
    let path = erindi_core::cli::locate(agent);
    let models = match (agent, &path) {
        (Agent::Claude, _) => Ok(claude_models()),
        (Agent::Codex, None) => Ok(vec![]),
        (Agent::Codex, Some(program)) => codex_models(program),
    };
    status_of(agent, path.map(|p| p.display().to_string()), models)
}

fn codex_models(program: &Path) -> Result<Vec<ModelOption>, String> {
    let mut command = std::process::Command::new(program);
    command.args(["debug", "models"]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let out = command.output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(stderr.lines().last().unwrap_or("failed").to_string());
    }
    erindi_core::codex::parse_models(&String::from_utf8_lossy(&out.stdout))
}

impl Agents {
    pub fn status(&self) -> Vec<AgentStatus> {
        self.0
            .lock()
            .unwrap()
            .as_ref()
            .map(|(_, s)| s.clone())
            .unwrap_or_default()
    }

    /// Checks every agent off the calling thread and emits `agents-changed` when anything changed.
    pub fn recheck(&self, app: &AppHandle) {
        let (this, app) = (self.clone(), app.clone());
        std::thread::spawn(move || {
            let fresh: Vec<_> = Agent::ALL.into_iter().map(check).collect();
            let changed = this.status() != fresh;
            *this.0.lock().unwrap() = Some((Instant::now(), fresh.clone()));
            if changed {
                let _ = app.emit_to("settings", "agents-changed", fresh);
            }
        });
    }

    pub fn recheck_if_stale(&self, app: &AppHandle) {
        let stale = self
            .0
            .lock()
            .unwrap()
            .as_ref()
            .is_none_or(|(at, _)| at.elapsed() > STALE);
        if stale {
            self.recheck(app);
        }
    }

    /// Finds the CLI now, and re-checks everything when it appeared, moved or went away.
    pub fn locate(&self, agent: Agent, app: &AppHandle) -> Option<PathBuf> {
        let path = erindi_core::cli::locate(agent);
        let known = self
            .status()
            .into_iter()
            .find(|s| s.agent == agent)
            .and_then(|s| s.path);
        if known != path.as_ref().map(|p| p.display().to_string()) {
            self.recheck(app);
        }
        path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_cli_message_names_the_agent() {
        assert_eq!(
            missing(Agent::Codex),
            "Codex CLI not found. Install it, then press Re-check in Settings."
        );
    }

    #[test]
    fn status_reports_a_model_list_error() {
        let s = status_of(
            Agent::Codex,
            Some("C:/codex.cmd".into()),
            Err("not JSON: x".into()),
        );
        assert_eq!(s.models, vec![]);
        assert_eq!(
            s.models_error.as_deref(),
            Some("Couldn't read Codex models: not JSON: x")
        );
        let s = status_of(Agent::Claude, None, Ok(vec![]));
        assert_eq!(s.path, None);
        assert_eq!(s.models_error, None);
    }
}
