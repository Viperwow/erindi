use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::{Duration, Instant};

use erindi_audio_asr::asr::Asr;
use erindi_audio_asr::capture::{self, Capture};
use erindi_audio_asr::dsp::{To16k, rms};
use erindi_audio_asr::vad::{Endpoint, Endpointer};
use erindi_core::agent::{self, Agent, AgentRequest, EventParser, Target};
use erindi_core::claude::Session;
use erindi_core::commands::Command;
use erindi_core::controller::{Controller, Effect, Msg};
use erindi_core::llama::LlamaServer;
use erindi_core::prompt::{Dictionary, PromptTransformer};
use erindi_core::run::{RunEnd, RunSpec, run};
use erindi_core::state::{AppState, OpId};
use erindi_core::stream::RunEvent;
use erindi_core::transcript::Details;
use tauri::{AppHandle, Emitter};
use tokio_util::sync::CancellationToken;

use crate::agents::{Agents, missing};
use crate::history::{Entry, History, Prompt, Start};
use crate::overlay;
use crate::settings::Settings;

const RUN_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const DISMISS_AFTER: Duration = Duration::from_secs(8);
const LEVEL_INTERVAL: Duration = Duration::from_millis(33);
const REFINE_BUDGET: Duration = Duration::from_millis(1500);

pub type SharedSettings = Arc<RwLock<Settings>>;

/// Sends messages to the controller thread, which owns all runtime state.
#[derive(Clone)]
pub struct Runtime {
    tx: Sender<Msg>,
    asr: Arc<OnceLock<Asr>>,
    refiner: Refiner,
    last_session: Arc<Mutex<Option<LastRun>>>,
    history: Arc<Mutex<History>>,
    active: Arc<Mutex<Option<uuid::Uuid>>>,
}

/// The most recent agent run, which the overlay can reopen in a terminal.
#[derive(Clone)]
struct LastRun {
    op: OpId,
    id: uuid::Uuid,
    cwd: String,
    agent: Agent,
}

impl Runtime {
    pub fn start(
        app: AppHandle,
        settings: SharedSettings,
        history_path: &Path,
        agents: Agents,
    ) -> Self {
        let (tx, rx) = mpsc::channel();
        let asr = Arc::new(OnceLock::new());
        let last_session = Arc::new(Mutex::new(None));
        let history = Arc::new(Mutex::new(History::load(history_path)));
        let active = Arc::new(Mutex::new(None));
        let refiner = Refiner::default();

        let mut executor = Executor {
            app,
            settings: settings.clone(),
            tx: tx.clone(),
            asr: asr.clone(),
            capture: None,
            cancel: None,
            last_session: last_session.clone(),
            clickable_op: Arc::new(AtomicU64::new(0)),
            history: history.clone(),
            active: active.clone(),
            refiner: refiner.clone(),
            agents,
        };
        std::thread::spawn(move || {
            let mut controller = Controller::new(Box::new(SettingsDictionary(settings.clone())));
            let first = settings.read().unwrap().session_msg();
            for effect in controller.handle(first, Instant::now()) {
                executor.execute(effect);
            }
            for msg in rx {
                for effect in controller.handle(msg, Instant::now()) {
                    executor.execute(effect);
                }
            }
        });
        let runtime = Self {
            tx,
            asr,
            refiner,
            last_session,
            history,
            active,
        };
        runtime.load_speech();
        runtime
    }

    /// Loads the speech model in the background, or reports that it is not downloaded yet.
    pub fn load_speech(&self) {
        let (tx, asr) = (self.tx.clone(), self.asr.clone());
        std::thread::spawn(move || {
            let dir = models_dir();
            if !erindi_core::models::SPEECH.installed(&dir) {
                let _ = tx.send(Msg::ModelMissing);
                return;
            }
            match Asr::load(&dir) {
                Ok(model) => {
                    let _ = asr.set(model);
                    let _ = tx.send(Msg::ModelReady);
                }
                Err(e) => {
                    let _ = tx.send(Msg::ModelFailed(e));
                }
            }
        });
    }

