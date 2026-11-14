//! Talking to the service: one streamed request in either of two protocols, the answer handed
//! out piece by piece. Nothing of the service's own words is put into an error, so the key
//! cannot come back out through one; the key itself is only ever a header of the request.

use std::{fmt, time::Duration};

use reqwest::header::HeaderValue;
use serde_json::{json, Value};
use tokio::time::timeout;

use super::{
    settings,
    sse::{SseParser, ThinkFilter, MAX_HIDDEN},
};
use crate::{model::AiProvider, Error, Result};

/// The Messages API version header. Older versions stay valid, so this one is the safe choice.
const ANTHROPIC_VERSION: &str = "2023-06-01";
/// A cap on one Anthropic answer; the field is required there.
const ANTHROPIC_MAX_TOKENS: u32 = 16_000;

/// More than this many characters of visible answer is a model that does not stop.
const MAX_ANSWER_CHARS: usize = 100_000;

/// How long the service may be silent.
#[derive(Debug, Clone, Copy)]
pub struct Timeouts {
    /// To reach it. A dead address fails fast.
    pub connect: Duration,
    /// Until the first visible text: the service may be loading a model or reading a long
    /// request (a model on the user's own machine sends nothing before its first word), or a
    /// reasoning model may be thinking.
    pub first: Duration,
    /// Without a byte between the pieces of the answer after that. There is no limit on the
    /// whole answer's time, only on its size.
    pub idle: Duration,
}

pub const TIMEOUTS: Timeouts =
    Timeouts { connect: Duration::from_secs(30), first: Duration::from_secs(300), idle: Duration::from_secs(60) };

fn cut_off() -> Error {
    Error::temporary("the answer was cut off by the service's length limit")
}

fn declined() -> Error {
    Error::temporary("the model declined to answer this request")
}

/// Where a request goes. The key is shown by `Debug` as set or not, never as itself.
#[derive(Clone)]
pub struct Endpoint {
    pub provider: AiProvider,
    pub base_url: String,
    pub model: String,
    pub key: Option<String>,
}

impl fmt::Debug for Endpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Endpoint")
            .field("provider", &self.provider)
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field("key", &if self.key.is_some() { "set" } else { "none" })
            .finish()
    }
}

