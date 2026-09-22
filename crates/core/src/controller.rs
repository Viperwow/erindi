use std::time::{Duration, Instant};

use serde::Serialize;
use uuid::Uuid;

use crate::claude::Session;
use crate::commands::{Command, Parser, Patterns};
use crate::prompt::PromptTransformer;
use crate::refine::{Refined, accept};
use crate::run::RunEnd;
use crate::session::{Active, SessionPolicy, choose};
use crate::state::{AppState, Event, Machine, OpId, Outcome};
use crate::stream::RunEvent;

/// How often the growing recording is re-decoded for the live transcript.
pub const LIVE_INTERVAL: Duration = Duration::from_millis(700);
/// Recordings stop on their own at this length (16 kHz samples).
pub const MAX_RECORDING: usize = 16_000 * 300;
/// A press shorter than this is a tap; longer is a hold.
pub const HOLD: Duration = Duration::from_millis(300);
/// A second tap within this time makes a double-press.
pub const DOUBLE: Duration = Duration::from_millis(300);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Talk,
    /// Talks into a new session.
    NewSession,
    /// Opens the active session in a terminal.
    Terminal,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Msg {
    ModelReady,
    ModelFailed(String),
    ModelMissing,
    KeyDown(Key),
    KeyUp(Key),
    Audio {
        op: OpId,
        samples: Vec<f32>,
    },
    SpeechEnded {
        op: OpId,
    },
    NoSpeech {
        op: OpId,
    },
    Live {
        op: OpId,
        text: String,
    },
    Transcribed {
        op: OpId,
        text: String,
    },
    /// `None` when the refiner failed or is unavailable.
    Refined {
        op: OpId,
        refined: Option<Refined>,
    },
    Run {
        op: OpId,
        event: RunEvent,
    },
    RunExited {
        op: OpId,
        end: RunEnd,
        stderr: String,
    },
    Dismiss {
        op: OpId,
    },
    Settings {
        policy: SessionPolicy,
        recent: Duration,
        cwd: String,
        patterns: Patterns,
        refine: bool,
    },
    /// `DOUBLE` has passed since the tap numbered `seq`.
    GestureTimeout {
        seq: u64,
    },
    /// Makes a session from history the one the next utterance continues.
    SetActive {
        id: Uuid,
        cwd: String,
    },
    /// The session was removed from history, so it can no longer be the active one.
    Forget {
        id: Uuid,
    },
    /// Microphone or recognizer failure for the current operation.
    Failed {
        op: OpId,
        error: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    StartCapture {
        op: OpId,
    },
    /// Send `GestureTimeout { seq }` after `DOUBLE`.
    GestureTimer {
        seq: u64,
    },
    OpenTerminal {
        id: Uuid,
        cwd: String,
    },
    StopCapture,
    LiveDecode {
        op: OpId,
        samples: Vec<f32>,
    },
    Transcribe {
        op: OpId,
        samples: Vec<f32>,
    },
    Refine {
        op: OpId,
        text: String,
    },
    StartRun {
        op: OpId,
        prompt: String,
        /// What the user said, when the refiner changed it.
        raw: Option<String>,
        session: Session,
        /// Resumed sessions run in their own project folder.
        cwd: String,
    },
    CancelRun,
    OpenSettings,
    ActiveChanged(Option<Uuid>),
    Show(View),
}

/// Everything the overlay renders.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    pub op: OpId,
    pub state: AppState,
    pub text: String,
    pub detail: String,
    pub session_id: Option<Uuid>,
    /// The run continues an earlier session rather than starting one.
    pub continued: bool,
}

pub struct Controller {
    machine: Machine,
    transformer: Box<dyn PromptTransformer>,
    mode: Key,
    buffer: Vec<f32>,
    last_live: Option<Instant>,
    live_in_flight: bool,
    result: Option<(bool, String)>,
    view: View,
    policy: SessionPolicy,
    recent: Duration,
    cwd: String,
    active: Option<Active>,
    running: Option<(Session, String)>,
    refine: bool,
    /// Whether the run starts a new session, and its text, while the refiner works on it.
    pending: Option<(bool, String)>,
    parser: Parser,
    /// The key being held and when it went down.
    held: Option<(Key, Instant)>,
    /// A tap that may still become a double-press.
    tap: Option<(Key, u64)>,
    seq: u64,
    hands_free: bool,
    /// The release that ends a double-press is not a tap of its own.
    swallow_up: bool,
}

impl Controller {
    pub fn new(transformer: Box<dyn PromptTransformer>) -> Self {
        Self {
            machine: Machine::new(),
            transformer,
            mode: Key::Talk,
            buffer: Vec::new(),
            last_live: None,
            live_in_flight: false,
            result: None,
            view: View {
                op: 0,
                state: AppState::LoadingModel,
                text: String::new(),
                detail: String::new(),
                session_id: None,
                continued: false,
            },
            policy: SessionPolicy::default(),
            recent: Duration::ZERO,
            cwd: String::new(),
            active: None,
            running: None,
            refine: false,
            pending: None,
            parser: Parser::new(&Patterns::default()).expect("default patterns compile"),
            held: None,
            tap: None,
            seq: 0,
            hands_free: false,
            swallow_up: false,
        }
    }

    pub fn state(&self) -> AppState {
        self.machine.state()
    }