    /// Sessions newest first, with the one the next utterance continues.
    pub fn sessions(&self) -> (Vec<Entry>, Option<uuid::Uuid>) {
        let entries = self.history.lock().unwrap().entries().to_vec();
        (entries, *self.active.lock().unwrap())
    }

    pub fn open_history_session(&self, id: uuid::Uuid) -> Result<(), String> {
        let (cwd, agent) = self.session(id)?;
        open_terminal(&self.history, &cwd, id, agent)
    }

    pub fn continue_session(&self, id: uuid::Uuid) -> Result<(), String> {
        let (cwd, agent) = self.session(id)?;
        if self
            .history
            .lock()
            .unwrap()
            .get(id)
            .and_then(Entry::native)
            .is_none()
        {
            return Err("This session can't be resumed".into());
        }
        self.send(Msg::SetActive { id, cwd, agent });
        Ok(())
    }

    pub fn delete_session(&self, id: uuid::Uuid) -> Result<(), String> {
        self.history.lock().unwrap().remove(id)?;
        self.send(Msg::Forget { id });
        Ok(())
    }

    fn session(&self, id: uuid::Uuid) -> Result<(String, Agent), String> {
        let history = self.history.lock().unwrap();
        let entry = history.get(id).ok_or("Session not found")?;
        Ok((entry.cwd.clone(), entry.agent))
    }

    pub fn set_cleanup(&self, on: bool) {
        self.refiner.set_enabled(on);
    }

    pub fn send(&self, msg: Msg) {
        let _ = self.tx.send(msg);
    }

    pub fn open_session(&self) -> Result<(), String> {
        let session = self.last_session.lock().unwrap().clone();
        let session = session.ok_or("No session yet")?;
        open_terminal(&self.history, &session.cwd, session.id, session.agent)?;
        self.send(Msg::Dismiss { op: session.op });
        Ok(())
    }
}

/// Reopens session `id` in Windows Terminal with the agent that started it.
fn open_terminal(
    history: &Mutex<History>,
    cwd: &str,
    id: uuid::Uuid,
    agent: Agent,
) -> Result<(), String> {
    let entry = history.lock().unwrap().get(id).cloned();
    let agent = entry.as_ref().map_or(agent, |e| e.agent);
    // A Claude session opened empty in a terminal never reached history, and uses Erindi's ID.
    let native = match (entry.and_then(|e| e.native_id), agent) {
        (Some(native), _) => native,
        (None, Agent::Claude) => id.to_string(),
        (None, _) => return Err("This session can't be resumed".into()),
    };
    let program = erindi_core::cli::locate(agent).ok_or_else(|| missing(agent))?;
    let args = agent::resume_in_terminal(&program.display().to_string(), cwd, agent, &native)
        .map_err(|_| format!("Cannot open a terminal in {cwd}"))?;
    std::process::Command::new("wt.exe")
        .args(args)
        .spawn()
        .map_err(|e| format!("Cannot start Windows Terminal: {e}"))?;
    Ok(())
}

pub fn models_dir() -> PathBuf {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(PathBuf::from));
    let data_dir = std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("Erindi"));
    pick_models_dir(
        std::env::var_os("ERINDI_MODELS"),
        exe_dir,
        data_dir,
        cfg!(debug_assertions),
    )
}

/// `ERINDI_MODELS` wins, then `models/` next to the executable. Development builds fall back to
/// `models/` in the repository; release builds to a per-user folder, since the exe folder may be read-only.
fn pick_models_dir(
    env: Option<std::ffi::OsString>,
    exe_dir: Option<PathBuf>,
    data_dir: Option<PathBuf>,
    debug: bool,
) -> PathBuf {
    if let Some(env) = env {
        return env.into();
    }
    if let Some(dir) = exe_dir.map(|d| d.join("models")).filter(|d| d.is_dir()) {
        return dir;
    }
    match data_dir {
        Some(data) if !debug => data.join("models"),
        _ => PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../models")),
    }
}

/// The command model server. It starts when model commands are turned on and stays loaded.
#[derive(Clone, Default)]
struct Refiner {
    server: Arc<Mutex<Option<LlamaServer>>>,
    enabled: Arc<AtomicBool>,
}

