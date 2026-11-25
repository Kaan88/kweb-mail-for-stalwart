//! Talking to the service: one streamed request in the OpenAI chat-completions protocol, the answer
//! handed out piece by piece. Nothing of the service's own words is put into an error, so the key
//! cannot come back out through one; the key itself is only ever a header of the request.
//!
//! The services that speak this protocol differ in what they accept and send (OpenAI, OpenRouter,
//! Gemini's OpenAI endpoint, Mistral, DeepSeek, Groq, a model on the user's own machine). The
//! request is shaped so that each of them takes it, and what a service wants back with its own tool
//! calls (Gemini's thought signatures, DeepSeek's `reasoning_content`, OpenRouter's
//! `reasoning_details`) is sent back to it unchanged and never shown.

use std::{fmt, time::Duration};

use reqwest::header::HeaderValue;
use serde_json::{json, Map, Value};
use tokio::time::timeout;

use super::{
    prompts, settings,
    sse::{SseParser, ThinkFilter, MAX_HIDDEN},
    wire::{Reply, ToolCall, ToolResult, ToolSpec, ToolsRefused, Turn},
};
use crate::{model::AiEffort, Error, Result};

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
    pub base_url: String,
    pub model: String,
    /// Sent only when it is not `Default`; the settings make it `Default` unless the service
    /// listed the level for this address and model (see `settings::Levels`).
    pub effort: AiEffort,
    pub key: Option<String>,
}

impl fmt::Debug for Endpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Endpoint")
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
    let mut body = json!({
        "model": ep.model,
        "stream": true,
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": user},
        ],
    });
    if let Some(level) = effort_name(ep.effort) {
        if is_openrouter(&ep.base_url) {
            // OpenRouter documents its own field and maps it for each provider behind it.
            body["reasoning"] = json!({"effort": level});
        } else {
            body["reasoning_effort"] = json!(level);
        }
    }
    Parts {
        url: format!("{}/chat/completions", ep.base_url),
        headers: ep.key.iter().map(|k| ("authorization", format!("Bearer {k}"))).collect(),
        body,
    }
}

/// The word of the protocol for an effort (`reasoning_effort`, OpenRouter's `reasoning.effort`);
/// `None` for `Default`, which is not sent.
pub fn effort_name(effort: AiEffort) -> Option<&'static str> {
    match effort {
        AiEffort::Default => None,
        AiEffort::Off => Some("none"),
        AiEffort::Minimal => Some("minimal"),
        AiEffort::Low => Some("low"),
        AiEffort::Medium => Some("medium"),
        AiEffort::High => Some("high"),
        AiEffort::XHigh => Some("xhigh"),
        AiEffort::Max => Some("max"),
    }
}

/// The level a service names with `word`; `None` for a word the app does not know.
pub fn effort_named(word: &str) -> Option<AiEffort> {
    Some(match word {
        "none" => AiEffort::Off,
        "minimal" => AiEffort::Minimal,
        "low" => AiEffort::Low,
        "medium" => AiEffort::Medium,
        "high" => AiEffort::High,
        "xhigh" => AiEffort::XHigh,
        "max" => AiEffort::Max,
        _ => return None,
    })
}

/// Whether `base_url` is OpenRouter's, which takes the effort as `reasoning.effort`.
pub fn is_openrouter(base_url: &str) -> bool {
    host_is(base_url, "openrouter.ai")
}

/// Whether `base_url` is Gemini's OpenAI endpoint, which refuses a function whose parameters
/// have no properties and wants the function's name with its result.
pub fn is_gemini(base_url: &str) -> bool {
    host_is(base_url, "generativelanguage.googleapis.com")
}

/// Whether a request goes out shaped for Gemini (see [`chat_parts`]): to Gemini's own endpoint, or
/// for a Gemini model through another one (a proxy of the user's that passes the tools on as they
/// are). Both changes are within the OpenAI protocol, so a service that does not need them takes
/// them too.
fn takes_gemini_rules(ep: &Endpoint) -> bool {
    is_gemini(&ep.base_url) || ep.model.to_ascii_lowercase().contains("gemini")
}

