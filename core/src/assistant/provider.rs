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
    wire::{Reply, ToolCall, ToolSpec, ToolsRefused, Turn},
};
use crate::{
    model::{AiEffort, AiProvider},
    Error, Result,
};

/// The Messages API version header. We send no sampling parameters, so nothing else depends on it.
pub const ANTHROPIC_VERSION: &str = "2026-10-21";
/// A cap on one Anthropic answer; the field is required there, and thinking counts against it.
const ANTHROPIC_MAX_TOKENS: u32 = 32_000;

/// The most that is read of one answer that is not a stream (a list of models, or a whole chat
/// answer from a service that ignored `stream`).
pub const MAX_BODY: usize = 2 * 1024 * 1024;

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
    /// Sent to Anthropic only, and only when it is not `Default`.
    pub effort: AiEffort,
    pub key: Option<String>,
}

impl fmt::Debug for Endpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Endpoint")
            .field("provider", &self.provider)
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field("effort", &self.effort)
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
            body: {
                let mut body = json!({
                    "model": ep.model,
                    "max_tokens": ANTHROPIC_MAX_TOKENS,
                    "stream": true,
                    "system": system,
                    "messages": [{"role": "user", "content": user}],
                });
                if let Some(level) = effort_name(ep.effort) {
                    body["output_config"] = json!({"effort": level});
                }
                body
            },
        },
    }
}

/// The word the Messages API uses for an effort; `None` for `Default`, which is not sent.
pub fn effort_name(effort: AiEffort) -> Option<&'static str> {
    match effort {
        AiEffort::Default => None,
        AiEffort::Low => Some("low"),
        AiEffort::Medium => Some("medium"),
        AiEffort::High => Some("high"),
        AiEffort::XHigh => Some("xhigh"),
        AiEffort::Max => Some("max"),
    }
}

/// A header value that never shows up in a log (the key goes through here).
pub fn sensitive(value: &str) -> Result<HeaderValue> {
    let mut value = HeaderValue::from_str(value).map_err(|_| Error::invalid("the key cannot be sent"))?;
    value.set_sensitive(true);
    Ok(value)
}

/// An HTTP client that follows no redirects (the key would be sent along to wherever the
/// service points). `total` limits the whole request, the body included.
pub fn client(connect: Duration, total: Option<Duration>) -> Result<reqwest::Client> {
    let mut builder = reqwest::Client::builder().connect_timeout(connect).redirect(reqwest::redirect::Policy::none());
    if let Some(total) = total {
        builder = builder.timeout(total);
    }
    builder.build().map_err(|_| Error::temporary("could not set up the connection"))
}

/// Why a body could not be read.
pub enum BodyError {
    TooLarge,
    Broken,
}

/// The body of `response`, at most `cap` bytes, with `idle` allowed between two chunks.
pub async fn read_capped(
    mut response: reqwest::Response,
    cap: usize,
    idle: Duration,
) -> std::result::Result<Vec<u8>, BodyError> {
    let mut body = Vec::new();
    loop {
        match timeout(idle, response.chunk()).await {
            Ok(Ok(Some(chunk))) => {
                if body.len() + chunk.len() > cap {
                    return Err(BodyError::TooLarge);
                }
                body.extend_from_slice(&chunk);
            }
            Ok(Ok(None)) => return Ok(body),
            Ok(Err(_)) | Err(_) => return Err(BodyError::Broken),
        }
    }
}

/// Tokens as the service reported them for one or more requests. A field is `None` when the
/// service said nothing. Only the numbers are read: nothing else of an event is kept.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Usage {
    pub input: Option<u32>,
    pub output: Option<u32>,
}

impl Usage {
    /// Takes the token counts of one `data` payload of either protocol: `usage` of the event (OpenAI
    /// style, the last chunk, or Anthropic's `message_delta`) and `message.usage` (Anthropic's
    /// `message_start`). A later count of the same request replaces an earlier one.
    fn note(&mut self, data: &str) {
        if !data.contains("usage") {
            return;
        }
        let Ok(event) = serde_json::from_str::<Value>(data) else { return };
        for usage in [&event["usage"], &event["message"]["usage"]] {
            let count =
                |a: &str, b: &str| usage[a].as_u64().or(usage[b].as_u64()).map(|n| n.min(u64::from(u32::MAX)) as u32);
            self.input = count("input_tokens", "prompt_tokens").or(self.input);
            self.output = count("output_tokens", "completion_tokens").or(self.output);
        }
    }

