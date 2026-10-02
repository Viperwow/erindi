//! A model behind an OpenAI-compatible HTTP API (LM Studio, Ollama, a cloud service) as an agent:
//! a phrase goes to `/chat/completions` and the streamed reply arrives as the run's events.

use std::io::BufRead;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde_json::{Value, json};

use crate::run::RunEnd;
use crate::stream::RunEvent;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

pub struct ApiConfig {
    pub base_url: String,
    pub key: Option<String>,
    pub model: String,
}

/// One earlier phrase of the conversation and the model's reply to it, if it gave one.
pub struct Turn {
    pub prompt: String,
    pub reply: Option<String>,
}

/// The conversation so far followed by `prompt`, as chat messages.
pub fn messages(history: &[Turn], prompt: &str) -> Vec<Value> {
    let mut out = vec![];
    for turn in history {
        out.push(json!({ "role": "user", "content": turn.prompt }));
        if let Some(reply) = &turn.reply {
            out.push(json!({ "role": "assistant", "content": reply }));
        }
    }
    out.push(json!({ "role": "user", "content": prompt }));
    out
}

fn base(url: &str) -> &str {
    url.trim().trim_end_matches('/')
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_connect(Some(CONNECT_TIMEOUT))
        .http_status_as_error(false)
        .build()
        .into()
}

fn unreachable(base_url: &str) -> String {
    format!("Cannot reach {} — is the server running?", base(base_url))
}

/// The error text for a failed status, with the server's own message when it gives one.
fn status_error(status: u16, body: &str) -> String {
    if status == 401 || status == 403 {
        return "The API key was rejected".into();
    }
    let message = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| error_message(&v));
    match message {
        Some(m) => format!("The server answered {status}: {m}"),
        None => format!("The server answered {status}"),
    }
}

fn error_message(v: &Value) -> Option<String> {
    let e = v.get("error")?;
    Some(
        e["message"]
            .as_str()
            .or(e.as_str())
            .unwrap_or("error")
            .to_string(),
    )
}

/// Sends `messages` and reports the reply as it streams: a `Reply` with the text so far after
/// every chunk that adds text, then one `Result`, unless `cancel` is set first.
pub fn stream_chat(
    config: &ApiConfig,
    messages: &[Value],
    cancel: &AtomicBool,
    mut on_event: impl FnMut(RunEvent),
) -> RunEnd {
    let finish = |ok: bool, text: String, on_event: &mut dyn FnMut(RunEvent)| {
        on_event(RunEvent::Result { ok, text });
        RunEnd::Exited { success: ok }
    };
    let mut request = agent().post(format!("{}/chat/completions", base(&config.base_url)));
    if let Some(key) = config.key.as_deref().filter(|k| !k.is_empty()) {
        request = request.header("Authorization", format!("Bearer {key}"));
    }
    let body = json!({ "model": config.model, "messages": messages, "stream": true });
    let response = match request.send_json(body) {
        Ok(r) => r,
        Err(_) => return finish(false, unreachable(&config.base_url), &mut on_event),
    };
    let status = response.status().as_u16();
    let streamed = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("text/event-stream"));
    let mut body = response.into_body();
    if status >= 400 {
        let text = body.read_to_string().unwrap_or_default();
        return finish(false, status_error(status, &text), &mut on_event);
    }
    if !streamed {
        let v: Value = body.read_json().unwrap_or(Value::Null);
        return match (
            error_message(&v),
            v["choices"][0]["message"]["content"].as_str(),
        ) {
            (Some(e), _) => finish(false, e, &mut on_event),
            (None, Some(text)) if !spoken(text).is_empty() => {
                finish(true, spoken(text).to_string(), &mut on_event)
            }
            (None, _) => finish(false, "The server gave no reply".into(), &mut on_event),
        };
    }
    let mut text = String::new();
    let mut shown = 0;
    for line in std::io::BufReader::new(body.into_reader()).lines() {
        if cancel.load(Ordering::SeqCst) {
            return RunEnd::Cancelled;
        }
        let Ok(line) = line else { break };
        let Some(data) = line.strip_prefix("data:").map(str::trim) else {
            continue;
        };
        if data == "[DONE]" {
            break;
        }
        let Ok(chunk) = serde_json::from_str::<Value>(data) else {
            continue;
        };
        if let Some(e) = error_message(&chunk) {
            return finish(false, e, &mut on_event);
        }
        if let Some(piece) = chunk["choices"][0]["delta"]["content"]
            .as_str()
            .filter(|p| !p.is_empty())
        {
            text.push_str(piece);
            let said = spoken(&text);
            if !said.is_empty() && said.len() != shown {
                shown = said.len();
                on_event(RunEvent::Reply {
                    text: said.to_string(),
                });
            }
        }
    }
    if cancel.load(Ordering::SeqCst) {
        return RunEnd::Cancelled;
    }
    // A stream cut before `[DONE]` still gave whatever text arrived.
    match spoken(&text) {
        "" => finish(false, "The server gave no reply".into(), &mut on_event),
        said => finish(true, said.to_string(), &mut on_event),
    }
}

