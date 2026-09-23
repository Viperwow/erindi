use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Where an utterance goes when the user does not say.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SessionPolicy {
    #[default]
    Continue,
    ContinueIfRecent,
    AlwaysNew,
}

/// The session the next utterance continues by default.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Active {
    pub id: Uuid,
    pub cwd: String,
    pub last_used: Instant,
}

/// Returns the session to resume, or `None` to start a new one.
pub fn choose(
    policy: SessionPolicy,
    recent: Duration,
    new: bool,
    active: Option<&Active>,
    now: Instant,
) -> Option<Uuid> {
    if new {
        return None;
    }
    let active = active?;
    let resume = match policy {
        SessionPolicy::Continue => true,
        SessionPolicy::AlwaysNew => false,
        SessionPolicy::ContinueIfRecent => now.duration_since(active.last_used) <= recent,
    };
    resume.then_some(active.id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn active(cwd: &str, age: Duration, now: Instant) -> Active {
        Active {
            id: Uuid::from_u128(7),
            cwd: cwd.into(),
            last_used: now - age,
        }
    }

    #[test]
    fn choosing_a_session() {
        let now = Instant::now() + Duration::from_secs(3600);
        let recent = Duration::from_secs(600);
        let id = Some(Uuid::from_u128(7));
        let fresh = active("C:/p", Duration::from_secs(60), now);
        let stale = active("C:/p", Duration::from_secs(1200), now);
        use SessionPolicy as P;

        let cases = [
            (P::Continue, false, None, None),
            (P::Continue, false, Some(&stale), id),
            (P::Continue, true, Some(&fresh), None),
            (P::AlwaysNew, false, Some(&fresh), None),
            (P::ContinueIfRecent, false, Some(&fresh), id),
            (P::ContinueIfRecent, false, Some(&stale), None),
            (P::ContinueIfRecent, true, Some(&fresh), None),
        ];
        for (policy, new, active, expected) in cases {
            assert_eq!(
                choose(policy, recent, new, active, now),
                expected,
                "{policy:?} {new}"
            );
        }
    }
}