    /// The sum of two requests; a field one of them did not report counts for what the other did.
    pub fn plus(self, other: Usage) -> Usage {
        let sum = |a: Option<u32>, b: Option<u32>| match (a, b) {
            (None, None) => None,
            (a, b) => Some(a.unwrap_or(0).saturating_add(b.unwrap_or(0))),
        };
        Usage { input: sum(self.input, other.input), output: sum(self.output, other.output) }
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
        // `delta` in a stream; a service that ignored `stream` answers with `message`.
        text: choice["delta"]["content"]
            .as_str()
            .or(choice["message"]["content"].as_str())
            .unwrap_or_default()
            .to_string(),
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
        // A service that ignored `stream` answers with the whole message.
        Some("message") if event["content"].is_array() => match event["stop_reason"].as_str() {
            Some("refusal") => Err(declined()),
            Some("max_tokens") => Err(cut_off()),
            _ => {
                let text: String = event["content"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|b| b["type"] == "text")
                    .filter_map(|b| b["text"].as_str())
                    .collect();
                Ok(Parsed { text, stop: true, ..Parsed::default() })
            }
        },
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
    usage: Usage,
}

impl Reader<'_> {
    fn event(&mut self, data: &str) -> Result<()> {
        self.usage.note(data);
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
    complete_with_usage(ep, system, user, timeouts, on_text, false).await.map(|(answer, _)| answer)
}

/// [`complete`], and the tokens the service reported. With `ask_usage` an OpenAI-style request also
/// carries `stream_options.include_usage` (without it such a service usually reports nothing); the
/// body of every other request is unchanged.
pub async fn complete_with_usage(
    ep: &Endpoint,
    system: &str,
    user: &str,
    timeouts: Timeouts,
    on_text: &mut (dyn FnMut(&str) + Send),
    ask_usage: bool,
) -> Result<(String, Usage)> {
    let url = settings::check_base_url(&ep.base_url)?;
    let host = settings::host_label(&url);
    if ep.provider == AiProvider::Anthropic && ep.key.is_none() {
        return Err(Error::invalid("this service needs a key"));
    }
    let mut parts = request_parts(ep, system, user);
    if ask_usage {
        ask_for_usage(ep, &mut parts);
    }
    let client = client(timeouts.connect, None)?;
    let mut request = client
        .post(&parts.url)
        .header("content-type", "application/json")
        .header("accept", "text/event-stream")
        .body(parts.body.to_string());
    for (name, value) in parts.headers {
        request = request.header(name, sensitive(&value)?);
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
        usage: Usage::default(),
    };
    let one_piece = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|t| t.to_ascii_lowercase().contains("application/json"));
    if one_piece {
        // A service that ignored `stream` and sent the whole answer as one JSON document.
        let body = match read_capped(response, MAX_BODY, timeouts.first).await {
            Ok(body) => body,
            Err(BodyError::TooLarge) => {
                return Err(Error::temporary("the answer grew longer than expected and was stopped"))
            }
            Err(BodyError::Broken) => return Err(Error::temporary("the service stopped answering")),
        };
        reader.event(&String::from_utf8_lossy(&body))?;
        reader.stopped = true;
    } else {
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
    Ok((answer, reader.usage))
}

/// Asks an OpenAI-style service to report its token counts in the last chunk of the stream.
fn ask_for_usage(ep: &Endpoint, parts: &mut Parts) {
    if ep.provider == AiProvider::OpenAiCompatible {
        parts.body["stream_options"] = json!({"include_usage": true});
    }
}

/// The most calls taken from one turn; what the model asks for beyond them is dropped.
const MAX_CALLS: usize = 8;
/// More than this many characters of arguments in one turn is a model that does not stop.
const MAX_ARGS_CHARS: usize = 20_000;
/// More content blocks than this in one turn is not an answer we understand.
const MAX_BLOCKS: usize = 64;
/// The most that is read of a refused request's answer, to see whether it is about the tools.
const REFUSAL_BODY: usize = 64 * 1024;

/// Whether the body of a refused request names tools: "tool", "tools", "tool_choice" or "function(s)"
/// (what OpenAI-compatible servers and Ollama say). Only this one fact is taken from it.
fn names_tools(body: &[u8]) -> bool {
    let body = body.to_ascii_lowercase();
    [&b"tool"[..], &b"function"[..]].iter().any(|word| body.windows(word.len()).any(|w| w == *word))
}

fn too_much() -> Error {
    Error::temporary("the model asked for too much at once and was stopped")
}

/// The request for a conversation with tools. With `tools` empty no `tools` key is sent.
///
/// Address, headers and the fields around the messages are those of [`request_parts`]. An
/// Anthropic assistant turn that has `raw` is sent back from it unchanged, so that thinking
/// blocks keep their signature; the results of one turn are one user message.
pub fn chat_parts(ep: &Endpoint, system: &str, turns: &[Turn], tools: &[ToolSpec]) -> Parts {
    let mut parts = request_parts(ep, system, "");
    match ep.provider {
        AiProvider::OpenAiCompatible => {
            let mut messages = vec![json!({"role": "system", "content": system})];
            for turn in turns {
                match turn {
                    Turn::User(text) => messages.push(json!({"role": "user", "content": text})),
                    Turn::Assistant { text, calls, .. } => {
                        let mut message = json!({"role": "assistant", "content": text});
                        if !calls.is_empty() {
                            message["tool_calls"] = calls
                                .iter()
                                .map(|c| {
                                    json!({
                                        "id": c.id,
                                        "type": "function",
                                        "function": {"name": c.name, "arguments": c.args.to_string()},
                                    })
                                })
                                .collect();
                        }
                        messages.push(message);
                    }
                    Turn::Results(results) => messages.extend(
                        results.iter().map(|r| json!({"role": "tool", "tool_call_id": r.id, "content": r.content})),
                    ),
                }
            }
            parts.body["messages"] = Value::Array(messages);
            if !tools.is_empty() {
                parts.body["tools"] = tools
                    .iter()
                    .map(|t| {
                        json!({
                            "type": "function",
                            "function": {"name": t.name, "description": t.description, "parameters": t.schema},
                        })
                    })
                    .collect();
            }
        }
        AiProvider::Anthropic => {
            let messages: Vec<Value> = turns
                .iter()
                .map(|turn| match turn {
                    Turn::User(text) => json!({"role": "user", "content": text}),
                    Turn::Assistant { raw: Some(raw), .. }
                        if raw.as_array().is_some_and(|blocks| !blocks.is_empty()) =>
                    {
                        json!({"role": "assistant", "content": raw})
                    }
                    Turn::Assistant { text, calls, .. } => {
                        let mut blocks = Vec::new();
                        if !text.is_empty() || calls.is_empty() {
                            blocks.push(json!({"type": "text", "text": text}));
                        }
                        blocks.extend(
                            calls
                                .iter()
                                .map(|c| json!({"type": "tool_use", "id": c.id, "name": c.name, "input": c.args})),
                        );
                        json!({"role": "assistant", "content": blocks})
                    }
                    Turn::Results(results) => {
                        let blocks: Vec<Value> = results
                            .iter()
                            .map(|r| {
                                json!({
                                    "type": "tool_result",
                                    "tool_use_id": r.id,
                                    "content": r.content,
                                    "is_error": r.is_error,
                                })
                            })
                            .collect();
                        json!({"role": "user", "content": blocks})
                    }
                })
                .collect();
            parts.body["messages"] = Value::Array(messages);
            if !tools.is_empty() {
                parts.body["tools"] = tools
                    .iter()
                    .map(|t| json!({"name": t.name, "description": t.description, "input_schema": t.schema}))
                    .collect();
            }
        }
    }
    parts
}

/// What the model sent as arguments, when it is a JSON object; `{}` when it is anything else.
fn object_or_empty(text: &str) -> Value {
    serde_json::from_str::<Value>(text).ok().filter(Value::is_object).unwrap_or_else(|| json!({}))
}

/// A call of an OpenAI-style answer while its pieces come in.
struct Pending {
    /// The `index` the stream names it by; a service that sends none starts a call with each
    /// entry that has an id or a name.
    key: Option<u64>,
    id: String,
    name: String,
    args: String,
}

/// A content block of an Anthropic answer while its pieces come in.
struct Block {
    index: u64,
    block: Value,
    /// The arguments of a `tool_use` block as they are streamed, until the block ends.
    partial: String,
}

impl Block {
    /// A `tool_use` block gets the arguments that were streamed for it as its `input`.
    fn close(&mut self) {
        if self.block["type"] != "tool_use" {
            return;
        }
        if !self.partial.is_empty() {
            self.block["input"] = object_or_empty(&std::mem::take(&mut self.partial));
        } else if !self.block["input"].is_object() {
            self.block["input"] = json!({});
        }
    }
}

/// What one turn holds besides its visible text: the calls, and for Anthropic every content
/// block as the service sent it.
#[derive(Default)]
struct Gather {
    pending: Vec<Pending>,
    /// The newest call without an `index` was dropped, and so are its further pieces.
    overflow: bool,
    blocks: Vec<Block>,
    /// Characters of arguments so far, of the dropped calls too.
    args_chars: usize,
    /// Characters of thinking and signatures so far: kept for the service, never shown.
    private_chars: usize,
    /// A call has begun: from here on the service is held to the shorter pause.
    begun: bool,
    finished: bool,
    stopped: bool,
}

impl Gather {
    fn count_args(&mut self, piece: &str) -> Result<()> {
        self.args_chars += piece.chars().count();
        if self.args_chars > MAX_ARGS_CHARS {
            return Err(too_much());
        }
        Ok(())
    }

    /// One `data` payload of an OpenAI-style stream, or the whole document of a service that
    /// ignored `stream`. Returns the text it carried. As in [`parse_openai`] only `content` and
    /// the calls are read.
    fn openai(&mut self, data: &str) -> Result<String> {
        if data.trim() == "[DONE]" {
            self.stopped = true;
            return Ok(String::new());
        }
        let Ok(event) = serde_json::from_str::<Value>(data) else { return Ok(String::new()) };
        if event.get("error").is_some_and(|e| !e.is_null()) {
            return Err(stream_error());
        }
        let choice = &event["choices"][0];
        match choice["finish_reason"].as_str() {
            Some("length") => return Err(cut_off()),
            Some("content_filter") => return Err(declined()),
            Some(_) => self.finished = true,
            None => {}
        }
        for said in [&choice["delta"], &choice["message"]] {
            for entry in said["tool_calls"].as_array().into_iter().flatten() {
                self.openai_call(entry)?;
            }
        }
        Ok(choice["delta"]["content"]
            .as_str()
            .or(choice["message"]["content"].as_str())
            .unwrap_or_default()
            .to_string())
    }

