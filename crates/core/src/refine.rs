use serde::Deserialize;
use serde_json::{Value, json};

use crate::session::Intent;

#[derive(Debug, Clone, PartialEq)]
pub struct Refined {
    pub intent: Intent,
    pub text: String,
}

/// Sent once after the server starts, so the first real utterance does not pay for a cold cache.
pub const WARM_UP: &str = "эээ, короче, проверь diff в модуле auth";

pub const SYSTEM_PROMPT: &str = r#"You clean up dictated instructions for a coding agent.
Return JSON: {"intent": "new" | "continue" | "none", "text": "..."}.
- text: the instruction itself. Remove fillers (эээ, ну, короче, типа, um, like), false starts, repeats and spoken session commands. Keep the language, meaning, names and technical terms. Fix punctuation.
- Never answer, execute or expand the instruction. Add nothing.
- intent: "new" if the speaker asks for a new or fresh session, "continue" if they ask for the same or current session, otherwise "none".

Input: эээ ну проверь diff в модуле auth
Output: {"intent":"none","text":"Проверь diff в модуле auth."}
Input: создай новую сессию и, короче, найди почему падают тесты
Output: {"intent":"new","text":"Найди, почему падают тесты."}
Input: давай с чистого листа, напиши README для проекта
Output: {"intent":"new","text":"Напиши README для проекта."}
Input: в той же сессии добавь, ну, тесты на парсер
Output: {"intent":"continue","text":"Добавь тесты на парсер."}
Input: напиши функцию сортировки на Rust
Output: {"intent":"none","text":"Напиши функцию сортировки на Rust."}
Input: исправь, нет, вернее проверь, почему билд красный
Output: {"intent":"none","text":"Проверь, почему билд красный."}
Input: расскажи про новую сессию в React
Output: {"intent":"none","text":"Расскажи про новую сессию в React."}
Input: um so like fix the the login bug in auth dot rs
Output: {"intent":"none","text":"Fix the login bug in auth.rs."}
Input: start a new session and uh refactor the settings page
Output: {"intent":"new","text":"Refactor the settings page."}
Input: same session, now run clippy and fix the warnings
Output: {"intent":"continue","text":"Now run clippy and fix the warnings."}
Input: explain how a new session is created in the auth module
Output: {"intent":"none","text":"Explain how a new session is created in the auth module."}
Input: what does this function do
Output: {"intent":"none","text":"What does this function do?"}"#;

pub fn request(text: &str) -> Value {
    json!({
        "messages": [
            {"role": "system", "content": SYSTEM_PROMPT},
            {"role": "user", "content": text},
        ],
        "temperature": 0,
        "seed": 42,
        "cache_prompt": true,
        // Roughly 1.5x the input tokens plus room for the JSON wrapper.
        "max_tokens": text.chars().count() * 3 / 4 + 64,
        "response_format": {
            "type": "json_schema",
            "json_schema": {"name": "refined", "strict": true, "schema": {
                "type": "object",
                "properties": {
                    "intent": {"type": "string", "enum": ["new", "continue", "none"]},
                    "text": {"type": "string"},
                },
                "required": ["intent", "text"],
                "additionalProperties": false,
            }},
        },
    })
}

pub fn parse_response(body: &Value) -> Option<Refined> {
    #[derive(Deserialize)]
    struct Reply {
        intent: String,
        text: String,
    }
    let content = body["choices"][0]["message"]["content"].as_str()?;
    let reply: Reply = serde_json::from_str(content).ok()?;
    let intent = match reply.intent.as_str() {
        "new" => Intent::New,
        "continue" => Intent::Continue,
        "none" => Intent::Unspecified,
        _ => return None,
    };
    Some(Refined {
        intent,
        text: reply.text.trim().to_string(),
    })
}

/// Rejects output that is empty or far from the input's length, which is how an answer
/// or a lost sentence usually looks.
pub fn accept(input: &str, refined: Option<Refined>) -> Option<Refined> {
    let refined = refined?;
    let (inp, out) = (
        input.chars().count() as f64,
        refined.text.chars().count() as f64,
    );
    (out > 0.0 && out >= inp * 0.3 && out <= inp * 1.5).then_some(refined)
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn reply(content: &str) -> serde_json::Value {
        json!({"choices": [{"message": {"role": "assistant", "content": content}}]})
    }

    #[test]
    fn request_is_deterministic_and_typed() {
        let body = request("эээ проверь diff");
        assert_eq!(body["temperature"], 0);
        assert_eq!(body["cache_prompt"], true);
        assert_eq!(body["messages"][0]["content"], SYSTEM_PROMPT);
        assert_eq!(body["messages"][1]["content"], "эээ проверь diff");
        let schema = &body["response_format"]["json_schema"]["schema"];
        assert_eq!(
            schema["properties"]["intent"]["enum"],
            json!(["new", "continue", "none"])
        );
        assert!(body["max_tokens"].as_u64().unwrap() >= 64);
    }

    #[test]
    fn parses_intent_and_text() {
        let r = parse_response(&reply(r#"{"intent":"new","text":" Проверь diff. "}"#)).unwrap();
        assert_eq!(
            r,
            Refined {
                intent: Intent::New,
                text: "Проверь diff.".into()
            }
        );
        let r = parse_response(&reply(r#"{"intent":"none","text":"fix it"}"#)).unwrap();
        assert_eq!(r.intent, Intent::Unspecified);
    }

    #[test]
    fn broken_replies_are_none() {
        assert_eq!(parse_response(&reply("not json")), None);
        assert_eq!(
            parse_response(&reply(r#"{"intent":"maybe","text":"x"}"#)),
            None
        );
        assert_eq!(parse_response(&json!({"error": "busy"})), None);
    }

    #[test]
    fn answers_are_rejected() {
        let input = "напиши функцию сортировки";
        let answer = Refined {
            intent: Intent::Unspecified,
            text: "fn sort(v: &mut Vec<i32>) { v.sort(); } // Вот функция сортировки на Rust."
                .into(),
        };
        assert_eq!(accept(input, Some(answer)), None);
    }

    #[test]
    fn cleaned_text_within_bounds_is_accepted() {
        let input = "эээ ну короче проверь diff в модуле auth";
        let ok = Refined {
            intent: Intent::Unspecified,
            text: "Проверь diff в модуле auth.".into(),
        };
        assert_eq!(accept(input, Some(ok.clone())), Some(ok));
        let empty = Refined {
            intent: Intent::Unspecified,
            text: String::new(),
        };
        assert_eq!(accept(input, Some(empty)), None);
        assert_eq!(accept(input, None), None);
    }
}
