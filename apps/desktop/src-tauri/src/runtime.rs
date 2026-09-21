use std::path::PathBuf;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, OnceLock, RwLock};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter};
use tokio_util::sync::CancellationToken;
use whispio_audio_asr::asr::Asr;
use whispio_audio_asr::capture::{self, Capture};
use whispio_audio_asr::dsp::{To16k, rms};
use whispio_audio_asr::vad::{Endpoint, Endpointer};
use whispio_core::claude::{ClaudeRequest, claude_args, claude_env};
use whispio_core::controller::{Controller, Effect, Msg};
use whispio_core::prompt::{Dictionary, PromptTransformer};
use whispio_core::run::{RunEnd, RunSpec, run};
use whispio_core::state::{AppState, OpId};
use whispio_core::stream::parse_line;

use crate::overlay;
use crate::settings::Settings;

const RUN_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const DISMISS_AFTER: Duration = Duration::from_secs(8);
const LEVEL_INTERVAL: Duration = Duration::from_millis(33);

pub type SharedSettings = Arc<RwLock<Settings>>;

/// Sends messages to the controller thread, which owns all runtime state.
#[derive(Clone)]
pub struct Runtime {
    tx: Sender<Msg>,
}

impl Runtime {
    pub fn start(app: AppHandle, settings: SharedSettings) -> Self {
        let (tx, rx) = mpsc::channel();
        let asr = Arc::new(OnceLock::new());

        let (load_tx, load_asr) = (tx.clone(), asr.clone());
        std::thread::spawn(move || match Asr::load(&models_dir()) {
            Ok(model) => {
                let _ = load_asr.set(model);
                let _ = load_tx.send(Msg::ModelReady);
            }
            Err(e) => {
                let _ = load_tx.send(Msg::ModelFailed(e));
            }
        });

        let mut executor = Executor {
            app,
            settings: settings.clone(),
            tx: tx.clone(),
            asr,
            capture: None,
            cancel: None,
        };
        std::thread::spawn(move || {
            let mut controller = Controller::new(Box::new(SettingsDictionary(settings)));
            for msg in rx {
                for effect in controller.handle(msg, Instant::now()) {
                    executor.execute(effect);
                }
            }
        });
        Self { tx }
    }

    pub fn send(&self, msg: Msg) {
        let _ = self.tx.send(msg);
    }
}

pub fn models_dir() -> PathBuf {
    std::env::var_os("WHISPIO_MODELS")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../models")))
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
}

impl Executor {
    fn execute(&mut self, effect: Effect) {
        match effect {
            Effect::StartCapture { op, endpointing } => self.start_capture(op, endpointing),
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
                session_id,
            } => self.start_run(op, prompt, session_id),
            Effect::CancelRun => {
                if let Some(token) = self.cancel.take() {
                    token.cancel();
                }
            }
            Effect::Show(view) => {
                let _ = self.app.emit_to("overlay", "view", &view);
                match view.state {
                    AppState::Idle | AppState::LoadingModel => overlay::hide(&self.app),
                    _ => overlay::show(&self.app),
                }
                if matches!(view.state, AppState::Succeeded | AppState::Failed) {
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

    fn start_run(&mut self, op: OpId, prompt: String, session_id: uuid::Uuid) {
        let settings = self.settings.read().unwrap().clone();
        let request = ClaudeRequest {
            mode: settings.mode,
            model: (!settings.model.is_empty()).then_some(settings.model),
            session_id,
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
            cwd: settings.cwd.into(),
            env: claude_env(std::env::vars()),
            stdin: prompt,
            timeout: RUN_TIMEOUT,
        };
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