    pub fn handle(&mut self, msg: Msg, now: Instant) -> Vec<Effect> {
        use AppState as S;
        let state = self.machine.state();
        let current = |op: OpId| op == self.machine.op();
        match msg {
            Msg::ModelReady => self.apply(Event::ModelReady),
            Msg::ModelFailed(error) => {
                self.view.detail = error;
                self.apply(Event::ModelFailed)
            }
            Msg::ModelMissing => self.apply(Event::ModelMissing),
            Msg::KeyDown(_) if state == S::NoModel => vec![Effect::OpenSettings],
            Msg::KeyDown(key) => {
                // Auto-repeat while held.
                if self.held.is_some_and(|(k, _)| k == key) {
                    return vec![];
                }
                self.held = Some((key, now));
                if key == Key::Terminal {
                    return self.open_terminal();
                }
                if self.tap.is_some_and(|(k, _)| k == key) {
                    self.tap = None;
                    self.swallow_up = true;
                    return self.double_press();
                }
                self.tap = None;
                match state {
                    S::Idle => self.start_listening(key, now),
                    S::Succeeded | S::Failed => {
                        self.apply(Event::Dismiss);
                        self.start_listening(key, now)
                    }
                    _ => vec![],
                }
            }
            Msg::KeyUp(key) => {
                let Some((held, down)) = self.held.filter(|(k, _)| *k == key) else {
                    return vec![];
                };
                self.held = None;
                if held == Key::Terminal || std::mem::take(&mut self.swallow_up) {
                    return vec![];
                }
                if now.duration_since(down) >= HOLD {
                    if state == S::Listening && !self.hands_free && self.mode == key {
                        return self.stop_listening();
                    }
                    return vec![];
                }
                self.seq += 1;
                self.tap = Some((key, self.seq));
                vec![Effect::GestureTimer { seq: self.seq }]
            }
            Msg::GestureTimeout { seq } if self.tap.is_some_and(|(_, s)| s == seq) => {
                self.tap = None;
                self.single_press()
            }
            Msg::Audio { op, samples } if current(op) && state == S::Listening => {
                self.buffer.extend_from_slice(&samples);
                if self.buffer.len() >= MAX_RECORDING {
                    return self.stop_listening();
                }
                let due = self
                    .last_live
                    .is_none_or(|t| now.duration_since(t) >= LIVE_INTERVAL);
                if self.live_in_flight || !due {
                    return vec![];
                }
                self.live_in_flight = true;
                self.last_live = Some(now);
                vec![Effect::LiveDecode {
                    op,
                    samples: self.buffer.clone(),
                }]
            }
            Msg::SpeechEnded { op } if current(op) && state == S::Listening && self.hands_free => {
                self.stop_listening()
            }
            Msg::NoSpeech { op } if current(op) && state == S::Listening && self.hands_free => {
                let mut fx = vec![Effect::StopCapture];
                fx.extend(self.apply(Event::CancelListening));
                fx
            }
            Msg::Live { op, text }
                if current(op) && matches!(state, S::Listening | S::Transcribing) =>
            {
                self.live_in_flight = false;
                self.view.text = self.transformer.transform(&text);
                vec![self.show()]
            }
            Msg::Transcribed { op, text } => {
                if !current(op) || state != S::Transcribing {
                    return vec![];
                }
                let text = self.transformer.transform(&text);
                let (command, rest) = self.parser.parse(&text);
                let new = command == Some(Command::NewSession) || self.mode == Key::NewSession;
                let mut fx = vec![];
                let prompt = match command {
                    Some(Command::Cancel) => String::new(),
                    Some(Command::OpenTerminal) if rest.trim().is_empty() => {
                        fx.extend(self.open_terminal());
                        String::new()
                    }
                    _ => rest.trim().to_string(),
                };
                if self.refine && !prompt.is_empty() {
                    if let Ok(Outcome::Changed(_)) = self.machine.apply(Event::Refine { op }) {
                        self.pending = Some((new, prompt.clone()));
                        self.view.text = prompt.clone();
                        return vec![Effect::Refine { op, text: prompt }, self.show()];
                    }
                    return vec![];
                }
                let empty = prompt.is_empty();
                fx.extend(match self.machine.apply(Event::Transcribed { op, empty }) {
                    Ok(Outcome::Changed(S::Running)) => self.start_run(op, new, prompt, None, now),
                    Ok(Outcome::Changed(_)) => vec![self.show()],
                    _ => vec![],
                });
                fx
            }
            Msg::Refined { op, refined } if current(op) && state == S::Refining => {
                let Some((new, input)) = self.pending.take() else {
                    return vec![];
                };
                // ponytail: the model's intent is ignored until the benchmark shows it beats the parser.
                let text = accept(&input, refined).map_or_else(|| input.clone(), |r| r.text);
                let raw = (text != input).then_some(input);
                match self.machine.apply(Event::Refined { op }) {
                    Ok(Outcome::Changed(S::Running)) => self.start_run(op, new, text, raw, now),
                    _ => vec![],
                }
            }
            Msg::Run { op, event } if current(op) && state == S::Running => match event {
                RunEvent::ToolUse { name } => {
                    self.view.detail = name;
                    vec![self.show()]
                }
                RunEvent::PermissionDenied { tool } => {
                    self.view.detail = format!("Permission denied: {tool}");
                    vec![self.show()]
                }
                RunEvent::Result { ok, text } => {
                    self.result = Some((ok, text));
                    vec![]
                }
            },
            Msg::RunExited { op, end, stderr } if current(op) => {
                let result_ok = self.result.as_ref().is_none_or(|(ok, _)| *ok);
                let ok = end == RunEnd::Exited { success: true } && result_ok;
                let mut fx = match self.running.take() {
                    Some((session, cwd)) => self.remember(session, cwd, &end, ok, now),
                    None => vec![],
                };
                self.view.detail = match (&self.result, &end) {
                    (Some((_, text)), _) if !text.is_empty() => text.clone(),
                    (_, RunEnd::TimedOut) => "Timed out".into(),
                    _ => stderr.trim().lines().last().unwrap_or_default().into(),
                };
                fx.extend(self.apply(Event::RunExited { op, ok }));
                fx
            }
            Msg::Dismiss { op } if current(op) => self.apply(Event::Dismiss),
            Msg::Settings {
                policy,
                recent,
                cwd,
                patterns,
                refine,
            } => {
                if let Ok(parser) = Parser::new(&patterns) {
                    self.parser = parser;
                }
                self.refine = refine;
                self.policy = policy;
                self.recent = recent;
                if cwd == self.cwd {
                    return vec![];
                }
                self.cwd = cwd;
                self.set_active(None)
            }
            Msg::Forget { id } if self.active.as_ref().is_some_and(|a| a.id == id) => {
                self.set_active(None)
            }
            Msg::SetActive { id, cwd } => self.set_active(Some(Active {
                id,
                cwd,
                last_used: now,
            })),
            Msg::Failed { op, error } if current(op) => {
                self.view.detail = error;
                let mut fx = match state {
                    S::Listening => vec![Effect::StopCapture],
                    _ => vec![],
                };
                fx.extend(self.apply(Event::StepFailed { op }));
                fx
            }
            _ => vec![],
        }
    }