/// The reply without the `<think>` block that reasoning models put before it. While that block
/// is still open, or its tag is still arriving, there is nothing to say yet.
fn spoken(text: &str) -> &str {
    const OPEN: &str = "<think>";
    let head = text.trim_start();
    if OPEN.starts_with(head) {
        return "";
    }
    let Some(inner) = head.strip_prefix(OPEN) else {
        return text;
    };
    match inner.find("</think>") {
        Some(end) => inner[end + "</think>".len()..].trim_start(),
        None => "",
    }
}

/// The model ids the server offers.
pub fn list_models(base_url: &str, key: Option<&str>) -> Result<Vec<String>, String> {
    let mut request = agent().get(format!("{}/models", base(base_url)));
    if let Some(key) = key.filter(|k| !k.is_empty()) {
        request = request.header("Authorization", format!("Bearer {key}"));
    }
    let response = request.call().map_err(|_| unreachable(base_url))?;
    let status = response.status().as_u16();
    let mut body = response.into_body();
    let text = body.read_to_string().map_err(|e| e.to_string())?;
    if status >= 400 {
        return Err(status_error(status, &text));
    }
    let v: Value = serde_json::from_str(&text)
        .map_err(|_| "The server's model list is not JSON".to_string())?;
    Ok(v["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| m["id"].as_str().map(String::from))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread::JoinHandle;

    /// Answers one request with `response` verbatim; gives the base URL and the request it read.
    fn serve(response: &'static str) -> (String, JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}/v1", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut head = String::new();
            let mut length = 0;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = v.trim().parse().unwrap();
                }
                head.push_str(&line);
                if line == "\r\n" {
                    break;
                }
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            let mut stream = stream;
            stream.write_all(response.as_bytes()).unwrap();
            head + &String::from_utf8(body).unwrap()
        });
        (base, handle)
    }

    const SSE: &str =
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n";

    fn config(base: &str, key: Option<&str>) -> ApiConfig {
        ApiConfig {
            base_url: base.into(),
            key: key.map(String::from),
            model: "m".into(),
        }
    }

    fn chat(base: &str, key: Option<&str>) -> (RunEnd, Vec<RunEvent>) {
        let mut events = vec![];
        let end = stream_chat(
            &config(base, key),
            &messages(&[], "hi"),
            &AtomicBool::new(false),
            |e| events.push(e),
        );
        (end, events)
    }

    fn replies(events: &[RunEvent]) -> Vec<&str> {
        events
            .iter()
            .filter_map(|e| match e {
                RunEvent::Reply { text } => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }

    fn result(events: &[RunEvent]) -> Option<(bool, &str)> {
        events.iter().find_map(|e| match e {
            RunEvent::Result { ok, text } => Some((*ok, text.as_str())),
            _ => None,
        })
    }

    #[test]
    fn messages_alternate_user_and_assistant() {
        let history = [
            Turn {
                prompt: "a".into(),
                reply: Some("ra".into()),
            },
            Turn {
                prompt: "b".into(),
                reply: Some("rb".into()),
            },
        ];
        let roles: Vec<_> = messages(&history, "c")
            .iter()
            .map(|m| m["role"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(roles, ["user", "assistant", "user", "assistant", "user"]);
        assert_eq!(messages(&history, "c")[4]["content"], "c");
    }

    #[test]
    fn a_streamed_reply_grows_then_ends() {
        let (base, _) = serve(concat!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"Hel\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\" you\"}}]}\n\n",
            "data: [DONE]\n\n"
        ));
        let (end, events) = chat(&base, None);
        assert_eq!(replies(&events), ["Hel", "Hello", "Hello you"]);
        assert_eq!(result(&events), Some((true, "Hello you")));
        assert_eq!(end, RunEnd::Exited { success: true });
    }

    #[test]
    fn the_request_names_the_model_and_streams() {
        let (base, request) = serve(concat!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
            "data: [DONE]\n\n"
        ));
        chat(&base, Some("k"));
        let request = request.join().unwrap();
        assert!(
            request.starts_with("POST /v1/chat/completions"),
            "{request}"
        );
        assert!(
            request
                .to_ascii_lowercase()
                .contains("authorization: bearer k")
        );
        let body: serde_json::Value =
            serde_json::from_str(&request[request.find("\r\n\r\n").unwrap() + 4..]).unwrap();
        assert_eq!(body["model"], "m");
        assert_eq!(body["stream"], true);

        let (base, request) = serve(concat!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
            "data: [DONE]\n\n"
        ));
        chat(&base, None);
        assert!(
            !request
                .join()
                .unwrap()
                .to_ascii_lowercase()
                .contains("authorization")
        );
    }

    #[test]
    fn keep_alives_and_empty_chunks_are_skipped() {
        let (base, _) = serve(concat!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
            ": ping\n\n",
            "\n",
            "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"hmm\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"a\"}}]}\n\n",
            ": ping\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"b\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            "data: [DONE]\n\n"
        ));
        let (_, events) = chat(&base, None);
        assert_eq!(replies(&events), ["a", "ab"]);
        assert_eq!(result(&events), Some((true, "ab")));
    }

    #[test]
    fn thinking_is_left_out_of_the_reply() {
        let (base, _) = serve(concat!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"<thi\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"nk>let me see</th\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"ink>\\n\\nParis\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\" it is.\"}}]}\n\n",
            "data: [DONE]\n\n"
        ));
        let (_, events) = chat(&base, None);
        assert_eq!(replies(&events), ["Paris", "Paris it is."]);
        assert_eq!(result(&events), Some((true, "Paris it is.")));
    }

    #[test]
    fn a_reply_that_never_ends_its_thinking_has_no_text() {
        let (base, _) = serve(concat!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"<think>still thinking\"}}]}\n\n",
            "data: [DONE]\n\n"
        ));
        let (_, events) = chat(&base, None);
        assert!(replies(&events).is_empty());
        assert_eq!(result(&events).map(|(ok, _)| ok), Some(false));
    }

    #[test]
    fn a_plain_json_body_is_the_reply() {
        let (base, _) = serve(concat!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n",
            "{\"choices\":[{\"message\":{\"role\":\"assistant\",\"content\":\"hi\"}}]}"
        ));
        let (end, events) = chat(&base, None);
        assert_eq!(result(&events), Some((true, "hi")));
        assert_eq!(end, RunEnd::Exited { success: true });
    }

    #[test]
    fn an_error_in_the_stream_fails_the_run() {
        let (base, _) = serve(concat!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
            "data: {\"error\":{\"message\":\"model not found\"}}\n\n"
        ));
        let (end, events) = chat(&base, None);
        let (ok, text) = result(&events).unwrap();
        assert!(!ok && text.contains("model not found"), "{text}");
        assert_eq!(end, RunEnd::Exited { success: false });
    }

    #[test]
    fn a_rejected_key_says_so() {
        let (base, _) =
            serve("HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        let (_, events) = chat(&base, Some("bad"));
        assert_eq!(result(&events), Some((false, "The API key was rejected")));
    }

    #[test]
    fn other_statuses_show_the_server_message() {
        let (base, _) = serve(concat!(
            "HTTP/1.1 500 Internal Server Error\r\nContent-Type: application/json\r\nContent-Length: 28\r\nConnection: close\r\n\r\n",
            "{\"error\":{\"message\":\"boom\"}}"
        ));
        let (_, events) = chat(&base, None);
        let (ok, text) = result(&events).unwrap();
        assert!(
            !ok && text.contains("500") && text.contains("boom"),
            "{text}"
        );
    }

    #[test]
    fn an_unreachable_server_names_the_address() {
        let base = {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            format!("http://{}/v1", listener.local_addr().unwrap())
        };
        let (_, events) = chat(&base, None);
        let expected = format!("Cannot reach {base} — is the server running?");
        assert_eq!(result(&events), Some((false, expected.as_str())));
    }

    #[test]
    fn a_cut_stream_keeps_its_text() {
        let (base, _) = serve(concat!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"part\"}}]}\n\n"
        ));
        let (_, events) = chat(&base, None);
        assert_eq!(result(&events), Some((true, "part")));

        let (base, _) = serve(SSE);
        let (_, events) = chat(&base, None);
        assert_eq!(result(&events).map(|(ok, _)| ok), Some(false));
    }

    #[test]
    fn cancel_ends_without_a_result() {
        let (base, _) = serve(concat!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"a\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"b\"}}]}\n\n",
            "data: [DONE]\n\n"
        ));
        let cancel = AtomicBool::new(false);
        let mut events = vec![];
        let end = stream_chat(&config(&base, None), &messages(&[], "hi"), &cancel, |e| {
            cancel.store(true, Ordering::SeqCst);
            events.push(e);
        });
        assert_eq!(end, RunEnd::Cancelled);
        assert_eq!(replies(&events), ["a"]);
        assert_eq!(result(&events), None);
    }

    #[test]
    fn a_trailing_slash_is_ignored() {
        let (base, request) = serve(concat!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
            "data: [DONE]\n\n"
        ));
        chat(&format!("  {base}/ "), None);
        assert!(
            request
                .join()
                .unwrap()
                .starts_with("POST /v1/chat/completions ")
        );
    }

    #[test]
    fn models_are_listed() {
        let (base, request) = serve(concat!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 32\r\nConnection: close\r\n\r\n",
            "{\"data\":[{\"id\":\"a\"},{\"id\":\"b\"}]}"
        ));
        assert_eq!(
            list_models(&base, None),
            Ok(vec!["a".to_string(), "b".to_string()])
        );
        assert!(request.join().unwrap().starts_with("GET /v1/models"));
    }
}