/// The request as it goes out.
pub struct Parts {
    pub url: String,
    /// Header names and values, the key's among them.
    pub headers: Vec<(&'static str, String)>,
    pub body: Value,
}

/// Builds the request for `system` and `user`.
pub fn request_parts(ep: &Endpoint, system: &str, user: &str) -> Parts {
    match ep.provider {
        AiProvider::OpenAiCompatible => Parts {
            url: format!("{}/chat/completions", ep.base_url),
            headers: ep.key.iter().map(|k| ("authorization", format!("Bearer {k}"))).collect(),
            body: json!({
                "model": ep.model,
                "stream": true,
                "messages": [
                    {"role": "system", "content": system},
                    {"role": "user", "content": user},
                ],
            }),
        },
        AiProvider::Anthropic => Parts {
            url: format!("{}/messages", ep.base_url),
            headers: ep
                .key
                .iter()
                .map(|k| ("x-api-key", k.clone()))
                .chain([("anthropic-version", ANTHROPIC_VERSION.to_string())])
                .collect(),
            body: json!({
                "model": ep.model,
                "max_tokens": ANTHROPIC_MAX_TOKENS,
                "stream": true,
                "system": system,
                "messages": [{"role": "user", "content": user}],
            }),
        },
    }
}

/// What one event of the stream said.
#[derive(Debug, Default, PartialEq)]
pub struct Parsed {
    pub text: String,
    /// The service said why the answer ended; the stream may still carry a last event or two.
    pub finished: bool,
    /// The service said the stream is over.
    pub stop: bool,
}

/// One `data` payload of an OpenAI-style stream. Only `content` is read: `reasoning_content`
/// and the like are never looked at. Payloads that are not JSON are skipped. An answer the
/// service ended at its length limit or held back is an error, not a complete answer.
pub fn parse_openai(data: &str) -> Result<Parsed> {
    if data.trim() == "[DONE]" {
        return Ok(Parsed { stop: true, ..Parsed::default() });
    }
    let Ok(event) = serde_json::from_str::<Value>(data) else { return Ok(Parsed::default()) };
    if event.get("error").is_some_and(|e| !e.is_null()) {
        return Err(stream_error());
    }
    let choice = &event["choices"][0];
    match choice["finish_reason"].as_str() {
        Some("length") => return Err(cut_off()),
        Some("content_filter") => return Err(declined()),
        _ => {}
    }
    Ok(Parsed {
        text: choice["delta"]["content"].as_str().unwrap_or_default().to_string(),
        finished: choice["finish_reason"].as_str().is_some(),
        stop: false,
    })
}

/// One `data` payload of an Anthropic stream.
pub fn parse_anthropic(data: &str) -> Result<Parsed> {
    let Ok(event) = serde_json::from_str::<Value>(data) else { return Ok(Parsed::default()) };
    match event["type"].as_str() {
        Some("content_block_delta") if event["delta"]["type"] == "text_delta" => {
            Ok(Parsed { text: event["delta"]["text"].as_str().unwrap_or_default().to_string(), ..Parsed::default() })
        }
        Some("message_delta") if event["delta"]["stop_reason"] == "refusal" => Err(declined()),
        Some("message_delta") if event["delta"]["stop_reason"] == "max_tokens" => Err(cut_off()),
        Some("message_stop") => Ok(Parsed { stop: true, ..Parsed::default() }),
        Some("error") => Err(stream_error()),
        _ => Ok(Parsed::default()),
    }
}

fn stream_error() -> Error {
    Error::temporary("the service reported an error while answering")
}

/// What to tell the user about an HTTP status. The body of the answer is never read.
pub fn status_error(status: u16) -> Error {
    match status {
        401 | 403 => Error::temporary("the service refused the key"),
        404 => Error::temporary("the service does not know this model or address"),
        429 => Error::temporary("the service is busy or the quota is used up"),
        500..=599 => Error::temporary(format!("the service has a problem (status {status}); try again later")),
        _ => Error::temporary(format!("the service did not accept the request (status {status})")),
    }
}

/// Collects the visible answer and hands each new piece out. Leading blank space is held back
/// until there is a first word, so the pieces together equal the trimmed answer.
struct Sink<'a> {
    answer: String,
    /// Characters in `answer`.
    chars: usize,
    on_text: &'a mut (dyn FnMut(&str) + Send),
}

impl Sink<'_> {
    fn push(&mut self, piece: &str) -> Result<()> {
        let piece = if self.answer.is_empty() { piece.trim_start() } else { piece };
        if !piece.is_empty() {
            self.chars += piece.chars().count();
            if self.chars > MAX_ANSWER_CHARS {
                return Err(Error::temporary("the answer grew longer than expected and was stopped"));
            }
            self.answer.push_str(piece);
            (self.on_text)(piece);
        }
        Ok(())
    }
}

/// The state of one answer being read.
struct Reader<'a> {
    parse: fn(&str) -> Result<Parsed>,
    think: ThinkFilter,
    sink: Sink<'a>,
    finished: bool,
    stopped: bool,
}

impl Reader<'_> {
    fn event(&mut self, data: &str) -> Result<()> {
        let parsed = (self.parse)(data)?;
        let shown = self.think.push(&parsed.text);
        if self.think.hidden > MAX_HIDDEN {
            return Err(Error::temporary("the model kept thinking for too long and was stopped"));
        }
        self.sink.push(&shown)?;
        self.finished |= parsed.finished;
        self.stopped |= parsed.stop;
        Ok(())
    }
}