    /// Updates the active session after a run. A run that produced a result proves its session
    /// exists; a resume that failed without one most likely points at a session Claude no longer has.
    fn remember(
        &mut self,
        session: Session,
        cwd: String,
        end: &RunEnd,
        ok: bool,
        now: Instant,
    ) -> Vec<Effect> {
        if *end == RunEnd::Cancelled {
            return vec![];
        }
        if ok || self.result.is_some() {
            self.set_active(Some(Active {
                id: session_id(session),
                cwd,
                last_used: now,
            }))
        } else if matches!(session, Session::Resume(_)) {
            self.set_active(None)
        } else {
            vec![]
        }
    }

    /// Replaces the active session, announcing only a change of session.
    fn set_active(&mut self, active: Option<Active>) -> Vec<Effect> {
        let before = self.active.as_ref().map(|a| a.id);
        let after = active.as_ref().map(|a| a.id);
        self.active = active;
        if before == after {
            vec![]
        } else {
            vec![Effect::ActiveChanged(after)]
        }
    }

    fn start_run(
        &mut self,
        op: OpId,
        new: bool,
        prompt: String,
        raw: Option<String>,
        now: Instant,
    ) -> Vec<Effect> {
        let resume = choose(self.policy, self.recent, new, self.active.as_ref(), now);
        let (session, cwd) = match (resume, &self.active) {
            (Some(id), Some(active)) => (Session::Resume(id), active.cwd.clone()),
            _ => (Session::New(Uuid::new_v4()), self.cwd.clone()),
        };
        self.running = Some((session, cwd.clone()));
        self.result = None;
        self.view.text = prompt.clone();
        self.view.session_id = Some(session_id(session));
        self.view.continued = resume.is_some();
        self.view.detail.clear();
        vec![
            Effect::StartRun {
                op,
                prompt,
                raw,
                session,
                cwd,
            },
            self.show(),
        ]
    }

    fn open_terminal(&self) -> Vec<Effect> {
        match &self.active {
            Some(active) => vec![Effect::OpenTerminal {
                id: active.id,
                cwd: active.cwd.clone(),
            }],
            None => vec![],
        }
    }

    /// One press of the key cancels whatever is in progress.
    fn single_press(&mut self) -> Vec<Effect> {
        use AppState as S;
        match self.machine.state() {
            S::Listening => {
                self.hands_free = false;
                let mut fx = vec![Effect::StopCapture];
                fx.extend(self.apply(Event::CancelListening));
                fx
            }
            S::Transcribing | S::Refining => self.apply(Event::Abandon),
            S::Running => {
                let mut fx = vec![Effect::CancelRun];
                fx.extend(self.apply(Event::Cancel));
                fx
            }
            _ => vec![],
        }
    }

    /// The first double-press goes hands-free; the next one sends without waiting for a pause.
    fn double_press(&mut self) -> Vec<Effect> {
        match self.machine.state() {
            AppState::Listening if !self.hands_free => {
                self.hands_free = true;
                vec![]
            }
            AppState::Listening => self.stop_listening(),
            _ => vec![],
        }
    }

    fn apply(&mut self, event: Event) -> Vec<Effect> {
        match self.machine.apply(event) {
            Ok(Outcome::Changed(_)) => vec![self.show()],
            _ => vec![],
        }
    }

    fn show(&mut self) -> Effect {
        self.view.op = self.machine.op();
        self.view.state = self.machine.state();
        Effect::Show(self.view.clone())
    }

    fn start_listening(&mut self, key: Key, now: Instant) -> Vec<Effect> {
        if self.machine.apply(Event::StartListening).is_err() {
            return vec![];
        }
        self.mode = key;
        self.hands_free = false;
        self.buffer.clear();
        self.last_live = Some(now);
        self.live_in_flight = false;
        self.view.text.clear();
        self.view.detail.clear();
        self.view.session_id = None;
        self.view.continued = false;
        vec![
            Effect::StartCapture {
                op: self.machine.op(),
            },
            self.show(),
        ]
    }

    fn stop_listening(&mut self) -> Vec<Effect> {
        if self.machine.apply(Event::StopListening).is_err() {
            return vec![];
        }
        self.hands_free = false;
        vec![
            Effect::StopCapture,
            Effect::Transcribe {
                op: self.machine.op(),
                samples: std::mem::take(&mut self.buffer),
            },
            self.show(),
        ]
    }
}