    /// One entry of `tool_calls`: the start of a call, a further piece of its arguments, or both.
    fn openai_call(&mut self, entry: &Value) -> Result<()> {
        let function = &entry["function"];
        let id = entry["id"].as_str().unwrap_or_default();
        let name = function["name"].as_str().unwrap_or_default();
        let piece = match &function["arguments"] {
            Value::String(text) => text.clone(),
            // Some services send the arguments as the object itself.
            whole @ Value::Object(_) => whole.to_string(),
            _ => String::new(),
        };
        self.count_args(&piece)?;
        self.begun = true;
        let at = match entry["index"].as_u64() {
            Some(key) => match self.pending.iter().position(|p| p.key == Some(key)) {
                Some(at) => Some(at),
                None => self.open(Some(key)),
            },
            None if !id.is_empty() || !name.is_empty() => self.open(None),
            None if self.overflow => None,
            None => self.pending.len().checked_sub(1),
        };
        if let Some(call) = at.and_then(|at| self.pending.get_mut(at)) {
            if call.id.is_empty() {
                call.id = id.to_string();
            }
            if call.name.is_empty() {
                call.name = name.to_string();
            }
            call.args.push_str(&piece);
        }
        Ok(())
    }

    /// A place for a new call, unless the turn already has as many as are taken.
    fn open(&mut self, key: Option<u64>) -> Option<usize> {
        self.overflow = self.pending.len() >= MAX_CALLS;
        if self.overflow {
            return None;
        }
        self.pending.push(Pending { key, id: String::new(), name: String::new(), args: String::new() });
        Some(self.pending.len() - 1)
    }

    fn block(&mut self, index: u64) -> Option<&mut Block> {
        self.blocks.iter_mut().find(|b| b.index == index)
    }

    fn tool_blocks(&self) -> usize {
        self.blocks.iter().filter(|b| b.block["type"] == "tool_use").count()
    }

    fn add_block(&mut self, index: u64, block: Value) -> Result<()> {
        if self.blocks.len() >= MAX_BLOCKS {
            return Err(too_much());
        }
        self.blocks.push(Block { index, block, partial: String::new() });
        Ok(())
    }

    /// A piece of something the service wants back but the user never sees: thinking, or its
    /// signature.
    fn private(&mut self, index: u64, field: &str, piece: &str) -> Result<()> {
        self.private_chars += piece.chars().count();
        if self.private_chars > MAX_HIDDEN {
            return Err(Error::temporary("the model kept thinking for too long and was stopped"));
        }
        if let Some(block) = self.block(index).filter(|b| b.block.is_object()) {
            match &mut block.block[field] {
                Value::String(whole) => whole.push_str(piece),
                other => *other = Value::String(piece.to_string()),
            }
        }
        Ok(())
    }

    /// One `data` payload of an Anthropic stream, or the whole message of a service that ignored
    /// `stream`. Returns the text it carried. Every block is kept as it was sent.
    fn anthropic(&mut self, data: &str) -> Result<String> {
        let Ok(event) = serde_json::from_str::<Value>(data) else { return Ok(String::new()) };
        let index = event["index"].as_u64().unwrap_or(0);
        match event["type"].as_str() {
            Some("content_block_start") if event["content_block"].is_object() => {
                let block = event["content_block"].clone();
                if block["type"] == "tool_use" {
                    self.begun = true;
                    if self.tool_blocks() >= MAX_CALLS {
                        return Ok(String::new());
                    }
                }
                let text = match block["type"].as_str() {
                    Some("text") => block["text"].as_str().unwrap_or_default().to_string(),
                    _ => String::new(),
                };
                self.add_block(index, block)?;
                Ok(text)
            }
            Some("content_block_delta") => {
                let delta = &event["delta"];
                match delta["type"].as_str() {
                    Some("text_delta") => {
                        let text = delta["text"].as_str().unwrap_or_default();
                        if self.block(index).is_none() {
                            self.add_block(index, json!({"type": "text", "text": ""}))?;
                        }
                        if let Some(Value::String(whole)) = self.block(index).and_then(|b| b.block.get_mut("text")) {
                            whole.push_str(text);
                        }
                        Ok(text.to_string())
                    }
                    Some("input_json_delta") => {
                        let piece = delta["partial_json"].as_str().unwrap_or_default();
                        self.count_args(piece)?;
                        if let Some(block) = self.block(index) {
                            block.partial.push_str(piece);
                        }
                        Ok(String::new())
                    }
                    Some("thinking_delta") => {
                        self.private(index, "thinking", delta["thinking"].as_str().unwrap_or_default())?;
                        Ok(String::new())
                    }
                    Some("signature_delta") => {
                        self.private(index, "signature", delta["signature"].as_str().unwrap_or_default())?;
                        Ok(String::new())
                    }
                    _ => Ok(String::new()),
                }
            }
            Some("content_block_stop") => {
                if let Some(block) = self.block(index) {
                    block.close();
                }
                Ok(String::new())
            }
            Some("message_delta") => match event["delta"]["stop_reason"].as_str() {
                Some("refusal") => Err(declined()),
                Some("max_tokens") => Err(cut_off()),
                Some(_) => {
                    self.finished = true;
                    Ok(String::new())
                }
                None => Ok(String::new()),
            },
            Some("message_stop") => {
                self.stopped = true;
                Ok(String::new())
            }
            Some("error") => Err(stream_error()),
            Some("message") if event["content"].is_array() => {
                match event["stop_reason"].as_str() {
                    Some("refusal") => return Err(declined()),
                    Some("max_tokens") => return Err(cut_off()),
                    _ => {}
                }
                let mut text = String::new();
                for (at, block) in event["content"].as_array().into_iter().flatten().enumerate() {
                    match block["type"].as_str() {
                        Some("text") => text.push_str(block["text"].as_str().unwrap_or_default()),
                        Some("tool_use") => {
                            self.begun = true;
                            self.count_args(&block["input"].to_string())?;
                            if self.tool_blocks() >= MAX_CALLS {
                                continue;
                            }
                        }
                        _ => {}
                    }
                    if block.is_object() {
                        self.add_block(at as u64, block.clone())?;
                    }
                }
                self.stopped = true;
                Ok(text)
            }
            _ => Ok(String::new()),
        }
    }