impl Refiner {
    /// Works off the calling thread, since starting holds the lock through the model load.
    /// The thread reads the latest flag, so a quick on-then-off never leaves a server behind.
    fn set_enabled(&self, on: bool) {
        self.enabled.store(on, Ordering::SeqCst);
        let this = self.clone();
        std::thread::spawn(move || {
            let mut server = this.server.lock().unwrap();
            if !this.enabled.load(Ordering::SeqCst) {
                server.take();
            } else if server.is_none() {
                *server = start_llama();
            }
        });
    }

    /// Waits for a starting server, so an utterance right after launch is still checked.
    fn classify(&self, text: &str) -> Option<(Option<Command>, String)> {
        let mut server = self.server.lock().unwrap();
        if server.is_none() {
            *server = start_llama();
        }
        let started = Instant::now();
        let result = server.as_ref()?.classify(text);
        let took = started.elapsed();
        if took > REFINE_BUDGET {
            eprintln!(
                "command check took {took:?} for {} chars",
                text.chars().count()
            );
        }
        result.unwrap_or_else(|e| {
            eprintln!("{e}");
            server.take();
            None
        })
    }
}

fn start_llama() -> Option<LlamaServer> {
    let model = models_dir().join(erindi_core::models::CLEANUP_GGUF);
    let started = Instant::now();
    let server = LlamaServer::start(&llama_server_exe(), &model)
        .map_err(|e| eprintln!("{e}"))
        .ok()?;
    let _ = server.classify(erindi_core::classify::WARM_UP);
    eprintln!("llama-server ready and warm in {:?}", started.elapsed());
    Some(server)
}

pub fn llama_server_exe() -> PathBuf {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(PathBuf::from));
    pick_llama_server(exe_dir, &models_dir())
}

/// The release zip ships `llama/` next to the exe; development builds use `models/llama/`.
fn pick_llama_server(exe_dir: Option<PathBuf>, models: &Path) -> PathBuf {
    exe_dir
        .map(|d| d.join("llama/llama-server.exe"))
        .filter(|p| p.is_file())
        .unwrap_or_else(|| models.join("llama/llama-server.exe"))
}

/// Applies the dictionary as currently saved in settings.
struct SettingsDictionary(SharedSettings);

impl PromptTransformer for SettingsDictionary {
    fn transform(&self, text: &str) -> String {
        let entries = self.0.read().unwrap().dictionary.clone();
        Dictionary::new(entries).transform(text)
    }
}

struct Executor {
    app: AppHandle,
    settings: SharedSettings,
    tx: Sender<Msg>,
    asr: Arc<OnceLock<Asr>>,
    capture: Option<Capture>,
    cancel: Option<CancellationToken>,
    last_session: Arc<Mutex<Option<LastRun>>>,
    clickable_op: Arc<AtomicU64>,
    history: Arc<Mutex<History>>,
    active: Arc<Mutex<Option<uuid::Uuid>>>,
    refiner: Refiner,
    agents: Agents,
}

