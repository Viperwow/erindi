use std::collections::VecDeque;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::agent::Agent;
use crate::classify::accept;
use crate::claude::Session;
use crate::commands::{Command, Parser, Patterns};
use crate::prompt::PromptTransformer;
use crate::run::RunEnd;
use crate::series::{Kind, Phrase, PhraseId, Series, Status};
use crate::session::{Active, SessionPolicy, choose};
use crate::state::OpId;
use crate::stream::RunEvent;

/// How often the growing recording is re-decoded for the live transcript.
pub const LIVE_INTERVAL: Duration = Duration::from_millis(700);
/// Recordings stop on their own at this length (16 kHz samples).
pub const MAX_RECORDING: usize = 16_000 * 300;
/// A press shorter than this is a tap; longer is a hold.
pub const HOLD: Duration = Duration::from_millis(500);
/// A second tap within this time makes a double-press, by default; a chord pressed twice is slow.
pub const DOUBLE: Duration = Duration::from_millis(400);
/// How long an idle bubble stays up by default before it hides.
pub const HIDE_AFTER: Duration = Duration::from_secs(5);
/// Audio kept before the first speech of a hands-free phrase (16 kHz samples).
pub const PRE_ROLL: usize = 16_000;

pub const TRANSCRIBE_FAILED: &str = "Couldn't transcribe this phrase";
pub const MIC_FAILED: &str = "Microphone unavailable · check the microphone in Settings";
pub const MODEL_FAILED: &str = "Speech model failed to load · open Settings to download it again";

/// What a shortcut does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Action {
    PushToTalk,
    /// Talks into a new session.
    NewSession,
    /// Turns continuous listening on or off.
    HandsFree,
    /// Turns listening on with its first phrase in a new session, or off.
    NewSessionHandsFree,
    Cancel,
    /// Opens the active session in a terminal.
    Terminal,
}

impl Action {
    /// Records one phrase.
    pub fn talks(self) -> bool {
        matches!(self, Action::PushToTalk | Action::NewSession)
    }
}

/// How a shortcut is pressed to fire its action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Gesture {
    Tap,
    Hold,
    DoubleTap,
}

/// A registered key combination, by its index in `Msg::Settings::bindings`.
pub type Combo = usize;

