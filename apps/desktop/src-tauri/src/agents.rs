use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use erindi_core::agent::{Agent, ModelOption, claude_models};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

/// How old a check may be before the Settings window re-checks on focus or a tab switch.
const STALE: Duration = Duration::from_secs(30);
/// A CLI that has not listed its models by then is treated as failed, so the model field never waits forever.
const CLI_TIMEOUT: Duration = Duration::from_secs(30);

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

/// A failed model list counts as stale, so every look at Settings asks again until it loads.
fn stale(checked: &Checked) -> bool {
    checked.as_ref().is_none_or(|(at, status)| {
        at.elapsed() > STALE || status.iter().any(|s| s.models_error.is_some())
    })
}

/// What Erindi last found out about each agent CLI.
#[derive(Clone, Default)]
pub struct Agents {
    checked: Arc<Mutex<Checked>>,
    /// Held while a check runs, so a request that arrives meanwhile waits and takes its result.
    running: Arc<Mutex<()>>,
}

pub fn missing(agent: Agent) -> String {
    format!(
        "{} CLI not found. Install it, or pick another agent in Settings.",
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

/// Every agent in the usual order: a fresh answer where one arrived, else the previous one.
fn merged(previous: &[AgentStatus], done: &[AgentStatus]) -> Vec<AgentStatus> {
    Agent::ALL
        .into_iter()
        .filter_map(|agent| {
            let pick = |list: &[AgentStatus]| list.iter().find(|s| s.agent == agent).cloned();
            pick(done).or_else(|| pick(previous))
        })
        .collect()
}

fn check(agent: Agent) -> AgentStatus {
    // The local model is a server, not a CLI; Settings reads its models from the server itself.
    if !agent.is_cli() {
        return status_of(agent, Some(String::new()), Ok(vec![]));
    }
    let path = erindi_core::cli::locate(agent);
    let models = match (agent, &path) {
        (Agent::Claude, _) => Ok(claude_models()),
        (Agent::Codex, None) => Ok(vec![]),
        (Agent::Codex, Some(program)) => cli_output(program, &["debug", "models"])
            .and_then(|out| erindi_core::codex::parse_models(&out)),
        (Agent::Api, _) => unreachable!("the local model is checked above"),
        (Agent::Pi, None) => Ok(vec![]),
        (Agent::Pi, Some(program)) => {
            cli_output(program, &["--list-models"]).map(|out| erindi_core::pi::parse_models(&out))
        }
        (Agent::Cursor, None) => Ok(vec![]),
        (Agent::Cursor, Some(program)) => {
            cli_output(program, &["models"]).map(|out| erindi_core::cursor::parse_models(&out))
        }
    };
    status_of(agent, path.map(|p| p.display().to_string()), models)
}

fn cli_output(program: &Path, args: &[&str]) -> Result<String, String> {
    cli_output_within(program, args, CLI_TIMEOUT)
}

fn cli_output_within(program: &Path, args: &[&str], timeout: Duration) -> Result<String, String> {
    let mut command = std::process::Command::new(program);
    // npm-installed CLIs look for `node` on PATH, which a macOS app only has from the login shell.
    command
        .args(args)
        .envs(erindi_core::shell_env::vars())
        .env("PATH", erindi_core::cli::current_path());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let child = command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    // `.cmd` CLIs start node underneath; the job takes the whole tree down when it drops on a timeout.
    #[cfg(windows)]
    let _job =
        erindi_core::job::KillOnClose::new().and_then(|job| job.assign(&child).map(|()| job));
    #[cfg(unix)]
    let pid = child.id();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });
    let Ok(out) = rx.recv_timeout(timeout) else {
        #[cfg(unix)]
        let _ = std::process::Command::new("kill")
            .arg(pid.to_string())
            .status();
        return Err(format!("no answer in {} s", timeout.as_secs()));
    };
    let out = out.map_err(|e| e.to_string())?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(stderr.lines().last().unwrap_or("failed").to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

impl Agents {
    pub fn status(&self) -> Vec<AgentStatus> {
        self.checked
            .lock()
            .unwrap()
            .as_ref()
            .map(|(_, s)| s.clone())
            .unwrap_or_default()
    }

    /// Checks every agent off the calling thread and emits `agents-changed` when anything changed.
    pub fn recheck(&self, app: &AppHandle) {
        let (this, app) = (self.clone(), app.clone());
        std::thread::spawn(move || this.check_now(&app));
    }

    /// Checks every agent on this thread, emits `agents-changed` when anything changed, and
    /// returns what it found.
    pub fn check_now(&self, app: &AppHandle) -> Vec<AgentStatus> {
        let (fresh, changed) = self.check_once(|| {
            erindi_core::shell_env::refresh();
            // In parallel, and each agent shows as soon as its own check ends, so the selected one
            // never waits for the slowest CLI.
            let previous = self.status();
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::scope(|s| {
                for agent in Agent::ALL {
                    let tx = tx.clone();
                    s.spawn(move || tx.send(check(agent)));
                }
                drop(tx);
                let mut done = Vec::new();
                for status in rx {
                    done.push(status);
                    let _ = app.emit_to("settings", "agents-changed", merged(&previous, &done));
                }
                merged(&[], &done)
            })
        });
        if changed {
            let _ = app.emit_to("settings", "agents-changed", fresh.clone());
        }
        fresh
    }

    /// Runs `check_all` unless a check that finished after this call began already answered.
    /// Returns the status and whether this call changed it.
    fn check_once(&self, check_all: impl FnOnce() -> Vec<AgentStatus>) -> (Vec<AgentStatus>, bool) {
        let asked = Instant::now();
        let _running = self.running.lock().unwrap();
        if let Some((at, status)) = self.checked.lock().unwrap().as_ref()
            && *at >= asked
        {
            return (status.clone(), false);
        }
        let fresh = check_all();
        let changed = self.status() != fresh;
        *self.checked.lock().unwrap() = Some((Instant::now(), fresh.clone()));
        (fresh, changed)
    }

    pub fn is_stale(&self) -> bool {
        stale(&self.checked.lock().unwrap())
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
    fn overlapping_checks_share_one_run() {
        let agents = Agents::default();
        let runs = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let threads: Vec<_> = (0..4)
            .map(|_| {
                let (agents, runs) = (agents.clone(), runs.clone());
                std::thread::spawn(move || {
                    agents.check_once(|| {
                        runs.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        std::thread::sleep(Duration::from_millis(200));
                        vec![status_of(Agent::Pi, None, Ok(vec![]))]
                    })
                })
            })
            .collect();
        for t in threads {
            assert_eq!(t.join().unwrap().0.len(), 1);
        }
        assert!(runs.load(std::sync::atomic::Ordering::SeqCst) <= 2);
        assert_eq!(agents.check_once(Vec::new).0.len(), 0);
    }

    #[test]
    fn a_model_list_error_makes_the_check_stale() {
        let ok = status_of(Agent::Pi, Some("pi".into()), Ok(vec![]));
        let failed = status_of(Agent::Pi, Some("pi".into()), Err("502".into()));
        let old = Instant::now().checked_sub(STALE * 2).unwrap();
        assert!(stale(&None));
        assert!(!stale(&Some((Instant::now(), vec![ok.clone()]))));
        assert!(stale(&Some((old, vec![ok.clone()]))));
        assert!(stale(&Some((Instant::now(), vec![ok, failed]))));
    }

    #[test]
    fn fresh_answers_replace_old_ones_in_the_usual_order() {
        let old = status_of(Agent::Pi, None, Ok(vec![]));
        let fresh = status_of(Agent::Pi, Some("pi".into()), Ok(vec![]));
        let claude = status_of(Agent::Claude, Some("claude".into()), Ok(vec![]));
        assert_eq!(
            merged(&[old.clone()], &[fresh.clone(), claude.clone()]),
            vec![claude.clone(), fresh]
        );
        assert_eq!(merged(&[old.clone()], &[claude.clone()]), vec![claude, old]);
    }

    #[test]
    fn a_cli_that_does_not_answer_times_out() {
        #[cfg(windows)]
        let (program, args) = ("ping", ["-n", "30", "127.0.0.1"]);
        #[cfg(unix)]
        let (program, args) = ("sleep", ["30", ""]);
        let args: Vec<&str> = args.into_iter().filter(|a| !a.is_empty()).collect();
        let started = Instant::now();
        let result = cli_output_within(Path::new(program), &args, Duration::from_secs(1));
        assert_eq!(result, Err("no answer in 1 s".to_string()));
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn missing_cli_message_names_the_agent() {
        assert_eq!(
            missing(Agent::Codex),
            "Codex CLI not found. Install it, or pick another agent in Settings."
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