fn session_id(session: Session) -> Uuid {
    match session {
        Session::New(id) | Session::Resume(id) => id,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prompt::Dictionary;
    use crate::refine::Refined;

    struct T {
        c: Controller,
        now: Instant,
    }

    impl T {
        fn new() -> Self {
            let dict = Dictionary::new([("клод".to_string(), "Claude".to_string())]);
            let mut t = T {
                c: Controller::new(Box::new(dict)),
                now: Instant::now(),
            };
            t.send(Msg::ModelReady);
            t.send(settings(SessionPolicy::Continue, "C:/p"));
            t
        }

        fn send(&mut self, msg: Msg) -> Vec<Effect> {
            self.c.handle(msg, self.now)
        }

        fn op(&self) -> OpId {
            self.c.view.op
        }

        fn listen(&mut self, key: Key) -> OpId {
            self.send(Msg::KeyDown(key));
            self.op()
        }

        /// Ends a hold that has lasted long enough to count as one.
        fn release(&mut self, key: Key) -> Vec<Effect> {
            self.now += HOLD;
            self.send(Msg::KeyUp(key))
        }

        fn quick(&mut self, key: Key) -> Vec<Effect> {
            let mut fx = self.send(Msg::KeyDown(key));
            self.now += Duration::from_millis(50);
            fx.extend(self.send(Msg::KeyUp(key)));
            self.now += Duration::from_millis(50);
            fx
        }

        /// A single press, confirmed once `DOUBLE` has passed.
        fn tap(&mut self, key: Key) -> Vec<Effect> {
            let mut fx = self.quick(key);
            let seq = fx
                .iter()
                .find_map(|e| match e {
                    Effect::GestureTimer { seq } => Some(*seq),
                    _ => None,
                })
                .expect("a tap starts the gesture timer");
            self.now += DOUBLE;
            fx.extend(self.send(Msg::GestureTimeout { seq }));
            fx
        }

        fn double(&mut self, key: Key) -> Vec<Effect> {
            let mut fx = self.quick(key);
            fx.extend(self.quick(key));
            fx
        }

        /// Starts a hands-free recording.
        fn hands_free(&mut self, key: Key) -> OpId {
            self.double(key);
            self.op()
        }

        fn run(&mut self) -> OpId {
            self.run_saying("клод, проверь diff")
        }

        /// Runs `text` to completion and returns the session it used.
        fn finish_saying(&mut self, text: &str) -> Session {
            let op = self.listen(Key::Talk);
            self.release(Key::Talk);
            let mut fx = self.send(Msg::Transcribed {
                op,
                text: text.into(),
            });
            if let Some(Effect::Refine { op, .. }) = fx.first() {
                let op = *op;
                fx = self.send(Msg::Refined { op, refined: None });
            }
            let Some(Effect::StartRun { session, .. }) = fx.first() else {
                panic!("{fx:?}")
            };
            let session = *session;
            self.send(Msg::Run {
                op,
                event: RunEvent::Result {
                    ok: true,
                    text: "done".into(),
                },
            });
            self.send(Msg::RunExited {
                op,
                end: RunEnd::Exited { success: true },
                stderr: String::new(),
            });
            session
        }

        fn run_saying(&mut self, text: &str) -> OpId {
            let op = self.listen(Key::Talk);
            self.send(Msg::Audio {
                op,
                samples: vec![0.1; 160],
            });
            self.release(Key::Talk);
            self.send(Msg::Transcribed {
                op,
                text: text.into(),
            });
            op
        }
    }

    fn settings(policy: SessionPolicy, cwd: &str) -> Msg {
        Msg::Settings {
            policy,
            recent: Duration::from_secs(600),
            cwd: cwd.into(),
            patterns: Patterns::default(),
            refine: false,
        }
    }

    fn id(session: Session) -> Uuid {
        match session {
            Session::New(id) | Session::Resume(id) => id,
        }
    }

    fn shown(effects: &[Effect]) -> Option<&View> {
        effects.iter().rev().find_map(|e| match e {
            Effect::Show(v) => Some(v),
            _ => None,
        })
    }

    #[test]
    fn keys_before_model_ready_are_ignored() {
        let mut c = Controller::new(Box::new(Dictionary::default()));
        assert_eq!(c.handle(Msg::KeyDown(Key::Talk), Instant::now()), []);
    }

    #[test]
    fn model_failure_is_shown() {
        let mut c = Controller::new(Box::new(Dictionary::default()));
        let fx = c.handle(Msg::ModelFailed("no model".into()), Instant::now());
        let v = shown(&fx).unwrap();
        assert_eq!(v.state, AppState::Failed);
        assert_eq!(v.detail, "no model");
    }

    #[test]
    fn hold_records_until_release_then_transcribes() {
        let mut t = T::new();
        let fx = t.send(Msg::KeyDown(Key::Talk));
        let op = t.op();
        assert_eq!(fx[0], Effect::StartCapture { op });
        assert_eq!(shown(&fx).unwrap().state, AppState::Listening);

        t.send(Msg::Audio {
            op,
            samples: vec![0.1; 100],
        });
        t.send(Msg::Audio {
            op,
            samples: vec![0.2; 50],
        });
        let fx = t.release(Key::Talk);

        assert_eq!(fx[0], Effect::StopCapture);
        let Effect::Transcribe { op: top, samples } = &fx[1] else {
            panic!("{fx:?}")
        };
        assert_eq!((*top, samples.len()), (op, 150));
        assert_eq!(shown(&fx).unwrap().state, AppState::Transcribing);
    }

    #[test]
    fn quick_tap_from_idle_drops_the_recording() {
        let mut t = T::new();
        let fx = t.tap(Key::Talk);
        assert!(fx.contains(&Effect::StopCapture));
        assert!(!fx.iter().any(|e| matches!(e, Effect::Transcribe { .. })));
        assert_eq!(t.c.state(), AppState::Idle);
    }

    #[test]
    fn double_press_goes_hands_free() {
        let mut t = T::new();
        let fx = t.double(Key::Talk);
        assert!(!fx.contains(&Effect::StopCapture));
        assert_eq!(t.c.state(), AppState::Listening);
        t.now += Duration::from_secs(2);
        t.send(Msg::GestureTimeout { seq: 1 });
        assert_eq!(
            t.c.state(),
            AppState::Listening,
            "the first tap's timer is spent"
        );
    }

    #[test]
    fn double_press_while_hands_free_sends_now() {
        let mut t = T::new();
        t.hands_free(Key::Talk);
        t.now += Duration::from_secs(1);
        let fx = t.double(Key::Talk);
        assert!(fx.iter().any(|e| matches!(e, Effect::Transcribe { .. })));
        assert_eq!(t.c.state(), AppState::Transcribing);
    }

    #[test]
    fn single_press_while_hands_free_cancels() {
        let mut t = T::new();
        t.hands_free(Key::Talk);
        t.now += Duration::from_secs(1);
        let fx = t.tap(Key::Talk);
        assert!(fx.contains(&Effect::StopCapture));
        assert_eq!(t.c.state(), AppState::Idle);
    }

    #[test]
    fn hold_ignores_the_pause_detector() {
        let mut t = T::new();
        let op = t.listen(Key::Talk);
        assert_eq!(t.send(Msg::SpeechEnded { op }), []);
        assert_eq!(t.c.state(), AppState::Listening);
    }

    #[test]
    fn auto_repeat_is_not_a_double_press() {
        let mut t = T::new();
        let op = t.listen(Key::Talk);
        for _ in 0..3 {
            t.now += Duration::from_millis(30);
            assert_eq!(t.send(Msg::KeyDown(Key::Talk)), []);
        }
        t.send(Msg::Audio {
            op,
            samples: vec![0.1; 10],
        });
        let fx = t.release(Key::Talk);
        assert!(fx.iter().any(|e| matches!(e, Effect::Transcribe { .. })));
    }

    #[test]
    fn press_during_transcription_cancels() {
        let mut t = T::new();
        let op = t.listen(Key::Talk);
        t.release(Key::Talk);
        t.tap(Key::Talk);
        assert_eq!(t.c.state(), AppState::Idle);
        let fx = t.send(Msg::Transcribed {
            op,
            text: "проверь diff".into(),
        });
        assert_eq!(fx, []);
    }

    #[test]
    fn double_press_while_running_does_nothing() {
        let mut t = T::new();
        t.run();
        let fx = t.double(Key::Talk);
        assert!(!fx.contains(&Effect::CancelRun));
        assert_eq!(t.c.state(), AppState::Running);
    }

    #[test]
    fn terminal_key_opens_the_active_session() {
        let mut t = T::new();
        let session = t.finish_saying("проверь diff");
        let fx = t.send(Msg::KeyDown(Key::Terminal));
        assert_eq!(
            fx,
            [Effect::OpenTerminal {
                id: id(session),
                cwd: "C:/p".into()
            }]
        );
        assert_eq!(t.send(Msg::KeyUp(Key::Terminal)), []);
    }

    #[test]
    fn terminal_without_active_session_does_nothing() {
        let mut t = T::new();
        assert_eq!(t.send(Msg::KeyDown(Key::Terminal)), []);
    }

    #[test]
    fn spoken_cancel_sends_nothing() {
        let mut t = T::new();
        let op = t.listen(Key::Talk);
        t.release(Key::Talk);
        let fx = t.send(Msg::Transcribed {
            op,
            text: "проверь diff, отмена".into(),
        });
        assert!(!fx.iter().any(|e| matches!(e, Effect::StartRun { .. })));
        assert_eq!(t.c.state(), AppState::Idle);
    }

    #[test]
    fn spoken_terminal_alone_opens_terminal_and_sends_nothing() {
        let mut t = T::new();
        let session = t.finish_saying("проверь diff");
        t.now += Duration::from_secs(1);
        let op = t.listen(Key::Talk);
        t.release(Key::Talk);
        let fx = t.send(Msg::Transcribed {
            op,
            text: "Открой в терминале.".into(),
        });
        assert!(fx.contains(&Effect::OpenTerminal {
            id: id(session),
            cwd: "C:/p".into()
        }));
        assert!(!fx.iter().any(|e| matches!(e, Effect::StartRun { .. })));
    }

    #[test]
    fn toggle_stops_on_speech_end() {
        let mut t = T::new();
        let op = t.hands_free(Key::Talk);
        let fx = t.send(Msg::SpeechEnded { op });
        assert_eq!(fx[0], Effect::StopCapture);
        assert_eq!(t.c.state(), AppState::Transcribing);
    }

    #[test]
    fn no_speech_cancels_recording() {
        let mut t = T::new();
        let op = t.hands_free(Key::Talk);
        let fx = t.send(Msg::NoSpeech { op });
        assert_eq!(fx[0], Effect::StopCapture);
        assert_eq!(shown(&fx).unwrap().state, AppState::Idle);
    }

    #[test]
    fn live_decode_is_throttled_and_never_overlaps() {
        let mut t = T::new();
        let op = t.listen(Key::Talk);
        let live = |fx: &[Effect]| fx.iter().any(|e| matches!(e, Effect::LiveDecode { .. }));

        assert!(!live(&t.send(Msg::Audio {
            op,
            samples: vec![0.1; 10]
        })));
        t.now += LIVE_INTERVAL;
        assert!(live(&t.send(Msg::Audio {
            op,
            samples: vec![0.1; 10]
        })));
        t.now += LIVE_INTERVAL;
        assert!(
            !live(&t.send(Msg::Audio {
                op,
                samples: vec![0.1; 10]
            })),
            "in flight"
        );

        let fx = t.send(Msg::Live {
            op,
            text: "клод  привет".into(),
        });
        assert_eq!(shown(&fx).unwrap().text, "Claude привет");
        assert!(live(&t.send(Msg::Audio {
            op,
            samples: vec![0.1; 10]
        })));
    }

    #[test]
    fn long_recording_stops_itself() {
        let mut t = T::new();
        let op = t.listen(Key::Talk);
        let fx = t.send(Msg::Audio {
            op,
            samples: vec![0.0; MAX_RECORDING],
        });
        assert!(fx.contains(&Effect::StopCapture));
        assert_eq!(t.c.state(), AppState::Transcribing);
    }

    #[test]
    fn transcript_is_transformed_and_dispatched() {
        let mut t = T::new();
        let op = t.run();
        assert_eq!(t.c.state(), AppState::Running);
        let v = &t.c.view;
        assert_eq!(v.text, "Claude, проверь diff");
        assert!(v.session_id.is_some());
        let _ = op;
    }

    #[test]
    fn start_run_effect_carries_prompt_and_session() {
        let mut t = T::new();
        let op = t.listen(Key::Talk);
        t.release(Key::Talk);
        let fx = t.send(Msg::Transcribed {
            op,
            text: " клод  go ".into(),
        });
        let Some(Effect::StartRun {
            op: rop,
            prompt,
            session: Session::New(session_id),
            ..
        }) = fx.first()
        else {
            panic!("{fx:?}")
        };
        assert_eq!((*rop, prompt.as_str()), (op, "Claude go"));
        assert_eq!(shown(&fx).unwrap().session_id, Some(*session_id));
        assert!(!shown(&fx).unwrap().continued);
    }

    #[test]
    fn empty_transcript_returns_to_idle() {
        let mut t = T::new();
        let op = t.listen(Key::Talk);
        t.release(Key::Talk);
        let fx = t.send(Msg::Transcribed {
            op,
            text: "  ".into(),
        });
        assert_eq!(shown(&fx).unwrap().state, AppState::Idle);
        assert!(!fx.iter().any(|e| matches!(e, Effect::StartRun { .. })));
    }

    #[test]
    fn run_progress_updates_detail() {
        let mut t = T::new();
        let op = t.run();
        let fx = t.send(Msg::Run {
            op,
            event: RunEvent::ToolUse {
                name: "Edit".into(),
            },
        });
        assert_eq!(shown(&fx).unwrap().detail, "Edit");
        let fx = t.send(Msg::Run {
            op,
            event: RunEvent::PermissionDenied {
                tool: "Bash".into(),
            },
        });
        assert_eq!(shown(&fx).unwrap().detail, "Permission denied: Bash");
    }

    #[test]
    fn run_success_then_dismiss() {
        let mut t = T::new();
        let op = t.run();
        t.send(Msg::Run {
            op,
            event: RunEvent::Result {
                ok: true,
                text: "Done".into(),
            },
        });
        let fx = t.send(Msg::RunExited {
            op,
            end: RunEnd::Exited { success: true },
            stderr: String::new(),
        });
        let v = shown(&fx).unwrap();
        assert_eq!((v.state, v.detail.as_str()), (AppState::Succeeded, "Done"));
        let fx = t.send(Msg::Dismiss { op });
        assert_eq!(shown(&fx).unwrap().state, AppState::Idle);
    }

    #[test]
    fn result_error_fails_run_even_with_zero_exit() {
        let mut t = T::new();
        let op = t.run();
        t.send(Msg::Run {
            op,
            event: RunEvent::Result {
                ok: false,
                text: "Invalid API key".into(),
            },
        });
        let fx = t.send(Msg::RunExited {
            op,
            end: RunEnd::Exited { success: true },
            stderr: String::new(),
        });
        let v = shown(&fx).unwrap();
        assert_eq!(
            (v.state, v.detail.as_str()),
            (AppState::Failed, "Invalid API key")
        );
    }

    #[test]
    fn crash_without_result_shows_stderr() {
        let mut t = T::new();
        let op = t.run();
        let fx = t.send(Msg::RunExited {
            op,
            end: RunEnd::Exited { success: false },
            stderr: "boom\n".into(),
        });
        assert_eq!(shown(&fx).unwrap().detail, "boom");
        let op = t.op();
        t.send(Msg::Dismiss { op });
        let op = t.run();
        let fx = t.send(Msg::RunExited {
            op,
            end: RunEnd::TimedOut,
            stderr: String::new(),
        });
        assert_eq!(shown(&fx).unwrap().detail, "Timed out");
    }

    #[test]
    fn key_during_run_cancels() {
        let mut t = T::new();
        let op = t.run();
        let fx = t.tap(Key::Talk);
        assert!(fx.contains(&Effect::CancelRun));
        assert_eq!(t.c.state(), AppState::Cancelling);
        let fx = t.send(Msg::RunExited {
            op,
            end: RunEnd::Cancelled,
            stderr: String::new(),
        });
        assert_eq!(shown(&fx).unwrap().state, AppState::Idle);
    }

    #[test]
    fn key_after_result_starts_new_recording() {
        let mut t = T::new();
        let op = t.run();
        t.send(Msg::RunExited {
            op,
            end: RunEnd::Exited { success: true },
            stderr: String::new(),
        });
        let fx = t.send(Msg::KeyDown(Key::Talk));
        assert!(matches!(fx[0], Effect::StartCapture { .. }));
        assert_ne!(t.op(), op);
        assert_eq!(t.send(Msg::Dismiss { op }), [], "stale dismiss");
    }

    #[test]
    fn stale_messages_are_ignored() {
        let mut t = T::new();
        let old = t.listen(Key::Talk);
        t.release(Key::Talk);
        t.send(Msg::Transcribed {
            op: old,
            text: String::new(),
        });
        t.listen(Key::Talk);
        for msg in [
            Msg::Audio {
                op: old,
                samples: vec![0.0; 10],
            },
            Msg::SpeechEnded { op: old },
            Msg::Live {
                op: old,
                text: "x".into(),
            },
            Msg::Transcribed {
                op: old,
                text: "x".into(),
            },
        ] {
            assert_eq!(t.send(msg), []);
        }
        assert_eq!(t.c.state(), AppState::Listening);
    }

    #[test]
    fn microphone_failure_stops_capture_and_shows_error() {
        let mut t = T::new();
        let op = t.listen(Key::Talk);
        let fx = t.send(Msg::Failed {
            op,
            error: "no microphone".into(),
        });
        assert_eq!(fx[0], Effect::StopCapture);
        let v = shown(&fx).unwrap();
        assert_eq!(
            (v.state, v.detail.as_str()),
            (AppState::Failed, "no microphone")
        );
        assert_eq!(
            t.send(Msg::Failed {
                op: op + 1,
                error: "x".into()
            }),
            []
        );
    }

    #[test]
    fn next_utterance_continues_the_session() {
        let mut t = T::new();
        let first = t.finish_saying("проверь diff");
        assert!(matches!(first, Session::New(_)));
        assert!(!t.c.view.continued);
        let second = t.finish_saying("теперь добавь тесты");
        assert_eq!(second, Session::Resume(id(first)));
        assert!(t.c.view.continued);
        assert_eq!(t.c.view.session_id, Some(id(first)));
    }

    #[test]
    fn spoken_command_starts_a_new_session_and_is_removed() {
        let mut t = T::new();
        let first = t.finish_saying("проверь diff");
        let op = t.listen(Key::Talk);
        t.release(Key::Talk);
        let fx = t.send(Msg::Transcribed {
            op,
            text: "в новой сессии клод, найди баг".into(),
        });
        let Some(Effect::StartRun {
            prompt, session, ..
        }) = fx.first()
        else {
            panic!("{fx:?}")
        };
        assert_eq!(prompt, "Claude, найди баг");
        assert!(matches!(session, Session::New(new) if *new != id(first)));
    }

    #[test]
    fn new_session_key_talks_into_a_new_session() {
        let mut t = T::new();
        t.finish_saying("проверь diff");
        t.now += Duration::from_secs(1);
        let op = t.hands_free(Key::NewSession);
        t.send(Msg::SpeechEnded { op });
        let fx = t.send(Msg::Transcribed {
            op,
            text: "найди баг".into(),
        });
        assert!(matches!(
            fx.first(),
            Some(Effect::StartRun {
                session: Session::New(_),
                ..
            })
        ));
    }

    #[test]
    fn changing_project_folder_starts_fresh() {
        let mut t = T::new();
        t.finish_saying("проверь diff");
        t.send(settings(SessionPolicy::Continue, "C:/other"));
        assert!(matches!(t.finish_saying("проверь diff"), Session::New(_)));
    }

    #[test]
    fn always_new_policy_ignores_the_active_session() {
        let mut t = T::new();
        t.send(settings(SessionPolicy::AlwaysNew, "C:/p"));
        t.finish_saying("проверь diff");
        assert!(matches!(t.finish_saying("ещё раз"), Session::New(_)));
    }

    #[test]
    fn recent_policy_expires() {
        let mut t = T::new();
        t.send(settings(SessionPolicy::ContinueIfRecent, "C:/p"));
        t.finish_saying("проверь diff");
        t.now += Duration::from_secs(601);
        assert!(matches!(t.finish_saying("ещё раз"), Session::New(_)));
    }

    #[test]
    fn failed_resume_without_result_forgets_the_session() {
        let mut t = T::new();
        t.finish_saying("проверь diff");
        let op = t.run_saying("продолжай");
        t.send(Msg::RunExited {
            op,
            end: RunEnd::Exited { success: false },
            stderr: "No conversation found".into(),
        });
        t.send(Msg::Dismiss { op });
        assert!(matches!(t.finish_saying("ещё раз"), Session::New(_)));
    }

    fn active_changes(effects: &[Effect]) -> Vec<Option<Uuid>> {
        effects
            .iter()
            .filter_map(|e| match e {
                Effect::ActiveChanged(id) => Some(*id),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn announces_the_active_session() {
        let mut t = T::new();
        let op = t.listen(Key::Talk);
        t.release(Key::Talk);
        let fx = t.send(Msg::Transcribed {
            op,
            text: "проверь diff".into(),
        });
        let Some(Effect::StartRun { session, .. }) = fx.first() else {
            panic!("{fx:?}")
        };
        let session = *session;
        let fx = t.send(Msg::RunExited {
            op,
            end: RunEnd::Exited { success: true },
            stderr: String::new(),
        });
        assert_eq!(active_changes(&fx), [Some(id(session))]);
        let fx = t.send(settings(SessionPolicy::Continue, "C:/other"));
        assert_eq!(active_changes(&fx), [None]);
    }

    #[test]
    fn picked_session_is_continued_in_its_own_folder() {
        let mut t = T::new();
        let picked = Uuid::from_u128(42);
        let fx = t.send(Msg::SetActive {
            id: picked,
            cwd: "D:/elsewhere".into(),
        });
        assert_eq!(active_changes(&fx), [Some(picked)]);

        let op = t.listen(Key::Talk);
        t.release(Key::Talk);
        let fx = t.send(Msg::Transcribed {
            op,
            text: "продолжай".into(),
        });
        let Some(Effect::StartRun { session, cwd, .. }) = fx.first() else {
            panic!("{fx:?}")
        };
        assert_eq!(*session, Session::Resume(picked));
        assert_eq!(cwd, "D:/elsewhere");
    }

    #[test]
    fn new_sessions_run_in_the_settings_folder() {
        let mut t = T::new();
        let op = t.listen(Key::Talk);
        t.release(Key::Talk);
        let fx = t.send(Msg::Transcribed {
            op,
            text: "проверь diff".into(),
        });
        let Some(Effect::StartRun { cwd, .. }) = fx.first() else {
            panic!("{fx:?}")
        };
        assert_eq!(cwd, "C:/p");
    }

    #[test]
    fn forgetting_the_active_session_clears_it() {
        let mut t = T::new();
        let picked = Uuid::from_u128(42);
        t.send(Msg::SetActive {
            id: picked,
            cwd: "D:/elsewhere".into(),
        });
        assert_eq!(
            t.send(Msg::Forget {
                id: Uuid::from_u128(7)
            }),
            []
        );
        let fx = t.send(Msg::Forget { id: picked });
        assert_eq!(active_changes(&fx), [None]);
        assert!(matches!(t.finish_saying("ещё раз"), Session::New(_)));
    }

    #[test]
    fn hotkey_without_model_opens_settings() {
        let mut c = Controller::new(Box::new(Dictionary::default()));
        let now = Instant::now();
        c.handle(Msg::ModelMissing, now);
        assert_eq!(c.state(), AppState::NoModel);
        assert_eq!(
            c.handle(Msg::KeyDown(Key::Talk), now),
            [Effect::OpenSettings]
        );
        c.handle(Msg::ModelReady, now);
        assert_eq!(c.state(), AppState::Idle);
    }

    fn refining() -> T {
        let mut t = T::new();
        t.send(Msg::Settings {
            policy: SessionPolicy::Continue,
            recent: Duration::from_secs(600),
            cwd: "C:/p".into(),
            patterns: Patterns::default(),
            refine: true,
        });
        t
    }

    #[test]
    fn refine_runs_the_cleaned_text_and_keeps_the_raw() {
        let mut t = refining();
        let op = t.listen(Key::Talk);
        t.release(Key::Talk);
        let fx = t.send(Msg::Transcribed {
            op,
            text: "эээ клод проверь diff".into(),
        });
        assert_eq!(
            fx[0],
            Effect::Refine {
                op,
                text: "эээ Claude проверь diff".into()
            }
        );
        assert_eq!(shown(&fx).unwrap().state, AppState::Refining);
        let fx = t.send(Msg::Refined {
            op,
            refined: Some(Refined {
                intent: crate::refine::Intent::Unspecified,
                text: "Claude, проверь diff.".into(),
            }),
        });
        let Some(Effect::StartRun { prompt, raw, .. }) = fx.first() else {
            panic!("{fx:?}")
        };
        assert_eq!(prompt, "Claude, проверь diff.");
        assert_eq!(raw.as_deref(), Some("эээ Claude проверь diff"));
    }

    #[test]
    fn failed_refine_runs_the_parser_text() {
        let mut t = refining();
        let op = t.listen(Key::Talk);
        t.release(Key::Talk);
        t.send(Msg::Transcribed {
            op,
            text: "в новой сессии проверь diff".into(),
        });
        let fx = t.send(Msg::Refined { op, refined: None });
        let Some(Effect::StartRun {
            prompt,
            raw,
            session,
            ..
        }) = fx.first()
        else {
            panic!("{fx:?}")
        };
        assert_eq!((prompt.as_str(), raw), ("проверь diff", &None));
        assert!(matches!(session, Session::New(_)));
    }

    #[test]
    fn parser_intent_wins_over_the_model() {
        let mut t = refining();
        t.finish_saying("проверь diff");
        let op = t.listen(Key::Talk);
        t.release(Key::Talk);
        t.send(Msg::Transcribed {
            op,
            text: "добавь тесты".into(),
        });
        let fx = t.send(Msg::Refined {
            op,
            refined: Some(Refined {
                intent: crate::refine::Intent::New,
                text: "Добавь тесты.".into(),
            }),
        });
        let Some(Effect::StartRun { session, .. }) = fx.first() else {
            panic!("{fx:?}")
        };
        assert!(matches!(session, Session::Resume(_)));
    }

    #[test]
    fn empty_transcript_skips_refining() {
        let mut t = refining();
        let op = t.listen(Key::Talk);
        t.release(Key::Talk);
        let fx = t.send(Msg::Transcribed {
            op,
            text: " ".into(),
        });
        assert!(!fx.iter().any(|e| matches!(e, Effect::Refine { .. })));
        assert_eq!(t.c.state(), AppState::Idle);
    }
}
