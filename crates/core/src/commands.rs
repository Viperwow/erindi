use regex::Regex;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Command {
    NewSession,
    OpenTerminal,
    /// Only at the end of a phrase: nothing is sent.
    Cancel,
    /// Only at the start: a new session with this agent.
    Claude,
    Codex,
}

impl Command {
    pub fn agent(self) -> Option<crate::agent::Agent> {
        match self {
            Command::Claude => Some(crate::agent::Agent::Claude),
            Command::Codex => Some(crate::agent::Agent::Codex),
            _ => None,
        }
    }
}

/// Regular expressions that trigger each command, as the user edits them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Patterns {
    pub new_session: Vec<String>,
    pub open_terminal: Vec<String>,
    pub cancel: Vec<String>,
    pub claude: Vec<String>,
    pub codex: Vec<String>,
}

impl Default for Patterns {
    fn default() -> Self {
        let list = |items: &[&str]| items.iter().map(|s| s.to_string()).collect();
        Self {
            new_session: list(&[
                r"((создай|открой|начни) )?((в|с) )?нов\w* сесси\w*",
                r"((start|create|in) )?(a )?new session",
            ]),
            open_terminal: list(&[r"открой (в )?термина\w*", r"open (in )?terminal"]),
            cancel: list(&[r"отмен\w*", "cancel", "scratch that"]),
            claude: list(&[r"((в|с|через) )?(клод|claude)\w*", r"((in|with) )?claude"]),
            codex: list(&[r"((в|с|через) )?(кодекс|codex)\w*", r"((in|with) )?codex"]),
        }
    }
}

struct Entry {
    command: Command,
    start: Regex,
    end: Regex,
}

/// Finds a command at the start or end of a phrase.
pub struct Parser {
    entries: Vec<Entry>,
    lead: Regex,
    tail: Regex,
}

/// Punctuation and joining words between a command and the task.
const JOIN: &str = r"(?:и|потом|затем|and|then)";

impl Parser {
    pub fn new(patterns: &Patterns) -> Result<Self, String> {
        let groups = [
            (Command::NewSession, &patterns.new_session),
            (Command::OpenTerminal, &patterns.open_terminal),
            (Command::Cancel, &patterns.cancel),
            (Command::Claude, &patterns.claude),
            (Command::Codex, &patterns.codex),
        ];
        let mut entries = vec![];
        for (command, list) in groups {
            for p in list {
                let bad = |e: regex::Error| format!("Pattern {p}: {e}");
                if Regex::new(&format!("^(?:{p})$")).map_err(bad)?.is_match("") {
                    return Err(format!("Pattern {p} matches an empty phrase"));
                }
                entries.push(Entry {
                    command,
                    start: Regex::new(&format!(r"(?i)^[\s\p{{P}}]*(?:{p})\b")).map_err(bad)?,
                    end: Regex::new(&format!(r"(?i)\b(?:{p})[\s\p{{P}}]*$")).map_err(bad)?,
                });
            }
        }
        Ok(Self {
            entries,
            lead: Regex::new(&format!(r"(?i)^[\s\p{{P}}]*(?:{JOIN}\b[\s\p{{P}}]*)*")).unwrap(),
            tail: Regex::new(&format!(r"(?i)(?:[\s\p{{P}}]+{JOIN})*[\s\p{{P}}]*$")).unwrap(),
        })
    }

