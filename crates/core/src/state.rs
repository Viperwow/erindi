use serde::Serialize;

pub type OpId = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum AppState {
    LoadingModel,
    Idle,
    Listening,
    Transcribing,
    Running,
    Cancelling,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    ModelReady,
    ModelFailed,
    StartListening,
    StopListening,
    CancelListening,
    Transcribed { op: OpId, empty: bool },
    Cancel,
    RunExited { op: OpId, ok: bool },
    StepFailed { op: OpId },
    Dismiss,
}

#[derive(Debug, PartialEq, Eq)]
pub struct InvalidTransition {
    pub from: AppState,
    pub event: Event,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Changed(AppState),
    /// The event belongs to an operation that is no longer current.
    Stale,
}

#[derive(Debug)]
pub struct Machine {
    state: AppState,
    op: OpId,
}

impl Machine {
    pub fn new() -> Self {
        Self {
            state: AppState::LoadingModel,
            op: 0,
        }
    }

    pub fn state(&self) -> AppState {
        self.state
    }

    pub fn op(&self) -> OpId {
        self.op
    }

    pub fn apply(&mut self, event: Event) -> Result<Outcome, InvalidTransition> {
        use AppState as S;
        use Event as E;

        if let E::Transcribed { op, .. } | E::RunExited { op, .. } | E::StepFailed { op } = event
            && op != self.op
        {
            return Ok(Outcome::Stale);
        }

        let next = match (self.state, event) {
            (S::LoadingModel, E::ModelReady) => S::Idle,
            (S::LoadingModel, E::ModelFailed) => S::Failed,
            (S::Idle, E::StartListening) => {
                self.op += 1;
                S::Listening
            }
            (S::Listening, E::StopListening) => S::Transcribing,
            (S::Listening, E::CancelListening) => S::Idle,
            (S::Transcribing, E::Transcribed { empty: true, .. }) => S::Idle,
            (S::Transcribing, E::Transcribed { empty: false, .. }) => S::Running,
            (S::Listening | S::Transcribing | S::Running, E::StepFailed { .. }) => S::Failed,
            (S::Running, E::Cancel) => S::Cancelling,
            (S::Running, E::RunExited { ok: true, .. }) => S::Succeeded,
            (S::Running, E::RunExited { ok: false, .. }) => S::Failed,
            (S::Cancelling, E::RunExited { .. }) => S::Idle,
            (S::Succeeded | S::Failed, E::Dismiss) => S::Idle,
            (from, event) => return Err(InvalidTransition { from, event }),
        };
        self.state = next;
        Ok(Outcome::Changed(next))
    }
}

impl Default for Machine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{AppState::*, Event::*, *};

    fn at(state: AppState) -> Machine {
        let mut m = Machine::new();
        let path: &[Event] = match state {
            LoadingModel => &[],
            Idle => &[ModelReady],
            Listening => &[ModelReady, StartListening],
            Transcribing => &[ModelReady, StartListening, StopListening],
            _ => unreachable!("build other states from Transcribing"),
        };
        for e in path {
            m.apply(*e).unwrap();
        }
        m
    }

    fn running() -> Machine {
        let mut m = at(Transcribing);
        let op = m.op();
        m.apply(Transcribed { op, empty: false }).unwrap();
        m
    }

    #[test]
    fn happy_path() {
        let mut m = running();
        assert_eq!(m.state(), Running);
        let op = m.op();
        assert_eq!(
            m.apply(RunExited { op, ok: true }),
            Ok(Outcome::Changed(Succeeded))
        );
        assert_eq!(m.apply(Dismiss), Ok(Outcome::Changed(Idle)));
    }

    #[test]
    fn model_failure_is_terminal_until_dismissed() {
        let mut m = Machine::new();
        assert_eq!(m.apply(ModelFailed), Ok(Outcome::Changed(Failed)));
        assert_eq!(m.apply(Dismiss), Ok(Outcome::Changed(Idle)));
    }

    #[test]
    fn each_listening_starts_new_op() {
        let mut m = at(Idle);
        m.apply(StartListening).unwrap();
        let first = m.op();
        m.apply(CancelListening).unwrap();
        assert_eq!(m.state(), Idle);
        m.apply(StartListening).unwrap();
        assert_ne!(m.op(), first);
    }

    #[test]
    fn empty_transcript_returns_to_idle() {
        let mut m = at(Transcribing);
        let op = m.op();
        assert_eq!(
            m.apply(Transcribed { op, empty: true }),
            Ok(Outcome::Changed(Idle))
        );
    }

    #[test]
    fn cancel_run_goes_through_cancelling() {
        let mut m = running();
        let op = m.op();
        assert_eq!(m.apply(Cancel), Ok(Outcome::Changed(Cancelling)));
        assert_eq!(
            m.apply(RunExited { op, ok: false }),
            Ok(Outcome::Changed(Idle))
        );
    }

    #[test]
    fn failures_from_async_steps() {
        let mut m = at(Listening);
        let op = m.op();
        assert_eq!(
            m.apply(StepFailed { op }),
            Ok(Outcome::Changed(AppState::Failed))
        );

        let mut m = at(Transcribing);
        let op = m.op();
        assert_eq!(
            m.apply(StepFailed { op }),
            Ok(Outcome::Changed(AppState::Failed))
        );

        let mut m = running();
        let op = m.op();
        assert_eq!(
            m.apply(RunExited { op, ok: false }),
            Ok(Outcome::Changed(AppState::Failed))
        );
    }

    #[test]
    fn stale_events_are_ignored() {
        let mut m = at(Transcribing);
        let old = m.op();
        m.apply(Transcribed {
            op: old,
            empty: true,
        })
        .unwrap();
        m.apply(StartListening).unwrap();
        m.apply(StopListening).unwrap();
        assert_eq!(
            m.apply(Transcribed {
                op: old,
                empty: false
            }),
            Ok(Outcome::Stale)
        );
        assert_eq!(m.apply(StepFailed { op: old }), Ok(Outcome::Stale));
        assert_eq!(m.state(), Transcribing);
    }

    #[test]
    fn invalid_transitions_are_rejected() {
        let cases = [
            (at(LoadingModel), StartListening),
            (at(Idle), StopListening),
            (at(Idle), Cancel),
            (at(Idle), Dismiss),
            (at(Listening), StartListening),
            (
                at(Listening),
                Transcribed {
                    op: 1,
                    empty: false,
                },
            ),
            (at(Transcribing), StartListening),
            (at(Transcribing), Dismiss),
            (running(), StartListening),
            (running(), ModelReady),
        ];
        for (mut m, event) in cases {
            let from = m.state();
            assert_eq!(m.apply(event), Err(InvalidTransition { from, event }));
            assert_eq!(m.state(), from, "state must not change on {event:?}");
        }
    }
}