impl Executor {
    fn execute(&mut self, effect: Effect) {
        match effect {
            Effect::StartCapture { op } => self.start_capture(op, true),
            Effect::GestureTimer { seq } => {
                let tx = self.tx.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(erindi_core::controller::DOUBLE);
                    let _ = tx.send(Msg::GestureTimeout { seq });
                });
            }
            Effect::OpenTerminal { id, cwd, agent } => {
                if let Err(e) = open_terminal(&self.history, &cwd, id, agent) {
                    eprintln!("{e}");
                }
            }
            Effect::RunInTerminal {
                session,
                cwd,
                prompt,
                agent,
            } => self.run_in_terminal(session, cwd, prompt, agent),
            Effect::StopCapture => self.capture = None,
            Effect::LiveDecode { op, samples } => {
                self.decode(op, samples, |op, text| Msg::Live { op, text })
            }
            Effect::Transcribe { op, samples } => {
                self.decode(op, samples, |op, text| Msg::Transcribed { op, text })
            }
            Effect::StartRun {
                op,
                prompt,
                session,
                cwd,
                agent,
            } => self.start_run(op, prompt, session, cwd, agent),
            Effect::Classify { op, text } => {
                let (refiner, tx) = (self.refiner.clone(), self.tx.clone());
                std::thread::spawn(move || {
                    let answer = refiner.classify(&text);
                    let _ = tx.send(Msg::Classified { op, answer });
                });
            }
            Effect::ActiveChanged(id) => {
                *self.active.lock().unwrap() = id;
                let _ = self.app.emit_to("settings", "sessions-changed", ());
            }
            Effect::OpenSettings => crate::show_settings(&self.app),
            Effect::CancelRun => {
                if let Some(token) = self.cancel.take() {
                    token.cancel();
                }
            }
            Effect::Show(view) => {
                let _ = self.app.emit_to("overlay", "view", &view);
                match view.state {
                    AppState::Idle | AppState::LoadingModel | AppState::NoModel => {
                        overlay::hide(&self.app)
                    }
                    _ => overlay::show(&self.app),
                }
                let finished = matches!(view.state, AppState::Succeeded | AppState::Failed);
                let clickable = finished && view.session_id.is_some();
                self.clickable_op
                    .store(if clickable { view.op } else { 0 }, Ordering::SeqCst);
                if clickable {
                    overlay::track_bubble_hover(&self.app, self.clickable_op.clone(), view.op);
                }
                if finished {
                    let tx = self.tx.clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(DISMISS_AFTER);
                        let _ = tx.send(Msg::Dismiss { op: view.op });
                    });
                }
            }
        }
    }

    fn start_capture(&mut self, op: OpId, endpointing: bool) {
        let settings = self.settings.read().unwrap().clone();
        let microphone = (!settings.microphone.is_empty()).then_some(settings.microphone.as_str());

        let mut endpointer = None;
        if endpointing {
            let silence = Duration::from_secs_f32(settings.silence_secs.max(0.3));
            match Endpointer::new(&models_dir(), silence) {
                Ok(e) => endpointer = Some(e),
                Err(error) => {
                    let _ = self.tx.send(Msg::Failed { op, error });
                    return;
                }
            }
        }

        let (raw_tx, raw_rx) = mpsc::sync_channel::<Vec<f32>>(64);
        // A full queue drops audio instead of blocking the audio callback.
        let started = capture::start(microphone, move |chunk| {
            let _ = raw_tx.try_send(chunk);
        });
        let (capture, rate) = match started {
            Ok(started) => started,
            Err(error) => {
                let _ = self.tx.send(Msg::Failed { op, error });
                return;
            }
        };
        self.capture = Some(capture);

        let (tx, app) = (self.tx.clone(), self.app.clone());
        std::thread::spawn(move || {
            let mut resampler = To16k::new(rate);
            let mut last_level = Instant::now();
            let mut ended = false;
            for chunk in raw_rx {
                if last_level.elapsed() >= LEVEL_INTERVAL {
                    last_level = Instant::now();
                    let _ = app.emit_to("overlay", "level", rms(&chunk));
                }
                let samples = resampler.push(&chunk);
                let endpoint = match (&mut endpointer, ended) {
                    (Some(e), false) => e.push(&samples),
                    _ => Endpoint::Continue,
                };
                let _ = tx.send(Msg::Audio { op, samples });
                match endpoint {
                    Endpoint::SpeechEnded => {
                        ended = true;
                        let _ = tx.send(Msg::SpeechEnded { op });
                    }
                    Endpoint::NoSpeech => {
                        ended = true;
                        let _ = tx.send(Msg::NoSpeech { op });
                    }
                    Endpoint::Continue => {}
                }
            }
        });
    }

    fn decode(&self, op: OpId, samples: Vec<f32>, done: fn(OpId, String) -> Msg) {
        let (asr, tx) = (self.asr.clone(), self.tx.clone());
        std::thread::spawn(move || {
            let msg = match asr.get() {
                Some(asr) => done(op, asr.transcribe(&samples)),
                None => Msg::Failed {
                    op,
                    error: "Speech model is not loaded".into(),
                },
            };
            let _ = tx.send(msg);
        });
    }

    /// The agent's CLI and request for `session`. A new session takes the agent's settings; a
    /// resumed one passes neither model nor permission and uses the agent's own session ID.
    fn request(
        &self,
        session: Session,
        agent: Agent,
    ) -> Result<(PathBuf, AgentRequest, Start), String> {
        let program = self
            .agents
            .locate(agent, &self.app)
            .ok_or_else(|| missing(agent))?;
        let (target, start) = match session {
            Session::New(id) => {
                let settings = self.settings.read().unwrap().agent_settings(agent);
                let start = Start {
                    agent,
                    native_id: (agent == Agent::Claude).then(|| id.to_string()),
                    model: settings.model_id().map(String::from),
                    permission: settings.permission_flag().map(String::from),
                };
                (Target::New(id), start)
            }
            Session::Resume(id) => {
                let entry = self.history.lock().unwrap().get(id).cloned();
                // A Claude session opened empty in a terminal never reached history.
                let native = entry
                    .as_ref()
                    .and_then(|e| e.native_id.clone())
                    .or_else(|| (agent == Agent::Claude).then(|| id.to_string()))
                    .ok_or("This session can't be resumed")?;
                let started = Start {
                    agent,
                    native_id: Some(native.clone()),
                    model: entry.as_ref().and_then(|e| e.started_model.clone()),
                    permission: entry.as_ref().and_then(|e| e.started_permission.clone()),
                };
                let live = session_details(agent, &native);
                let (model, permission) = continue_flags(&started, live);
                let start = Start {
                    model,
                    permission,
                    ..started
                };
                (Target::Resume(native), start)
            }
        };
        let request = AgentRequest {
            agent,
            model: start.model.clone(),
            permission: start.permission.clone(),
            target,
        };
        Ok((program, request, start))
    }

    fn run_in_terminal(&mut self, session: Session, cwd: String, prompt: String, agent: Agent) {
        let started = self
            .request(session, agent)
            .and_then(|(program, request, start)| {
                let args =
                    agent::terminal_args(&program.display().to_string(), &cwd, &request, &prompt)
                        .map_err(|e| format!("Cannot open a terminal in {cwd}: {e:?}"))?;
                std::process::Command::new("wt.exe")
                    .args(args)
                    .spawn()
                    .map_err(|e| format!("Cannot start Windows Terminal: {e}"))?;
                Ok(start)
            });
        let start = match started {
            Ok(start) => start,
            Err(e) => {
                eprintln!("{e}");
                // The controller already made this session active; a session that never started must not be resumed.
                if let Session::New(id) = session {
                    let _ = self.tx.send(Msg::Forget { id });
                }
                return;
            }
        };
        // An interactive Codex picks its own session ID, which Erindi never sees.
        if let (Agent::Codex, Session::New(id)) = (agent, session) {
            let _ = self.tx.send(Msg::Forget { id });
        }
        if !prompt.is_empty() {
            self.remember_prompt(session, &cwd, prompt, &start);
        }
    }

    fn remember_prompt(&mut self, session: Session, cwd: &str, prompt: String, start: &Start) {
        let id = match session {
            Session::New(id) | Session::Resume(id) => id,
        };
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as u64);
        let entry = Prompt::Plain(prompt);
        if let Err(e) = self
            .history
            .lock()
            .unwrap()
            .record(id, cwd, entry, now_ms, start)
        {
            eprintln!("cannot save session history: {e}");
        }
        let _ = self.app.emit_to("settings", "sessions-changed", ());
    }

    fn start_run(&mut self, op: OpId, prompt: String, session: Session, cwd: String, agent: Agent) {
        let fail = |tx: &Sender<Msg>, stderr: String| {
            let _ = tx.send(Msg::RunExited {
                op,
                end: RunEnd::Exited { success: false },
                stderr,
            });
        };
        let (program, request, start) = match self.request(session, agent) {
            Ok(r) => r,
            Err(e) => return fail(&self.tx, e),
        };
        let args = match agent::headless_args(&request, &cwd) {
            Ok(args) => args,
            Err(e) => {
                return fail(
                    &self.tx,
                    format!("Invalid {} settings: {e:?}", agent.label()),
                );
            }
        };
        let spec = RunSpec {
            program,
            args,
            cwd: cwd.clone().into(),
            env: agent::env(
                agent,
                run_env(std::env::vars(), erindi_core::cli::current_path()),
            ),
            stdin: prompt.clone(),
            timeout: RUN_TIMEOUT,
        };
        let id = match session {
            Session::New(id) | Session::Resume(id) => id,
        };
        *self.last_session.lock().unwrap() = Some(LastRun {
            op,
            id,
            cwd: cwd.clone(),
            agent,
        });
        self.remember_prompt(session, &cwd, prompt.clone(), &start);
        if agent == Agent::Codex && codex_limited(&cwd) {
            let _ = self.tx.send(Msg::Run {
                op,
                event: RunEvent::Limited,
            });
        }
        let token = CancellationToken::new();
        self.cancel = Some(token.clone());
        let (tx, history, app) = (self.tx.clone(), self.history.clone(), self.app.clone());
        let new = matches!(session, Session::New(_));
        tauri::async_runtime::spawn(async move {
            let mut parser = EventParser::new(agent);
            let mut native_seen = false;
            let outcome = run(spec, token, |line| {
                for event in parser.feed(line) {
                    if let RunEvent::SessionStarted { native_id } = &event {
                        native_seen = true;
                        if let Err(e) = history.lock().unwrap().set_native(id, native_id) {
                            eprintln!("cannot save session history: {e}");
                        }
                        let _ = app.emit_to("settings", "sessions-changed", ());
                    }
                    let _ = tx.send(Msg::Run { op, event });
                }
            })
            .await;
            let msg = match outcome {
                Ok(outcome) => Msg::RunExited {
                    op,
                    end: outcome.end,
                    stderr: outcome.stderr_tail,
                },
                Err(e) => Msg::RunExited {
                    op,
                    end: RunEnd::Exited { success: false },
                    stderr: format!("Cannot start {}: {e}", agent.cli()),
                },
            };
            let _ = tx.send(msg);
            if forget_after_run(agent, new, native_seen) {
                let _ = tx.send(Msg::Forget { id });
            }
        });
    }
}