    /// The calls of the turn, and for Anthropic the blocks to send back. `prior` is how many
    /// calls the conversation already holds: a call that came without an id is named after its
    /// place in the conversation, so that no two share a name.
    fn calls(&mut self, provider: AiProvider, prior: usize) -> (Vec<ToolCall>, Option<Value>) {
        let made = |n: usize| format!("call_{}", prior + n + 1);
        match provider {
            AiProvider::OpenAiCompatible => {
                let calls = self
                    .pending
                    .iter()
                    .enumerate()
                    .map(|(n, p)| ToolCall {
                        id: if p.id.is_empty() { made(n) } else { p.id.clone() },
                        name: p.name.clone(),
                        args: object_or_empty(&p.args),
                    })
                    .collect();
                (calls, None)
            }
            AiProvider::Anthropic => {
                let mut calls = Vec::new();
                for block in &mut self.blocks {
                    block.close();
                    if block.block["type"] != "tool_use" {
                        continue;
                    }
                    if block.block["id"].as_str().is_none_or(str::is_empty) {
                        block.block["id"] = Value::String(made(calls.len()));
                    }
                    calls.push(ToolCall {
                        id: block.block["id"].as_str().unwrap_or_default().to_string(),
                        name: block.block["name"].as_str().unwrap_or_default().to_string(),
                        args: block.block["input"].clone(),
                    });
                }
                // An empty text block is refused when it is sent back; everything else stays.
                let raw: Vec<Value> = self
                    .blocks
                    .iter()
                    .filter(|b| {
                        b.block["type"] != "text" || !b.block["text"].as_str().unwrap_or_default().trim().is_empty()
                    })
                    .map(|b| b.block.clone())
                    .collect();
                (calls, (!raw.is_empty()).then_some(Value::Array(raw)))
            }
        }
    }
}

/// The state of one turn being read: the text goes the way of [`complete`], the rest is gathered.
struct ChatReader<'a> {
    provider: AiProvider,
    think: ThinkFilter,
    sink: Sink<'a>,
    gather: Gather,
    usage: Usage,
}

