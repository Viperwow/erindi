use serde::Deserialize;
use serde_json::{Value, json};

use crate::commands::Command;

/// Sent once after the server starts, so the first real utterance does not pay for a cold cache.
pub const WARM_UP: &str = "давай с чистого листа, проверь diff";

pub const SYSTEM_PROMPT: &str = r#"You find a voice command at the start or end of a dictated phrase for a coding agent.
Commands: "new_session" (start fresh, a new chat or session), "open_terminal" (show or open the session in a terminal), "cancel" (at the end only: never mind, forget it, don't send). Anything else is "none".
Return JSON {"command": ..., "rest": ...}. "rest" is the phrase with the command words removed, copied exactly, in the original language. Never rewrite, translate or answer. If the command is in the middle of the phrase or there is no command, return "none" and the whole phrase as "rest".

Input: давай с чистого листа, напиши README
Output: {"command":"new_session","rest":"напиши README"}
Input: let's start fresh and write a CLI for the benchmark
Output: {"command":"new_session","rest":"write a CLI for the benchmark"}
Input: покажи эту сессию в терминале
Output: {"command":"open_terminal","rest":""}
Input: проверь diff, хотя нет, забудь
Output: {"command":"cancel","rest":"проверь diff"}
Input: fix the login bug, never mind
Output: {"command":"cancel","rest":"fix the login bug"}
Input: расскажи, как начать новую сессию в React
Output: {"command":"none","rest":"расскажи, как начать новую сессию в React"}
Input: проверь diff в модуле auth
Output: {"command":"none","rest":"проверь diff в модуле auth"}"#;

pub fn request(text: &str) -> Value {
    json!({
        "messages": [
            {"role": "system", "content": SYSTEM_PROMPT},
            {"role": "user", "content": text},
        ],
        "temperature": 0,
        "seed": 42,
        "cache_prompt": true,
        "max_tokens": text.chars().count() + 48,
        "response_format": {
            "type": "json_schema",
            "json_schema": {"name": "command", "strict": true, "schema": {
                "type": "object",
                "properties": {
                    "command": {"type": "string", "enum": ["new_session", "open_terminal", "cancel", "none"]},
                    "rest": {"type": "string"},
                },
                "required": ["command", "rest"],
                "additionalProperties": false,
            }},
        },
    })
}

/// The model's command, `None` for "none", and its rest of the phrase.
pub fn parse_response(body: &Value) -> Option<(Option<Command>, String)> {
    #[derive(Deserialize)]
    struct Reply {
        command: String,
        rest: String,
    }
    let content = body["choices"][0]["message"]["content"].as_str()?;
    let reply: Reply = serde_json::from_str(content).ok()?;
    let command = match reply.command.as_str() {
        "new_session" => Some(Command::NewSession),
        "open_terminal" => Some(Command::OpenTerminal),
        "cancel" => Some(Command::Cancel),
        "none" => None,
        _ => return None,
    };
    Some((command, reply.rest))
}

/// Accepts a command only when the model removed words from one edge of `text` and changed
/// nothing else. The rest is cut from `text` itself, so the model's wording never reaches the agent.
pub fn accept(text: &str, answer: Option<(Option<Command>, String)>) -> Option<(Command, String)> {
    let (command, rest) = answer?;
    let command = command?;
    // Words with their byte spans; punctuation-only tokens such as "--" carry nothing to compare
    // but stay in the text that is cut.
    let mut spans = vec![];
    let mut at = 0;
    for token in text.split_whitespace() {
        let start = at + text[at..].find(token).unwrap_or(0);
        at = start + token.len();
        let word = bare(token);
        if !word.is_empty() {
            spans.push((word, start, at));
        }
    }
    let kept: Vec<String> = rest
        .split_whitespace()
        .map(bare)
        .filter(|w| !w.is_empty())
        .collect();
    if kept.len() >= spans.len() {
        return None;
    }
    let same = |words: &[(String, usize, usize)]| words.iter().map(|w| &w.0).eq(kept.iter());
    let cut = spans.len() - kept.len();
    if command != Command::Cancel && same(&spans[cut..]) {
        let from = spans.get(cut).map_or(text.len(), |w| w.1);
        return Some((command, text[from..].to_string()));
    }
    if same(&spans[..kept.len()]) {
        let to = kept.len().checked_sub(1).map_or(0, |i| spans[i].2);
        let rest = text[..to].trim_end_matches([',', ';', ':', '-', ' ']);
        return Some((command, rest.to_string()));
    }
    None
}

/// Lowercase word without surrounding punctuation.
fn bare(word: &str) -> String {
    word.trim_matches(|c: char| !c.is_alphanumeric())
        .to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn reply(content: &str) -> Value {
        json!({"choices": [{"message": {"role": "assistant", "content": content}}]})
    }

    fn answer(command: Option<Command>, rest: &str) -> Option<(Option<Command>, String)> {
        Some((command, rest.into()))
    }

    #[test]
    fn request_is_typed_and_deterministic() {
        let body = request("давай с чистого листа, напиши README");
        assert_eq!(body["temperature"], 0);
        assert_eq!(
            body["messages"][1]["content"],
            "давай с чистого листа, напиши README"
        );
        let schema = &body["response_format"]["json_schema"]["schema"];
        assert_eq!(
            schema["properties"]["command"]["enum"],
            json!(["new_session", "open_terminal", "cancel", "none"])
        );
    }

    #[test]
    fn parses_the_answer() {
        let r = parse_response(&reply(
            r#"{"command":"new_session","rest":"напиши README"}"#,
        ));
        assert_eq!(r, answer(Some(Command::NewSession), "напиши README"));
        let r = parse_response(&reply(r#"{"command":"none","rest":"x"}"#));
        assert_eq!(r, answer(None, "x"));
        assert_eq!(parse_response(&reply("nope")), None);
        assert_eq!(
            parse_response(&reply(r#"{"command":"shutdown","rest":""}"#)),
            None
        );
    }

    #[test]
    fn edge_cut_is_accepted_and_taken_from_the_transcript() {
        let text = "Давай с чистого листа, напиши README для проекта.";
        assert_eq!(
            accept(
                text,
                answer(Some(Command::NewSession), "напиши readme для проекта")
            ),
            Some((Command::NewSession, "напиши README для проекта.".into()))
        );
        let text = "Напиши README, а потом начнём всё заново";
        assert_eq!(
            accept(text, answer(Some(Command::NewSession), "напиши README")),
            Some((Command::NewSession, "Напиши README".into()))
        );
        assert_eq!(
            accept(
                "покажи мне сессию в терминале",
                answer(Some(Command::OpenTerminal), "")
            ),
            Some((Command::OpenTerminal, String::new()))
        );
    }

    #[test]
    fn rewritten_rest_is_rejected() {
        let text = "давай с чистого листа, напиши README";
        assert_eq!(
            accept(
                text,
                answer(Some(Command::NewSession), "Создай файл README.md")
            ),
            None
        );
    }

    #[test]
    fn middle_cut_is_rejected() {
        let text = "проверь diff давай заново и тесты";
        assert_eq!(
            accept(
                text,
                answer(Some(Command::NewSession), "проверь diff и тесты")
            ),
            None
        );
    }

    #[test]
    fn none_and_uncut_answers_are_rejected() {
        let text = "проверь diff";
        assert_eq!(accept(text, answer(None, "")), None);
        assert_eq!(
            accept(text, answer(Some(Command::NewSession), "проверь diff")),
            None
        );
        assert_eq!(accept(text, None), None);
    }

    #[test]
    fn cancel_counts_only_at_the_end() {
        let text = "забудь, проверь diff";
        assert_eq!(
            accept(text, answer(Some(Command::Cancel), "проверь diff")),
            None
        );
        let text = "проверь diff, хотя нет, забудь";
        assert_eq!(
            accept(text, answer(Some(Command::Cancel), "проверь diff")),
            Some((Command::Cancel, "проверь diff".into()))
        );
    }

    #[test]
    fn rest_keeps_the_transcript_exactly() {
        let text = "let's start fresh, run cargo -- --ignored";
        assert_eq!(
            accept(
                text,
                answer(Some(Command::NewSession), "run cargo -- --ignored")
            ),
            Some((Command::NewSession, "run cargo -- --ignored".into()))
        );
        let text = "run cargo  --  --ignored, never mind";
        assert_eq!(
            accept(
                text,
                answer(Some(Command::Cancel), "run cargo -- --ignored")
            ),
            Some((Command::Cancel, "run cargo  --  --ignored".into()))
        );
    }
}
