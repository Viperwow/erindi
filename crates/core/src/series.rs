use serde::Serialize;

pub type PhraseId = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    Speaking,
    Transcribing,
    Queued,
    Classifying,
    Running,
    Cancelling,
    Done,
    Failed,
    Cancelled,
}

impl Status {
    pub fn finished(self) -> bool {
        matches!(self, Status::Done | Status::Failed | Status::Cancelled)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Speech,
    /// The terminal hotkey, pressed while the agent was busy.
    Terminal,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Phrase {
    pub id: PhraseId,
    pub kind: Kind,
    pub status: Status,
    pub text: String,
    pub outcome: String,
    /// Said with the new-session key.
    #[serde(skip)]
    pub new_session: bool,
}

pub const KEEP_FINISHED: usize = 3;

/// The phrases of one series in the order they were said; the `Queued` ones are the queue.
#[derive(Debug, Default)]
pub struct Series {
    phrases: Vec<Phrase>,
    next: PhraseId,
    id: u64,
}

impl Series {
    /// Adds a phrase, starting a new series when nothing is in progress.
    pub fn start(&mut self, kind: Kind, status: Status, new_session: bool) -> PhraseId {
        if !self.active() {
            self.clear_finished();
        }
        self.add(kind, status, new_session)
    }

    /// Adds a phrase to the current series.
    pub fn add(&mut self, kind: Kind, status: Status, new_session: bool) -> PhraseId {
        self.next += 1;
        self.phrases.push(Phrase {
            id: self.next,
            kind,
            status,
            text: String::new(),
            outcome: String::new(),
            new_session,
        });
        self.next
    }

    pub fn get(&self, id: PhraseId) -> Option<&Phrase> {
        self.phrases.iter().find(|p| p.id == id)
    }

    pub fn get_mut(&mut self, id: PhraseId) -> Option<&mut Phrase> {
        self.phrases.iter_mut().find(|p| p.id == id)
    }

    pub fn remove(&mut self, id: PhraseId) {
        self.phrases.retain(|p| p.id != id);
    }

    pub fn next_queued(&self) -> Option<PhraseId> {
        self.phrases
            .iter()
            .find(|p| p.status == Status::Queued)
            .map(|p| p.id)
    }

    pub fn agent(&self) -> Option<&Phrase> {
        self.phrases.iter().find(|p| {
            matches!(
                p.status,
                Status::Classifying | Status::Running | Status::Cancelling
            )
        })
    }

    pub fn finish(&mut self, id: PhraseId, status: Status, outcome: String) {
        if let Some(p) = self.get_mut(id) {
            p.status = status;
            p.outcome = outcome;
        }
        let finished = self.phrases.iter().filter(|p| p.status.finished()).count();
        let mut extra = finished.saturating_sub(KEEP_FINISHED);
        self.phrases.retain(|p| {
            let drop = extra > 0 && p.status.finished();
            extra -= usize::from(drop);
            !drop
        });
    }

    pub fn active(&self) -> bool {
        self.phrases.iter().any(|p| !p.status.finished())
    }

    pub fn clear_finished(&mut self) {
        let before = self.phrases.len();
        self.phrases.retain(|p| !p.status.finished());
        if self.phrases.len() != before {
            self.id += 1;
        }
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn phrases(&self) -> &[Phrase] {
        &self.phrases
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finished(s: &mut Series, n: usize) -> Vec<PhraseId> {
        (0..n)
            .map(|_| {
                let id = s.start(Kind::Speech, Status::Running, false);
                s.finish(id, Status::Done, String::new());
                id
            })
            .collect()
    }

    #[test]
    fn queue_is_first_queued_in_order() {
        let mut s = Series::default();
        let ids: Vec<_> = (0..3)
            .map(|_| s.start(Kind::Speech, Status::Queued, false))
            .collect();
        assert_eq!(s.next_queued(), Some(ids[0]));
        s.finish(ids[0], Status::Done, String::new());
        assert_eq!(s.next_queued(), Some(ids[1]));
    }

    #[test]
    fn keeps_only_the_last_three_finished() {
        let mut s = Series::default();
        let running = s.start(Kind::Speech, Status::Running, false);
        let ids = finished(&mut s, 4);
        let kept: Vec<_> = s.phrases().iter().map(|p| p.id).collect();
        assert_eq!(kept, [running, ids[1], ids[2], ids[3]]);
    }

    #[test]
    fn a_new_phrase_after_an_idle_series_clears_it() {
        let mut s = Series::default();
        finished(&mut s, 2);
        let before = s.id();
        let id = s.start(Kind::Speech, Status::Speaking, false);
        assert_eq!(s.phrases().iter().map(|p| p.id).collect::<Vec<_>>(), [id]);
        assert_ne!(s.id(), before);
    }

    #[test]
    fn a_new_phrase_during_a_series_keeps_it() {
        let mut s = Series::default();
        s.start(Kind::Speech, Status::Running, false);
        let done = s.start(Kind::Speech, Status::Running, false);
        s.finish(done, Status::Done, String::new());
        let before = s.id();
        s.start(Kind::Speech, Status::Speaking, false);
        assert_eq!(s.phrases().len(), 3);
        assert_eq!(s.id(), before);
    }

    #[test]
    fn agent_is_the_classifying_running_or_cancelling_phrase() {
        let mut s = Series::default();
        s.start(Kind::Speech, Status::Queued, false);
        assert!(s.agent().is_none());
        for status in [Status::Classifying, Status::Running, Status::Cancelling] {
            let id = s.start(Kind::Speech, status, false);
            assert_eq!(s.agent().map(|p| p.id), Some(id));
            s.finish(id, Status::Done, String::new());
        }
    }
}
