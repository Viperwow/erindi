use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::{Duration, Instant};

use erindi_audio_asr::asr::Asr;
use erindi_audio_asr::capture::{self, Capture};
use erindi_audio_asr::dsp::{To16k, rms};
use erindi_audio_asr::vad::{Endpoint, Endpointer};
use erindi_core::claude::{ClaudeRequest, Session, claude_args, claude_env, resume_in_terminal};
use erindi_core::commands::Command;
use erindi_core::controller::{Controller, Effect, Msg};
use erindi_core::llama::LlamaServer;
use erindi_core::prompt::{Dictionary, PromptTransformer};
use erindi_core::run::{RunEnd, RunSpec, run};
use erindi_core::state::{AppState, OpId};
use erindi_core::stream::parse_line;
use tauri::{AppHandle, Emitter};
use tokio_util::sync::CancellationToken;

use crate::history::{Entry, History, Prompt};
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

/// The most recent Claude run, which the overlay can reopen in a terminal.
#[derive(Clone)]
struct LastRun {
    op: OpId,
    id: uuid::Uuid,
    cwd: String,
}

impl Runtime {
    pub fn start(app: AppHandle, settings: SharedSettings, history_path: &Path) -> Self {
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
        let cwd = self.session_cwd(id)?;
        open_terminal(&cwd, id)
    }

    pub fn continue_session(&self, id: uuid::Uuid) -> Result<(), String> {
        let cwd = self.session_cwd(id)?;
        self.send(Msg::SetActive { id, cwd });
        Ok(())
    }

    pub fn delete_session(&self, id: uuid::Uuid) -> Result<(), String> {
        self.history.lock().unwrap().remove(id)?;
        self.send(Msg::Forget { id });
        Ok(())
    }

    fn session_cwd(&self, id: uuid::Uuid) -> Result<String, String> {
        let history = self.history.lock().unwrap();
        let entry = history.get(id).ok_or("Session not found")?;
        Ok(entry.cwd.clone())
    }

    pub fn set_cleanup(&self, on: bool) {
        self.refiner.set_enabled(on);
    }

    pub fn send(&self, msg: Msg) {
        let _ = self.tx.send(msg);
    }

    pub fn open_session(&self) -> Result<(), String> {
        let session = self.last_session.lock().unwrap().clone();
        let session = session.ok_or("No Claude session yet")?;
        open_terminal(&session.cwd, session.id)?;
        self.send(Msg::Dismiss { op: session.op });
        Ok(())
    }
}

fn open_terminal(cwd: &str, id: uuid::Uuid) -> Result<(), String> {
    let args =
        resume_in_terminal(cwd, id).map_err(|_| format!("Cannot open a terminal in {cwd}"))?;
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
            Effect::OpenTerminal { id, cwd } => {
                if let Err(e) = open_terminal(&cwd, id) {
                    eprintln!("{e}");
                }
            }
            Effect::RunInTerminal {
                session,
                cwd,
                prompt,
            } => self.run_in_terminal(session, cwd, prompt),
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
            } => self.start_run(op, prompt, session, cwd),
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

    fn run_in_terminal(&mut self, session: Session, cwd: String, prompt: String) {
        let settings = self.settings.read().unwrap().clone();
        let request = ClaudeRequest {
            mode: settings.mode,
            model: (!settings.model.is_empty()).then_some(settings.model),
            session,
        };
        let started = erindi_core::claude::run_in_terminal(&cwd, &request, &prompt)
            .map_err(|e| format!("Cannot open a terminal in {cwd}: {e:?}"))
            .and_then(|args| {
                std::process::Command::new("wt.exe")
                    .args(args)
                    .spawn()
                    .map_err(|e| format!("Cannot start Windows Terminal: {e}"))
            });
        if let Err(e) = started {
            eprintln!("{e}");
            // The controller already made this session active; a session that never started must not be resumed.
            if let Session::New(id) = session {
                let _ = self.tx.send(Msg::Forget { id });
            }
            return;
        }
        if !prompt.is_empty() {
            self.remember_prompt(session, &cwd, prompt);
        }
    }

    fn remember_prompt(&mut self, session: Session, cwd: &str, prompt: String) {
        let id = match session {
            Session::New(id) | Session::Resume(id) => id,
        };
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as u64);
        let entry = Prompt::Plain(prompt);
        if let Err(e) = self.history.lock().unwrap().record(id, cwd, entry, now_ms) {
            eprintln!("cannot save session history: {e}");
        }
        let _ = self.app.emit_to("settings", "sessions-changed", ());
    }

    fn start_run(&mut self, op: OpId, prompt: String, session: Session, cwd: String) {
        let settings = self.settings.read().unwrap().clone();
        let request = ClaudeRequest {
            mode: settings.mode,
            model: (!settings.model.is_empty()).then_some(settings.model),
            session,
        };
        let Ok(args) = claude_args(&request) else {
            let _ = self.tx.send(Msg::RunExited {
                op,
                end: RunEnd::Exited { success: false },
                stderr: "Invalid model name".into(),
            });
            return;
        };
        let spec = RunSpec {
            program: "claude".into(),
            args,
            cwd: cwd.clone().into(),
            env: claude_env(std::env::vars()),
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
        });
        self.remember_prompt(session, &cwd, prompt.clone());
        let token = CancellationToken::new();
        self.cancel = Some(token.clone());
        let tx = self.tx.clone();
        tauri::async_runtime::spawn(async move {
            let outcome = run(spec, token, |line| {
                for event in parse_line(line) {
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
                    stderr: format!("Cannot start claude: {e}"),
                },
            };
            let _ = tx.send(msg);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