    /// Returns the commands found at either edge, peeled off one by one, and the task left
    /// between them. Cancel counts only at the end and replaces everything else.
    pub fn parse(&self, text: &str) -> (Vec<Command>, String) {
        let cancel = |c: Command| c == Command::Cancel;
        if let Some((_, at)) = self.at_end(text, cancel) {
            return (vec![Command::Cancel], self.before(&text[..at]));
        }
        let mut commands = vec![];
        let mut rest = text.to_string();
        loop {
            let start = self
                .entries
                .iter()
                .filter(|e| !cancel(e.command))
                .filter_map(|e| Some((e.command, nonempty(e.start.find(&rest))?.end())))
                .filter(|(command, end)| command.agent().is_none() || agent_break(&rest[*end..]))
                .max_by_key(|(_, end)| *end);
            if let Some((command, end)) = start {
                let tail = &rest[end..];
                rest = tail[self.lead.find(tail).map_or(0, |m| m.end())..].to_string();
                if !commands.contains(&command) {
                    commands.push(command);
                }
                continue;
            }
            if let Some((command, at)) = self.at_end(&rest, |c| !cancel(c) && c.agent().is_none()) {
                rest = self.before(&rest[..at]);
                if !commands.contains(&command) {
                    commands.push(command);
                }
                continue;
            }
            break;
        }
        (commands, rest)
    }

    /// The longest match at the end among commands that pass `pick`.
    fn at_end(&self, text: &str, pick: impl Fn(Command) -> bool) -> Option<(Command, usize)> {
        self.entries
            .iter()
            .filter(|e| pick(e.command))
            .filter_map(|e| Some((e.command, nonempty(e.end.find(text))?.start())))
            .min_by_key(|(_, start)| *start)
    }

    fn before(&self, rest: &str) -> String {
        let cut = self.tail.find(rest).map_or(rest.len(), |m| m.start());
        rest[..cut].to_string()
    }
}

/// An agent name counts only as a word of its own: "Claude.md" and "codex-smoke" are file names.
fn agent_break(after: &str) -> bool {
    after
        .chars()
        .next()
        .is_none_or(|c| c.is_whitespace() || matches!(c, ',' | ':' | '!' | '—' | '–'))
}

