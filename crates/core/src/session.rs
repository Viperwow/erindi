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

/// A session request spoken as part of the utterance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    Unspecified,
    New,
    Continue,
}

/// The session the next utterance continues by default.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Active {
    pub id: Uuid,
    pub cwd: String,
    pub last_used: Instant,
}

/// Finds a spoken session command at the start or end of `text` and returns it with the command removed.
/// Commands in the middle are left alone, so a task that merely mentions sessions is not misread.
pub fn parse_intent(text: &str) -> (Intent, String) {
    let words: Vec<&str> = text.split_whitespace().collect();
    let plain: Vec<String> = words.iter().map(|w| bare(w)).collect();

    let skippable = |w: &String| FILLERS.contains(&w.as_str()) || CONNECTORS.contains(&w.as_str());
    let start = plain
        .iter()
        .take_while(|w| FILLERS.contains(&w.as_str()))
        .count();
    for (intent, phrase) in PHRASES {
        let n = phrase.len();
        if plain.len() >= start + n && matches(&plain[start..start + n], phrase) {
            let from = start
                + n
                + plain[start + n..]
                    .iter()
                    .take_while(|w| skippable(w))
                    .count();
            return (*intent, words[from..].join(" "));
        }
        if plain.len() >= n && matches(&plain[plain.len() - n..], phrase) {
            let end = plain.len() - n;
            let end = end
                - plain[..end]
                    .iter()
                    .rev()
                    .take_while(|w| skippable(w))
                    .count();
            let rest = words[..end].join(" ");
            let rest = rest.trim_end_matches([',', ';', ':', '-', ' ']);
            return (*intent, rest.to_string());
        }
    }
    (Intent::Unspecified, text.to_string())
}

fn matches(words: &[String], phrase: &[&str]) -> bool {
    words.iter().zip(phrase).all(|(w, p)| w == p)
}

/// Lowercase word without surrounding punctuation.
fn bare(word: &str) -> String {
    word.trim_matches(|c: char| !c.is_alphanumeric())
        .to_lowercase()
}

/// Leading words skipped before a command, as in "open in a new session".
const FILLERS: &[&str] = &[
    "открой",
    "мне",
    "начни",
    "запусти",
    "создай",
    "open",
    "start",
    "create",
];

/// Words that join a command to the task, as in "create a new session and fix the tests".
const CONNECTORS: &[&str] = &["и", "потом", "затем", "and", "then"];

/// Longest phrases first, so "in a new session" wins over "new session".
const PHRASES: &[(Intent, &[&str])] = &[
    (Intent::New, &["in", "a", "new", "session"]),
    (Intent::Continue, &["in", "the", "same", "session"]),
    (Intent::Continue, &["in", "the", "current", "session"]),
    (Intent::Continue, &["в", "той", "же", "сессии"]),
    (Intent::Continue, &["в", "этой", "же", "сессии"]),
    (Intent::New, &["in", "new", "session"]),
    (Intent::New, &["в", "новой", "сессии"]),
    (Intent::New, &["с", "новой", "сессии"]),
    (Intent::Continue, &["в", "этой", "сессии"]),
    (Intent::Continue, &["в", "текущей", "сессии"]),
    (Intent::Continue, &["in", "this", "session"]),
    (Intent::New, &["a", "new", "session"]),
    (Intent::New, &["new", "session"]),
    (Intent::New, &["новая", "сессия"]),
    (Intent::New, &["новую", "сессию"]),
    (Intent::Continue, &["same", "session"]),
    (Intent::Continue, &["continue", "session"]),
];

/// Returns the session to resume, or `None` to start a new one.
pub fn choose(
    policy: SessionPolicy,
    recent: Duration,
    intent: Intent,
    active: Option<&Active>,
    now: Instant,
) -> Option<Uuid> {
    if intent == Intent::New {
        return None;
    }
    let active = active?;
    let resume = match (intent, policy) {
        (Intent::Continue, _) | (_, SessionPolicy::Continue) => true,
        (_, SessionPolicy::AlwaysNew) => false,
        (_, SessionPolicy::ContinueIfRecent) => now.duration_since(active.last_used) <= recent,
    };
    resume.then_some(active.id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_has_no_intent() {
        let text = "проверь текущий diff";
        assert_eq!(parse_intent(text), (Intent::Unspecified, text.to_string()));
    }

    #[test]
    fn new_session_commands() {
        let cases = [
            ("в новой сессии проверь diff", "проверь diff"),
            ("Открой мне в новой сессии: найди баг.", "найди баг."),
            ("New session, fix the tests", "fix the tests"),
            ("fix the tests in a new session", "fix the tests"),
            ("исправь сборку, в новой сессии.", "исправь сборку"),
            ("Создай новую сессию и проверь diff", "проверь diff"),
            ("Create a new session, then fix the tests", "fix the tests"),
            ("Проверь diff и создай новую сессию.", "Проверь diff"),
            ("fix the tests and start a new session", "fix the tests"),
        ];
        for (input, prompt) in cases {
            assert_eq!(
                parse_intent(input),
                (Intent::New, prompt.to_string()),
                "{input}"
            );
        }
    }

    #[test]
    fn continue_commands() {
        let cases = [
            ("в той же сессии добавь тесты", "добавь тесты"),
            ("Проверь ещё раз, в этой же сессии", "Проверь ещё раз"),
            ("same session: now run the linter", "now run the linter"),
        ];
        for (input, prompt) in cases {
            assert_eq!(
                parse_intent(input),
                (Intent::Continue, prompt.to_string()),
                "{input}"
            );
        }
    }

    #[test]
    fn commands_inside_the_task_are_ignored() {
        for text in [
            "расскажи про новую сессию в React и как её закрыть",
            "explain how a new session is created in the auth module",
        ] {
            assert_eq!(parse_intent(text), (Intent::Unspecified, text.to_string()));
        }
    }

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
        use Intent as I;
        use SessionPolicy as P;

        let cases = [
            (P::Continue, I::Unspecified, None, None),
            (P::Continue, I::Unspecified, Some(&stale), id),
            (P::Continue, I::New, Some(&fresh), None),
            (P::AlwaysNew, I::Unspecified, Some(&fresh), None),
            (P::AlwaysNew, I::Continue, Some(&fresh), id),
            (P::ContinueIfRecent, I::Unspecified, Some(&fresh), id),
            (P::ContinueIfRecent, I::Unspecified, Some(&stale), None),
            (P::ContinueIfRecent, I::Continue, Some(&stale), id),
            (P::AlwaysNew, I::Continue, None, None),
        ];
        for (policy, intent, active, expected) in cases {
            assert_eq!(
                choose(policy, recent, intent, active, now),
                expected,
                "{policy:?} {intent:?}"
            );
        }
    }
}