/// The model and permission a continued run passes. `claude -p --resume` falls back to the default
/// permission mode unless it is passed again, so Claude gets the session's current values; Codex
/// keeps its own and gets none.
fn continue_flags(started: &Start, live: Option<Details>) -> (Option<String>, Option<String>) {
    if started.agent != Agent::Claude {
        return (None, None);
    }
    let live = live.unwrap_or(Details {
        model: None,
        permission: None,
    });
    let model = live.model.or_else(|| started.model.clone());
    let permission = live
        .permission
        .or_else(|| started.permission.clone())
        .filter(|p| p != "default");
    (model, permission)
}

/// Codex skips this folder's hooks and MCP servers until it trusts the folder.
pub fn codex_limited(cwd: &str) -> bool {
    let Some(home) = std::env::var_os("USERPROFILE").map(PathBuf::from) else {
        return false;
    };
    let config =
        std::fs::read_to_string(erindi_core::codex::config_path(&home)).unwrap_or_default();
    erindi_core::codex::limited(std::path::Path::new(cwd), &config)
}

/// Opens Codex in `cwd`, where Codex asks on its own whether to trust the folder and its hooks.
pub fn trust_in_codex(cwd: &str) -> Result<(), String> {
    let program = erindi_core::cli::locate(Agent::Codex).ok_or_else(|| missing(Agent::Codex))?;
    let request = AgentRequest {
        agent: Agent::Codex,
        model: None,
        permission: None,
        target: Target::New(uuid::Uuid::nil()),
    };
    let args = agent::terminal_args(&program.display().to_string(), cwd, &request, "")
        .map_err(|_| format!("Cannot open a terminal in {cwd}"))?;
    std::process::Command::new("wt.exe")
        .args(args)
        .spawn()
        .map_err(|e| format!("Cannot start Windows Terminal: {e}"))?;
    Ok(())
}