/// A zero-width match, such as a bare `\b` pattern, would peel nothing and loop forever.
fn nonempty(m: Option<regex::Match>) -> Option<regex::Match> {
    m.filter(|m| !m.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> (Option<Command>, String) {
        let (commands, rest) = Parser::new(&Patterns::default()).unwrap().parse(text);
        assert!(commands.len() <= 1, "{commands:?}");
        (commands.first().copied(), rest)
    }

    #[test]
    fn new_session_at_start_and_end() {
        let cases = [
            ("создай новую сессию и проверь diff", "проверь diff"),
            ("Проверь diff и создай новую сессию.", "Проверь diff"),
            ("New session, fix the tests", "fix the tests"),
            ("fix the tests in a new session", "fix the tests"),
            ("Открой в новой сессии: найди баг.", "найди баг."),
            ("в новой сессии проверь diff", "проверь diff"),
            (
                "start a new session and then refactor settings",
                "refactor settings",
            ),
        ];
        for (text, rest) in cases {
            assert_eq!(
                parse(text),
                (Some(Command::NewSession), rest.to_string()),
                "{text}"
            );
        }
    }

    #[test]
    fn commands_in_the_middle_are_ignored() {
        for text in [
            "расскажи про новую сессию в React и как её закрыть",
            "explain how a new session is created in the auth module",
            "проверь diff",
        ] {
            assert_eq!(parse(text), (None, text.to_string()), "{text}");
        }
    }

    #[test]
    fn cancel_only_at_the_end_and_wins() {
        assert_eq!(
            parse("новая сессия, проверь diff, отмена").0,
            Some(Command::Cancel)
        );
        assert_eq!(
            parse("fix the login bug. Scratch that.").0,
            Some(Command::Cancel)
        );
        let text = "отмени последний коммит";
        assert_eq!(parse(text), (None, text.to_string()));
    }

    #[test]
    fn open_terminal_alone() {
        assert_eq!(
            parse("Открой в терминале."),
            (Some(Command::OpenTerminal), String::new())
        );
        assert_eq!(
            parse("open terminal"),
            (Some(Command::OpenTerminal), String::new())
        );
    }

    #[test]
    fn patterns_matching_empty_are_rejected() {
        for bad in [".*", "(a)?", "("] {
            let patterns = Patterns {
                cancel: vec![bad.into()],
                ..Patterns::default()
            };
            let err = Parser::new(&patterns).err().unwrap();
            assert!(err.contains(bad), "{err}");
        }
    }

    #[test]
    fn zero_width_patterns_match_nothing() {
        let patterns = Patterns {
            new_session: vec![r"\b".into()],
            cancel: vec![r"\b".into()],
            ..Patterns::default()
        };
        let parser = Parser::new(&patterns).unwrap();
        assert_eq!(parser.parse("fix bug"), (vec![], "fix bug".to_string()));
    }

    #[test]
    fn user_patterns_replace_the_defaults() {
        let patterns = Patterns {
            new_session: vec!["с чистого листа".into()],
            ..Patterns::default()
        };
        let parser = Parser::new(&patterns).unwrap();
        assert_eq!(
            parser.parse("С чистого листа, напиши README"),
            (vec![Command::NewSession], "напиши README".to_string())
        );
        assert_eq!(parser.parse("новая сессия, напиши README").0, []);
    }

    #[test]
    fn several_commands_in_one_phrase() {
        let parser = Parser::new(&Patterns::default()).unwrap();
        let cases = [
            "Открой в терминале в новой сессии, проверь diff",
            "в новой сессии проверь diff и открой в терминале",
            "open in terminal, new session and check the diff",
        ];
        for text in cases {
            let (mut commands, rest) = parser.parse(text);
            commands.sort_by_key(|c| *c as u8);
            assert_eq!(
                commands,
                [Command::NewSession, Command::OpenTerminal],
                "{text}"
            );
            assert!(
                rest.contains("diff") && !rest.contains("сесси"),
                "{text}: {rest}"
            );
        }
    }

    #[test]
    fn cancel_overrides_other_commands() {
        let parser = Parser::new(&Patterns::default()).unwrap();
        assert_eq!(
            parser.parse("в новой сессии проверь diff, отмена").0,
            [Command::Cancel]
        );
    }

    #[test]
    fn agent_names_count_only_at_the_start() {
        let parser = Parser::new(&Patterns::default()).unwrap();
        assert_eq!(
            parser.parse("Codex, проверь diff"),
            (vec![Command::Codex], "проверь diff".to_string())
        );
        assert_eq!(
            parser.parse("в клоде напиши тесты"),
            (vec![Command::Claude], "напиши тесты".to_string())
        );
        for text in ["проверь diff в codex", "review this with claude"] {
            assert_eq!(parser.parse(text), (vec![], text.to_string()), "{text}");
        }
    }

    #[test]
    fn agent_and_terminal_combine() {
        let parser = Parser::new(&Patterns::default()).unwrap();
        let (mut commands, rest) = parser.parse("codex, открой в терминале, проверь diff");
        commands.sort_by_key(|c| *c as u8);
        assert_eq!(commands, [Command::OpenTerminal, Command::Codex]);
        assert_eq!(rest, "проверь diff");
    }

    #[test]
    fn commands_name_their_agent() {
        assert_eq!(Command::Codex.agent(), Some(crate::agent::Agent::Codex));
        assert_eq!(Command::NewSession.agent(), None);
    }

    #[test]
    fn agent_names_need_a_break_after_them() {
        let parser = Parser::new(&Patterns::default()).unwrap();
        for text in [
            "Claude.md обнови",
            "codex-smoke почини",
            "CLAUDE.md: add a rule",
        ] {
            assert_eq!(parser.parse(text), (vec![], text.to_string()), "{text}");
        }
        for text in [
            "codex: проверь diff",
            "Codex! проверь diff",
            "codex — проверь diff",
        ] {
            assert_eq!(parser.parse(text).0, [Command::Codex], "{text}");
        }
        assert_eq!(parser.parse("codex").0, [Command::Codex]);
    }
}