impl<'a> ChatReader<'a> {
    fn new(provider: AiProvider, on_text: &'a mut (dyn FnMut(&str) + Send)) -> ChatReader<'a> {
        ChatReader {
            provider,
            think: ThinkFilter::default(),
            sink: Sink { answer: String::new(), chars: 0, on_text },
            gather: Gather::default(),
            usage: Usage::default(),
        }
    }

    fn event(&mut self, data: &str) -> Result<()> {
        self.usage.note(data);
        let text = match self.provider {
            AiProvider::OpenAiCompatible => self.gather.openai(data)?,
            AiProvider::Anthropic => self.gather.anthropic(data)?,
        };
        let shown = self.think.push(&text);
        if self.think.hidden > MAX_HIDDEN {
            return Err(Error::temporary("the model kept thinking for too long and was stopped"));
        }
        self.sink.push(&shown)
    }

    /// Visible text or a call has begun.
    fn begun(&self) -> bool {
        !self.sink.answer.is_empty() || self.gather.begun
    }

    fn done(&self) -> bool {
        self.gather.finished || self.gather.stopped
    }

    /// The turn as a whole. One with neither text nor calls is an error.
    fn finish(mut self, prior: usize) -> Result<Reply> {
        let rest = self.think.finish();
        self.sink.push(&rest)?;
        let text = self.sink.answer.trim();
        let text = match self.provider {
            AiProvider::OpenAiCompatible => self.think.after_orphan_close(text),
            AiProvider::Anthropic => text,
        }
        .trim()
        .to_string();
        let (calls, raw) = self.gather.calls(self.provider, prior);
        if text.is_empty() && calls.is_empty() {
            return Err(Error::temporary("the service sent an empty answer"));
        }
        Ok(Reply { text, calls, raw })
    }
}

/// Sends one request of a conversation. The status is the caller's to judge.
async fn post(client: &reqwest::Client, parts: Parts, host: &str, timeouts: Timeouts) -> Result<reqwest::Response> {
    let mut request = client
        .post(&parts.url)
        .header("content-type", "application/json")
        .header("accept", "text/event-stream")
        .body(parts.body.to_string());
    for (name, value) in parts.headers {
        request = request.header(name, sensitive(&value)?);
    }
    match timeout(timeouts.first, request.send()).await {
        Err(_) => Err(Error::temporary("the service did not answer in time")),
        Ok(Err(e)) if e.is_timeout() => Err(Error::temporary("the service did not answer in time")),
        Ok(Err(_)) => Err(Error::temporary(format!("could not reach the service at {host}"))),
        Ok(Ok(response)) => Ok(response),
    }
}

/// One model turn. A turn with calls may have no text. OpenAI protocol only: a 400 to a request that
/// carried tools, whose body mentions tools, is tried once more without them on the same endpoint and
/// reported as ToolsRefused::Yes. When that second request is refused with a 400 as well, the result
/// is `Error::ToolsUnsupported`. Nothing of the service's words goes into an Error.
///
/// `on_text` hears each visible piece as it comes; the text of the reply is the whole of it,
/// trimmed, as with [`complete`]. Timeouts, the size limits and the handling of the key are
/// those of [`complete`]; once a call has begun the service is held to the pause between pieces.
pub async fn converse(
    ep: &Endpoint,
    system: &str,
    turns: &[Turn],
    tools: &[ToolSpec],
    timeouts: Timeouts,
    on_text: &mut (dyn FnMut(&str) + Send),
) -> Result<(Reply, ToolsRefused)> {
    converse_with_usage(ep, system, turns, tools, timeouts, on_text, false)
        .await
        .map(|(reply, refused, _)| (reply, refused))
}

/// [`converse`], and the tokens the service reported for this turn (see [`complete_with_usage`]).
pub async fn converse_with_usage(
    ep: &Endpoint,
    system: &str,
    turns: &[Turn],
    tools: &[ToolSpec],
    timeouts: Timeouts,
    on_text: &mut (dyn FnMut(&str) + Send),
    ask_usage: bool,
) -> Result<(Reply, ToolsRefused, Usage)> {
    let url = settings::check_base_url(&ep.base_url)?;
    let host = settings::host_label(&url);
    if ep.provider == AiProvider::Anthropic && ep.key.is_none() {
        return Err(Error::invalid("this service needs a key"));
    }
    let client = client(timeouts.connect, None)?;
    let mut refused = ToolsRefused::No;
    let parts = |tools: &[ToolSpec]| {
        let mut parts = chat_parts(ep, system, turns, tools);
        if ask_usage {
            ask_for_usage(ep, &mut parts);
        }
        parts
    };
    let mut response = post(&client, parts(tools), &host, timeouts).await?;
    let status = response.status().as_u16();
    if status == 400 && ep.provider == AiProvider::OpenAiCompatible && !tools.is_empty() {
        // The only thing ever read of an error's body, and only for one word: nothing of it is
        // kept, shown or logged.
        let about_tools = match read_capped(response, REFUSAL_BODY, timeouts.idle).await {
            Ok(body) => names_tools(&body),
            Err(_) => false,
        };
        if !about_tools {
            return Err(status_error(400));
        }
        // The same endpoint, so the key goes where it went before, and only once more.
        response = post(&client, parts(&[]), &host, timeouts).await?;
        if !response.status().is_success() {
            // Still a 400 without tools: the model cannot be used for this at all, and that is the
            // fact to report. Any other status is that status.
            return Err(match response.status().as_u16() {
                400 => Error::ToolsUnsupported,
                other => status_error(other),
            });
        }
        refused = ToolsRefused::Yes;
    } else if !response.status().is_success() {
        return Err(status_error(status));
    }

    let mut reader = ChatReader::new(ep.provider, on_text);
    let one_piece = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|t| t.to_ascii_lowercase().contains("application/json"));
    if one_piece {
        // A service that ignored `stream` and sent the whole answer as one JSON document.
        let body = match read_capped(response, MAX_BODY, timeouts.first).await {
            Ok(body) => body,
            Err(BodyError::TooLarge) => {
                return Err(Error::temporary("the answer grew longer than expected and was stopped"))
            }
            Err(BodyError::Broken) => return Err(Error::temporary("the service stopped answering")),
        };
        reader.event(&String::from_utf8_lossy(&body))?;
        reader.gather.stopped = true;
    } else {
        let mut sse = SseParser::default();
        'stream: loop {
            let limit = if reader.begun() { timeouts.idle } else { timeouts.first };
            let chunk = match timeout(limit, response.chunk()).await {
                Err(_) => return Err(Error::temporary("the service stopped answering")),
                Ok(Err(_)) | Ok(Ok(None)) => break,
                Ok(Ok(Some(chunk))) => chunk,
            };
            for data in sse.feed(&chunk)? {
                reader.event(&data)?;
                if reader.gather.stopped {
                    break 'stream;
                }
            }
        }
        if let Some(data) = sse.finish()?.filter(|_| !reader.gather.stopped) {
            reader.event(&data)?;
        }
    }
    if !reader.done() {
        return Err(Error::temporary("the connection to the service broke while it was answering"));
    }
    let prior = turns
        .iter()
        .map(|turn| match turn {
            Turn::Assistant { calls, .. } => calls.len(),
            _ => 0,
        })
        .sum();
    let usage = reader.usage;
    Ok((reader.finish(prior)?, refused, usage))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ep(provider: AiProvider, key: Option<&str>) -> Endpoint {
        Endpoint {
            provider,
            base_url: "https://api.example.com/v1".into(),
            model: "m-1".into(),
            effort: AiEffort::Default,
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
            json!({"model": "m-1", "max_tokens": 32000, "stream": true, "system": "sys",
                   "messages": [{"role": "user", "content": "usr"}]})
        );
    }

    #[test]
    fn effort_goes_into_the_anthropic_body_only() {
        for (effort, word) in [
            (AiEffort::Low, "low"),
            (AiEffort::Medium, "medium"),
            (AiEffort::High, "high"),
            (AiEffort::XHigh, "xhigh"),
            (AiEffort::Max, "max"),
        ] {
            let anthropic = Endpoint { effort, ..ep(AiProvider::Anthropic, Some("k")) };
            assert_eq!(request_parts(&anthropic, "s", "u").body["output_config"], json!({"effort": word}));
            let openai = Endpoint { effort, ..ep(AiProvider::OpenAiCompatible, None) };
            assert!(request_parts(&openai, "s", "u").body.get("output_config").is_none());
        }
        let default = request_parts(&ep(AiProvider::Anthropic, Some("k")), "s", "u");
        assert!(default.body.get("output_config").is_none());
    }

    #[test]
    fn a_whole_answer_is_read_from_message_content() {
        let whole = r#"{"choices":[{"message":{"role":"assistant","content":"Hi"},"finish_reason":"stop"}]}"#;
        assert_eq!(parse_openai(whole).unwrap(), Parsed { text: "Hi".into(), finished: true, stop: false });
        let long = r#"{"choices":[{"message":{"content":"Hi"},"finish_reason":"length"}]}"#;
        assert!(matches!(parse_openai(long), Err(Error::Temporary { .. })));
    }

    #[test]
    fn the_token_counts_are_read_from_either_protocol_and_nothing_else() {
        let mut usage = Usage::default();
        usage.note(r#"{"choices":[{"delta":{"content":"hi"}}]}"#);
        assert_eq!(usage, Usage::default());
        usage.note(r#"{"choices":[],"usage":{"prompt_tokens":12,"completion_tokens":3,"total_tokens":15}}"#);
        assert_eq!(usage, Usage { input: Some(12), output: Some(3) });
        // Anthropic: the start gives the input, the delta the output so far (a later count replaces).
        let mut usage = Usage::default();
        usage.note(r#"{"type":"message_start","message":{"usage":{"input_tokens":100,"output_tokens":1}}}"#);
        usage.note(r#"{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":40}}"#);
        assert_eq!(usage, Usage { input: Some(100), output: Some(40) });
        usage.note("not json with usage in it");
        usage.note(r#"{"usage":null}"#);
        assert_eq!(usage, Usage { input: Some(100), output: Some(40) });
        let sum = usage.plus(Usage { input: Some(5), output: None });
        assert_eq!(sum, Usage { input: Some(105), output: Some(40) });
        assert_eq!(Usage::default().plus(Usage::default()), Usage::default());
    }

    #[test]
    fn only_a_request_that_asks_gets_stream_options() {
        let mut parts = request_parts(&ep(AiProvider::OpenAiCompatible, None), "s", "u");
        assert!(parts.body.get("stream_options").is_none());
        ask_for_usage(&ep(AiProvider::OpenAiCompatible, None), &mut parts);
        assert_eq!(parts.body["stream_options"], json!({"include_usage": true}));
        let mut anthropic = request_parts(&ep(AiProvider::Anthropic, Some("k")), "s", "u");
        ask_for_usage(&ep(AiProvider::Anthropic, Some("k")), &mut anthropic);
        assert!(anthropic.body.get("stream_options").is_none());
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

    // ---- a conversation with tools --------------------------------------------------------

    use super::super::wire::ToolResult;

    fn tools() -> Vec<ToolSpec> {
        vec![
            ToolSpec {
                name: "search_mail",
                description: "Searches the mail.",
                schema: json!({"type": "object", "properties": {"query": {"type": "string"}}, "required": ["query"]}),
            },
            ToolSpec { name: "list_folders", description: "Lists the folders.", schema: json!({"type": "object"}) },
        ]
    }

    fn call(id: &str, name: &str, args: Value) -> ToolCall {
        ToolCall { id: id.into(), name: name.into(), args }
    }

    /// A question, a turn with two calls, their results, and a second question.
    fn turns(raw: Option<Value>) -> Vec<Turn> {
        vec![
            Turn::User("Any mail from Maria?".into()),
            Turn::Assistant {
                text: "Looking.".into(),
                calls: vec![
                    call("c1", "search_mail", json!({"query": "maria"})),
                    call("c2", "list_folders", json!({})),
                ],
                raw,
            },
            Turn::Results(vec![
                ToolResult { id: "c1".into(), name: "search_mail".into(), content: "one hit".into(), is_error: false },
                ToolResult { id: "c2".into(), name: "list_folders".into(), content: "no".into(), is_error: true },
            ]),
            Turn::Assistant { text: "One.".into(), calls: Vec::new(), raw: None },
            Turn::User("And today?".into()),
        ]
    }

    /// Reads these events as one turn; also what the listener heard.
    fn read(provider: AiProvider, prior: usize, events: &[Value]) -> (Result<Reply>, String) {
        let mut heard = String::new();
        let result = {
            let mut on_text = |piece: &str| heard.push_str(piece);
            let mut reader = ChatReader::new(provider, &mut on_text);
            events.iter().try_for_each(|e| reader.event(&e.to_string())).and_then(|()| reader.finish(prior))
        };
        (result, heard)
    }

    fn openai_delta(delta: Value) -> Value {
        json!({"choices": [{"index": 0, "delta": delta}]})
    }

    fn openai_end(reason: &str) -> Value {
        json!({"choices": [{"index": 0, "delta": {}, "finish_reason": reason}]})
    }

    #[test]
    fn the_openai_conversation_carries_calls_results_and_tools() {
        let parts = chat_parts(&ep(AiProvider::OpenAiCompatible, Some("k-1")), "sys", &turns(None), &tools());
        assert_eq!(parts.url, "https://api.example.com/v1/chat/completions");
        assert_eq!(parts.headers, [("authorization", "Bearer k-1".to_string())]);
        assert_eq!(
            parts.body,
            json!({"model": "m-1", "stream": true,
                "messages": [
                    {"role": "system", "content": "sys"},
                    {"role": "user", "content": "Any mail from Maria?"},
                    {"role": "assistant", "content": "Looking.", "tool_calls": [
                        {"id": "c1", "type": "function",
                         "function": {"name": "search_mail", "arguments": "{\"query\":\"maria\"}"}},
                        {"id": "c2", "type": "function", "function": {"name": "list_folders", "arguments": "{}"}}]},
                    {"role": "tool", "tool_call_id": "c1", "content": "one hit"},
                    {"role": "tool", "tool_call_id": "c2", "content": "no"},
                    {"role": "assistant", "content": "One."},
                    {"role": "user", "content": "And today?"}],
                "tools": [
                    {"type": "function", "function": {"name": "search_mail", "description": "Searches the mail.",
                        "parameters": {"type": "object", "properties": {"query": {"type": "string"}},
                                       "required": ["query"]}}},
                    {"type": "function", "function": {"name": "list_folders", "description": "Lists the folders.",
                        "parameters": {"type": "object"}}}]})
        );
        // The service's own blocks are an Anthropic matter: here they change nothing.
        let with_raw = chat_parts(
            &ep(AiProvider::OpenAiCompatible, Some("k-1")),
            "sys",
            &turns(Some(json!([{"type": "text", "text": "other"}]))),
            &tools(),
        );
        assert_eq!(with_raw.body, parts.body);
        assert!(chat_parts(&ep(AiProvider::OpenAiCompatible, None), "sys", &turns(None), &tools()).headers.is_empty());
    }

    #[test]
    fn the_anthropic_conversation_carries_calls_results_and_tools() {
        let effort = Endpoint { effort: AiEffort::High, ..ep(AiProvider::Anthropic, Some("k-2")) };
        let parts = chat_parts(&effort, "sys", &turns(None), &tools());
        assert_eq!(parts.url, "https://api.example.com/v1/messages");
        assert_eq!(header(&parts, "x-api-key"), Some("k-2"));
        assert_eq!(header(&parts, "anthropic-version"), Some(ANTHROPIC_VERSION));
        assert_eq!(header(&parts, "authorization"), None);
        assert_eq!(
            parts.body,
            json!({"model": "m-1", "max_tokens": 32000, "stream": true, "system": "sys",
                "output_config": {"effort": "high"},
                "messages": [
                    {"role": "user", "content": "Any mail from Maria?"},
                    {"role": "assistant", "content": [
                        {"type": "text", "text": "Looking."},
                        {"type": "tool_use", "id": "c1", "name": "search_mail", "input": {"query": "maria"}},
                        {"type": "tool_use", "id": "c2", "name": "list_folders", "input": {}}]},
                    {"role": "user", "content": [
                        {"type": "tool_result", "tool_use_id": "c1", "content": "one hit", "is_error": false},
                        {"type": "tool_result", "tool_use_id": "c2", "content": "no", "is_error": true}]},
                    {"role": "assistant", "content": [{"type": "text", "text": "One."}]},
                    {"role": "user", "content": "And today?"}],
                "tools": [
                    {"name": "search_mail", "description": "Searches the mail.",
                     "input_schema": {"type": "object", "properties": {"query": {"type": "string"}},
                                      "required": ["query"]}},
                    {"name": "list_folders", "description": "Lists the folders.",
                     "input_schema": {"type": "object"}}]})
        );
        // A turn that is only calls has no empty text block.
        let only_calls =
            [Turn::Assistant { text: String::new(), calls: vec![call("c9", "list_folders", json!({}))], raw: None }];
        assert_eq!(
            chat_parts(&ep(AiProvider::Anthropic, Some("k")), "s", &only_calls, &[]).body["messages"],
            json!([{"role": "assistant", "content": [
                {"type": "tool_use", "id": "c9", "name": "list_folders", "input": {}}]}])
        );
    }

    #[test]
    fn without_tools_no_tools_key_is_sent() {
        for provider in [AiProvider::OpenAiCompatible, AiProvider::Anthropic] {
            let parts = chat_parts(&ep(provider, Some("k")), "sys", &[Turn::User("Hi".into())], &[]);
            assert!(parts.body.get("tools").is_none(), "{provider:?}");
            // With one user turn it is the stage 1 request.
            assert_eq!(parts.body, request_parts(&ep(provider, Some("k")), "sys", "Hi").body, "{provider:?}");
        }
    }

    #[test]
    fn the_services_own_blocks_are_sent_back_unchanged() {
        let raw = json!([
            {"type": "thinking", "thinking": "", "signature": "sig-abc"},
            {"type": "redacted_thinking", "data": "opaque"},
            {"type": "text", "text": "Looking."},
            {"type": "tool_use", "id": "c1", "name": "search_mail", "input": {"query": "maria"}},
            {"type": "tool_use", "id": "c2", "name": "list_folders", "input": {}}]);
        let parts = chat_parts(&ep(AiProvider::Anthropic, Some("k")), "sys", &turns(Some(raw.clone())), &tools());
        assert_eq!(parts.body["messages"][1], json!({"role": "assistant", "content": raw}));
        // An empty list of blocks is no answer to send back: the turn is built from its parts.
        let parts = chat_parts(&ep(AiProvider::Anthropic, Some("k")), "sys", &turns(Some(json!([]))), &tools());
        assert_eq!(parts.body["messages"][1]["content"][0], json!({"type": "text", "text": "Looking."}));
    }

    #[test]
    fn openai_calls_are_put_together_from_their_fragments() {
        let events = [
            openai_delta(json!({"role": "assistant", "content": ""})),
            openai_delta(json!({"content": "Let me "})),
            openai_delta(json!({"content": "look."})),
            openai_delta(json!({"tool_calls": [{"index": 0, "id": "c1", "type": "function",
                "function": {"name": "search_mail", "arguments": ""}}]})),
            openai_delta(json!({"tool_calls": [{"index": 0, "function": {"arguments": "{\"que"}}]})),
            openai_delta(json!({"tool_calls": [{"index": 1, "id": "c2", "type": "function",
                "function": {"name": "list_folders", "arguments": "{"}}]})),
            // Pieces of two calls in one event, and a name said again.
            openai_delta(json!({"tool_calls": [
                {"index": 0, "function": {"name": "search_mail", "arguments": "ry\":\"maria\"}"}},
                {"index": 1, "function": {"arguments": "}"}}]})),
            openai_end("tool_calls"),
        ];
        let (reply, heard) = read(AiProvider::OpenAiCompatible, 0, &events);
        assert_eq!(
            reply.unwrap(),
            Reply {
                text: "Let me look.".into(),
                calls: vec![
                    call("c1", "search_mail", json!({"query": "maria"})),
                    call("c2", "list_folders", json!({}))
                ],
                raw: None,
            }
        );
        assert_eq!(heard, "Let me look.");
    }

    #[test]
    fn a_whole_openai_call_in_one_chunk_and_a_missing_id_are_taken() {
        // No `index`, no id: each entry with a name is a call, named after its place in the
        // conversation (three calls came before).
        let events = [
            openai_delta(json!({"tool_calls": [{"function": {"name": "list_folders", "arguments": "{}"}}]})),
            openai_delta(json!({"tool_calls": [{"function": {"name": "search_mail", "arguments": "{\"query\""}}]})),
            openai_delta(json!({"tool_calls": [{"function": {"arguments": ":\"x\"}"}}]})),
            openai_end("stop"),
        ];
        let (reply, heard) = read(AiProvider::OpenAiCompatible, 3, &events);
        assert_eq!(
            reply.unwrap().calls,
            [call("call_4", "list_folders", json!({})), call("call_5", "search_mail", json!({"query": "x"}))]
        );
        assert_eq!(heard, "");
        // Arguments sent as the object itself.
        let events = [
            openai_delta(json!({"tool_calls": [{"index": 0, "id": "c1",
                "function": {"name": "search_mail", "arguments": {"query": "y"}}}]})),
            openai_end("tool_calls"),
        ];
        let (reply, _) = read(AiProvider::OpenAiCompatible, 0, &events);
        assert_eq!(reply.unwrap().calls, [call("c1", "search_mail", json!({"query": "y"}))]);
    }

    #[test]
    fn a_whole_document_with_calls_is_read_in_both_protocols() {
        let openai = json!({"choices": [{"index": 0, "finish_reason": "tool_calls", "message": {
            "role": "assistant", "content": null, "tool_calls": [
                {"id": "c1", "type": "function", "function": {"name": "search_mail", "arguments": "{\"query\":\"a\"}"}},
                {"id": "c2", "type": "function", "function": {"name": "list_folders", "arguments": "{}"}}]}}]});
        let (reply, _) = read(AiProvider::OpenAiCompatible, 0, &[openai]);
        assert_eq!(
            reply.unwrap(),
            Reply {
                text: String::new(),
                calls: vec![call("c1", "search_mail", json!({"query": "a"})), call("c2", "list_folders", json!({}))],
                raw: None,
            }
        );
        let content = json!([
            {"type": "thinking", "thinking": "", "signature": "sig-1"},
            {"type": "text", "text": "Looking."},
            {"type": "tool_use", "id": "t1", "name": "search_mail", "input": {"query": "a"}}]);
        let anthropic =
            json!({"type": "message", "role": "assistant", "content": content.clone(), "stop_reason": "tool_use"});
        let (reply, heard) = read(AiProvider::Anthropic, 0, &[anthropic]);
        assert_eq!(
            reply.unwrap(),
            Reply {
                text: "Looking.".into(),
                calls: vec![call("t1", "search_mail", json!({"query": "a"}))],
                raw: Some(content),
            }
        );
        assert_eq!(heard, "Looking.");
        let cut =
            json!({"type": "message", "content": [{"type": "text", "text": "Half"}], "stop_reason": "max_tokens"});
        assert!(
            matches!(read(AiProvider::Anthropic, 0, &[cut]).0, Err(Error::Temporary { msg }) if msg.contains("length limit"))
        );
    }

    #[test]
    fn anthropic_calls_and_thinking_are_put_together_and_kept_for_the_service() {
        let start =
            |index: u64, block: Value| json!({"type": "content_block_start", "index": index, "content_block": block});
        let delta = |index: u64, delta: Value| json!({"type": "content_block_delta", "index": index, "delta": delta});
        let stop = |index: u64| json!({"type": "content_block_stop", "index": index});
        let events = [
            json!({"type": "message_start", "message": {"id": "msg_1", "role": "assistant"}}),
            start(0, json!({"type": "thinking", "thinking": ""})),
            delta(0, json!({"type": "thinking_delta", "thinking": "private "})),
            delta(0, json!({"type": "thinking_delta", "thinking": "reasoning"})),
            delta(0, json!({"type": "signature_delta", "signature": "sig-xyz"})),
            stop(0),
            start(1, json!({"type": "redacted_thinking", "data": "opaque"})),
            stop(1),
            start(2, json!({"type": "text", "text": ""})),
            json!({"type": "ping"}),
            delta(2, json!({"type": "text_delta", "text": "Let me "})),
            delta(2, json!({"type": "text_delta", "text": "look."})),
            stop(2),
            start(3, json!({"type": "tool_use", "id": "t1", "name": "search_mail", "input": {}})),
            delta(3, json!({"type": "input_json_delta", "partial_json": ""})),
            delta(3, json!({"type": "input_json_delta", "partial_json": "{\"query\": \"ma"})),
            delta(3, json!({"type": "input_json_delta", "partial_json": "ria\"}"})),
            stop(3),
            start(4, json!({"type": "tool_use", "id": "t2", "name": "list_folders", "input": {}})),
            stop(4),
            json!({"type": "message_delta", "delta": {"stop_reason": "tool_use"}}),
            json!({"type": "message_stop"}),
        ];
        let (reply, heard) = read(AiProvider::Anthropic, 0, &events);
        let reply = reply.unwrap();
        assert_eq!(reply.text, "Let me look.");
        assert_eq!(heard, "Let me look.", "thinking is never shown");
        assert_eq!(
            reply.calls,
            [call("t1", "search_mail", json!({"query": "maria"})), call("t2", "list_folders", json!({}))]
        );
        let raw = json!([
            {"type": "thinking", "thinking": "private reasoning", "signature": "sig-xyz"},
            {"type": "redacted_thinking", "data": "opaque"},
            {"type": "text", "text": "Let me look."},
            {"type": "tool_use", "id": "t1", "name": "search_mail", "input": {"query": "maria"}},
            {"type": "tool_use", "id": "t2", "name": "list_folders", "input": {}}]);
        assert_eq!(reply.raw, Some(raw.clone()));
        // And that is, to the letter, what the next request sends back.
        let next = [Turn::Assistant { text: reply.text, calls: reply.calls, raw: reply.raw }];
        let parts = chat_parts(&ep(AiProvider::Anthropic, Some("k")), "s", &next, &tools());
        assert_eq!(parts.body["messages"][0]["content"], raw);
    }

    #[test]
    fn a_turn_of_calls_alone_is_an_answer_and_one_of_nothing_is_not() {
        let events = [
            openai_delta(json!({"role": "assistant", "content": ""})),
            openai_delta(
                json!({"tool_calls": [{"index": 0, "id": "c1", "function": {"name": "list_folders", "arguments": "{}"}}]}),
            ),
            openai_end("tool_calls"),
        ];
        let (reply, heard) = read(AiProvider::OpenAiCompatible, 0, &events);
        assert_eq!(
            reply.unwrap(),
            Reply { text: String::new(), calls: vec![call("c1", "list_folders", json!({}))], raw: None }
        );
        assert_eq!(heard, "");
        let events = [
            json!({"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}}),
            json!({"type": "content_block_start", "index": 1,
                "content_block": {"type": "tool_use", "id": "t1", "name": "list_folders", "input": {}}}),
            json!({"type": "message_delta", "delta": {"stop_reason": "tool_use"}}),
        ];
        let reply = read(AiProvider::Anthropic, 0, &events).0.unwrap();
        assert_eq!(reply.calls, [call("t1", "list_folders", json!({}))]);
        // The empty text block is not sent back: the service would refuse it.
        assert_eq!(reply.raw, Some(json!([{"type": "tool_use", "id": "t1", "name": "list_folders", "input": {}}])));

        for (provider, events) in [
            (AiProvider::OpenAiCompatible, vec![openai_delta(json!({"content": "  \n"})), openai_end("stop")]),
            (AiProvider::OpenAiCompatible, vec![openai_delta(json!({"content": "<think>only this</think>"}))]),
            (
                AiProvider::Anthropic,
                vec![
                    json!({"type": "content_block_start", "index": 0, "content_block": {"type": "thinking", "thinking": "x"}}),
                    json!({"type": "message_stop"}),
                ],
            ),
        ] {
            let result = read(provider, 0, &events).0;
            assert!(
                matches!(&result, Err(Error::Temporary { msg }) if msg == "the service sent an empty answer"),
                "{result:?}"
            );
        }
    }

    #[test]
    fn arguments_that_are_not_an_object_become_an_empty_one_and_the_call_stays() {
        for arguments in ["", "[1,2]", "\"text\"", "{\"query\": \"cut", "null", "7"] {
            let events = [
                openai_delta(json!({"tool_calls": [{"index": 0, "id": "c1",
                    "function": {"name": "search_mail", "arguments": arguments}}]})),
                openai_end("tool_calls"),
            ];
            let reply = read(AiProvider::OpenAiCompatible, 0, &events).0.unwrap();
            assert_eq!(reply.calls, [call("c1", "search_mail", json!({}))], "{arguments}");

            let events = [
                json!({"type": "content_block_start", "index": 0,
                    "content_block": {"type": "tool_use", "id": "t1", "name": "search_mail", "input": {}}}),
                json!({"type": "content_block_delta", "index": 0,
                    "delta": {"type": "input_json_delta", "partial_json": arguments}}),
                json!({"type": "message_delta", "delta": {"stop_reason": "tool_use"}}),
            ];
            let reply = read(AiProvider::Anthropic, 0, &events).0.unwrap();
            assert_eq!(reply.calls, [call("t1", "search_mail", json!({}))], "{arguments}");
            assert_eq!(reply.raw.unwrap()[0]["input"], json!({}), "{arguments}");
        }
        let whole = json!({"type": "message", "stop_reason": "tool_use", "content": [
            {"type": "tool_use", "id": "t1", "name": "search_mail", "input": "text"}, {"type": "tool_use", "name": "x"}]});
        let reply = read(AiProvider::Anthropic, 2, &[whole]).0.unwrap();
        assert_eq!(reply.calls, [call("t1", "search_mail", json!({})), call("call_4", "x", json!({}))]);
        assert_eq!(reply.raw.unwrap()[1], json!({"type": "tool_use", "id": "call_4", "name": "x", "input": {}}));
    }

    #[test]
    fn the_first_eight_calls_are_kept_and_endless_arguments_are_an_error() {
        let many: Vec<Value> = (0..12)
            .map(|n| {
                openai_delta(json!({"tool_calls": [{"index": n, "id": format!("c{n}"),
                    "function": {"name": "list_folders", "arguments": "{}"}}]}))
            })
            .chain([openai_delta(json!({"tool_calls": [{"index": 11, "function": {"arguments": " "}}]}))])
            .chain([openai_end("tool_calls")])
            .collect();
        let reply = read(AiProvider::OpenAiCompatible, 0, &many).0.unwrap();
        assert_eq!(
            reply.calls.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
            ["c0", "c1", "c2", "c3", "c4", "c5", "c6", "c7"]
        );
        // The same without `index`: the pieces of a dropped call do not land in the one before.
        let many: Vec<Value> = (0..9)
            .map(|n| {
                openai_delta(json!({"tool_calls": [{"id": format!("c{n}"), "function": {"name": "list_folders"}}]}))
            })
            .chain([openai_delta(json!({"tool_calls": [{"function": {"arguments": "[]"}}]}))])
            .collect();
        let reply = read(AiProvider::OpenAiCompatible, 0, &many).0.unwrap();
        assert_eq!(reply.calls.len(), 8);
        assert_eq!(reply.calls[7], call("c7", "list_folders", json!({})));

        let many: Vec<Value> = (0..12)
            .flat_map(|n| {
                [
                    json!({"type": "content_block_start", "index": n, "content_block":
                        {"type": "tool_use", "id": format!("t{n}"), "name": "list_folders", "input": {}}}),
                    json!({"type": "content_block_delta", "index": n, "delta": {"type": "input_json_delta", "partial_json": "{}"}}),
                    json!({"type": "content_block_stop", "index": n}),
                ]
            })
            .collect();
        let reply = read(AiProvider::Anthropic, 0, &many).0.unwrap();
        assert_eq!(reply.calls.len(), 8);
        assert_eq!(reply.calls[7].id, "t7");
        // What is sent back holds exactly the calls that will get a result.
        assert_eq!(reply.raw.unwrap().as_array().unwrap().len(), 8);

        let long = "x".repeat(MAX_ARGS_CHARS / 2 + 1);
        let piece = |n: u64| {
            openai_delta(
                json!({"tool_calls": [{"index": n, "id": "c", "function": {"name": "search_mail", "arguments": long}}]}),
            )
        };
        let result = read(AiProvider::OpenAiCompatible, 0, &[piece(0), piece(1)]).0;
        assert!(
            matches!(&result, Err(Error::Temporary { msg }) if msg == "the model asked for too much at once and was stopped")
        );
        let piece = json!({"type": "content_block_delta", "index": 0, "delta": {"type": "input_json_delta", "partial_json": long}});
        let result = read(AiProvider::Anthropic, 0, &[piece.clone(), piece]).0;
        assert!(matches!(&result, Err(Error::Temporary { msg }) if msg.contains("too much at once")));
        let whole = json!({"type": "message", "content": [{"type": "tool_use", "id": "t", "name": "n", "input": {"a": long, "b": long}}]});
        assert!(matches!(read(AiProvider::Anthropic, 0, &[whole]).0, Err(Error::Temporary { .. })));
    }

    #[test]
    fn a_turn_with_tools_keeps_the_rules_of_a_plain_answer() {
        // Reasoning in a `<think>` block stays hidden, also in front of a call.
        let events = [
            openai_delta(json!({"content": "<think>secret plan</think>"})),
            openai_delta(json!({"content": "Found it."})),
            openai_delta(json!({"reasoning_content": "never read"})),
            openai_end("stop"),
        ];
        let (reply, heard) = read(AiProvider::OpenAiCompatible, 0, &events);
        assert_eq!(reply.unwrap().text, "Found it.");
        assert_eq!(heard, "Found it.");
        // The service's own words never come out through an error.
        for (provider, event) in [
            (AiProvider::OpenAiCompatible, json!({"error": {"message": "sk-leak"}})),
            (
                AiProvider::Anthropic,
                json!({"type": "error", "error": {"type": "overloaded_error", "message": "sk-leak"}}),
            ),
        ] {
            let result = read(provider, 0, &[event]).0;
            assert!(matches!(&result, Err(Error::Temporary { msg }) if !msg.contains("sk-leak")), "{result:?}");
        }
        for (provider, event, word) in [
            (AiProvider::OpenAiCompatible, openai_end("length"), "length limit"),
            (AiProvider::OpenAiCompatible, openai_end("content_filter"), "declined"),
            (
                AiProvider::Anthropic,
                json!({"type": "message_delta", "delta": {"stop_reason": "max_tokens"}}),
                "length limit",
            ),
            (AiProvider::Anthropic, json!({"type": "message_delta", "delta": {"stop_reason": "refusal"}}), "declined"),
        ] {
            let result = read(provider, 0, &[event]).0;
            assert!(matches!(&result, Err(Error::Temporary { msg }) if msg.contains(word)), "{result:?}");
        }
        // An answer that never stops is cut at the same size.
        let chunk = "y".repeat(60_000);
        let events = [openai_delta(json!({"content": chunk})), openai_delta(json!({"content": chunk}))];
        let result = read(AiProvider::OpenAiCompatible, 0, &events).0;
        assert!(matches!(&result, Err(Error::Temporary { msg }) if msg.contains("longer than expected")));
    }
}