/// Whether the host of `base_url` is `name` or one of its subdomains.
pub fn host_is(base_url: &str, name: &str) -> bool {
    let host = url::Url::parse(base_url).ok().and_then(|u| u.host_str().map(str::to_ascii_lowercase));
    host.is_some_and(|host| {
        let host = host.trim_end_matches('.');
        host == name || host.strip_suffix(name).is_some_and(|rest| rest.ends_with('.'))
    })
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
    /// Takes the token counts of one `data` payload: `usage` of the event (the last chunk of a
    /// stream, or the whole answer). A later count of the same request replaces an earlier one.
    fn note(&mut self, data: &str) {
        if !data.contains("usage") {
            return;
        }
        let Ok(event) = serde_json::from_str::<Value>(data) else { return };
        let usage = &event["usage"];
        let count =
            |a: &str, b: &str| usage[a].as_u64().or(usage[b].as_u64()).map(|n| n.min(u64::from(u32::MAX)) as u32);
        self.input = count("prompt_tokens", "input_tokens").or(self.input);
        self.output = count("completion_tokens", "output_tokens").or(self.output);
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

/// One `data` payload of the stream. Only `content` is read: `reasoning_content` and the like are
/// never looked at. Payloads that are not JSON are skipped. An answer the service ended at its
/// length limit or held back is an error, not a complete answer.
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

fn stream_error() -> Error {
    Error::temporary("the service reported an error while answering")
}

/// What to tell the user about an HTTP status, without its body (see [`refusal`] for a 404).
pub fn status_error(status: u16) -> Error {
    match status {
        401 | 403 => Error::temporary("the service refused the key"),
        404 => Error::temporary(NOT_HERE),
        429 => Error::temporary("the service is busy or the quota is used up"),
        500..=599 => Error::temporary(format!("the service has a problem (status {status}); try again later")),
        _ => Error::temporary(format!("the service did not accept the request (status {status})")),
    }
}

/// A 404 whose body says nothing more: most likely the address is wrong.
pub const NOT_HERE: &str = "the service has nothing at this address; check the address of the service";
/// A 404 about the model: the service has no such model, or not for this key.
pub const UNKNOWN_MODEL: &str = "the service does not know this model, or this key may not use it";
/// A 404 that says the model works only through another interface than chat (OpenAI's Responses
/// API, an embedding or speech model).
pub const NOT_CHAT: &str = "this model does not take chat requests; pick another model";
/// A 404 from a router (OpenRouter) that has the model but no provider for this request.
pub const NO_ROUTE: &str = "the service has no provider that can answer this request with this model \
     (for example one that takes tools, or one your account's privacy settings allow)";
/// A refused request with tools whose body is about them, but does not say that the model takes
/// none (a description the service did not accept, a part of the conversation it did not take).
pub const TOOLS_NOT_ACCEPTED: &str = "the service did not accept the request with the assistant's tools \
     (status 400); try again, or try another model";
/// The service said the model wrote a call it could not read (Groq's `tool_use_failed`).
pub const BROKEN_CALL: &str = "the model wrote a tool call the service could not read; try again";

/// What to tell the user about a refused request. For a 404 the body is looked at for one fact
/// only, which kind of thing was not there, and that becomes one of the fixed sentences above:
/// nothing of the body itself is kept, shown or logged. Any other status is [`status_error`].
pub fn refusal(status: u16, body: &[u8]) -> Error {
    if status != 404 {
        return status_error(status);
    }
    let body = body.to_ascii_lowercase();
    let text = if has(&body, "not a chat model") || has(&body, "v1/responses") || has(&body, "only supported in") {
        NOT_CHAT
    } else if has(&body, "no endpoints found") || has(&body, "no allowed providers") {
        NO_ROUTE
    } else if has(&body, "model") {
        UNKNOWN_MODEL
    } else {
        NOT_HERE
    };
    Error::temporary(text)
}

/// Whether the lowercased `body` holds `word`.
fn has(body: &[u8], word: &str) -> bool {
    body.windows(word.len()).any(|w| w == word.as_bytes())
}

/// [`refusal`] for `response`; its body is read (at most [`REFUSAL_BODY`]) only for a 404.
pub async fn refused(response: reqwest::Response, idle: Duration) -> Error {
    let status = response.status().as_u16();
    if status != 404 {
        return status_error(status);
    }
    match read_capped(response, REFUSAL_BODY, idle).await {
        Ok(body) => refusal(status, &body),
        Err(_) => status_error(status),
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
    think: ThinkFilter,
    sink: Sink<'a>,
    finished: bool,
    stopped: bool,
    usage: Usage,
}

impl Reader<'_> {
    fn event(&mut self, data: &str) -> Result<()> {
        self.usage.note(data);
        let parsed = parse_openai(data)?;
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

/// Sends one request. The status is the caller's to judge.
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

/// Whether a refused request is one of those whose body is worth one look: 400, and 422, which
/// Mistral answers for a request it cannot take.
fn is_bad_request(status: u16) -> bool {
    matches!(status, 400 | 422)
}

/// Whether the service ignored `stream` and sent the whole answer as one JSON document.
fn is_one_piece(response: &reqwest::Response) -> bool {
    response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|t| t.to_ascii_lowercase().contains("application/json"))
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

/// [`complete`], and the tokens the service reported. With `ask_usage` the request also carries
/// `stream_options.include_usage` (without it a service usually reports nothing); a service that
/// refuses the request with it (400 or 422) is asked once more without it.
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
    let client = client(timeouts.connect, None)?;
    let parts = |usage: bool| {
        let mut parts = request_parts(ep, system, user);
        if usage {
            ask_for_usage(&mut parts);
        }
        parts
    };
    let mut response = post(&client, parts(ask_usage), &host, timeouts).await?;
    if ask_usage && is_bad_request(response.status().as_u16()) {
        // The same endpoint, so the key goes where it went before, and only once more.
        response = post(&client, parts(false), &host, timeouts).await?;
    }
    if !response.status().is_success() {
        return Err(refused(response, timeouts.idle).await);
    }

    let mut reader = Reader {
        think: ThinkFilter::default(),
        sink: Sink { answer: String::new(), chars: 0, on_text },
        finished: false,
        stopped: false,
        usage: Usage::default(),
    };
    if is_one_piece(&response) {
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
    let answer = reader.think.after_orphan_close(reader.sink.answer.trim()).trim().to_string();
    if answer.is_empty() {
        return Err(Error::temporary("the service sent an empty answer"));
    }
    Ok((answer, reader.usage))
}

/// Asks the service to report its token counts in the last chunk of the stream.
fn ask_for_usage(parts: &mut Parts) {
    parts.body["stream_options"] = json!({"include_usage": true});
}

/// The most calls taken from one turn; what the model asks for beyond them is dropped.
const MAX_CALLS: usize = 8;
/// More than this many characters of arguments in one turn is a model that does not stop.
const MAX_ARGS_CHARS: usize = 20_000;
/// The most that is read of a refused request's answer, to see whether it is about the tools.
pub const REFUSAL_BODY: usize = 64 * 1024;

/// Whether the body of a refused request names tools: "tool", "tools", "tool_choice" or
/// "function(s)". Only this one fact is taken from it.
fn names_tools(body: &[u8]) -> bool {
    let body = body.to_ascii_lowercase();
    has(&body, "tool") || has(&body, "function")
}

/// Whether the body of a refused request says that the model takes no tools at all (what Ollama,
/// vLLM, LM Studio, Groq, DeepSeek's reasoner and others say), as opposed to a request with tools
/// it did not like: a description it refused (Gemini's "Unknown name", a schema), a part of the
/// conversation it did not take (an id, a role, a missing thought signature). Only this one fact
/// is taken from it.
fn says_no_tools(body: &[u8]) -> bool {
    let body = body.to_ascii_lowercase();
    if !names_tools(&body) {
        return false;
    }
    const ABOUT_THE_REQUEST: [&str; 9] = [
        "schema",
        "properties",
        "invalid json payload",
        "unknown name",
        "thought_signature",
        "thought signature",
        "tool_call_id",
        "unexpected role",
        "tool_use_failed",
    ];
    if ABOUT_THE_REQUEST.iter().any(|word| has(&body, word)) {
        return false;
    }
    const NO_SUPPORT: [&str; 7] = [
        "not support",
        "not supported",
        "unsupported",
        "doesn't support",
        "not available for",
        "not enabled",
        "enable-auto-tool-choice",
    ];
    NO_SUPPORT.iter().any(|word| has(&body, word))
}

/// What to say about a 400 or 422 to a request with tools that did not say the model takes none.
fn tools_refusal(status: u16, body: &[u8]) -> Error {
    let body = body.to_ascii_lowercase();
    if has(&body, "tool_use_failed") || has(&body, "failed to call a function") {
        Error::temporary(BROKEN_CALL)
    } else if names_tools(&body) {
        Error::temporary(TOOLS_NOT_ACCEPTED)
    } else {
        status_error(status)
    }
}

fn too_much() -> Error {
    Error::temporary("the model asked for too much at once and was stopped")
}

/// What an assistant turn in a request says when the run before ended on tool results: some
/// services refuse a user message right after a tool result (Mistral).
pub const ENDED: &str = "[This request ended here.]";

/// The request for a conversation with tools.
///
/// Address, headers and the fields around the messages are those of [`request_parts`]. An
/// assistant turn with calls has no `content` when it has no text (some services turn an empty
/// text into an empty block they then refuse), and gets back what the service sent with it (see
/// [`Gather::calls`]). A user message right after tool results gets an assistant turn of
/// [`ENDED`] in front of it.
///
/// With `tools` empty no `tools` key is sent, and the turns are sent as plain text: a call the
/// model made earlier is left out and its result goes in a user message, framed as data as
/// always. A service refuses tool calls and results in a request that has no tools.
pub fn chat_parts(ep: &Endpoint, system: &str, turns: &[Turn], tools: &[ToolSpec]) -> Parts {
    let mut parts = request_parts(ep, system, "");
    let gemini = takes_gemini_rules(ep);
    let mut messages = vec![json!({"role": "system", "content": system})];
    if tools.is_empty() {
        for turn in turns {
            let (role, text) = match turn {
                Turn::User(text) => ("user", text.clone()),
                Turn::Assistant { text, .. } => ("assistant", text.clone()),
                Turn::Results(results) => ("user", results.iter().map(plain_result).collect::<Vec<_>>().join("\n\n")),
            };
            if text.trim().is_empty() {
                continue;
            }
            // Two turns of one side in a row become one message (never the system text).
            let past_system = messages.len() > 1;
            match messages.last_mut() {
                Some(last) if past_system && last["role"] == role => {
                    let joined = format!("{}\n\n{text}", last["content"].as_str().unwrap_or_default());
                    last["content"] = Value::String(joined);
                }
                _ => messages.push(json!({"role": role, "content": text})),
            }
        }
    } else {
        let mut after_results = false;
        for turn in turns {
            match turn {
                Turn::User(text) => {
                    if after_results {
                        messages.push(json!({"role": "assistant", "content": ENDED}));
                    }
                    messages.push(json!({"role": "user", "content": text}));
                }
                Turn::Assistant { text, calls, raw } => {
                    let mut message = json!({"role": "assistant", "content": text});
                    if !calls.is_empty() {
                        if text.is_empty() {
                            message["content"] = Value::Null;
                        }
                        let extra = |n: usize| raw.as_ref().and_then(|r| r["extra"].get(n)).and_then(Value::as_object);
                        message["tool_calls"] = calls
                            .iter()
                            .enumerate()
                            .map(|(n, c)| {
                                let mut call = json!({
                                    "id": c.id,
                                    "type": "function",
                                    "function": {"name": c.name, "arguments": c.args.to_string()},
                                });
                                for (key, value) in extra(n).into_iter().flatten() {
                                    call[key] = value.clone();
                                }
                                call
                            })
                            .collect();
                        if let Some(raw) = raw {
                            for key in KEPT_FIELDS {
                                if !raw[key].is_null() {
                                    message[key] = raw[key].clone();
                                }
                            }
                        }
                    }
                    messages.push(message);
                }
                Turn::Results(results) => messages.extend(results.iter().map(|r| {
                    let mut message = json!({"role": "tool", "tool_call_id": r.id, "content": r.content});
                    if gemini {
                        message["name"] = Value::String(r.name.clone());
                    }
                    message
                })),
            }
            after_results = matches!(turn, Turn::Results(_));
        }
    }
    parts.body["messages"] = Value::Array(messages);
    if !tools.is_empty() {
        parts.body["tools"] = tools
            .iter()
            .map(|t| {
                let mut function = json!({"name": t.name, "description": t.description, "parameters": t.schema});
                // Gemini refuses an object without properties: a function without arguments has none.
                if gemini && t.schema["properties"].as_object().is_none_or(Map::is_empty) {
                    function.as_object_mut().map(|f| f.remove("parameters"));
                }
                json!({"type": "function", "function": function})
            })
            .collect();
    }
    parts
}

/// A tool result as plain text, framed as data (the results the tools give are framed already).
fn plain_result(result: &ToolResult) -> String {
    if result.content.starts_with(prompts::TOOL_RESULT_OPEN) {
        result.content.clone()
    } else {
        prompts::tool_frame(&result.name, &result.content)
    }
}

/// What of the assistant message a service sent is kept and sent back with its calls: the reasoning
/// some services want back with a call (DeepSeek, Moonshot and vLLM's `reasoning_content`,
/// OpenRouter's `reasoning_details`). Never shown, never logged, never stored.
const KEPT_FIELDS: [&str; 2] = ["reasoning_content", "reasoning_details"];

/// What the model sent as arguments, when it is a JSON object; `{}` when it is anything else.
fn object_or_empty(text: &str) -> Value {
    serde_json::from_str::<Value>(text).ok().filter(Value::is_object).unwrap_or_else(|| json!({}))
}

/// A call of an answer while its pieces come in.
struct Pending {
    /// The `index` the stream names it by; a service that sends none starts a call with each
    /// entry that has an id or a name.
    key: Option<u64>,
    id: String,
    name: String,
    args: String,
    /// The fields of the entry beside `index`, `id`, `type` and `function` (Gemini's
    /// `extra_content` with its thought signature), sent back with the call.
    extra: Map<String, Value>,
}

/// What one turn holds besides its visible text: the calls, and what goes back with them.
#[derive(Default)]
struct Gather {
    pending: Vec<Pending>,
    /// The newest call without an `index` was dropped, and so are its further pieces.
    overflow: bool,
    /// Characters of arguments so far, of the dropped calls too.
    args_chars: usize,
    /// `reasoning_content` as it came, to send back with the calls.
    reasoning: String,
    /// `reasoning_details` as they came, pieces of one entry put together.
    details: Vec<Value>,
    /// Characters of reasoning kept so far; beyond [`MAX_HIDDEN`] nothing more is kept.
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

    /// Whether `more` characters of reasoning may still be kept.
    fn room(&mut self, more: usize) -> bool {
        if self.private_chars + more > MAX_HIDDEN {
            return false;
        }
        self.private_chars += more;
        true
    }

    /// One `data` payload of the stream, or the whole document of a service that ignored `stream`.
    /// Returns the text it carried. As in [`parse_openai`] only `content` is shown; the calls are
    /// read, and the reasoning is kept to be sent back.
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
            if let Some(piece) = said["reasoning_content"].as_str() {
                if self.room(piece.chars().count()) {
                    self.reasoning.push_str(piece);
                }
            }
            for item in said["reasoning_details"].as_array().into_iter().flatten() {
                self.detail(item);
            }
        }
        Ok(choice["delta"]["content"]
            .as_str()
            .or(choice["message"]["content"].as_str())
            .unwrap_or_default()
            .to_string())
    }

    /// One entry of `reasoning_details`: a piece of the one before it (same `index` and `type`)
    /// is added to it, anything else is a new entry.
    fn detail(&mut self, item: &Value) {
        let Some(item) = item.as_object() else { return };
        if !self.room(Value::Object(item.clone()).to_string().chars().count()) {
            return;
        }
        let same = |last: &Value| {
            last["index"] == item.get("index").cloned().unwrap_or(Value::Null)
                && last["type"] == item.get("type").cloned().unwrap_or(Value::Null)
        };
        match self.details.last_mut().filter(|last| same(last)) {
            Some(last) => {
                for (key, value) in item {
                    match (&mut last[key], value) {
                        (Value::String(whole), Value::String(piece))
                            if matches!(key.as_str(), "text" | "summary" | "data") =>
                        {
                            whole.push_str(piece)
                        }
                        (_, Value::Null) => {}
                        (slot, value) => *slot = value.clone(),
                    }
                }
            }
            None => self.details.push(Value::Object(item.clone())),
        }
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
        let extra: Map<String, Value> = entry
            .as_object()
            .into_iter()
            .flatten()
            .filter(|(key, value)| !matches!(key.as_str(), "index" | "id" | "type" | "function") && !value.is_null())
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        self.count_args(&piece)?;
        if !extra.is_empty() {
            self.count_args(&Value::Object(extra.clone()).to_string())?;
        }
        self.begun = true;
        let at = match entry["index"].as_u64() {
            // A service that numbers every call 0 (some do, for calls that each come whole) is
            // told apart by the new id.
            Some(key) => match self.pending.iter().rposition(|p| p.key == Some(key)) {
                Some(at) if id.is_empty() || self.pending[at].id.is_empty() || self.pending[at].id == id => Some(at),
                _ => self.open(Some(key)),
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
            call.extra.extend(extra);
        }
        Ok(())
    }

    /// A place for a new call, unless the turn already has as many as are taken.
    fn open(&mut self, key: Option<u64>) -> Option<usize> {
        self.overflow = self.pending.len() >= MAX_CALLS;
        if self.overflow {
            return None;
        }
        self.pending.push(Pending {
            key,
            id: String::new(),
            name: String::new(),
            args: String::new(),
            extra: Map::new(),
        });
        Some(self.pending.len() - 1)
    }

    /// The calls of the turn, and what goes back with them: `{"extra": [per call, or null],
    /// "reasoning_content": .., "reasoning_details": [..]}`, `None` when there is nothing.
    /// `prior` is how many calls the conversation already holds: a call that came without an id is
    /// named after its place in the conversation, so that no two share a name.
    fn calls(&mut self, prior: usize) -> (Vec<ToolCall>, Option<Value>) {
        let made = |n: usize| format!("call_{}", prior + n + 1);
        let calls: Vec<ToolCall> = self
            .pending
            .iter()
            .enumerate()
            .map(|(n, p)| ToolCall {
                id: if p.id.is_empty() { made(n) } else { p.id.clone() },
                name: p.name.clone(),
                args: object_or_empty(&p.args),
            })
            .collect();
        if calls.is_empty() {
            return (calls, None);
        }
        let mut raw = Map::new();
        if self.pending.iter().any(|p| !p.extra.is_empty()) {
            let extra = self
                .pending
                .iter()
                .map(|p| if p.extra.is_empty() { Value::Null } else { Value::Object(p.extra.clone()) })
                .collect();
            raw.insert("extra".into(), Value::Array(extra));
        }
        if !self.reasoning.is_empty() {
            raw.insert("reasoning_content".into(), Value::String(std::mem::take(&mut self.reasoning)));
        }
        if !self.details.is_empty() {
            raw.insert("reasoning_details".into(), Value::Array(std::mem::take(&mut self.details)));
        }
        (calls, (!raw.is_empty()).then_some(Value::Object(raw)))
    }
}

/// The state of one turn being read: the text goes the way of [`complete`], the rest is gathered.
struct ChatReader<'a> {
    think: ThinkFilter,
    sink: Sink<'a>,
    gather: Gather,
    usage: Usage,
}

impl<'a> ChatReader<'a> {
    fn new(on_text: &'a mut (dyn FnMut(&str) + Send)) -> ChatReader<'a> {
        ChatReader {
            think: ThinkFilter::default(),
            sink: Sink { answer: String::new(), chars: 0, on_text },
            gather: Gather::default(),
            usage: Usage::default(),
        }
    }

    fn event(&mut self, data: &str) -> Result<()> {
        self.usage.note(data);
        let text = self.gather.openai(data)?;
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
        let text = self.think.after_orphan_close(self.sink.answer.trim()).trim().to_string();
        let (calls, raw) = self.gather.calls(prior);
        if text.is_empty() && calls.is_empty() {
            return Err(Error::temporary("the service sent an empty answer"));
        }
        Ok(Reply { text, calls, raw })
    }
}

/// One model turn. A 400 (or Mistral's 422) to a request that carried tools, whose body says that
/// the model takes no tools, is tried once more without them on the same endpoint (the earlier
/// calls and results then go as plain text) and reported as ToolsRefused::Yes; when that second
/// request is refused with a 400 or 422 as well, the result is `Error::ToolsUnsupported`. A refusal
/// that is about the tools in another way is [`TOOLS_NOT_ACCEPTED`] (or [`BROKEN_CALL`]), never a
/// model without tools. Nothing of the service's words goes into an Error.
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
    let client = client(timeouts.connect, None)?;
    let mut refused = ToolsRefused::No;
    let (mut with_tools, mut with_usage) = (!tools.is_empty(), ask_usage);
    let parts = |with_tools: bool, with_usage: bool| {
        let mut parts = chat_parts(ep, system, turns, if with_tools { tools } else { &[] });
        if with_usage {
            ask_for_usage(&mut parts);
        }
        parts
    };
    // At most three requests, all to the same endpoint, so the key goes where it went before.
    let mut response = post(&client, parts(with_tools, with_usage), &host, timeouts).await?;
    loop {
        let status = response.status().as_u16();
        if response.status().is_success() {
            break;
        }
        if !is_bad_request(status) || (!with_tools && !with_usage) {
            if refused == ToolsRefused::Yes && is_bad_request(status) {
                // Still refused without tools: the model cannot be used for this at all.
                return Err(Error::ToolsUnsupported);
            }
            return Err(self::refused(response, timeouts.idle).await);
        }
        // The only thing ever read of an error's body, and only for a few words: nothing of it
        // is kept, shown or logged.
        let body = read_capped(response, REFUSAL_BODY, timeouts.idle).await.unwrap_or_default();
        if with_tools && says_no_tools(&body) {
            with_tools = false;
            refused = ToolsRefused::Yes;
        } else if with_usage {
            with_usage = false;
        } else if refused == ToolsRefused::Yes {
            return Err(Error::ToolsUnsupported);
        } else {
            return Err(tools_refusal(status, &body));
        }
        response = post(&client, parts(with_tools, with_usage), &host, timeouts).await?;
    }

    let mut reader = ChatReader::new(on_text);
    if is_one_piece(&response) {
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

    fn ep(key: Option<&str>) -> Endpoint {
        Endpoint {
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
    fn the_request_has_a_bearer_header_only_when_there_is_a_key() {
        let with = request_parts(&ep(Some("k-1")), "sys", "usr");
        assert_eq!(with.url, "https://api.example.com/v1/chat/completions");
        assert_eq!(header(&with, "authorization"), Some("Bearer k-1"));
        assert_eq!(header(&with, "x-api-key"), None);
        assert_eq!(
            with.body,
            json!({"model": "m-1", "stream": true, "messages": [
                {"role": "system", "content": "sys"}, {"role": "user", "content": "usr"}]})
        );
        let without = request_parts(&ep(None), "sys", "usr");
        assert!(without.headers.is_empty());
    }

    #[test]
    fn effort_goes_into_its_field_and_openrouter_gets_its_own() {
        for (effort, word) in [
            (AiEffort::Off, "none"),
            (AiEffort::Minimal, "minimal"),
            (AiEffort::Low, "low"),
            (AiEffort::Medium, "medium"),
            (AiEffort::High, "high"),
            (AiEffort::XHigh, "xhigh"),
            (AiEffort::Max, "max"),
        ] {
            let openai = Endpoint { effort, ..ep(None) };
            let body = request_parts(&openai, "s", "u").body;
            assert_eq!(body["reasoning_effort"], json!(word));
            assert!(body.get("reasoning").is_none());
            let router = Endpoint { base_url: "https://openrouter.ai/api/v1".into(), ..openai };
            let body = request_parts(&router, "s", "u").body;
            assert_eq!(body["reasoning"], json!({"effort": word}));
            assert!(body.get("reasoning_effort").is_none());
        }
        let body = request_parts(&ep(Some("k")), "s", "u").body;
        for field in ["output_config", "reasoning_effort", "reasoning"] {
            assert!(body.get(field).is_none(), "{field} sent for Default");
        }
    }

    #[test]
    fn effort_words_go_both_ways() {
        for word in ["none", "minimal", "low", "medium", "high", "xhigh", "max"] {
            let effort = effort_named(word).unwrap();
            assert_eq!(effort_name(effort), Some(word));
        }
        assert_eq!(effort_named("default"), None);
        assert_eq!(effort_named("HIGH"), None);
    }

    #[test]
    fn openrouter_and_gemini_are_known_by_their_host_only() {
        assert!(is_openrouter("https://openrouter.ai/api/v1"));
        assert!(is_openrouter("https://eu.openrouter.ai/api/v1"));
        assert!(!is_openrouter("https://notopenrouter.ai/api/v1"));
        assert!(!is_openrouter("https://openrouter.ai.example.com/v1"));
        assert!(!is_openrouter("https://example.com/openrouter.ai"));
        assert!(is_gemini("https://generativelanguage.googleapis.com/v1beta/openai"));
        assert!(!is_gemini("https://api.example.com/generativelanguage.googleapis.com"));
    }

    #[test]
    fn a_whole_answer_is_read_from_message_content() {
        let whole = r#"{"choices":[{"message":{"role":"assistant","content":"Hi"},"finish_reason":"stop"}]}"#;
        assert_eq!(parse_openai(whole).unwrap(), Parsed { text: "Hi".into(), finished: true, stop: false });
        let long = r#"{"choices":[{"message":{"content":"Hi"},"finish_reason":"length"}]}"#;
        assert!(matches!(parse_openai(long), Err(Error::Temporary { .. })));
    }

    #[test]
    fn the_token_counts_are_read_and_nothing_else() {
        let mut usage = Usage::default();
        usage.note(r#"{"choices":[{"delta":{"content":"hi"}}]}"#);
        assert_eq!(usage, Usage::default());
        usage.note(r#"{"choices":[],"usage":{"prompt_tokens":12,"completion_tokens":3,"total_tokens":15}}"#);
        assert_eq!(usage, Usage { input: Some(12), output: Some(3) });
        // A later count of the same request replaces an earlier one.
        usage.note(r#"{"usage":{"prompt_tokens":100,"completion_tokens":40}}"#);
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
        let mut parts = request_parts(&ep(None), "s", "u");
        assert!(parts.body.get("stream_options").is_none());
        ask_for_usage(&mut parts);
        assert_eq!(parts.body["stream_options"], json!({"include_usage": true}));
    }

    #[test]
    fn debug_never_shows_the_key() {
        let shown = format!("{:?}", ep(Some("sk-secret-123")));
        assert!(!shown.contains("secret") && shown.contains("set"));
    }

    #[test]
    fn events_give_content_and_ignore_reasoning() {
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
    fn statuses_become_plain_messages() {
        let msg = |s| status_error(s).to_string();
        assert_eq!(msg(401), "the service refused the key");
        assert_eq!(msg(403), "the service refused the key");
        assert_eq!(msg(404), NOT_HERE);
        assert_eq!(msg(429), "the service is busy or the quota is used up");
        assert!(msg(503).contains("status 503"));
        assert!(msg(400).contains("status 400"));
        assert!((401..=599).all(|s| matches!(status_error(s), Error::Temporary { .. })));
    }

    #[test]
    fn a_404_says_which_thing_was_not_there_and_nothing_of_the_body() {
        let msg = |body: &str| refusal(404, body.as_bytes()).to_string();
        // OpenAI, Gemini: the model.
        assert_eq!(
            msg(r#"{"error":{"message":"The model `gpt-x` does not exist sk-SECRET","code":"model_not_found"}}"#),
            UNKNOWN_MODEL
        );
        assert_eq!(
            msg(r#"{"error":{"code":404,"message":"models/x is not found for API version v1beta"}}"#),
            UNKNOWN_MODEL
        );
        // OpenAI: a model of another interface.
        assert_eq!(msg("This model is only supported in v1/responses and not in v1/chat/completions."), NOT_CHAT);
        assert_eq!(
            msg("This is not a chat model and thus not supported in the v1/chat/completions endpoint."),
            NOT_CHAT
        );
        // OpenRouter: the model is there, but no provider for this request.
        assert_eq!(msg(r#"{"error":{"message":"No endpoints found that support tool use.","code":404}}"#), NO_ROUTE);
        assert_eq!(
            msg(r#"{"error":{"message":"No allowed providers are available for the selected model."}}"#),
            NO_ROUTE
        );
        // A wrong path: a page, or nothing.
        assert_eq!(msg("<html>Not Found</html>"), NOT_HERE);
        assert_eq!(msg(""), NOT_HERE);
        assert!(!msg("model sk-SECRET").contains("SECRET"));
        // Any other status ignores the body.
        assert_eq!(refusal(400, b"model not found").to_string(), status_error(400).to_string());
        assert_eq!(refusal(401, b"no endpoints found").to_string(), "the service refused the key");
    }

    #[test]
    fn only_a_body_that_says_the_model_takes_no_tools_counts_as_one() {
        // What services say when the model takes none.
        for body in [
            r#"{"error":{"message":"registry.ollama.ai/library/gemma3:4b does not support tools"}}"#,
            r#"{"error":{"message":"This model does not support tools."}}"#,
            r#"{"error":{"message":"Functions are not supported by this model."}}"#,
            r#"{"error":{"message":"deepseek-reasoner does not support Function Calling"}}"#,
            r#"{"error":{"message":"tool calling is not supported with this model"}}"#,
            r#"{"object":"error","message":"\"auto\" tool choice requires --enable-auto-tool-choice and --tool-call-parser to be set"}"#,
            r#"{"error":"Model does not support tool use"}"#,
        ] {
            assert!(says_no_tools(body.as_bytes()), "{body}");
        }
        // What services say about a request with tools they did not like: never "no tools".
        for body in [
            // Gemini: a schema feature, an object without properties, a missing thought signature.
            r#"{"error":{"code":400,"message":"Invalid JSON payload received. Unknown name \"additionalProperties\" at 'tools[0].function_declarations[0].parameters': Cannot find field."}}"#,
            r#"{"error":{"message":"* GenerateContentRequest.tools[0].function_declarations[4].parameters.properties: should be non-empty for OBJECT type"}}"#,
            r#"{"error":{"message":"Function call is missing a thought_signature in functionCall parts. This is required for tools to work correctly"}}"#,
            // Mistral: the order of the turns; OpenAI: a result without its call.
            r#"{"object":"error","message":"Unexpected role 'user' after role 'tool'","type":"invalid_request_message_order"}"#,
            r#"{"error":{"message":"Invalid parameter: messages with role 'tool' must be a response to a preceeding message with 'tool_calls'."}}"#,
            r#"{"error":{"message":"Invalid 'messages[3].tool_call_id': not found"}}"#,
            // Groq: a broken call.
            r#"{"error":{"message":"Failed to call a function. Please adjust your prompt.","code":"tool_use_failed"}}"#,
            // Not about tools at all.
            r#"{"error":{"message":"max_tokens is not supported with this model"}}"#,
            "",
        ] {
            assert!(!says_no_tools(body.as_bytes()), "{body}");
        }
        assert_eq!(tools_refusal(400, br#"{"code":"tool_use_failed"}"#).to_string(), BROKEN_CALL);
        assert_eq!(
            tools_refusal(400, b"Unknown name \"additionalProperties\" at 'tools[0]'").to_string(),
            TOOLS_NOT_ACCEPTED
        );
        assert_eq!(
            tools_refusal(422, b"Extra inputs are not permitted sk-SECRET").to_string(),
            status_error(422).to_string()
        );
    }

    // ---- a conversation with tools --------------------------------------------------------

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
    fn read(prior: usize, events: &[Value]) -> (Result<Reply>, String) {
        let mut heard = String::new();
        let result = {
            let mut on_text = |piece: &str| heard.push_str(piece);
            let mut reader = ChatReader::new(&mut on_text);
            events.iter().try_for_each(|e| reader.event(&e.to_string())).and_then(|()| reader.finish(prior))
        };
        (result, heard)
    }

    fn delta(delta: Value) -> Value {
        json!({"choices": [{"index": 0, "delta": delta}]})
    }

    fn end(reason: &str) -> Value {
        json!({"choices": [{"index": 0, "delta": {}, "finish_reason": reason}]})
    }

    #[test]
    fn the_conversation_carries_calls_results_and_tools() {
        let parts = chat_parts(&ep(Some("k-1")), "sys", &turns(None), &tools());
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
        assert!(chat_parts(&ep(None), "sys", &turns(None), &tools()).headers.is_empty());
    }

    #[test]
    fn a_turn_of_calls_alone_has_no_content_and_a_question_after_results_gets_a_turn_before_it() {
        let turns = vec![
            Turn::User("Folders?".into()),
            Turn::Assistant { text: String::new(), calls: vec![call("c9", "list_folders", json!({}))], raw: None },
            Turn::Results(vec![ToolResult {
                id: "c9".into(),
                name: "list_folders".into(),
                content: "step limit reached".into(),
                is_error: true,
            }]),
            Turn::User("Then?".into()),
        ];
        let messages = chat_parts(&ep(None), "s", &turns, &tools()).body["messages"].clone();
        assert_eq!(messages[2]["content"], Value::Null);
        assert_eq!(messages[2]["tool_calls"][0]["id"], "c9");
        assert_eq!(messages[3]["role"], "tool");
        assert_eq!(messages[4], json!({"role": "assistant", "content": ENDED}));
        assert_eq!(messages[5], json!({"role": "user", "content": "Then?"}));
    }

    #[test]
    fn gemini_gets_no_parameters_for_a_tool_without_arguments_and_the_name_with_a_result() {
        let gemini =
            Endpoint { base_url: "https://generativelanguage.googleapis.com/v1beta/openai".into(), ..ep(Some("k")) };
        let parts = chat_parts(&gemini, "sys", &turns(None), &tools());
        let tools = &parts.body["tools"];
        assert!(tools[0]["function"]["parameters"].is_object());
        assert!(tools[1]["function"].get("parameters").is_none());
        assert_eq!(
            parts.body["messages"][3],
            json!({"role": "tool", "tool_call_id": "c1", "content": "one hit", "name": "search_mail"})
        );
        // A Gemini model through another address gets the same.
        let proxied = Endpoint { model: "Gemini-3-Flash".into(), ..ep(None) };
        assert!(chat_parts(&proxied, "sys", &turns(None), &self::tools()).body["tools"][1]["function"]
            .get("parameters")
            .is_none());
        // Elsewhere the name is not sent (OpenAI's tool messages have none).
        assert!(chat_parts(&ep(None), "sys", &turns(None), &self::tools()).body["messages"][3].get("name").is_none());
    }

    #[test]
    fn without_tools_the_conversation_goes_as_plain_text() {
        let parts = chat_parts(&ep(Some("k")), "sys", &[Turn::User("Hi".into())], &[]);
        assert!(parts.body.get("tools").is_none());
        // With one user turn it is the stage 1 request.
        assert_eq!(parts.body, request_parts(&ep(Some("k")), "sys", "Hi").body);
        // Earlier calls are left out and their results go as data in a user message.
        let parts = chat_parts(&ep(None), "sys", &turns(Some(json!({"reasoning_content": "x"}))), &[]);
        let messages = parts.body["messages"].as_array().unwrap().clone();
        assert!(messages.iter().all(|m| m.get("tool_calls").is_none() && m["role"] != "tool"));
        assert_eq!(messages[1], json!({"role": "user", "content": "Any mail from Maria?"}));
        assert_eq!(messages[2], json!({"role": "assistant", "content": "Looking."}));
        let results = messages[3]["content"].as_str().unwrap();
        assert_eq!(messages[3]["role"], "user");
        assert!(results.contains("<tool_result name=\"search_mail\">\none hit\n</tool_result>"), "{results}");
        assert!(results.contains("<tool_result name=\"list_folders\">\nno\n</tool_result>"));
        assert_eq!(messages[4], json!({"role": "assistant", "content": "One."}));
        assert_eq!(messages[5], json!({"role": "user", "content": "And today?"}));
        assert_eq!(messages.len(), 6);
        // Results right before a question become one user message with it.
        let turns = vec![
            Turn::User("Q".into()),
            Turn::Assistant { text: String::new(), calls: vec![call("c", "list_folders", json!({}))], raw: None },
            Turn::Results(vec![ToolResult {
                id: "c".into(),
                name: "list_folders".into(),
                content: "r".into(),
                is_error: false,
            }]),
            Turn::User("Q2".into()),
        ];
        let messages = chat_parts(&ep(None), "sys", &turns, &[]).body["messages"].as_array().unwrap().clone();
        assert_eq!(messages.len(), 2);
        assert!(messages[1]["content"].as_str().unwrap().starts_with("Q\n\n<tool_result"));
        assert!(messages[1]["content"].as_str().unwrap().ends_with("</tool_result>\n\nQ2"));
    }

    #[test]
    fn calls_are_put_together_from_their_fragments() {
        let events = [
            delta(json!({"role": "assistant", "content": ""})),
            delta(json!({"content": "Let me "})),
            delta(json!({"content": "look."})),
            delta(json!({"tool_calls": [{"index": 0, "id": "c1", "type": "function",
                "function": {"name": "search_mail", "arguments": ""}}]})),
            delta(json!({"tool_calls": [{"index": 0, "function": {"arguments": "{\"que"}}]})),
            delta(json!({"tool_calls": [{"index": 1, "id": "c2", "type": "function",
                "function": {"name": "list_folders", "arguments": "{"}}]})),
            // Pieces of two calls in one event, and a name said again.
            delta(json!({"tool_calls": [
                {"index": 0, "function": {"name": "search_mail", "arguments": "ry\":\"maria\"}"}},
                {"index": 1, "function": {"arguments": "}"}}]})),
            end("tool_calls"),
        ];
        let (reply, heard) = read(0, &events);
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
    fn a_whole_call_in_one_chunk_a_missing_id_and_calls_that_share_an_index_are_taken() {
        // No `index`, no id: each entry with a name is a call, named after its place in the
        // conversation (three calls came before).
        let events = [
            delta(json!({"tool_calls": [{"function": {"name": "list_folders", "arguments": "{}"}}]})),
            delta(json!({"tool_calls": [{"function": {"name": "search_mail", "arguments": "{\"query\""}}]})),
            delta(json!({"tool_calls": [{"function": {"arguments": ":\"x\"}"}}]})),
            end("stop"),
        ];
        let (reply, heard) = read(3, &events);
        assert_eq!(
            reply.unwrap().calls,
            [call("call_4", "list_folders", json!({})), call("call_5", "search_mail", json!({"query": "x"}))]
        );
        assert_eq!(heard, "");
        // Arguments sent as the object itself.
        let events = [
            delta(json!({"tool_calls": [{"index": 0, "id": "c1",
                "function": {"name": "search_mail", "arguments": {"query": "y"}}}]})),
            end("tool_calls"),
        ];
        assert_eq!(read(0, &events).0.unwrap().calls, [call("c1", "search_mail", json!({"query": "y"}))]);
        // Two whole calls, both numbered 0, in one event and in two: two calls, not one.
        let both = json!({"tool_calls": [
            {"index": 0, "id": "a", "function": {"name": "search_mail", "arguments": "{\"query\":\"a\"}"}},
            {"index": 0, "id": "b", "function": {"name": "list_folders", "arguments": "{}"}}]});
        let reply = read(0, &[delta(both), end("stop")]).0.unwrap();
        assert_eq!(
            reply.calls,
            [call("a", "search_mail", json!({"query": "a"})), call("b", "list_folders", json!({}))]
        );
        let one = |id: &str| {
            delta(
                json!({"tool_calls": [{"index": 0, "id": id, "function": {"name": "list_folders", "arguments": "{}"}}]}),
            )
        };
        assert_eq!(read(0, &[one("a"), one("b"), end("tool_calls")]).0.unwrap().calls.len(), 2);
    }

    #[test]
    fn a_whole_document_with_calls_is_read() {
        let whole = json!({"choices": [{"index": 0, "finish_reason": "tool_calls", "message": {
            "role": "assistant", "content": null, "tool_calls": [
                {"id": "c1", "type": "function", "function": {"name": "search_mail", "arguments": "{\"query\":\"a\"}"}},
                {"id": "c2", "type": "function", "function": {"name": "list_folders", "arguments": "{}"}}]}}]});
        let (reply, _) = read(0, &[whole]);
        assert_eq!(
            reply.unwrap(),
            Reply {
                text: String::new(),
                calls: vec![call("c1", "search_mail", json!({"query": "a"})), call("c2", "list_folders", json!({}))],
                raw: None,
            }
        );
    }

    #[test]
    fn what_a_service_sends_with_its_calls_goes_back_to_it_unchanged_and_is_never_shown() {
        // Gemini: a thought signature on each call. DeepSeek: the reasoning of the turn.
        // OpenRouter: reasoning details in pieces.
        let events = [
            delta(json!({"reasoning_content": "first "})),
            delta(json!({"reasoning_content": "think", "reasoning_details": [
                {"type": "reasoning.text", "text": "Plan ", "index": 0, "format": "x"}]})),
            delta(
                json!({"reasoning_details": [{"type": "reasoning.text", "text": "this.", "index": 0, "signature": "sig-1"}]}),
            ),
            delta(json!({"reasoning_details": [{"type": "reasoning.encrypted", "data": "opaque", "index": 1}]})),
            delta(json!({"tool_calls": [{"index": 0, "id": "g1", "type": "function",
                "function": {"name": "search_mail", "arguments": "{\"query\":\"a\"}"},
                "extra_content": {"google": {"thought_signature": "c2lnbmF0dXJl"}}}]})),
            delta(json!({"tool_calls": [{"index": 1, "id": "g2", "type": "function",
                "function": {"name": "list_folders", "arguments": "{}"}}]})),
            end("tool_calls"),
        ];
        let (reply, heard) = read(0, &events);
        let reply = reply.unwrap();
        assert_eq!(heard, "", "nothing of it is shown");
        let raw = json!({
            "extra": [{"extra_content": {"google": {"thought_signature": "c2lnbmF0dXJl"}}}, null],
            "reasoning_content": "first think",
            "reasoning_details": [
                {"type": "reasoning.text", "text": "Plan this.", "index": 0, "format": "x", "signature": "sig-1"},
                {"type": "reasoning.encrypted", "data": "opaque", "index": 1}],
        });
        assert_eq!(reply.raw, Some(raw));
        // And that is what the next request sends back with the calls.
        let next = [
            Turn::User("Q".into()),
            Turn::Assistant { text: reply.text.clone(), calls: reply.calls.clone(), raw: reply.raw.clone() },
        ];
        let sent = chat_parts(&ep(None), "s", &next, &tools()).body["messages"][2].clone();
        assert_eq!(sent["content"], Value::Null);
        assert_eq!(sent["reasoning_content"], "first think");
        assert_eq!(sent["reasoning_details"][0]["text"], "Plan this.");
        assert_eq!(sent["tool_calls"][0]["extra_content"]["google"]["thought_signature"], "c2lnbmF0dXJl");
        assert!(sent["tool_calls"][1].get("extra_content").is_none());
        // A turn without calls keeps nothing of it.
        let plain = [delta(json!({"reasoning_content": "hmm"})), delta(json!({"content": "Done."})), end("stop")];
        assert_eq!(read(0, &plain).0.unwrap().raw, None);
    }

    #[test]
    fn a_turn_of_calls_alone_is_an_answer_and_one_of_nothing_is_not() {
        let events = [
            delta(json!({"role": "assistant", "content": ""})),
            delta(
                json!({"tool_calls": [{"index": 0, "id": "c1", "function": {"name": "list_folders", "arguments": "{}"}}]}),
            ),
            end("tool_calls"),
        ];
        let (reply, heard) = read(0, &events);
        assert_eq!(
            reply.unwrap(),
            Reply { text: String::new(), calls: vec![call("c1", "list_folders", json!({}))], raw: None }
        );
        assert_eq!(heard, "");
        for events in [
            vec![delta(json!({"content": "  \n"})), end("stop")],
            vec![delta(json!({"content": "<think>only this</think>"}))],
        ] {
            let result = read(0, &events).0;
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
                delta(json!({"tool_calls": [{"index": 0, "id": "c1",
                    "function": {"name": "search_mail", "arguments": arguments}}]})),
                end("tool_calls"),
            ];
            let reply = read(0, &events).0.unwrap();
            assert_eq!(reply.calls, [call("c1", "search_mail", json!({}))], "{arguments}");
        }
    }

    #[test]
    fn the_first_eight_calls_are_kept_and_endless_arguments_are_an_error() {
        let many: Vec<Value> = (0..12)
            .map(|n| {
                delta(json!({"tool_calls": [{"index": n, "id": format!("c{n}"),
                    "function": {"name": "list_folders", "arguments": "{}"}}]}))
            })
            .chain([delta(json!({"tool_calls": [{"index": 11, "function": {"arguments": " "}}]}))])
            .chain([end("tool_calls")])
            .collect();
        let reply = read(0, &many).0.unwrap();
        assert_eq!(
            reply.calls.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
            ["c0", "c1", "c2", "c3", "c4", "c5", "c6", "c7"]
        );
        // The same without `index`: the pieces of a dropped call do not land in the one before.
        let many: Vec<Value> = (0..9)
            .map(|n| delta(json!({"tool_calls": [{"id": format!("c{n}"), "function": {"name": "list_folders"}}]})))
            .chain([delta(json!({"tool_calls": [{"function": {"arguments": "[]"}}]}))])
            .collect();
        let reply = read(0, &many).0.unwrap();
        assert_eq!(reply.calls.len(), 8);
        assert_eq!(reply.calls[7], call("c7", "list_folders", json!({})));

        let long = "x".repeat(MAX_ARGS_CHARS / 2 + 1);
        let piece = |n: u64| {
            delta(
                json!({"tool_calls": [{"index": n, "id": "c", "function": {"name": "search_mail", "arguments": long}}]}),
            )
        };
        let result = read(0, &[piece(0), piece(1)]).0;
        assert!(
            matches!(&result, Err(Error::Temporary { msg }) if msg == "the model asked for too much at once and was stopped")
        );
    }

    #[test]
    fn a_turn_with_tools_keeps_the_rules_of_a_plain_answer() {
        // Reasoning in a `<think>` block stays hidden, also in front of a call.
        let events = [
            delta(json!({"content": "<think>secret plan</think>"})),
            delta(json!({"content": "Found it."})),
            delta(json!({"reasoning_content": "never shown"})),
            end("stop"),
        ];
        let (reply, heard) = read(0, &events);
        assert_eq!(reply.unwrap().text, "Found it.");
        assert_eq!(heard, "Found it.");
        // The service's own words never come out through an error.
        let result = read(0, &[json!({"error": {"message": "sk-leak"}})]).0;
        assert!(matches!(&result, Err(Error::Temporary { msg }) if !msg.contains("sk-leak")), "{result:?}");
        for (event, word) in [(end("length"), "length limit"), (end("content_filter"), "declined")] {
            let result = read(0, &[event]).0;
            assert!(matches!(&result, Err(Error::Temporary { msg }) if msg.contains(word)), "{result:?}");
        }
        // An answer that never stops is cut at the same size.
        let chunk = "y".repeat(60_000);
        let events = [delta(json!({"content": chunk})), delta(json!({"content": chunk}))];
        let result = read(0, &events).0;
        assert!(matches!(&result, Err(Error::Temporary { msg }) if msg.contains("longer than expected")));
    }
}