/// What the agent's own log says about session `native_id` now.
fn session_details(agent: Agent, native_id: &str) -> Option<Details> {
    let home = std::env::var_os("USERPROFILE").map(PathBuf::from)?;
    let logs = erindi_core::transcript::find_logs(agent, &home);
    erindi_core::transcript::read(agent, logs.get(native_id)?)
}

/// A new Codex session that never reported its ID cannot be continued, so it must not stay active.
fn forget_after_run(agent: Agent, new: bool, native_seen: bool) -> bool {
    agent == Agent::Codex && new && !native_seen
}

/// The launcher's environment with PATH as it is now, so tools installed after launch are found.
fn run_env(
    vars: impl IntoIterator<Item = (String, String)>,
    path: String,
) -> Vec<(String, String)> {
    vars.into_iter()
        .filter(|(k, _)| !k.eq_ignore_ascii_case("PATH"))
        .chain([("PATH".to_string(), path)])
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history;

    fn details(model: Option<&str>, permission: Option<&str>) -> Details {
        Details {
            model: model.map(String::from),
            permission: permission.map(String::from),
        }
    }

    #[test]
    fn continued_claude_keeps_the_session_permission_and_model() {
        let entry = history::Start {
            agent: Agent::Claude,
            native_id: None,
            model: Some("opus".into()),
            permission: Some("plan".into()),
        };
        let live = Some(details(Some("claude-opus-5-5"), Some("acceptEdits")));
        assert_eq!(
            continue_flags(&entry, live),
            (Some("claude-opus-5-5".into()), Some("acceptEdits".into()))
        );
        assert_eq!(
            continue_flags(&entry, None),
            (Some("opus".into()), Some("plan".into()))
        );
        let live = Some(details(None, Some("default")));
        assert_eq!(continue_flags(&entry, live), (Some("opus".into()), None));
    }

    #[test]
    fn continued_codex_passes_no_flags() {
        let entry = history::Start {
            agent: Agent::Codex,
            native_id: None,
            model: Some("gpt-5.5".into()),
            permission: Some("read-only".into()),
        };
        let live = Some(details(Some("gpt-5.5"), Some("workspace-write")));
        assert_eq!(continue_flags(&entry, live), (None, None));
    }

    #[test]
    fn codex_session_without_native_id_is_forgotten() {
        assert!(forget_after_run(Agent::Codex, true, false));
        assert!(!forget_after_run(Agent::Codex, true, true));
        assert!(!forget_after_run(Agent::Claude, true, false));
        assert!(!forget_after_run(Agent::Codex, false, false));
    }

    #[test]
    fn run_env_replaces_path_whatever_its_case() {
        let vars = [("Path", "old"), ("TEMP", "t")].map(|(k, v)| (k.to_string(), v.to_string()));
        assert_eq!(
            run_env(vars, "new".into()),
            [
                ("TEMP".to_string(), "t".to_string()),
                ("PATH".to_string(), "new".to_string())
            ]
        );
    }

    #[test]
    fn env_var_wins() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("models")).unwrap();
        assert_eq!(
            pick_models_dir(Some("X:\\m".into()), Some(dir.path().into()), None, false),
            PathBuf::from("X:\\m")
        );
    }

    #[test]
    fn models_next_to_exe_beat_repository() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("models")).unwrap();
        assert_eq!(
            pick_models_dir(None, Some(dir.path().into()), None, false),
            dir.path().join("models")
        );
    }

    #[test]
    fn debug_falls_back_to_repository_models() {
        let dir = tempfile::tempdir().unwrap();
        let picked = pick_models_dir(None, Some(dir.path().into()), Some(r"D:\data".into()), true);
        assert!(picked.ends_with("models") && picked.starts_with(env!("CARGO_MANIFEST_DIR")));
    }

    #[test]
    fn release_uses_data_dir() {
        let dir = tempfile::tempdir().unwrap();
        let picked = pick_models_dir(
            None,
            Some(dir.path().into()),
            Some(r"D:\data".into()),
            false,
        );
        assert_eq!(picked, PathBuf::from(r"D:\data\models"));
    }

    #[test]
    fn bundled_llama_server_beats_models_folder() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("llama")).unwrap();
        std::fs::write(dir.path().join("llama/llama-server.exe"), "").unwrap();
        assert_eq!(
            pick_llama_server(Some(dir.path().into()), Path::new("M:/models")),
            dir.path().join("llama/llama-server.exe")
        );
        assert_eq!(
            pick_llama_server(None, Path::new("M:/models")),
            Path::new("M:/models").join("llama/llama-server.exe")
        );
    }
}
