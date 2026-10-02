use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::{Duration, Instant};

use erindi_audio_asr::asr::Asr;
use erindi_audio_asr::capture::{self, Capture};
use erindi_audio_asr::dsp::To16k;
use erindi_audio_asr::vad::{Endpoint, Endpointer};
use erindi_core::agent::{self, Agent, AgentRequest, EventParser, Target};
use erindi_core::claude::Session;
use erindi_core::commands::Command;
use erindi_core::controller::{Controller, Effect, Msg};
use erindi_core::llama::LlamaServer;
use erindi_core::prompt::{Dictionary, PromptTransformer};
use erindi_core::run::{RunEnd, RunSpec, run};
use erindi_core::state::OpId;
use erindi_core::stream::RunEvent;
use erindi_core::transcript::Details;
use tauri::{AppHandle, Emitter};
use tokio_util::sync::CancellationToken;

use crate::agents::{Agents, missing};
use crate::history::{Entry, History, Prompt, Start};
use crate::overlay;
use crate::settings::Settings;

const RUN_TIMEOUT: Duration = Duration::from_secs(30 * 60);
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
        let restore = restore_active(&history.lock().unwrap(), now_ms());
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
            hover_op: Arc::new(AtomicU64::new(0)),
            armed: 0,
            history: history.clone(),
            active: active.clone(),
            refiner: refiner.clone(),
            agents,
        };
        std::thread::spawn(move || {
            let mut controller = Controller::new(Box::new(SettingsDictionary(settings.clone())));
            let first = settings.read().unwrap().session_msg();
            for msg in std::iter::once(first).chain(restore) {
                for effect in controller.handle(msg, Instant::now()) {
                    executor.execute(effect);
                }
            }
            for msg in rx {
                crate::trace::msg(&msg);
                for effect in controller.handle(msg, Instant::now()) {
                    crate::trace::effect(&effect);
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
        self.send(Msg::SetActive {
            id,
            cwd,
            agent,
            idle: Duration::ZERO,
        });
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
        self.send(Msg::Dismiss { rest: None });
        Ok(())
    }
}

/// Reopens session `id` in a terminal with the agent that started it.
fn open_terminal(
    history: &Mutex<History>,
    cwd: &str,
    id: uuid::Uuid,
    agent: Agent,
) -> Result<(), String> {
    let entry = history.lock().unwrap().get(id).cloned();
    let agent = entry.as_ref().map_or(agent, |e| e.agent);
    if !agent.is_cli() {
        return Err("This agent has no terminal".into());
    }
    // A Claude or Pi session opened empty in a terminal never reached history, and uses Erindi's ID.
    let native = match (entry.and_then(|e| e.native_id), agent) {
        (Some(native), _) => native,
        (None, agent) if agent.uses_erindi_id() => id.to_string(),
        (None, _) => return Err("This session can't be resumed".into()),
    };
    let program = erindi_core::cli::locate(agent).ok_or_else(|| missing(agent))?;
    let args = agent::resume_in_terminal(&program.display().to_string(), cwd, agent, &native)
        .map_err(|_| format!("Cannot open a terminal in {cwd}"))?;
    crate::terminal::open(&args)
}

pub fn home() -> Option<PathBuf> {
    std::env::home_dir()
}

pub fn models_dir() -> PathBuf {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(PathBuf::from));
    #[cfg(windows)]
    let data_dir = std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("Erindi"));
    #[cfg(not(windows))]
    let data_dir = home().map(|h| h.join("Library/Application Support/Erindi"));
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
    // Writing inside the macOS app bundle breaks its signature, so only Windows looks there.
    if let Some(dir) = exe_dir
        .filter(|_| cfg!(windows))
        .map(|d| d.join("models"))
        .filter(|d| d.is_dir())
    {
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
    server: Arc<Mutex<Option<Guarded>>>,
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

/// A server the process guard knows about for as long as it lives.
struct Guarded(LlamaServer);

impl std::ops::Deref for Guarded {
    type Target = LlamaServer;
    fn deref(&self) -> &LlamaServer {
        &self.0
    }
}

impl Drop for Guarded {
    fn drop(&mut self) {
        crate::guard::untrack(self.0.pid());
    }
}

fn start_llama() -> Option<Guarded> {
    let model = models_dir().join(erindi_core::models::CLEANUP_GGUF);
    let started = Instant::now();
    let spawned = std::cell::Cell::new(None);
    let server = LlamaServer::start(&llama_server_exe(), &model, |pid| {
        crate::guard::track(pid);
        spawned.set(Some(pid));
    })
    .map_err(|e| {
        if let Some(pid) = spawned.get() {
            crate::guard::untrack(pid);
        }
        eprintln!("{e}");
    })
    .ok()?;
    let server = Guarded(server);
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
    #[cfg(windows)]
    let (bundled, name) = ("llama/llama-server.exe", "llama/llama-server.exe");
    #[cfg(not(windows))]
    let (bundled, name) = ("../Resources/llama/llama-server", "llama/llama-server");
    exe_dir
        .map(|d| d.join(bundled))
        .filter(|p| p.is_file())
        .unwrap_or_else(|| models.join(name))
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
    hover_op: Arc<AtomicU64>,
    /// The idle stretch whose countdown is running.
    armed: u64,
    history: Arc<Mutex<History>>,
    active: Arc<Mutex<Option<uuid::Uuid>>>,
    refiner: Refiner,
    agents: Agents,
}

impl Executor {
    fn execute(&mut self, effect: Effect) {
        match effect {
            Effect::StartCapture { op } => self.start_capture(op, true),
            Effect::GestureTimer { seq, after } => {
                let tx = self.tx.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(after);
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
                if let Err(e) = self.history.lock().unwrap().set_active(id) {
                    eprintln!("cannot save session history: {e}");
                }
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
                if view.visible {
                    overlay::show(&self.app);
                } else {
                    overlay::hide(&self.app);
                }
                // Tooltips need the mouse whenever the bubble is up; one tracker per series.
                let key = if view.visible { view.series + 1 } else { 0 };
                if self.hover_op.swap(key, Ordering::SeqCst) != key && key != 0 {
                    overlay::track_bubble_hover(&self.app, self.hover_op.clone(), key);
                }
                // One countdown per idle stretch; the controller ignores it once the stretch ends.
                if let Some(ms) = view.hide_after_ms
                    && view.rest != self.armed
                {
                    self.armed = view.rest;
                    let (tx, rest) = (self.tx.clone(), view.rest);
                    std::thread::spawn(move || {
                        std::thread::sleep(Duration::from_millis(ms));
                        let _ = tx.send(Msg::Dismiss { rest: Some(rest) });
                    });
                }
            }
        }
    }

    fn start_capture(&mut self, op: OpId, endpointing: bool) {
        let opening = Instant::now();
        let settings = self.settings.read().unwrap().clone();
        let microphone = (!settings.microphone.is_empty()).then_some(settings.microphone.as_str());

        let mut endpointer = None;
        if endpointing {
            let silence = Duration::from_secs_f32(settings.silence_secs.max(0.3));
            match Endpointer::new(&models_dir(), silence) {
                Ok(e) => endpointer = Some(e),
                Err(error) => {
                    let _ = self.tx.send(Msg::MicFailed { op, error });
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
                let _ = self.tx.send(Msg::MicFailed { op, error });
                return;
            }
        };
        self.capture = Some(capture);
        crate::trace::line(format!(
            "capture {op} open after {} ms",
            opening.elapsed().as_millis()
        ));

        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let mut resampler = To16k::new(rate);
            let mut speaking = false;
            for chunk in raw_rx {
                let samples = resampler.push(&chunk);
                let endpoint = match &mut endpointer {
                    Some(e) => e.push(&samples),
                    None => Endpoint::Continue,
                };
                let _ = tx.send(Msg::Audio { op, samples });
                if let Some(e) = &endpointer
                    && e.in_speech() != speaking
                {
                    speaking = !speaking;
                    let _ = tx.send(Msg::Speaking { op, speaking });
                }
                if endpoint == Endpoint::SpeechEnded {
                    let _ = tx.send(Msg::SpeechEnded { op });
                    if let Some(e) = &mut endpointer {
                        e.reset();
                    }
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
                    native_id: agent.uses_erindi_id().then(|| id.to_string()),
                    model: settings.model_id().map(String::from),
                    permission: settings.permission_flag().map(String::from),
                };
                (Target::New(id), start)
            }
            Session::Resume(id) => {
                let entry = self.history.lock().unwrap().get(id).cloned();
                // A Claude or Pi session opened empty in a terminal never reached history.
                let native = entry
                    .as_ref()
                    .and_then(|e| e.native_id.clone())
                    .or_else(|| agent.uses_erindi_id().then(|| id.to_string()))
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
                crate::terminal::open(&args)?;
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

    /// Sends the phrase to the local model; the reply streams into the bubble and joins the history.
    fn start_api_run(&mut self, op: OpId, prompt: String, session: Session, cwd: String) {
        let config = match self
            .settings
            .read()
            .unwrap()
            .api_config(crate::api_key::get())
        {
            Ok(c) => c,
            Err(e) => {
                let _ = self.tx.send(Msg::RunExited {
                    op,
                    end: RunEnd::Exited { success: false },
                    stderr: e,
                });
                return;
            }
        };
        let id = match session {
            Session::New(id) | Session::Resume(id) => id,
        };
        let start = Start {
            agent: Agent::Api,
            native_id: Some(id.to_string()),
            model: Some(config.model.clone()),
            permission: None,
        };
        *self.last_session.lock().unwrap() = Some(LastRun {
            id,
            cwd: cwd.clone(),
            agent: Agent::Api,
        });
        self.remember_prompt(session, &cwd, prompt, &start);
        let token = CancellationToken::new();
        self.cancel = Some(token.clone());
        let (tx, history, app) = (self.tx.clone(), self.history.clone(), self.app.clone());
        tauri::async_runtime::spawn(async move {
            let cancel = Arc::new(AtomicBool::new(false));
            let (flag, events, sessions) = (cancel.clone(), tx.clone(), app.clone());
            let blocking = tauri::async_runtime::spawn_blocking(move || {
                run_api(&config, &history, id, &flag, |event| {
                    // A cancelled run's late chunks must not reach the next phrase's bubble.
                    if flag.load(Ordering::SeqCst) {
                        return;
                    }
                    if let RunEvent::Reply { text } = &event {
                        let reply = serde_json::json!({ "id": id, "text": text });
                        let _ = sessions.emit_to("settings", "session-reply", reply);
                    }
                    let _ = events.send(Msg::Run { op, event });
                })
            });
            let end = tokio::select! {
                end = blocking => end.unwrap_or(RunEnd::Exited { success: false }),
                _ = token.cancelled() => {
                    cancel.store(true, Ordering::SeqCst);
                    RunEnd::Cancelled
                }
            };
            let _ = app.emit_to("settings", "sessions-changed", ());
            let _ = tx.send(Msg::RunExited {
                op,
                end,
                stderr: String::new(),
            });
        });
    }

    fn remember_prompt(&mut self, session: Session, cwd: &str, prompt: String, start: &Start) {
        let id = match session {
            Session::New(id) | Session::Resume(id) => id,
        };
        let now_ms = now_ms();
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
        if agent == Agent::Api {
            return self.start_api_run(op, prompt, session, cwd);
        }
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
            env: agent_env(
                erindi_core::shell_env::vars(),
                erindi_core::cli::current_path(),
            ),
            stdin: prompt.clone(),
            timeout: RUN_TIMEOUT,
        };
        let id = match session {
            Session::New(id) | Session::Resume(id) => id,
        };
        *self.last_session.lock().unwrap() = Some(LastRun {
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
            let mut pgid = None;
            let outcome = run(
                spec,
                token,
                |line| {
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
                },
                |pid| {
                    pgid = Some(pid);
                    crate::guard::track(pid);
                },
            )
            .await;
            if let Some(pid) = pgid {
                crate::guard::untrack(pid);
            }
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

/// Sends session `id`'s last phrase with the conversation before it, and stores the reply.
fn run_api(
    config: &erindi_core::api::ApiConfig,
    history: &Mutex<History>,
    id: uuid::Uuid,
    cancel: &AtomicBool,
    mut on_event: impl FnMut(RunEvent),
) -> RunEnd {
    let mut turns = history.lock().unwrap().turns(id);
    let Some(last) = turns.pop() else {
        return RunEnd::Exited { success: false };
    };
    let messages = erindi_core::api::messages(&turns, &last.prompt);
    let mut reply = None;
    let end = erindi_core::api::stream_chat(config, &messages, cancel, |event| {
        if let RunEvent::Result { ok: true, text } = &event {
            reply = Some(text.clone());
        }
        on_event(event);
    });
    // A run cancelled as it finished shows "Cancelled", so its reply must not join the conversation.
    if let Some(reply) = reply.filter(|_| !cancel.load(Ordering::SeqCst))
        && let Err(e) = history.lock().unwrap().set_reply(id, reply)
    {
        eprintln!("cannot save session history: {e}");
    }
    end
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
    let Some(path) = erindi_core::codex::config_path(home()) else {
        return false;
    };
    let config = std::fs::read_to_string(path).unwrap_or_default();
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
    crate::terminal::open(&args)
}

/// What the agent's own log says about session `native_id` now.
fn session_details(agent: Agent, native_id: &str) -> Option<Details> {
    let logs = erindi_core::transcript::find_logs(agent, &home()?);
    erindi_core::transcript::read(agent, logs.get(native_id)?)
}

/// The session that was active when Erindi last ran, if it can still be continued.
fn restore_active(history: &History, now_ms: u64) -> Option<Msg> {
    let e = history.active().filter(|e| e.native().is_some())?;
    Some(Msg::SetActive {
        id: e.id,
        cwd: e.cwd.clone(),
        agent: e.agent,
        idle: Duration::from_millis(now_ms.saturating_sub(e.updated_ms)),
    })
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// A new Codex session that never reported its ID cannot be continued, so it must not stay active.
fn forget_after_run(agent: Agent, new: bool, native_seen: bool) -> bool {
    agent == Agent::Codex && new && !native_seen
}

/// The launcher's environment with PATH as it is now, so tools installed after launch are found.
/// An agent gets the full environment, as when started by hand; only PATH is refreshed.
fn agent_env(
    vars: impl IntoIterator<Item = (String, String)>,
    path: String,
) -> Vec<(String, String)> {
    run_env(vars, path)
}

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

    #[test]
    fn restores_the_marked_session_with_its_age() {
        let dir = tempfile::tempdir().unwrap();
        let mut h = History::load(&dir.path().join("s.json"));
        let id = uuid::Uuid::from_u128(1);
        let start = Start {
            agent: Agent::Pi,
            native_id: Some(id.to_string()),
            model: None,
            permission: None,
        };
        h.record(id, "C:/a", Prompt::Plain("x".into()), 1_000, &start)
            .unwrap();
        assert!(restore_active(&h, 5_000).is_none());
        h.set_active(Some(id)).unwrap();
        let Some(Msg::SetActive {
            id: got,
            cwd,
            agent,
            idle,
        }) = restore_active(&h, 5_000)
        else {
            panic!("nothing restored")
        };
        assert_eq!(
            (got, cwd.as_str(), agent, idle),
            (id, "C:/a", Agent::Pi, Duration::from_secs(4))
        );
    }

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
    fn agents_keep_every_variable() {
        let vars = [
            ("GITHUB_TOKEN", "t"),
            ("OPENAI_API_KEY", "k"),
            ("Path", "old"),
        ]
        .map(|(k, v)| (k.to_string(), v.to_string()));
        let env = agent_env(vars, "new".into());
        assert!(env.contains(&("GITHUB_TOKEN".into(), "t".into())));
        assert!(env.contains(&("OPENAI_API_KEY".into(), "k".into())));
        assert!(env.contains(&("PATH".into(), "new".into())));
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

    #[cfg(target_os = "macos")]
    #[test]
    fn models_next_to_the_app_are_ignored() {
        let exe = tempfile::tempdir().unwrap();
        std::fs::create_dir(exe.path().join("models")).unwrap();
        let data = PathBuf::from("/Users/me/Library/Application Support/Erindi");
        assert_eq!(
            pick_models_dir(None, Some(exe.path().into()), Some(data.clone()), false),
            data.join("models")
        );
    }

    #[cfg(windows)]
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
        assert_eq!(picked, PathBuf::from(r"D:\data").join("models"));
    }

    #[cfg(windows)]
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

    #[cfg(target_os = "macos")]
    #[test]
    fn llama_server_is_found_in_the_bundle() {
        let dir = tempfile::tempdir().unwrap();
        let contents = dir.path().join("Erindi.app/Contents");
        std::fs::create_dir_all(contents.join("MacOS")).unwrap();
        std::fs::create_dir_all(contents.join("Resources/llama")).unwrap();
        std::fs::write(contents.join("Resources/llama/llama-server"), "").unwrap();
        assert_eq!(
            pick_llama_server(Some(contents.join("MacOS")), Path::new("/models")),
            contents.join("MacOS/../Resources/llama/llama-server")
        );
        assert_eq!(
            pick_llama_server(None, Path::new("/models")),
            Path::new("/models").join("llama/llama-server")
        );
    }

    /// Answers each of `replies.len()` requests with one streamed reply; gives the base URL and
    /// the request bodies it read.
    fn model_server(
        replies: &'static [&'static str],
    ) -> (String, std::thread::JoinHandle<Vec<String>>) {
        use std::io::{BufRead, BufReader, Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}/v1", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            replies
                .iter()
                .map(|reply| {
                    let (stream, _) = listener.accept().unwrap();
                    let mut reader = BufReader::new(stream.try_clone().unwrap());
                    let mut length = 0;
                    loop {
                        let mut line = String::new();
                        reader.read_line(&mut line).unwrap();
                        if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                            length = v.trim().parse().unwrap();
                        }
                        if line == "\r\n" {
                            break;
                        }
                    }
                    let mut body = vec![0; length];
                    reader.read_exact(&mut body).unwrap();
                    let chunk =
                        serde_json::json!({ "choices": [{ "delta": { "content": reply } }] });
                    let mut stream = stream;
                    write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\ndata: {chunk}\n\ndata: [DONE]\n\n"
                    )
                    .unwrap();
                    String::from_utf8(body).unwrap()
                })
                .collect()
        });
        (base, handle)
    }

    #[test]
    fn a_cancelled_api_run_keeps_no_reply() {
        let dir = tempfile::tempdir().unwrap();
        let history = Mutex::new(History::load(&dir.path().join("s.json")));
        let (base, _) = model_server(&["Paris"]);
        let config = erindi_core::api::ApiConfig {
            base_url: base,
            key: None,
            model: "m".into(),
        };
        let id = uuid::Uuid::from_u128(8);
        let start = Start {
            agent: Agent::Api,
            native_id: Some(id.to_string()),
            model: Some("m".into()),
            permission: None,
        };
        history
            .lock()
            .unwrap()
            .record(
                id,
                "C:/a",
                Prompt::Plain("Capital of France?".into()),
                0,
                &start,
            )
            .unwrap();
        let cancel = AtomicBool::new(false);
        // The person cancels as the last chunk arrives.
        run_api(&config, &history, id, &cancel, |event| {
            if matches!(event, RunEvent::Result { .. }) {
                cancel.store(true, Ordering::SeqCst);
            }
        });
        let prompts = history.lock().unwrap().get(id).unwrap().prompts.clone();
        assert_eq!(prompts, [Prompt::Plain("Capital of France?".into())]);
    }

    #[test]
    fn api_run_records_the_reply() {
        let dir = tempfile::tempdir().unwrap();
        let history = Mutex::new(History::load(&dir.path().join("s.json")));
        let (base, bodies) = model_server(&["Paris", "About 2 million"]);
        let config = erindi_core::api::ApiConfig {
            base_url: base,
            key: None,
            model: "m".into(),
        };
        let id = uuid::Uuid::from_u128(7);
        let start = Start {
            agent: Agent::Api,
            native_id: Some(id.to_string()),
            model: Some("m".into()),
            permission: None,
        };
        let cancel = AtomicBool::new(false);
        for (n, prompt) in ["Capital of France?", "How many people live there?"]
            .into_iter()
            .enumerate()
        {
            history
                .lock()
                .unwrap()
                .record(id, "C:/a", Prompt::Plain(prompt.into()), n as u64, &start)
                .unwrap();
            let end = run_api(&config, &history, id, &cancel, |_| {});
            assert_eq!(end, RunEnd::Exited { success: true });
        }
        let bodies = bodies.join().unwrap();
        let second: serde_json::Value = serde_json::from_str(&bodies[1]).unwrap();
        let contents: Vec<_> = second["messages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["content"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(
            contents,
            ["Capital of France?", "Paris", "How many people live there?"]
        );
        let prompts = history.lock().unwrap().get(id).unwrap().prompts.clone();
        assert_eq!(
            prompts,
            [
                Prompt::Answered {
                    text: "Capital of France?".into(),
                    reply: "Paris".into()
                },
                Prompt::Answered {
                    text: "How many people live there?".into(),
                    reply: "About 2 million".into()
                },
            ]
        );
    }
}