// `Settings` is sent once per save, so its size costs nothing worth boxing it for.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq)]
pub enum Msg {
    ModelReady,
    ModelFailed(String),
    ModelMissing,
    /// A key event with the moment the hotkey saw it, which can be well before it is handled.
    KeyDown(Combo, Instant),
    KeyUp(Combo, Instant),
    Audio {
        op: OpId,
        samples: Vec<f32>,
    },
    SpeechEnded {
        op: OpId,
    },
    /// The voice detector started or stopped hearing speech.
    Speaking {
        op: OpId,
        speaking: bool,
    },
    Live {
        op: PhraseId,
        text: String,
    },
    Transcribed {
        op: PhraseId,
        text: String,
    },
    /// The model's command and rest of the phrase; `None` when it failed.
    Classified {
        op: PhraseId,
        answer: Option<(Option<Command>, String)>,
    },
    Run {
        op: PhraseId,
        event: RunEvent,
    },
    RunExited {
        op: PhraseId,
        end: RunEnd,
        stderr: String,
    },
    /// Hides the bubble if it is still in the idle stretch `rest`;
    /// `None` means the current one.
    Dismiss {
        rest: Option<u64>,
    },
    Settings {
        policy: SessionPolicy,
        recent: Duration,
        cwd: String,
        patterns: Patterns,
        /// Ask the local model when the patterns find no command.
        model_commands: bool,
        /// The agent of new sessions nobody named an agent for.
        agent: Agent,
        /// How long an idle bubble stays up.
        hide_after: Duration,
        /// How soon a second press must follow to make a double-press.
        double: Duration,
        /// The actions of each combination and the gesture that fires each.
        bindings: Vec<Vec<(Action, Gesture)>>,
    },
    /// The double-press window has passed since the tap numbered `seq`.
    GestureTimeout {
        seq: u64,
    },
    /// Makes a session from history the one the next utterance continues.
    SetActive {
        id: Uuid,
        cwd: String,
        agent: Agent,
        /// How long ago the session was last used, for "continue if used recently".
        idle: Duration,
    },
    /// The session was removed from history, so it can no longer be the active one.
    Forget {
        id: Uuid,
    },
    /// The microphone of the capture `op` failed.
    MicFailed {
        op: OpId,
        error: String,
    },
    /// A step of the phrase `op` failed: its transcription or its run.
    Failed {
        op: PhraseId,
        error: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    StartCapture {
        op: OpId,
    },
    /// Send `GestureTimeout { seq }` after `after`.
    GestureTimer {
        seq: u64,
        after: Duration,
    },
    OpenTerminal {
        id: Uuid,
        cwd: String,
        agent: Agent,
    },
    /// Starts an interactive agent in a terminal, with `prompt` as its first message when not empty.
    RunInTerminal {
        session: Session,
        cwd: String,
        prompt: String,
        agent: Agent,
    },
    StopCapture,
    LiveDecode {
        op: PhraseId,
        samples: Vec<f32>,
    },
    Transcribe {
        op: PhraseId,
        samples: Vec<f32>,
    },
    Classify {
        op: PhraseId,
        text: String,
    },
    StartRun {
        op: PhraseId,
        prompt: String,
        session: Session,
        /// Resumed sessions run in their own project folder.
        cwd: String,
        agent: Agent,
    },
    CancelRun,
    OpenSettings,
    ActiveChanged(Option<Uuid>),
    Show(View),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Model {
    Loading,
    Missing,
    Ready,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Mic {
    Off,
    Waiting,
    Listening,
    Error,
}

/// Everything the overlay renders.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    pub series: u64,
    pub visible: bool,
    /// Nothing is being said, transcribed, queued or run.
    pub idle: bool,
    /// Numbers the idle stretches, so a countdown for an earlier one does nothing.
    pub rest: u64,
    /// The countdown before the bubble hides, while idle.
    pub hide_after_ms: Option<u64>,
    pub mic: Mic,
    /// A final transcription is in flight.
    pub transcribing: bool,
    pub phrases: Vec<Phrase>,
    /// The running agent's current step.
    pub detail: String,
    pub agent: Agent,
    pub limited: bool,
    pub session_id: Option<Uuid>,
    pub global_error: Option<String>,
}

/// An open microphone. A hold capture takes its phrase's id as its op.
struct Capture {
    op: OpId,
    /// The combination that opened it and the talk action it serves.
    combo: Combo,
    action: Action,
    hands_free: bool,
    /// Opened by a tap: the next talk gesture sends the phrase.
    latched: bool,
    /// The phrase being said; a hands-free capture has none until speech is heard.
    phrase: Option<PhraseId>,
    buffer: Vec<f32>,
    /// The voice detector has heard speech on this capture.
    heard: bool,
}

pub struct Controller {
    model: Model,
    transformer: Box<dyn PromptTransformer>,
    series: Series,
    capture: Option<Capture>,
    speaking: bool,
    last_live: Option<Instant>,
    live_in_flight: bool,
    /// The one final transcription in flight, and the phrases waiting for the model.
    decoding: Option<PhraseId>,
    waiting: VecDeque<(PhraseId, Vec<f32>)>,
    result: Option<(bool, String)>,
    detail: String,
    run_agent: Agent,
    limited: bool,
    global_error: Option<String>,
    rest: u64,
    was_idle: bool,
    hide_after: Duration,
    double: Duration,
    bindings: Vec<Vec<(Action, Gesture)>>,
    policy: SessionPolicy,
    recent: Duration,
    cwd: String,
    active: Option<Active>,
    running: Option<(Session, String, Agent)>,
    /// The agent of new sessions nobody named an agent for.
    agent: Agent,
    model_commands: bool,
    /// The phrase text while the model looks for a command in it.
    pending: Option<String>,
    parser: Parser,
    /// The combination being held and when it went down.
    held: Option<(Combo, Instant)>,
    /// A tap that may still become a double-tap.
    tap: Option<(Combo, u64)>,
    seq: u64,
    /// The release that ends a double-press is not a tap of its own.
    swallow_up: bool,
}

impl Controller {
    pub fn new(transformer: Box<dyn PromptTransformer>) -> Self {
        Self {
            model: Model::Loading,
            transformer,
            series: Series::default(),
            capture: None,
            speaking: false,
            last_live: None,
            live_in_flight: false,
            decoding: None,
            waiting: VecDeque::new(),
            result: None,
            detail: String::new(),
            run_agent: Agent::Claude,
            limited: false,
            global_error: None,
            rest: 0,
            was_idle: false,
            hide_after: HIDE_AFTER,
            double: DOUBLE,
            bindings: Vec::new(),
            policy: SessionPolicy::default(),
            recent: Duration::ZERO,
            cwd: String::new(),
            active: None,
            running: None,
            agent: Agent::Claude,
            model_commands: false,
            pending: None,
            parser: Parser::new(&Patterns::default()).expect("default patterns compile"),
            held: None,
            tap: None,
            seq: 0,
            swallow_up: false,
        }
    }

    pub fn view(&self) -> View {
        let mic = match (&self.global_error, &self.capture) {
            (Some(_), None) => Mic::Error,
            (_, None) => Mic::Off,
            _ if self.speaking => Mic::Listening,
            _ => Mic::Waiting,
        };
        let phrases = self.series.phrases().to_vec();
        let shown = self.capture.is_some() || !phrases.is_empty() || self.global_error.is_some();
        let visible = matches!(self.model, Model::Ready | Model::Failed) && shown;
        View {
            series: self.series.id(),
            visible,
            idle: self.idle(),
            rest: self.rest,
            hide_after_ms: (visible && self.idle()).then_some(self.hide_after.as_millis() as u64),
            mic,
            transcribing: self
                .decoding
                .is_some_and(|id| self.series.get(id).is_some()),
            phrases,
            detail: self.detail.clone(),
            agent: self.run_agent,
            limited: self.limited,
            session_id: self.active.as_ref().map(|a| a.id),
            global_error: self.global_error.clone(),
        }
    }

    /// Listening mode is a toggle: while it is on, nothing counts down.
    fn idle(&self) -> bool {
        self.capture.is_none() && self.decoding.is_none() && !self.series.active()
    }

    fn show(&mut self) -> Effect {
        let idle = self.idle();
        if idle && !self.was_idle {
            self.rest += 1;
        }
        self.was_idle = idle;
        Effect::Show(self.view())
    }

    fn capturing(&self, op: OpId) -> bool {
        self.capture.as_ref().is_some_and(|c| c.op == op)
    }

    fn agent_phrase(&self, op: PhraseId) -> Option<Status> {
        self.series.agent().filter(|p| p.id == op).map(|p| p.status)
    }

    pub fn handle(&mut self, msg: Msg, now: Instant) -> Vec<Effect> {
        match msg {
            Msg::ModelReady => {
                self.model = Model::Ready;
                self.global_error = None;
                vec![self.show()]
            }
            Msg::ModelFailed(_) => {
                self.model = Model::Failed;
                self.global_error = Some(MODEL_FAILED.into());
                vec![self.show()]
            }
            Msg::ModelMissing => {
                self.model = Model::Missing;
                vec![self.show()]
            }
            Msg::KeyDown(..) if self.model != Model::Ready => match self.model {
                Model::Missing | Model::Failed => vec![Effect::OpenSettings],
                _ => vec![],
            },
            // Hotkeys never auto-repeat, and the release is polled: a second press while still
            // held means the release in between was missed.
            Msg::KeyDown(key, at) if self.held.is_some_and(|(k, _)| k == key) => {
                let mut fx = self.handle(Msg::KeyUp(key, at), now);
                fx.extend(self.handle(Msg::KeyDown(key, at), now));
                fx
            }
            Msg::KeyDown(combo, now) => {
                self.held = Some((combo, now));
                if self.tap.is_some_and(|(c, _)| c == combo) {
                    self.tap = None;
                    self.swallow_up = true;
                    return self.fire(combo, Gesture::DoubleTap, now);
                }
                // A talk action held on this combination records from the first moment.
                let talk = self.bound(combo, Gesture::Hold).find(|a| a.talks());
                match talk {
                    Some(action) if self.capture.is_none() => self.start(action, combo, false, now),
                    _ => vec![],
                }
            }
            Msg::KeyUp(combo, now) => {
                let Some((_, down)) = self.held.filter(|(c, _)| *c == combo) else {
                    return vec![];
                };
                self.held = None;
                if std::mem::take(&mut self.swallow_up) {
                    return vec![];
                }
                let spoke = self.holding(combo).is_some_and(|c| c.heard);
                // A long press is a hold; a short one with speech in it is a quick phrase.
                if now.duration_since(down) >= HOLD || spoke {
                    return self.fire(combo, Gesture::Hold, now);
                }
                // A tap acts at once unless a double-tap on the same combination may follow.
                if self.bound(combo, Gesture::DoubleTap).next().is_none() {
                    return self.fire(combo, Gesture::Tap, now);
                }
                self.seq += 1;
                self.tap = Some((combo, self.seq));
                vec![Effect::GestureTimer {
                    seq: self.seq,
                    after: self.double,
                }]
            }
            Msg::GestureTimeout { seq } => match self.tap.filter(|(_, s)| *s == seq) {
                Some((combo, _)) => {
                    self.tap = None;
                    self.fire(combo, Gesture::Tap, now)
                }
                None => vec![],
            },
            Msg::Audio { op, samples } if self.capturing(op) => self.audio(samples, now),
            Msg::Speaking { op, speaking } if self.capturing(op) => {
                self.speaking = speaking;
                if let Some(c) = &mut self.capture {
                    c.heard |= speaking;
                }
                if let Some(c) = &mut self.capture
                    && speaking
                    && c.hands_free
                    && c.phrase.is_none()
                {
                    let new_session = c.action == Action::NewSession;
                    c.phrase = Some(
                        self.series
                            .start(Kind::Speech, Status::Speaking, new_session),
                    );
                    // Only the first phrase opens the new session; the rest continue it.
                    c.action = Action::PushToTalk;
                }
                vec![self.show()]
            }
            Msg::SpeechEnded { op } if self.capturing(op) => self.cut(),
            Msg::Live { op, text } => {
                self.live_in_flight = false;
                let text = self.transformer.transform(&text);
                match self.series.get_mut(op) {
                    Some(p) if matches!(p.status, Status::Speaking | Status::Transcribing) => {
                        p.text = text;
                        vec![self.show()]
                    }
                    _ => vec![],
                }
            }
            Msg::Transcribed { op, text } => self.transcribed(op, text, now),
            Msg::Classified { op, answer }
                if self.agent_phrase(op) == Some(Status::Classifying) =>
            {
                let Some(text) = self.pending.take() else {
                    return vec![];
                };
                let (commands, rest) = match accept(&text, answer) {
                    Some((command, rest)) => (vec![command], rest),
                    None => (vec![], text),
                };
                let new_session = self.series.get(op).is_some_and(|p| p.new_session);
                let mut fx = self.act(op, &commands, rest, new_session, now);
                fx.extend(self.pump(now));
                fx.push(self.show());
                fx
            }
            Msg::Run { op, event } if self.agent_phrase(op) == Some(Status::Running) => match event
            {
                RunEvent::ToolUse { name } => {
                    self.detail = name;
                    vec![self.show()]
                }
                RunEvent::PermissionDenied { tool } => {
                    self.detail = format!("Permission denied: {tool}");
                    vec![self.show()]
                }
                RunEvent::Result { ok, text } => {
                    self.result = Some((ok, text));
                    vec![]
                }
                RunEvent::Limited => {
                    self.limited = true;
                    vec![self.show()]
                }
                RunEvent::SessionStarted { .. } | RunEvent::Reply { .. } => vec![],
            },
            Msg::RunExited { op, end, stderr } => {
                let Some(status) = self.agent_phrase(op) else {
                    return vec![];
                };
                let result_ok = self.result.as_ref().is_none_or(|(ok, _)| *ok);
                let ok = end == RunEnd::Exited { success: true } && result_ok;
                let mut fx = match self.running.take() {
                    Some((session, cwd, agent)) => {
                        self.remember(session, cwd, agent, &end, ok, now)
                    }
                    None => vec![],
                };
                let outcome = match (&self.result, &end) {
                    (Some((_, text)), _) if !text.is_empty() => text.clone(),
                    (_, RunEnd::TimedOut) => "Timed out".into(),
                    _ => stderr.trim().lines().last().unwrap_or_default().into(),
                };
                let status = match status {
                    Status::Cancelling => Status::Cancelled,
                    _ if ok => Status::Done,
                    _ => Status::Failed,
                };
                self.series.finish(op, status, outcome);
                self.detail.clear();
                fx.extend(self.pump(now));
                fx.push(self.show());
                fx
            }
            Msg::Dismiss { rest } if rest.is_none_or(|r| r == self.rest) && self.idle() => {
                self.series.clear_finished();
                self.global_error = None;
                vec![self.show()]
            }
            Msg::Settings {
                policy,
                recent,
                cwd,
                patterns,
                model_commands,
                agent,
                hide_after,
                double,
                bindings,
            } => {
                self.bindings = bindings;
                self.agent = agent;
                self.hide_after = hide_after;
                self.double = double;
                if let Ok(parser) = Parser::new(&patterns) {
                    self.parser = parser;
                }
                self.model_commands = model_commands;
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
            Msg::SetActive {
                id,
                cwd,
                agent,
                idle,
            } => self.set_active(Some(Active {
                id,
                cwd,
                last_used: now.checked_sub(idle).unwrap_or(now),
                agent,
            })),
            Msg::MicFailed { op, .. } if self.capturing(op) => {
                if let Some(id) = self.capture.take().and_then(|c| c.phrase) {
                    self.series.remove(id);
                }
                self.speaking = false;
                self.global_error = Some(MIC_FAILED.into());
                vec![Effect::StopCapture, self.show()]
            }
            Msg::Failed { op, error } => {
                let mut fx = self.decoded(op);
                match self.series.get(op).map(|p| p.status) {
                    Some(Status::Transcribing) => {
                        self.series
                            .finish(op, Status::Failed, TRANSCRIBE_FAILED.into())
                    }
                    Some(Status::Classifying | Status::Running | Status::Cancelling) => {
                        self.running = None;
                        self.pending = None;
                        self.series.finish(op, Status::Failed, error);
                        fx.extend(self.pump(now));
                    }
                    _ => return fx,
                }
                fx.push(self.show());
                fx
            }
            _ => vec![],
        }
    }

    /// Opens the microphone for `action`: for one phrase, or for listening mode.
    fn start(&mut self, action: Action, combo: Combo, latched: bool, now: Instant) -> Vec<Effect> {
        let hands_free = matches!(action, Action::HandsFree | Action::NewSessionHandsFree);
        // A tap may be about to cancel something, so earlier results stay until the phrase is sent.
        let phrase = (!hands_free).then(|| {
            let id = self
                .series
                .add(Kind::Speech, Status::Speaking, action == Action::NewSession);
            if let Some(p) = self.series.get_mut(id) {
                p.held = true;
            }
            id
        });
        let op = phrase.unwrap_or_else(|| self.series.reserve());
        self.capture = Some(Capture {
            op,
            combo,
            action: match action {
                Action::NewSessionHandsFree => Action::NewSession,
                Action::HandsFree => Action::PushToTalk,
                other => other,
            },
            hands_free,
            latched,
            phrase,
            buffer: Vec::new(),
            heard: false,
        });
        if self.model == Model::Ready {
            self.global_error = None;
        }
        self.speaking = false;
        self.last_live = Some(now);
        self.live_in_flight = false;
        vec![Effect::StartCapture { op }, self.show()]
    }

    /// The actions `combo` fires with `gesture`.
    fn bound(&self, combo: Combo, gesture: Gesture) -> impl Iterator<Item = Action> + '_ {
        self.bindings
            .get(combo)
            .into_iter()
            .flatten()
            .filter(move |(_, g)| *g == gesture)
            .map(|(a, _)| *a)
    }

    /// The recording `combo` is holding open, if any.
    fn holding(&self, combo: Combo) -> Option<&Capture> {
        self.capture
            .as_ref()
            .filter(|c| !c.hands_free && !c.latched && c.combo == combo)
    }

    /// Carries out what `combo` does with `gesture`.
    fn fire(&mut self, combo: Combo, gesture: Gesture, now: Instant) -> Vec<Effect> {
        let actions: Vec<Action> = self.bound(combo, gesture).collect();
        let mut fx = vec![];
        // A press that was not a hold opened a recording for nothing: drop it quietly, unless
        // listening mode takes it over.
        if gesture != Gesture::Hold
            && !actions
                .iter()
                .any(|a| matches!(a, Action::HandsFree | Action::NewSessionHandsFree))
            && self.holding(combo).is_some()
            && let Some(c) = self.capture.take()
        {
            if let Some(id) = c.phrase {
                self.series.remove(id);
            }
            self.speaking = false;
            fx.push(Effect::StopCapture);
        }
        for action in actions {
            fx.extend(match action {
                Action::PushToTalk | Action::NewSession if gesture == Gesture::Hold => {
                    if self.holding(combo).is_some() {
                        self.end_capture()
                    } else {
                        vec![]
                    }
                }
                Action::PushToTalk | Action::NewSession => match &self.capture {
                    Some(c) if c.latched => self.end_capture(),
                    None => self.start(action, combo, true, now),
                    Some(_) => vec![],
                },
                Action::HandsFree | Action::NewSessionHandsFree => {
                    self.toggle_listening(action, combo, now)
                }
                Action::Cancel => self.cancel(),
                Action::Terminal => self.terminal_key(),
            });
        }
        fx.push(self.show());
        fx
    }

    /// Closes the microphone; a phrase that was being said goes to transcription.
    fn end_capture(&mut self) -> Vec<Effect> {
        let Some(capture) = self.capture.take() else {
            return vec![];
        };
        self.speaking = false;
        let mut fx = vec![Effect::StopCapture];
        if let Some(id) = capture.phrase {
            fx.extend(self.transcribe(id, capture.buffer));
        }
        fx.push(self.show());
        fx
    }

    fn audio(&mut self, samples: Vec<f32>, now: Instant) -> Vec<Effect> {
        let Some(capture) = &mut self.capture else {
            return vec![];
        };
        capture.buffer.extend_from_slice(&samples);
        let Some(id) = capture.phrase else {
            let extra = capture.buffer.len().saturating_sub(PRE_ROLL);
            capture.buffer.drain(..extra);
            return vec![];
        };
        if capture.buffer.len() >= MAX_RECORDING {
            return if capture.hands_free {
                self.cut()
            } else {
                self.end_capture()
            };
        }
        let due = self
            .last_live
            .is_none_or(|t| now.duration_since(t) >= LIVE_INTERVAL);
        if !self.speaking || self.live_in_flight || self.decoding.is_some() || !due {
            return vec![];
        }
        self.live_in_flight = true;
        self.last_live = Some(now);
        vec![Effect::LiveDecode {
            op: id,
            samples: capture.buffer.clone(),
        }]
    }

    /// Ends the phrase of a hands-free capture and keeps listening for the next one.
    fn cut(&mut self) -> Vec<Effect> {
        let Some(c) = self.capture.as_mut().filter(|c| c.hands_free) else {
            return vec![];
        };
        let Some(id) = c.phrase.take() else {
            return vec![];
        };
        let samples = std::mem::take(&mut c.buffer);
        // Cut at the length limit mid-speech: the rest of the speech is the next phrase.
        if self.speaking {
            let next = self.series.add(Kind::Speech, Status::Speaking, false);
            c.phrase = Some(next);
        }
        let mut fx = self.transcribe(id, samples);
        fx.push(self.show());
        fx
    }

    /// Sends a phrase to the speech model, or queues it behind the one in flight.
    fn transcribe(&mut self, id: PhraseId, samples: Vec<f32>) -> Vec<Effect> {
        let alone = self
            .series
            .phrases()
            .iter()
            .all(|p| p.id == id || p.status.finished());
        if alone {
            self.series.clear_finished();
        }
        if let Some(p) = self.series.get_mut(id) {
            p.status = Status::Transcribing;
        }
        if self.decoding.is_some() {
            self.waiting.push_back((id, samples));
            return vec![];
        }
        self.decoding = Some(id);
        vec![Effect::Transcribe { op: id, samples }]
    }

    /// The model is free again once the transcription of `op` returns, whatever became of its phrase.
    fn decoded(&mut self, op: PhraseId) -> Vec<Effect> {
        if self.decoding != Some(op) {
            return vec![];
        }
        self.decoding = None;
        match self.waiting.pop_front() {
            Some((id, samples)) => {
                self.decoding = Some(id);
                vec![Effect::Transcribe { op: id, samples }]
            }
            None => vec![],
        }
    }

    fn transcribed(&mut self, op: PhraseId, text: String, now: Instant) -> Vec<Effect> {
        let mut fx = self.decoded(op);
        if self.series.get(op).map(|p| p.status) != Some(Status::Transcribing) {
            return fx;
        }
        let text = self.transformer.transform(&text);
        let (commands, _) = self.parser.parse(&text);
        let held = self.series.get(op).is_some_and(|p| p.held);
        if text.trim().is_empty() && held {
            self.series
                .finish(op, Status::Failed, TRANSCRIBE_FAILED.into());
        } else if text.trim().is_empty() || commands.contains(&Command::Cancel) {
            self.series.remove(op);
        } else if let Some(p) = self.series.get_mut(op) {
            p.text = text;
            p.status = Status::Queued;
            fx.extend(self.pump(now));
        }
        fx.push(self.show());
        fx
    }

    /// Hands queued phrases to the agent while it is free.
    fn pump(&mut self, now: Instant) -> Vec<Effect> {
        let mut fx = vec![];
        while self.series.agent().is_none() {
            let Some(id) = self.series.next_queued() else {
                break;
            };
            let Some(phrase) = self.series.get(id).cloned() else {
                break;
            };
            if phrase.kind == Kind::Terminal {
                fx.extend(self.open_terminal());
                self.series.finish(id, Status::Done, String::new());
                continue;
            }
            let (commands, rest) = self.parser.parse(&phrase.text);
            if commands.is_empty() && self.model_commands && !rest.trim().is_empty() {
                if let Some(p) = self.series.get_mut(id) {
                    p.status = Status::Classifying;
                }
                self.pending = Some(phrase.text.clone());
                fx.push(Effect::Classify {
                    op: id,
                    text: phrase.text,
                });
                break;
            }
            fx.extend(self.act(id, &commands, rest, phrase.new_session, now));
        }
        fx
    }

    fn terminal_key(&mut self) -> Vec<Effect> {
        if self.series.agent().is_none() && self.series.next_queued().is_none() {
            return self.open_terminal();
        }
        let id = self.series.start(Kind::Terminal, Status::Queued, false);
        if let Some(p) = self.series.get_mut(id) {
            p.text = "Open in terminal".into();
        }
        vec![self.show()]
    }

    /// Cancels by priority: the newest phrase being transcribed, then the agent's phrase, and
    /// only then the phrase being said, so a phrase already sent can always be cancelled.
    fn cancel(&mut self) -> Vec<Effect> {
        let mut fx = vec![];
        let transcribing = self
            .series
            .phrases()
            .iter()
            .rev()
            .find(|p| p.status == Status::Transcribing)
            .map(|p| p.id);
        if let Some(id) = transcribing {
            self.series.remove(id);
            self.waiting.retain(|(w, _)| *w != id);
        } else if let Some(agent) = self.series.agent() {
            let id = agent.id;
            match agent.status {
                Status::Classifying => {
                    self.pending = None;
                    self.series.finish(id, Status::Cancelled, String::new());
                    fx.extend(self.pump(Instant::now()));
                }
                Status::Running => {
                    if let Some(p) = self.series.get_mut(id) {
                        p.status = Status::Cancelling;
                    }
                    fx.push(Effect::CancelRun);
                }
                _ => {}
            }
        } else if let Some(c) = &mut self.capture
            && let Some(id) = c.phrase.take()
        {
            c.buffer.clear();
            self.series.remove(id);
            // A one-phrase recording has nothing left to do; listening mode goes on.
            if !c.hands_free {
                self.capture = None;
                self.speaking = false;
                fx.push(Effect::StopCapture);
            }
        }
        fx
    }

    /// Turns listening mode on, taking over a recording in progress, or off.
    fn toggle_listening(&mut self, action: Action, combo: Combo, now: Instant) -> Vec<Effect> {
        match &mut self.capture {
            // In listening mode speech starts phrases; an empty one-phrase recording is dropped.
            Some(c) if !c.hands_free => {
                c.hands_free = true;
                c.latched = false;
                if !self.speaking
                    && let Some(id) = c.phrase.take()
                {
                    self.series.remove(id);
                }
                vec![]
            }
            Some(_) => self.end_capture(),
            None => self.start(action, combo, false, now),
        }
    }

    /// Updates the active session after a run. A run that produced a result proves its session
    /// exists; a resume that failed without one most likely points at a session Claude no longer has.
    fn remember(
        &mut self,
        session: Session,
        cwd: String,
        agent: Agent,
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
                agent,
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
        op: PhraseId,
        new: bool,
        spoken: Option<Agent>,
        prompt: String,
        now: Instant,
    ) -> Vec<Effect> {
        let (session, cwd, _, agent) = self.target(new, spoken, now);
        self.running = Some((session, cwd.clone(), agent));
        self.result = None;
        self.run_agent = agent;
        self.limited = false;
        self.detail.clear();
        if let Some(p) = self.series.get_mut(op) {
            p.status = Status::Running;
            p.text = prompt.clone();
        }
        vec![Effect::StartRun {
            op,
            prompt,
            session,
            cwd,
            agent,
        }]
    }

    /// Carries out `commands` and sends the rest of the phrase, if any, to the agent: in the
    /// background, or in a terminal when "open in terminal" is among them. A phrase with nothing
    /// left for the background agent leaves the series.
    fn act(
        &mut self,
        op: PhraseId,
        commands: &[Command],
        rest: String,
        new_session: bool,
        now: Instant,
    ) -> Vec<Effect> {
        let has = |c: Command| commands.contains(&c);
        let new = has(Command::NewSession) || new_session;
        let spoken = commands.iter().find_map(|c| c.agent());
        let prompt = if has(Command::Cancel) {
            String::new()
        } else {
            rest.trim().to_string()
        };
        let mut fx = vec![];
        let terminal = has(Command::OpenTerminal) && !has(Command::Cancel);
        if terminal && prompt.is_empty() && !new && spoken.is_none() {
            fx.extend(self.open_terminal());
        } else if terminal {
            let (session, cwd, _, agent) = self.target(new, spoken, now);
            fx.push(Effect::RunInTerminal {
                session,
                cwd: cwd.clone(),
                prompt,
                agent,
            });
            fx.extend(self.set_active(Some(Active {
                id: session_id(session),
                cwd,
                last_used: now,
                agent,
            })));
            self.series.remove(op);
            return fx;
        }
        if prompt.is_empty() {
            self.series.remove(op);
            return fx;
        }
        fx.extend(self.start_run(op, new, spoken, prompt, now));
        fx
    }

    /// The session a phrase goes to, its folder, whether it continues an earlier one, and its
    /// agent. A spoken agent always starts a new session with that agent.
    fn target(
        &self,
        new: bool,
        spoken: Option<Agent>,
        now: Instant,
    ) -> (Session, String, bool, Agent) {
        let new = new || spoken.is_some();
        let resume = choose(self.policy, self.recent, new, self.active.as_ref(), now);
        match (resume, &self.active) {
            (Some(id), Some(active)) => {
                (Session::Resume(id), active.cwd.clone(), true, active.agent)
            }
            _ => (
                Session::New(Uuid::new_v4()),
                self.cwd.clone(),
                false,
                spoken.unwrap_or(self.agent),
            ),
        }
    }

    fn open_terminal(&self) -> Vec<Effect> {
        match &self.active {
            Some(active) => vec![Effect::OpenTerminal {
                id: active.id,
                cwd: active.cwd.clone(),
                agent: active.agent,
            }],
            None => vec![],
        }
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
    use crate::agent::Agent;
    use crate::prompt::Dictionary;

    /// The test shortcuts, by index into `bindings()`.
    #[allow(non_snake_case, non_upper_case_globals)]
    mod Key {
        use super::Combo;
        pub const Talk: Combo = 0;
        pub const NewSession: Combo = 1;
        pub const Terminal: Combo = 2;
        pub const HandsFree: Combo = 3;
        pub const NewSessionHandsFree: Combo = 4;
    }

    fn bindings() -> Vec<Vec<(Action, Gesture)>> {
        vec![
            vec![
                (Action::PushToTalk, Gesture::Hold),
                (Action::Cancel, Gesture::Tap),
            ],
            vec![(Action::NewSession, Gesture::Hold)],
            vec![(Action::Terminal, Gesture::Tap)],
            vec![(Action::HandsFree, Gesture::DoubleTap)],
            vec![(Action::NewSessionHandsFree, Gesture::DoubleTap)],
        ]
    }

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

        fn down(&mut self, key: Combo) -> Vec<Effect> {
            self.send(Msg::KeyDown(key, self.now))
        }

        fn up(&mut self, key: Combo) -> Vec<Effect> {
            self.send(Msg::KeyUp(key, self.now))
        }

        /// The newest phrase.
        fn op(&self) -> OpId {
            self.c.series.phrases().last().map_or(0, |p| p.id)
        }

        fn state(&self) -> S {
            st(&self.c.view())
        }

        fn statuses(&self) -> Vec<Status> {
            self.c.series.phrases().iter().map(|p| p.status).collect()
        }

        fn hands_free_on(&self) -> bool {
            self.c.capture.as_ref().is_some_and(|c| c.hands_free)
        }

        fn phrase(&self, id: PhraseId) -> Phrase {
            self.c.series.get(id).cloned().expect("phrase")
        }

        /// Holds the key, says `text` and returns the phrase.
        fn say(&mut self, text: &str) -> PhraseId {
            let op = self.listen(Key::Talk);
            self.release(Key::Talk);
            self.send(Msg::Transcribed {
                op,
                text: text.into(),
            });
            self.now += Duration::from_millis(10);
            op
        }

        fn finish_run(&mut self, op: PhraseId, ok: bool) -> Vec<Effect> {
            self.send(Msg::RunExited {
                op,
                end: RunEnd::Exited { success: ok },
                stderr: if ok { String::new() } else { "boom".into() },
            })
        }

        fn listen(&mut self, key: Combo) -> OpId {
            self.down(key);
            self.op()
        }

        /// Ends a hold that has lasted long enough to count as one.
        fn release(&mut self, key: Combo) -> Vec<Effect> {
            self.now += HOLD;
            self.up(key)
        }

        fn quick(&mut self, key: Combo) -> Vec<Effect> {
            let mut fx = self.down(key);
            self.now += Duration::from_millis(50);
            fx.extend(self.up(key));
            self.now += Duration::from_millis(50);
            fx
        }

        /// A single press, confirmed once the double-tap window has passed if one applies.
        fn tap(&mut self, key: Combo) -> Vec<Effect> {
            let mut fx = self.quick(key);
            let seq = fx.iter().find_map(|e| match e {
                Effect::GestureTimer { seq, .. } => Some(*seq),
                _ => None,
            });
            if let Some(seq) = seq {
                self.now += DOUBLE;
                fx.extend(self.send(Msg::GestureTimeout { seq }));
            }
            fx
        }

        fn double(&mut self, key: Combo) -> Vec<Effect> {
            let mut fx = self.quick(key);
            fx.extend(self.quick(key));
            fx
        }

        /// Turns listening on with the hands-free shortcut and returns the capture op.
        fn hands_free(&mut self, _key: Combo) -> OpId {
            self.double(Key::HandsFree);
            self.c.capture.as_ref().map_or(0, |c| c.op)
        }

        fn run(&mut self) -> OpId {
            self.run_saying("проверь, что клод видит diff")
        }

        /// Runs `text` to completion and returns the session it used.
        fn finish_saying(&mut self, text: &str) -> Session {
            let op = self.listen(Key::Talk);
            self.release(Key::Talk);
            let mut fx = self.send(Msg::Transcribed {
                op,
                text: text.into(),
            });
            if let Some(Effect::Classify { op, .. }) = fx.first() {
                let op = *op;
                fx = self.send(Msg::Classified { op, answer: None });
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

    fn settings_with(bindings: Vec<Vec<(Action, Gesture)>>) -> Msg {
        match settings(SessionPolicy::Continue, "C:/p") {
            Msg::Settings {
                policy,
                recent,
                cwd,
                patterns,
                model_commands,
                agent,
                hide_after,
                double,
                ..
            } => Msg::Settings {
                policy,
                recent,
                cwd,
                patterns,
                model_commands,
                agent,
                hide_after,
                double,
                bindings,
            },
            _ => unreachable!(),
        }
    }

    fn settings(policy: SessionPolicy, cwd: &str) -> Msg {
        Msg::Settings {
            policy,
            recent: Duration::from_secs(600),
            cwd: cwd.into(),
            patterns: Patterns::default(),
            model_commands: false,
            agent: Agent::Claude,
            hide_after: HIDE_AFTER,
            double: DOUBLE,
            bindings: bindings(),
        }
    }

    fn id(session: Session) -> Uuid {
        match session {
            Session::New(id) | Session::Resume(id) => id,
        }
    }

    /// The old single state, read from the view.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum S {
        Idle,
        Listening,
        Transcribing,
        Queued,
        Classifying,
        Running,
        Cancelling,
        Succeeded,
        Failed,
    }

    fn st(v: &View) -> S {
        if matches!(v.mic, Mic::Waiting | Mic::Listening) {
            return S::Listening;
        }
        let active = v.phrases.iter().rev().find(|p| !p.status.finished());
        match active.or(v.phrases.last()).map(|p| p.status) {
            Some(Status::Speaking) => S::Listening,
            Some(Status::Transcribing) => S::Transcribing,
            Some(Status::Queued) => S::Queued,
            Some(Status::Classifying) => S::Classifying,
            Some(Status::Running) => S::Running,
            Some(Status::Cancelling) => S::Cancelling,
            Some(Status::Done) => S::Succeeded,
            Some(Status::Failed) => S::Failed,
            Some(Status::Cancelled) | None => S::Idle,
        }
    }

    fn text(v: &View) -> &str {
        v.phrases.last().map_or("", |p| p.text.as_str())
    }

    fn outcome(v: &View) -> &str {
        v.phrases.last().map_or("", |p| p.outcome.as_str())
    }

    fn shown(effects: &[Effect]) -> Option<&View> {
        effects.iter().rev().find_map(|e| match e {
            Effect::Show(v) => Some(v),
            _ => None,
        })
    }

    fn say_while_busy(t: &mut T, text: &str) -> Vec<Effect> {
        let op = t.listen(Key::Talk);
        t.release(Key::Talk);
        t.send(Msg::Transcribed {
            op,
            text: text.into(),
        })
    }

    #[test]
    fn a_phrase_said_during_a_run_waits_in_the_queue() {
        let mut t = T::new();
        t.say("проверь diff");
        let fx = say_while_busy(&mut t, "потом тесты");
        assert_eq!(t.statuses(), [Status::Running, Status::Queued]);
        assert!(!fx.iter().any(|e| matches!(e, Effect::StartRun { .. })));
        assert_eq!(t.phrase(t.op()).text, "потом тесты");
    }

    #[test]
    fn queued_phrases_run_one_by_one_in_order() {
        let mut t = T::new();
        let a = t.say("проверь diff");
        t.say("потом тесты");
        let fx = t.finish_run(a, true);
        assert_eq!(t.statuses(), [Status::Done, Status::Running]);
        assert!(
            fx.iter()
                .any(|e| matches!(e, Effect::StartRun { prompt, .. } if prompt == "потом тесты"))
        );
    }

    #[test]
    fn after_a_failed_run_the_next_phrase_starts() {
        let mut t = T::new();
        let a = t.say("проверь diff");
        t.say("потом тесты");
        t.finish_run(a, false);
        assert_eq!(t.statuses(), [Status::Failed, Status::Running]);
        assert_eq!(t.phrase(a).outcome, "boom");
    }

    #[test]
    fn single_press_cancels_the_run_and_the_next_starts() {
        let mut t = T::new();
        let a = t.say("проверь diff");
        t.say("потом тесты");
        let fx = t.tap(Key::Talk);
        assert!(fx.contains(&Effect::CancelRun));
        assert_eq!(t.statuses(), [Status::Cancelling, Status::Queued]);
        t.send(Msg::RunExited {
            op: a,
            end: RunEnd::Cancelled,
            stderr: String::new(),
        });
        assert_eq!(t.statuses(), [Status::Cancelled, Status::Running]);
    }

    #[test]
    fn a_tap_during_a_run_is_not_taken_by_its_own_capture() {
        let mut t = T::new();
        t.say("проверь diff");
        let fx = t.tap(Key::Talk);
        assert!(fx.contains(&Effect::CancelRun));
        assert!(!fx.iter().any(|e| matches!(e, Effect::Transcribe { .. })));
        assert_eq!(t.c.view().mic, Mic::Off);
        assert_eq!(t.statuses(), [Status::Cancelling]);
    }

    #[test]
    fn single_press_with_nothing_running_does_nothing() {
        let mut t = T::new();
        let fx = t.tap(Key::Talk);
        assert!(fx.iter().all(|e| matches!(
            e,
            Effect::StartCapture { .. }
                | Effect::StopCapture
                | Effect::GestureTimer { .. }
                | Effect::Show(_)
        )));
        assert!(t.statuses().is_empty());
    }

    #[test]
    fn commands_apply_when_the_phrase_leaves_the_queue() {
        let mut t = T::new();
        let first = t.finish_saying("проверь diff");
        t.now += Duration::from_secs(1);
        let a = t.say("проверь тесты");
        t.say("новая сессия проверь lint");
        let fx = t.finish_run(a, true);
        let Some(Effect::StartRun { session, .. }) =
            fx.iter().find(|e| matches!(e, Effect::StartRun { .. }))
        else {
            panic!("{fx:?}")
        };
        assert!(matches!(session, Session::New(_)));
        assert_ne!(id(*session), id(first));
    }

    #[test]
    fn terminal_key_during_a_run_waits_in_the_queue() {
        let mut t = T::new();
        let session = t.finish_saying("проверь diff");
        t.now += Duration::from_secs(1);
        let a = t.say("проверь тесты");
        t.down(Key::Terminal);
        let fx = t.up(Key::Terminal);
        assert!(!fx.iter().any(|e| matches!(e, Effect::OpenTerminal { .. })));
        assert_eq!(t.statuses(), [Status::Running, Status::Queued]);
        let fx = t.finish_run(a, true);
        assert!(fx.contains(&Effect::OpenTerminal {
            id: id(session),
            cwd: "C:/p".into(),
            agent: Agent::Claude,
        }));
        assert!(t.statuses().iter().all(|s| s.finished()));
    }

    #[test]
    fn a_stale_run_end_changes_nothing() {
        let mut t = T::new();
        let a = t.say("проверь diff");
        t.say("потом тесты");
        t.finish_run(a, true);
        assert_eq!(t.finish_run(a, false), []);
        assert_eq!(t.statuses(), [Status::Done, Status::Running]);
    }

    #[test]
    fn a_transcription_result_with_a_run_op_is_ignored() {
        let mut t = T::new();
        let a = t.say("проверь diff");
        let fx = t.send(Msg::Transcribed {
            op: a,
            text: "другое".into(),
        });
        assert_eq!(fx, []);
        assert_eq!(t.phrase(a).status, Status::Running);
    }

    #[test]
    fn succeeded_does_not_wait_for_dismiss() {
        let mut t = T::new();
        let a = t.say("проверь diff");
        t.finish_run(a, true);
        let fx = say_while_busy(&mut t, "потом тесты");
        assert!(fx.iter().any(|e| matches!(e, Effect::StartRun { .. })));
    }

    #[test]
    fn dismiss_clears_an_idle_series() {
        let mut t = T::new();
        let a = t.say("проверь diff");
        let fx = t.finish_run(a, true);
        let rest = shown(&fx).unwrap().rest;
        assert_eq!(
            t.send(Msg::Dismiss {
                rest: Some(rest + 1)
            }),
            [],
            "stale"
        );
        let fx = t.send(Msg::Dismiss { rest: Some(rest) });
        let v = shown(&fx).unwrap();
        assert!(v.phrases.is_empty());
        assert!(!v.visible);
    }

    /// Speech on the hands-free capture `cap`; returns the phrase it started.
    fn speak(t: &mut T, cap: OpId, samples: usize) -> PhraseId {
        t.send(Msg::Speaking {
            op: cap,
            speaking: true,
        });
        t.send(Msg::Audio {
            op: cap,
            samples: vec![0.1; samples],
        });
        t.op()
    }

    fn pause(t: &mut T, cap: OpId) -> Vec<Effect> {
        t.send(Msg::Speaking {
            op: cap,
            speaking: false,
        });
        t.send(Msg::SpeechEnded { op: cap })
    }

    fn transcribes(fx: &[Effect], id: PhraseId) -> bool {
        fx.iter()
            .any(|e| matches!(e, Effect::Transcribe { op, .. } if *op == id))
    }

    #[test]
    fn double_press_turns_listening_on_and_off() {
        let mut t = T::new();
        t.double(Key::HandsFree);
        assert!(t.hands_free_on());
        t.now += Duration::from_secs(1);
        let fx = t.double(Key::HandsFree);
        assert!(fx.contains(&Effect::StopCapture));
        assert_eq!(t.c.view().mic, Mic::Off);
    }

    #[test]
    fn a_pause_sends_the_phrase_and_listening_goes_on() {
        let mut t = T::new();
        let cap = t.hands_free(Key::Talk);
        let p = speak(&mut t, cap, 1600);
        let fx = pause(&mut t, cap);
        assert!(transcribes(&fx, p));
        assert!(!fx.contains(&Effect::StopCapture));
        assert_eq!(t.statuses(), [Status::Transcribing]);
        speak(&mut t, cap, 160);
        assert_eq!(t.statuses(), [Status::Transcribing, Status::Speaking]);
    }

    #[test]
    fn a_phrase_finished_while_the_agent_runs_joins_the_queue() {
        let mut t = T::new();
        t.say("проверь diff");
        let cap = t.hands_free(Key::Talk);
        let p = speak(&mut t, cap, 1600);
        pause(&mut t, cap);
        t.send(Msg::Transcribed {
            op: p,
            text: "потом тесты".into(),
        });
        assert_eq!(t.statuses(), [Status::Running, Status::Queued]);
    }

    #[test]
    fn transcriptions_run_one_at_a_time_in_order() {
        let mut t = T::new();
        let cap = t.hands_free(Key::Talk);
        let p1 = speak(&mut t, cap, 1600);
        assert!(transcribes(&pause(&mut t, cap), p1));
        let p2 = speak(&mut t, cap, 1600);
        assert!(!transcribes(&pause(&mut t, cap), p2));
        let fx = t.send(Msg::Transcribed {
            op: p1,
            text: "проверь diff".into(),
        });
        assert!(transcribes(&fx, p2));
    }

    #[test]
    fn a_dropped_phrase_still_lets_the_next_transcription_start() {
        let mut t = T::new();
        t.say("проверь diff");
        let cap = t.hands_free(Key::Talk);
        let p1 = speak(&mut t, cap, 1600);
        pause(&mut t, cap);
        t.tap(Key::Talk);
        assert_eq!(t.statuses(), [Status::Running]);
        let p2 = speak(&mut t, cap, 1600);
        assert!(!transcribes(&pause(&mut t, cap), p2));
        let fx = t.send(Msg::Transcribed {
            op: p1,
            text: "проверь diff".into(),
        });
        assert!(transcribes(&fx, p2));
        assert_eq!(t.statuses(), [Status::Running, Status::Transcribing]);
    }

    #[test]
    fn single_press_drops_the_phrase_being_spoken() {
        let mut t = T::new();
        let cap = t.hands_free(Key::Talk);
        speak(&mut t, cap, 1600);
        let fx = t.tap(Key::Talk);
        assert!(!fx.contains(&Effect::StopCapture));
        assert!(t.statuses().is_empty());
        assert_ne!(t.c.view().mic, Mic::Off);
    }

    #[test]
    fn single_press_cancels_the_sent_phrase_before_the_one_being_spoken() {
        let mut t = T::new();
        t.say("проверь diff");
        let cap = t.hands_free(Key::Talk);
        speak(&mut t, cap, 1600);
        let fx = t.tap(Key::Talk);
        assert!(fx.contains(&Effect::CancelRun));
        assert_eq!(t.statuses(), [Status::Cancelling, Status::Speaking]);
    }

    #[test]
    fn silence_before_speech_keeps_only_the_pre_roll() {
        let mut t = T::new();
        let cap = t.hands_free(Key::Talk);
        t.send(Msg::Audio {
            op: cap,
            samples: vec![0.0; 3 * PRE_ROLL],
        });
        speak(&mut t, cap, 1000);
        let fx = pause(&mut t, cap);
        let Some(Effect::Transcribe { samples, .. }) =
            fx.iter().find(|e| matches!(e, Effect::Transcribe { .. }))
        else {
            panic!("{fx:?}")
        };
        assert_eq!(samples.len(), PRE_ROLL + 1000);
    }

    #[test]
    fn holding_the_key_while_listening_does_nothing() {
        let mut t = T::new();
        t.hands_free(Key::Talk);
        t.now += Duration::from_secs(1);
        let cap = t.c.capture.as_ref().map_or(0, |c| c.op);
        speak(&mut t, cap, 1600);
        let before = t.statuses();
        let mut fx = t.down(Key::Talk);
        fx.extend(t.release(Key::Talk));
        let timers: Vec<u64> = fx
            .iter()
            .filter_map(|e| match e {
                Effect::GestureTimer { seq, .. } => Some(*seq),
                _ => None,
            })
            .collect();
        t.now += DOUBLE;
        for seq in timers {
            fx.extend(t.send(Msg::GestureTimeout { seq }));
        }
        assert!(!fx.iter().any(|e| matches!(
            e,
            Effect::StartCapture { .. }
                | Effect::StopCapture
                | Effect::Transcribe { .. }
                | Effect::CancelRun
        )));
        assert!(t.hands_free_on());
        assert_eq!(t.statuses(), before);
    }

    #[test]
    fn a_tap_with_nothing_running_keeps_the_results() {
        let mut t = T::new();
        let a = t.say("проверь diff");
        t.finish_run(a, true);
        t.tap(Key::Talk);
        assert_eq!(t.statuses(), [Status::Done]);
    }

    #[test]
    fn speech_under_way_when_listening_turns_on_is_kept() {
        let mut t = T::new();
        t.down(Key::Talk);
        let cap = t.c.capture.as_ref().map_or(0, |c| c.op);
        t.send(Msg::Speaking {
            op: cap,
            speaking: true,
        });
        t.double(Key::HandsFree);
        assert!(t.hands_free_on());
        let fx = pause(&mut t, cap);
        assert!(fx.iter().any(|e| matches!(e, Effect::Transcribe { .. })));
    }

    #[test]
    fn turning_listening_off_leaves_the_run_and_queue() {
        let mut t = T::new();
        t.say("проверь diff");
        t.say("потом тесты");
        let cap = t.hands_free(Key::Talk);
        let p = speak(&mut t, cap, 1600);
        t.now += Duration::from_secs(1);
        let fx = t.double(Key::HandsFree);
        assert!(!fx.contains(&Effect::CancelRun));
        assert!(fx.contains(&Effect::StopCapture));
        assert!(transcribes(&fx, p));
        assert_eq!(
            t.statuses(),
            [Status::Running, Status::Queued, Status::Transcribing]
        );
    }

    #[test]
    fn turning_listening_off_without_speech_drops_nothing_else() {
        let mut t = T::new();
        t.say("проверь diff");
        t.hands_free(Key::Talk);
        t.now += Duration::from_secs(1);
        t.double(Key::HandsFree);
        assert_eq!(t.statuses(), [Status::Running]);
        assert_eq!(t.c.view().mic, Mic::Off);
    }

    #[test]
    fn live_decode_is_for_the_phrase_being_spoken_and_waits_for_a_final_one() {
        let mut t = T::new();
        let cap = t.hands_free(Key::Talk);
        let p1 = speak(&mut t, cap, 160);
        t.now += LIVE_INTERVAL;
        let fx = t.send(Msg::Audio {
            op: cap,
            samples: vec![0.1; 160],
        });
        assert!(
            fx.iter()
                .any(|e| matches!(e, Effect::LiveDecode { op, .. } if *op == p1))
        );
        t.send(Msg::Live {
            op: p1,
            text: "проверь".into(),
        });
        pause(&mut t, cap);
        speak(&mut t, cap, 160);
        t.now += LIVE_INTERVAL;
        let fx = t.send(Msg::Audio {
            op: cap,
            samples: vec![0.1; 160],
        });
        assert!(!fx.iter().any(|e| matches!(e, Effect::LiveDecode { .. })));
    }

    #[test]
    fn microphone_failure_turns_listening_off_only() {
        let mut t = T::new();
        t.say("проверь diff");
        let cap = t.hands_free(Key::Talk);
        let fx = t.send(Msg::MicFailed {
            op: cap,
            error: "gone".into(),
        });
        assert!(fx.contains(&Effect::StopCapture));
        let v = shown(&fx).unwrap();
        assert_eq!(v.mic, Mic::Error);
        assert_eq!(v.global_error.as_deref(), Some(MIC_FAILED));
        assert_eq!(t.statuses(), [Status::Running]);
    }

    #[test]
    fn listening_mode_never_counts_down() {
        let mut t = T::new();
        t.hands_free(Key::Talk);
        let v = t.c.view();
        assert!(!v.idle);
        assert_eq!(v.hide_after_ms, None);
        assert_eq!(t.send(Msg::Dismiss { rest: Some(v.rest) }), []);
        assert!(t.hands_free_on());
    }

    #[test]
    fn the_countdown_starts_once_listening_is_off() {
        let mut t = T::new();
        let a = t.say("проверь diff");
        t.finish_run(a, true);
        t.hands_free(Key::Talk);
        assert_eq!(t.c.view().hide_after_ms, None);
        t.now += Duration::from_secs(1);
        let fx = t.double(Key::HandsFree);
        let v = shown(&fx).unwrap();
        assert_eq!(v.mic, Mic::Off);
        assert_eq!(v.hide_after_ms, Some(HIDE_AFTER.as_millis() as u64));
    }

    #[test]
    fn speech_during_the_countdown_starts_a_new_one() {
        let mut t = T::new();
        let a = t.say("проверь diff");
        t.finish_run(a, true);
        let first = t.c.view().rest;
        assert!(t.c.view().idle);
        t.listen(Key::Talk);
        assert_eq!(t.c.view().hide_after_ms, None);
        assert_eq!(
            t.send(Msg::Dismiss { rest: Some(first) }),
            [],
            "old countdown"
        );
        t.release(Key::Talk);
        let p = t.op();
        t.send(Msg::Transcribed {
            op: p,
            text: String::new(),
        });
        let v = t.c.view();
        assert!(v.idle);
        assert_ne!(v.rest, first);
    }

    #[test]
    fn a_long_phrase_in_listening_mode_goes_on_after_the_cut() {
        let mut t = T::new();
        let cap = t.hands_free(Key::Talk);
        speak(&mut t, cap, 1600);
        let fx = t.send(Msg::Audio {
            op: cap,
            samples: vec![0.1; MAX_RECORDING],
        });
        assert!(fx.iter().any(|e| matches!(e, Effect::Transcribe { .. })));
        assert_eq!(t.statuses(), [Status::Transcribing, Status::Speaking]);
    }

    #[test]
    fn a_dropped_phrase_is_not_shown_as_transcribing() {
        let mut t = T::new();
        t.say("проверь diff");
        let cap = t.hands_free(Key::Talk);
        speak(&mut t, cap, 1600);
        pause(&mut t, cap);
        assert!(t.c.view().transcribing);
        t.tap(Key::Talk);
        assert!(!t.c.view().transcribing);
    }

    #[test]
    fn a_deliberate_press_during_a_run_cancels() {
        let mut t = T::new();
        t.say("проверь diff");
        t.down(Key::Talk);
        t.now += Duration::from_millis(400);
        let fx = t.up(Key::Talk);
        assert!(fx.contains(&Effect::CancelRun));
    }

    #[test]
    fn a_quick_word_while_holding_is_sent() {
        let mut t = T::new();
        let op = t.listen(Key::Talk);
        t.send(Msg::Speaking { op, speaking: true });
        t.now += Duration::from_millis(250);
        let fx = t.up(Key::Talk);
        assert!(fx.iter().any(|e| matches!(e, Effect::Transcribe { .. })));
    }

    #[test]
    fn a_held_phrase_that_came_back_empty_is_shown_not_dropped() {
        let mut t = T::new();
        let op = t.listen(Key::Talk);
        t.release(Key::Talk);
        t.send(Msg::Transcribed {
            op,
            text: String::new(),
        });
        assert_eq!(t.statuses(), [Status::Failed]);
        assert_eq!(t.phrase(op).outcome, TRANSCRIBE_FAILED);
    }

    #[test]
    fn noise_in_listening_mode_is_ignored() {
        let mut t = T::new();
        let cap = t.hands_free(Key::Talk);
        let p = speak(&mut t, cap, 1600);
        pause(&mut t, cap);
        t.send(Msg::Transcribed {
            op: p,
            text: String::new(),
        });
        assert!(t.statuses().is_empty());
    }

    #[test]
    fn the_double_press_window_comes_from_settings() {
        let mut t = T::new();
        t.send(Msg::Settings {
            policy: SessionPolicy::Continue,
            recent: Duration::from_secs(600),
            cwd: "C:/p".into(),
            patterns: Patterns::default(),
            model_commands: false,
            agent: Agent::Claude,
            hide_after: HIDE_AFTER,
            double: Duration::from_millis(700),
            bindings: bindings(),
        });
        let fx = t.quick(Key::HandsFree);
        assert!(fx.contains(&Effect::GestureTimer {
            seq: 1,
            after: Duration::from_millis(700)
        }));
    }

    #[test]
    fn cancel_is_instant_when_no_double_tap_shares_its_shortcut() {
        let mut t = T::new();
        t.say("проверь diff");
        let fx = t.quick(Key::Talk);
        assert!(fx.contains(&Effect::CancelRun));
        assert!(!fx.iter().any(|e| matches!(e, Effect::GestureTimer { .. })));
    }

    #[test]
    fn the_hands_free_shortcut_turns_listening_on_and_off() {
        let mut t = T::new();
        let fx = t.double(Key::HandsFree);
        assert!(fx.iter().any(|e| matches!(e, Effect::StartCapture { .. })));
        assert!(t.hands_free_on());
        t.now += Duration::from_secs(1);
        let fx = t.double(Key::HandsFree);
        assert!(fx.contains(&Effect::StopCapture));
        assert_eq!(t.c.view().mic, Mic::Off);
    }

    #[test]
    fn a_double_tap_on_the_talk_shortcut_is_not_hands_free() {
        let mut t = T::new();
        t.double(Key::Talk);
        assert!(!t.hands_free_on());
    }

    #[test]
    fn push_to_talk_in_tap_mode_starts_and_sends_a_phrase() {
        let mut t = T::new();
        let mut b = bindings();
        b[Key::Talk] = vec![(Action::PushToTalk, Gesture::Tap)];
        t.send(settings_with(b));
        let fx = t.quick(Key::Talk);
        assert!(fx.iter().any(|e| matches!(e, Effect::StartCapture { .. })));
        let fx = t.quick(Key::Talk);
        assert!(fx.iter().any(|e| matches!(e, Effect::Transcribe { .. })));
    }

    #[test]
    fn the_terminal_shortcut_opens_on_its_gesture() {
        let mut t = T::new();
        let session = t.finish_saying("проверь diff");
        assert_eq!(t.down(Key::Terminal), []);
        let fx = t.up(Key::Terminal);
        assert!(fx.contains(&Effect::OpenTerminal {
            id: id(session),
            cwd: "C:/p".into(),
            agent: Agent::Claude,
        }));
    }

    #[test]
    fn hands_free_into_a_new_session_starts_one_session() {
        let mut t = T::new();
        let first = t.finish_saying("проверь diff");
        t.now += Duration::from_secs(1);
        t.double(Key::NewSessionHandsFree);
        assert!(t.hands_free_on());
        let cap = t.c.capture.as_ref().map_or(0, |c| c.op);
        let p1 = speak(&mut t, cap, 1600);
        pause(&mut t, cap);
        let fx = t.send(Msg::Transcribed {
            op: p1,
            text: "найди баг".into(),
        });
        let Some(Effect::StartRun { session, .. }) = fx
            .iter()
            .find(|e| matches!(e, Effect::StartRun { .. }))
            .cloned()
        else {
            panic!("{fx:?}")
        };
        assert!(matches!(session, Session::New(new) if new != id(first)));
        t.send(Msg::Run {
            op: p1,
            event: RunEvent::Result {
                ok: true,
                text: "done".into(),
            },
        });
        t.finish_run(p1, true);
        let p2 = speak(&mut t, cap, 1600);
        pause(&mut t, cap);
        let fx = t.send(Msg::Transcribed {
            op: p2,
            text: "теперь почини".into(),
        });
        assert!(fx.iter().any(|e| matches!(
            e,
            Effect::StartRun { session: Session::Resume(r), .. } if *r == id(session)
        )));
    }

    #[test]
    fn keys_before_model_ready_are_ignored() {
        let mut c = Controller::new(Box::new(Dictionary::default()));
        assert_eq!(
            c.handle(Msg::KeyDown(Key::Talk, Instant::now()), Instant::now()),
            []
        );
    }

    #[test]
    fn model_failure_is_shown() {
        let mut c = Controller::new(Box::new(Dictionary::default()));
        let fx = c.handle(Msg::ModelFailed("no model".into()), Instant::now());
        let v = shown(&fx).unwrap();
        assert_eq!(v.mic, Mic::Error);
        assert_eq!(v.global_error.as_deref(), Some(MODEL_FAILED));
    }

    #[test]
    fn hold_records_until_release_then_transcribes() {
        let mut t = T::new();
        let fx = t.down(Key::Talk);
        let op = t.op();
        assert_eq!(fx[0], Effect::StartCapture { op });
        assert_eq!(st(shown(&fx).unwrap()), S::Listening);

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
        assert_eq!(st(shown(&fx).unwrap()), S::Transcribing);
    }

    #[test]
    fn quick_tap_from_idle_drops_the_recording() {
        let mut t = T::new();
        let fx = t.tap(Key::Talk);
        assert!(fx.contains(&Effect::StopCapture));
        assert!(!fx.iter().any(|e| matches!(e, Effect::Transcribe { .. })));
        assert_eq!(t.state(), S::Idle);
    }

    #[test]
    fn double_press_goes_hands_free() {
        let mut t = T::new();
        let fx = t.double(Key::HandsFree);
        assert!(!fx.contains(&Effect::StopCapture));
        assert_eq!(t.state(), S::Listening);
        t.now += Duration::from_secs(2);
        t.send(Msg::GestureTimeout { seq: 1 });
        assert_eq!(t.state(), S::Listening, "the first tap's timer is spent");
    }

    #[test]
    fn double_press_counts_when_keys_were_seen_not_when_handled() {
        let mut t = T::new();
        let seen = t.now;
        t.down(Key::HandsFree);
        // Opening the microphone held up the queue; both events are handled only now.
        t.now += Duration::from_millis(600);
        t.send(Msg::KeyUp(Key::HandsFree, seen + Duration::from_millis(80)));
        t.send(Msg::KeyDown(
            Key::HandsFree,
            seen + Duration::from_millis(200),
        ));
        assert_eq!(t.state(), S::Listening);
        assert!(t.hands_free_on());
    }

    #[test]
    fn second_press_before_the_release_was_seen_is_a_double_press() {
        let mut t = T::new();
        t.down(Key::HandsFree);
        t.now += Duration::from_millis(150);
        t.down(Key::HandsFree);
        assert_eq!(t.state(), S::Listening);
        assert!(t.hands_free_on());
    }

    #[test]
    fn hold_ignores_the_pause_detector() {
        let mut t = T::new();
        let op = t.listen(Key::Talk);
        assert_eq!(t.send(Msg::SpeechEnded { op }), []);
        assert_eq!(t.state(), S::Listening);
    }

    #[test]
    fn press_during_transcription_cancels() {
        let mut t = T::new();
        let op = t.listen(Key::Talk);
        t.release(Key::Talk);
        t.tap(Key::Talk);
        assert_eq!(t.state(), S::Idle);
        let fx = t.send(Msg::Transcribed {
            op,
            text: "проверь diff".into(),
        });
        assert_eq!(fx, []);
    }

    #[test]
    fn terminal_without_active_session_does_nothing() {
        let mut t = T::new();
        assert_eq!(t.down(Key::Terminal), []);
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
        assert_eq!(t.state(), S::Idle);
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
            cwd: "C:/p".into(),
            agent: Agent::Claude,
        }));
        assert!(!fx.iter().any(|e| matches!(e, Effect::StartRun { .. })));
    }

    #[test]
    fn live_decode_is_throttled_and_never_overlaps() {
        let mut t = T::new();
        let op = t.listen(Key::Talk);
        let live = |fx: &[Effect]| fx.iter().any(|e| matches!(e, Effect::LiveDecode { .. }));
        t.send(Msg::Speaking { op, speaking: true });

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
        assert_eq!(text(shown(&fx).unwrap()), "Claude привет");
        assert!(live(&t.send(Msg::Audio {
            op,
            samples: vec![0.1; 10]
        })));
    }

    #[test]
    fn live_decode_waits_for_speech() {
        let mut t = T::new();
        let op = t.listen(Key::Talk);
        t.now += LIVE_INTERVAL;
        let fx = t.send(Msg::Audio {
            op,
            samples: vec![0.1; 10],
        });
        assert!(!fx.iter().any(|e| matches!(e, Effect::LiveDecode { .. })));
    }

    #[test]
    fn speech_shows_on_the_overlay() {
        let mut t = T::new();
        let op = t.listen(Key::Talk);
        let fx = t.send(Msg::Speaking { op, speaking: true });
        assert_eq!(shown(&fx).unwrap().mic, Mic::Listening);
        let fx = t.send(Msg::Speaking {
            op,
            speaking: false,
        });
        assert_eq!(shown(&fx).unwrap().mic, Mic::Waiting);
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
        assert_eq!(t.state(), S::Transcribing);
    }

    #[test]
    fn transcript_is_transformed_and_dispatched() {
        let mut t = T::new();
        let op = t.run();
        assert_eq!(t.state(), S::Running);
        let v = t.c.view();
        assert_eq!(v.agent, Agent::Claude);
        assert_eq!(text(&v), "проверь, что Claude видит diff");
        let _ = op;
    }

    #[test]
    fn start_run_effect_carries_prompt_and_session() {
        let mut t = T::new();
        let op = t.listen(Key::Talk);
        t.release(Key::Talk);
        let fx = t.send(Msg::Transcribed {
            op,
            text: " go  клод ".into(),
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
        assert_eq!((*rop, prompt.as_str()), (op, "go Claude"));
        let _ = session_id;
    }

    #[test]
    fn empty_transcript_is_a_failure_and_sends_nothing() {
        let mut t = T::new();
        let op = t.listen(Key::Talk);
        t.release(Key::Talk);
        let fx = t.send(Msg::Transcribed {
            op,
            text: "  ".into(),
        });
        assert_eq!(st(shown(&fx).unwrap()), S::Failed);
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
    fn limited_run_is_marked_until_the_next_run() {
        let mut t = T::new();
        let op = t.run();
        let fx = t.send(Msg::Run {
            op,
            event: RunEvent::Limited,
        });
        assert!(shown(&fx).unwrap().limited);
        t.send(Msg::RunExited {
            op,
            end: RunEnd::Exited { success: true },
            stderr: String::new(),
        });
        t.run();
        assert!(!t.c.view().limited);
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
        assert_eq!((st(v), outcome(v)), (S::Succeeded, "Done"));
        let fx = t.send(Msg::Dismiss { rest: Some(v.rest) });
        assert_eq!(st(shown(&fx).unwrap()), S::Idle);
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
        assert_eq!((st(v), outcome(v)), (S::Failed, "Invalid API key"));
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
        assert_eq!(outcome(shown(&fx).unwrap()), "boom");
        let op = t.run();
        let fx = t.send(Msg::RunExited {
            op,
            end: RunEnd::TimedOut,
            stderr: String::new(),
        });
        assert_eq!(outcome(shown(&fx).unwrap()), "Timed out");
    }

    #[test]
    fn key_during_run_cancels() {
        let mut t = T::new();
        let op = t.run();
        let fx = t.tap(Key::Talk);
        assert!(fx.contains(&Effect::CancelRun));
        assert_eq!(t.state(), S::Cancelling);
        let fx = t.send(Msg::RunExited {
            op,
            end: RunEnd::Cancelled,
            stderr: String::new(),
        });
        assert_eq!(st(shown(&fx).unwrap()), S::Idle);
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
        let fx = t.down(Key::Talk);
        assert!(matches!(fx[0], Effect::StartCapture { .. }));
        assert_ne!(t.op(), op);
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
        assert_eq!(t.state(), S::Listening);
    }

    #[test]
    fn microphone_failure_stops_capture_and_shows_error() {
        let mut t = T::new();
        let op = t.listen(Key::Talk);
        let fx = t.send(Msg::MicFailed {
            op,
            error: "no microphone".into(),
        });
        assert_eq!(fx[0], Effect::StopCapture);
        let v = shown(&fx).unwrap();
        assert_eq!(v.mic, Mic::Error);
        assert_eq!(v.global_error.as_deref(), Some(MIC_FAILED));
        assert_eq!(
            t.send(Msg::MicFailed {
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
        let second = t.finish_saying("теперь добавь тесты");
        assert_eq!(second, Session::Resume(id(first)));
        assert_eq!(t.c.view().session_id, Some(id(first)));
    }

    #[test]
    fn spoken_command_starts_a_new_session_and_is_removed() {
        let mut t = T::new();
        let first = t.finish_saying("проверь diff");
        let op = t.listen(Key::Talk);
        t.release(Key::Talk);
        let fx = t.send(Msg::Transcribed {
            op,
            text: "в новой сессии найди баг у клод".into(),
        });
        let Some(Effect::StartRun {
            prompt, session, ..
        }) = fx.first()
        else {
            panic!("{fx:?}")
        };
        assert_eq!(prompt, "найди баг у Claude");
        assert!(matches!(session, Session::New(new) if *new != id(first)));
    }

    #[test]
    fn new_session_key_talks_into_a_new_session() {
        let mut t = T::new();
        t.finish_saying("проверь diff");
        t.now += Duration::from_secs(1);
        let op = t.listen(Key::NewSession);
        t.release(Key::NewSession);
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
    fn a_restored_session_keeps_its_age() {
        let mut t = T::new();
        t.send(settings(SessionPolicy::ContinueIfRecent, "C:/p"));
        t.now += Duration::from_secs(3600);
        let restored = |idle| Msg::SetActive {
            id: Uuid::from_u128(42),
            cwd: "C:/p".into(),
            agent: Agent::Claude,
            idle,
        };
        t.send(restored(Duration::from_secs(601)));
        assert!(matches!(t.finish_saying("ещё раз"), Session::New(_)));
        t.send(restored(Duration::from_secs(60)));
        assert_eq!(
            t.finish_saying("ещё раз"),
            Session::Resume(Uuid::from_u128(42))
        );
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
            agent: Agent::Claude,
            idle: Duration::ZERO,
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
            agent: Agent::Claude,
            idle: Duration::ZERO,
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
        assert!(!c.view().visible);
        assert_eq!(
            c.handle(Msg::KeyDown(Key::Talk, now), now),
            [Effect::OpenSettings]
        );
        c.handle(Msg::ModelReady, now);
        assert_eq!(st(&c.view()), S::Idle);
    }

    fn refining() -> T {
        let mut t = T::new();
        t.send(Msg::Settings {
            policy: SessionPolicy::Continue,
            recent: Duration::from_secs(600),
            cwd: "C:/p".into(),
            patterns: Patterns::default(),
            model_commands: true,
            agent: Agent::Claude,
            hide_after: HIDE_AFTER,
            double: DOUBLE,
            bindings: bindings(),
        });
        t
    }

    #[test]
    fn model_command_applies() {
        let mut t = refining();
        let op = t.listen(Key::Talk);
        t.release(Key::Talk);
        let text = "давай с чистого листа, напиши README";
        let fx = t.send(Msg::Transcribed {
            op,
            text: text.into(),
        });
        assert_eq!(
            fx[0],
            Effect::Classify {
                op,
                text: text.into()
            }
        );
        assert_eq!(st(shown(&fx).unwrap()), S::Classifying);
        let fx = t.send(Msg::Classified {
            op,
            answer: Some((Some(Command::NewSession), "напиши README".into())),
        });
        let Some(Effect::StartRun {
            prompt, session, ..
        }) = fx.first()
        else {
            panic!("{fx:?}")
        };
        assert_eq!(prompt, "напиши README");
        assert!(matches!(session, Session::New(_)));
    }

    #[test]
    fn model_failure_sends_whole_text() {
        let mut t = refining();
        let op = t.listen(Key::Talk);
        t.release(Key::Talk);
        t.send(Msg::Transcribed {
            op,
            text: "проверь diff".into(),
        });
        let fx = t.send(Msg::Classified { op, answer: None });
        let Some(Effect::StartRun { prompt, .. }) = fx.first() else {
            panic!("{fx:?}")
        };
        assert_eq!(prompt, "проверь diff");
    }

    #[test]
    fn pattern_command_skips_the_model() {
        let mut t = refining();
        let op = t.listen(Key::Talk);
        t.release(Key::Talk);
        let fx = t.send(Msg::Transcribed {
            op,
            text: "новая сессия, проверь diff".into(),
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
    fn empty_transcript_skips_the_model() {
        let mut t = refining();
        let op = t.listen(Key::Talk);
        t.release(Key::Talk);
        let fx = t.send(Msg::Transcribed {
            op,
            text: " ".into(),
        });
        assert!(!fx.iter().any(|e| matches!(e, Effect::Classify { .. })));
        assert_eq!(t.state(), S::Failed);
    }

    fn run_in_terminal(fx: &[Effect]) -> Option<(Session, String, String)> {
        fx.iter().find_map(|e| match e {
            Effect::RunInTerminal {
                session,
                cwd,
                prompt,
                ..
            } => Some((*session, cwd.clone(), prompt.clone())),
            _ => None,
        })
    }

    fn say(t: &mut T, text: &str) -> Vec<Effect> {
        t.now += Duration::from_secs(1);
        let op = t.listen(Key::Talk);
        t.release(Key::Talk);
        t.send(Msg::Transcribed {
            op,
            text: text.into(),
        })
    }

    #[test]
    fn terminal_and_new_session_run_the_task_in_a_terminal() {
        let mut t = T::new();
        let first = t.finish_saying("проверь diff");
        let fx = say(&mut t, "Открой в терминале в новой сессии, найди баг");
        let (session, cwd, prompt) = run_in_terminal(&fx).expect("runs in a terminal");
        assert!(matches!(session, Session::New(new) if new != id(first)));
        assert_eq!((cwd.as_str(), prompt.as_str()), ("C:/p", "найди баг"));
        assert!(!fx.iter().any(|e| matches!(e, Effect::StartRun { .. })));
        assert_eq!(active_changes(&fx), [Some(id(session))]);
        assert_eq!(t.state(), S::Idle);
    }

    #[test]
    fn terminal_with_a_task_continues_the_active_session() {
        let mut t = T::new();
        let first = t.finish_saying("проверь diff");
        let fx = say(&mut t, "найди баг и открой в терминале");
        let (session, _, prompt) = run_in_terminal(&fx).expect("runs in a terminal");
        assert_eq!(session, Session::Resume(id(first)));
        assert_eq!(prompt, "найди баг");
    }

    #[test]
    fn terminal_and_new_session_alone_open_an_empty_session() {
        let mut t = T::new();
        let fx = say(&mut t, "в новой сессии открой в терминале");
        let (session, _, prompt) = run_in_terminal(&fx).expect("runs in a terminal");
        assert!(matches!(session, Session::New(_)));
        assert_eq!(prompt, "");
    }

    fn with_default(t: &mut T, agent: Agent) {
        t.send(Msg::Settings {
            policy: SessionPolicy::Continue,
            recent: Duration::from_secs(600),
            cwd: "C:/p".into(),
            patterns: Patterns::default(),
            model_commands: false,
            agent,
            hide_after: HIDE_AFTER,
            double: DOUBLE,
            bindings: bindings(),
        });
    }

    fn run_agent(fx: &[Effect]) -> Option<(Session, Agent)> {
        fx.iter().find_map(|e| match e {
            Effect::StartRun { session, agent, .. } => Some((*session, *agent)),
            _ => None,
        })
    }

    #[test]
    fn spoken_agent_starts_a_new_session_with_it() {
        let mut t = T::new();
        let first = t.finish_saying("проверь diff");
        let fx = say(&mut t, "codex, напиши тесты");
        let (session, agent) = run_agent(&fx).unwrap();
        assert!(matches!(session, Session::New(new) if new != id(first)));
        assert_eq!(agent, Agent::Codex);
        assert_eq!(t.c.view().agent, Agent::Codex);
    }

    #[test]
    fn plain_phrase_continues_with_the_session_agent() {
        let mut t = T::new();
        let first = t.finish_saying("codex, напиши тесты");
        let fx = say(&mut t, "а теперь поправь");
        let (session, agent) = run_agent(&fx).unwrap();
        assert_eq!(session, Session::Resume(id(first)));
        assert_eq!(agent, Agent::Codex);
    }

    #[test]
    fn default_agent_starts_new_sessions() {
        let mut t = T::new();
        with_default(&mut t, Agent::Codex);
        let (_, agent) = run_agent(&say(&mut t, "проверь diff")).unwrap();
        assert_eq!(agent, Agent::Codex);
    }

    #[test]
    fn picked_session_keeps_its_agent() {
        let mut t = T::new();
        t.send(Msg::SetActive {
            id: Uuid::from_u128(9),
            cwd: "C:/q".into(),
            agent: Agent::Codex,
            idle: Duration::ZERO,
        });
        let (session, agent) = run_agent(&say(&mut t, "продолжай")).unwrap();
        assert_eq!(session, Session::Resume(Uuid::from_u128(9)));
        assert_eq!(agent, Agent::Codex);
    }
}