/// Sends the request and reads the answer. `on_text` hears each visible piece as it comes.
/// Returns the whole answer, trimmed (the caller shows this text in place of what it heard: it
/// can differ when a server left out the opening `<think>` tag); an answer that is empty, or
/// that the service cut off, is an error.
pub async fn complete(
    ep: &Endpoint,
    system: &str,
    user: &str,
    timeouts: Timeouts,
    on_text: &mut (dyn FnMut(&str) + Send),
) -> Result<String> {
    let url = settings::check_base_url(&ep.base_url)?;
    let host = settings::host_label(&url);
    if ep.provider == AiProvider::Anthropic && ep.key.is_none() {
        return Err(Error::invalid("this service needs a key"));
    }
    let parts = request_parts(ep, system, user);
    // No redirects: `x-api-key` would be sent along to wherever the service points.
    let client = reqwest::Client::builder()
        .connect_timeout(timeouts.connect)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| Error::temporary("could not set up the connection"))?;
    let mut request = client
        .post(&parts.url)
        .header("content-type", "application/json")
        .header("accept", "text/event-stream")
        .body(parts.body.to_string());
    for (name, value) in parts.headers {
        let mut value = HeaderValue::from_str(&value).map_err(|_| Error::invalid("the key cannot be sent"))?;
        value.set_sensitive(true);
        request = request.header(name, value);
    }
    let mut response = match timeout(timeouts.first, request.send()).await {
        Err(_) => return Err(Error::temporary("the service did not answer in time")),
        Ok(Err(e)) if e.is_timeout() => return Err(Error::temporary("the service did not answer in time")),
        Ok(Err(_)) => return Err(Error::temporary(format!("could not reach the service at {host}"))),
        Ok(Ok(response)) => response,
    };
    if !response.status().is_success() {
        return Err(status_error(response.status().as_u16()));
    }

    let mut reader = Reader {
        parse: match ep.provider {
            AiProvider::OpenAiCompatible => parse_openai,
            AiProvider::Anthropic => parse_anthropic,
        },
        think: ThinkFilter::default(),
        sink: Sink { answer: String::new(), chars: 0, on_text },
        finished: false,
        stopped: false,
    };
    let mut sse = SseParser::default();
    'stream: loop {
        let limit = if reader.sink.answer.is_empty() { timeouts.first } else { timeouts.idle };
        let chunk = match timeout(limit, response.chunk()).await {
            Err(_) => return Err(Error::temporary("the service stopped answering")),
            Ok(Err(_)) | Ok(Ok(None)) => break,
            Ok(Ok(Some(chunk))) => chunk,
        };
        for data in sse.feed(&chunk)? {
            reader.event(&data)?;
            if reader.stopped {
                break 'stream;
            }
        }
    }
    if let Some(data) = sse.finish()?.filter(|_| !reader.stopped) {
        reader.event(&data)?;
    }
    if !(reader.finished || reader.stopped) {
        return Err(Error::temporary("the connection to the service broke while it was answering"));
    }
    let rest = reader.think.finish();
    reader.sink.push(&rest)?;
    let answer = reader.sink.answer.trim();
    let answer = match ep.provider {
        AiProvider::OpenAiCompatible => reader.think.after_orphan_close(answer),
        AiProvider::Anthropic => answer,
    }
    .trim()
    .to_string();
    if answer.is_empty() {
        return Err(Error::temporary("the service sent an empty answer"));
    }
    Ok(answer)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ep(provider: AiProvider, key: Option<&str>) -> Endpoint {
        Endpoint {
            provider,
            base_url: "https://api.example.com/v1".into(),
            model: "m-1".into(),
            key: key.map(String::from),
        }
    }

    fn header<'a>(parts: &'a Parts, name: &str) -> Option<&'a str> {
        parts.headers.iter().find(|(n, _)| *n == name).map(|(_, v)| v.as_str())
    }

    #[test]
    fn the_openai_request_has_a_bearer_header_only_when_there_is_a_key() {
        let with = request_parts(&ep(AiProvider::OpenAiCompatible, Some("k-1")), "sys", "usr");
        assert_eq!(with.url, "https://api.example.com/v1/chat/completions");
        assert_eq!(header(&with, "authorization"), Some("Bearer k-1"));
        assert_eq!(header(&with, "x-api-key"), None);
        assert_eq!(
            with.body,
            json!({"model": "m-1", "stream": true, "messages": [
                {"role": "system", "content": "sys"}, {"role": "user", "content": "usr"}]})
        );
        let without = request_parts(&ep(AiProvider::OpenAiCompatible, None), "sys", "usr");
        assert!(without.headers.is_empty());
    }

    #[test]
    fn the_anthropic_request_has_the_key_the_version_and_the_system_prompt_apart() {
        let parts = request_parts(&ep(AiProvider::Anthropic, Some("k-2")), "sys", "usr");
        assert_eq!(parts.url, "https://api.example.com/v1/messages");
        assert_eq!(header(&parts, "x-api-key"), Some("k-2"));
        assert_eq!(header(&parts, "anthropic-version"), Some(ANTHROPIC_VERSION));
        assert_eq!(header(&parts, "authorization"), None);
        assert_eq!(
            parts.body,
            json!({"model": "m-1", "max_tokens": 16000, "stream": true, "system": "sys",
                   "messages": [{"role": "user", "content": "usr"}]})
        );
    }

    #[test]
    fn debug_never_shows_the_key() {
        let shown = format!("{:?}", ep(AiProvider::Anthropic, Some("sk-secret-123")));
        assert!(!shown.contains("secret") && shown.contains("set"));
    }

    #[test]
    fn openai_events_give_content_and_ignore_reasoning() {
        let text = |t: &str| Parsed { text: t.into(), ..Parsed::default() };
        assert_eq!(parse_openai(r#"{"choices":[{"delta":{"content":"Hi"}}]}"#).unwrap(), text("Hi"));
        assert_eq!(
            parse_openai(r#"{"choices":[{"delta":{"reasoning_content":"hmm","content":null}}]}"#).unwrap(),
            text("")
        );
        assert_eq!(parse_openai(r#"{"choices":[{"delta":{"reasoning":"hmm"}}]}"#).unwrap(), text(""));
        let end = parse_openai(r#"{"choices":[{"delta":{"content":"!"},"finish_reason":"stop"}]}"#).unwrap();
        assert_eq!(end, Parsed { text: "!".into(), finished: true, stop: false });
        assert!(parse_openai("[DONE]").unwrap().stop);
        assert_eq!(parse_openai(r#"{"choices":[]}"#).unwrap(), text(""));
        assert_eq!(parse_openai("not json").unwrap(), text(""));
        assert!(matches!(parse_openai(r#"{"error":{"message":"sk-leak"}}"#), Err(Error::Temporary { .. })));
        let ended =
            |reason: &str| parse_openai(&format!(r#"{{"choices":[{{"delta":{{}},"finish_reason":"{reason}"}}]}}"#));
        assert!(ended("tool_calls").unwrap().finished);
        assert!(matches!(ended("length"), Err(Error::Temporary { msg }) if msg.contains("length limit")));
        assert!(matches!(ended("content_filter"), Err(Error::Temporary { msg }) if msg.contains("declined")));
    }

    #[test]
    fn anthropic_events_give_text_deltas_and_end_on_message_stop() {
        let text = |t: &str| Parsed { text: t.into(), ..Parsed::default() };
        let delta = r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hi"}}"#;
        assert_eq!(parse_anthropic(delta).unwrap(), text("Hi"));
        let thinking = r#"{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"hmm"}}"#;
        assert_eq!(parse_anthropic(thinking).unwrap(), text(""));
        assert_eq!(parse_anthropic(r#"{"type":"ping"}"#).unwrap(), text(""));
        assert_eq!(
            parse_anthropic(r#"{"type":"message_delta","delta":{"stop_reason":"end_turn"}}"#).unwrap(),
            text("")
        );
        assert!(parse_anthropic(r#"{"type":"message_stop"}"#).unwrap().stop);
        let err = parse_anthropic(r#"{"type":"error","error":{"type":"overloaded_error","message":"sk-leak"}}"#);
        assert!(matches!(&err, Err(Error::Temporary { msg }) if !msg.contains("sk-leak")));
        let refusal = parse_anthropic(r#"{"type":"message_delta","delta":{"stop_reason":"refusal"}}"#);
        assert!(matches!(refusal, Err(Error::Temporary { .. })));
        let max = parse_anthropic(r#"{"type":"message_delta","delta":{"stop_reason":"max_tokens"}}"#);
        assert!(matches!(max, Err(Error::Temporary { msg }) if msg.contains("length limit")));
    }

    #[test]
    fn statuses_become_plain_messages() {
        let msg = |s| status_error(s).to_string();
        assert_eq!(msg(401), "the service refused the key");
        assert_eq!(msg(403), "the service refused the key");
        assert_eq!(msg(404), "the service does not know this model or address");
        assert_eq!(msg(429), "the service is busy or the quota is used up");
        assert!(msg(503).contains("status 503"));
        assert!(msg(400).contains("status 400"));
        assert!((401..=599).all(|s| matches!(status_error(s), Error::Temporary { .. })));
    }
}
