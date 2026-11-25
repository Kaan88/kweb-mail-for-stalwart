//! An in-process mock of an AI service speaking the OpenAI protocol over plain http on 127.0.0.1:
//! `POST /v1/chat/completions` (streaming, or one JSON document for `Script::Whole`) and
//! `GET /v1/models`. It records what it received, so a test can assert exactly what left the
//! device, and it echoes whatever credentials it got into its error answers, so a test can check
//! that the key never comes back out through an error message.
//!
//! [`Vendor`] makes it as strict as one real service is about the request, with that service's
//! own refusal (Gemini, Mistral, DeepSeek, OpenRouter in front of Anthropic), and makes the
//! answers carry what that service sends with its tool calls.

use std::{
    collections::{HashMap, VecDeque},
    convert::Infallible,
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{
    body::{Body, Bytes},
    extract::{Request, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Router,
};
use futures_util::stream;
use serde_json::{json, Value};
use tokio::task::JoinHandle;

/// A tool call the mock asks for.
#[derive(Clone, Debug)]
pub struct MockCall {
    pub id: String,
    pub name: String,
    pub args: Value,
}

/// What the mock does with the next requests.
#[derive(Clone, Debug)]
pub enum Script {
    /// A normal answer, one event per piece, ended properly.
    Answer(Vec<String>),
    /// An error status; the body echoes the credentials it was sent.
    Status(u16),
    /// An error status with this body, followed by the credentials it was sent.
    StatusBody(u16, String),
    /// A redirect to this address.
    Redirect(String),
    /// The pieces, then silence for ever: the connection stays open.
    Hang(Vec<String>),
    /// The pieces, then the connection closes without saying the answer is complete.
    Cut(Vec<String>),
    /// The pieces, then an error event that echoes the credentials.
    StreamError(Vec<String>),
    /// A service that ignores `stream` and answers with this text as one JSON document.
    Whole(String),
    /// The pieces, then the service's own output limit: `finish_reason: length`.
    Length(Vec<String>),
    /// Text pieces, then these tool calls (arguments streamed in two fragments), ended with
    /// finish_reason "tool_calls".
    Tools { text: Vec<String>, calls: Vec<MockCall> },
    /// A request that carries `tools` gets a 400 whose body says the model does not support
    /// tools (and echoes the credentials); one without gets these pieces as a normal answer.
    NoTools(Vec<String>),
    /// A model that takes no functions at all: every request, with tools or without, gets a 400 whose
    /// body says so (and echoes the credentials).
    NoFunctions,
}

/// How strict the mock is, and what it sends with its calls, like one real service.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Vendor {
    /// Takes any well-formed request.
    #[default]
    Plain,
    /// Gemini's OpenAI endpoint: refuses `additionalProperties` and an object without properties
    /// in a tool's parameters, sends a thought signature with each call
    /// (`extra_content.google.thought_signature`) and refuses a later request whose assistant
    /// turn lost it.
    Gemini,
    /// Mistral: refuses a user message right after a tool message, and answers 422 to
    /// `stream_options`.
    Mistral,
    /// DeepSeek in thinking mode: sends `reasoning_content` with its calls and refuses a later
    /// request whose assistant turn with calls lost it.
    DeepSeek,
    /// OpenRouter in front of Anthropic: refuses an assistant turn whose content is an empty text.
    OpenRouterAnthropic,
}

/// The thought signature the Gemini mock gives the call `id`.
pub fn signature_of(id: &str) -> String {
    format!("sig-{id}")
}

/// The reasoning the DeepSeek mock sends with its calls.
pub const DEEPSEEK_REASONING: &str = "deepseek private reasoning";

/// What `GET /v1/models` answers.
#[derive(Clone, Debug)]
pub enum Models {
    /// `{"data":[{"id":..}]}`, in this order.
    Ids(Vec<String>),
    /// An error status; the body echoes the credentials it was sent.
    Status(u16),
    /// A redirect to this address.
    Redirect(String),
    /// This JSON document as it is (any service's shape).
    Raw(Value),
    /// A 200 that is not JSON.
    Garbage,
    /// A JSON document longer than any list should be.
    Huge,
}

/// One request as it arrived.
#[derive(Clone, Debug)]
pub struct Seen {
    pub path: String,
    pub authorization: Option<String>,
    /// An `x-api-key` header; the app never sends one.
    pub api_key: Option<String>,
    pub body: Value,
    /// The mock refused it as the vendor would have.
    pub refused: bool,
}

impl Seen {
    /// The system prompt.
    pub fn system(&self) -> String {
        self.body["messages"][0]["content"].as_str().unwrap_or_default().to_string()
    }

    /// The user's message.
    pub fn user(&self) -> String {
        let last = self.body["messages"].as_array().and_then(|m| m.last()).cloned().unwrap_or_default();
        last["content"].as_str().unwrap_or_default().to_string()
    }

    /// The messages of the request as they were sent.
    pub fn messages(&self) -> Vec<Value> {
        self.body["messages"].as_array().cloned().unwrap_or_default()
    }

    /// The names of the tools the request offered, in order; empty when it had no `tools` key.
    pub fn tool_names(&self) -> Vec<String> {
        let name = |tool: &Value| tool["function"]["name"].as_str().map(str::to_string);
        self.body["tools"].as_array().into_iter().flatten().filter_map(name).collect()
    }

    /// (call id, content) of every tool result in the request.
    pub fn tool_results(&self) -> Vec<(String, String)> {
        let text = |value: &Value| value.as_str().unwrap_or_default().to_string();
        self.messages()
            .iter()
            .filter(|m| m["role"] == "tool")
            .map(|m| (text(&m["tool_call_id"]), text(&m["content"])))
            .collect()
    }
}

struct Shared {
    script: Mutex<Script>,
    /// Scripts that answer the next requests, one each, before `script` does.
    queue: Mutex<VecDeque<Script>>,
    models: Mutex<Models>,
    vendor: Mutex<Vendor>,
    split_events: Mutex<bool>,
    seen: Mutex<Vec<Seen>>,
    hits: Mutex<HashMap<String, usize>>,
}

pub struct MockAi {
    shared: Arc<Shared>,
    base: String,
    task: JoinHandle<()>,
}

impl Drop for MockAi {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl MockAi {
    pub async fn start() -> MockAi {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind the mock service");
        let base = format!("http://{}/v1", listener.local_addr().unwrap());
        let shared = Arc::new(Shared {
            script: Mutex::new(Script::Answer(vec!["ok".into()])),
            queue: Mutex::default(),
            models: Mutex::new(Models::Ids(Vec::new())),
            vendor: Mutex::default(),
            split_events: Mutex::new(false),
            seen: Mutex::default(),
            hits: Mutex::default(),
        });
        let app = Router::new()
            .route("/v1/chat/completions", post(chat))
            .route("/v1/models", get(models))
            .fallback(other)
            .with_state(shared.clone());
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        MockAi { shared, base, task }
    }

    /// The address to put in the settings: `http://127.0.0.1:<port>/v1`.
    pub fn base_url(&self) -> String {
        self.base.clone()
    }

    /// `127.0.0.1:<port>`.
    pub fn host(&self) -> String {
        self.base.trim_start_matches("http://").trim_end_matches("/v1").to_string()
    }

    pub fn set(&self, script: Script) {
        *self.shared.script.lock().unwrap() = script;
    }

    /// What the model list answers with.
    pub fn set_models(&self, models: Models) {
        *self.shared.models.lock().unwrap() = models;
    }

    /// Which real service the mock behaves like.
    pub fn set_vendor(&self, vendor: Vendor) {
        *self.shared.vendor.lock().unwrap() = vendor;
    }

    /// Each request takes the next script; when the queue is empty the `set` script answers.
    pub fn queue(&self, scripts: Vec<Script>) {
        self.shared.queue.lock().unwrap().extend(scripts);
    }

    /// Answers with these pieces.
    pub fn answer(&self, pieces: &[&str]) {
        self.set(Script::Answer(pieces.iter().map(|p| p.to_string()).collect()));
    }

    /// Cuts every event in two network chunks, in the middle.
    pub fn split_events(&self, on: bool) {
        *self.shared.split_events.lock().unwrap() = on;
    }

    /// The requests of the assistant, oldest first.
    pub fn seen(&self) -> Vec<Seen> {
        self.shared.seen.lock().unwrap().clone()
    }

    pub fn last(&self) -> Seen {
        self.seen().pop().expect("the service got no request")
    }

    /// Requests per path, the unknown paths included: a remote image would show up here.
    pub fn hits(&self, path: &str) -> usize {
        self.shared.hits.lock().unwrap().get(path).copied().unwrap_or(0)
    }

    pub fn requests(&self) -> usize {
        self.shared.hits.lock().unwrap().values().sum()
    }
}

async fn other(State(s): State<Arc<Shared>>, req: Request) -> StatusCode {
    *s.hits.lock().unwrap().entry(req.uri().path().to_string()).or_default() += 1;
    StatusCode::NOT_FOUND
}

fn credentials(headers: &HeaderMap) -> String {
    let get = |n: &str| headers.get(n).and_then(|v| v.to_str().ok()).unwrap_or_default().to_string();
    format!("{} {}", get("authorization"), get("x-api-key")).trim().to_string()
}

fn record(s: &Shared, path: &str, headers: &HeaderMap, body: Value, refused: bool) {
    *s.hits.lock().unwrap().entry(path.to_string()).or_default() += 1;
    let get = |n: &str| headers.get(n).and_then(|v| v.to_str().ok()).map(str::to_string);
    s.seen.lock().unwrap().push(Seen {
        path: path.to_string(),
        authorization: get("authorization"),
        api_key: get("x-api-key"),
        body,
        refused,
    });
}

/// The token counts the mock reports in the last chunk of a stream that asked for them
/// (`stream_options.include_usage`).
pub const USAGE_IN: u32 = 123;
pub const USAGE_OUT: u32 = 45;

/// What the vendor would refuse in `body`: its status and the words of its answer.
fn vendor_refusal(vendor: Vendor, body: &Value) -> Option<(StatusCode, String)> {
    let messages = body["messages"].as_array().cloned().unwrap_or_default();
    let with_calls = || messages.iter().filter(|m| m["role"] == "assistant" && m["tool_calls"].is_array());
    let bad = |text: &str| Some((StatusCode::BAD_REQUEST, text.to_string()));
    match vendor {
        Vendor::Plain => None,
        Vendor::Gemini => {
            for (n, tool) in body["tools"].as_array().into_iter().flatten().enumerate() {
                let parameters = &tool["function"]["parameters"];
                if parameters.get("additionalProperties").is_some() {
                    return bad(&format!(
                        r#"[{{"error":{{"code":400,"message":"Invalid JSON payload received. Unknown name \"additionalProperties\" at 'tools[0].function_declarations[{n}].parameters': Cannot find field.","status":"INVALID_ARGUMENT"}}}}]"#
                    ));
                }
                if parameters.is_object() && parameters["properties"].as_object().is_none_or(|p| p.is_empty()) {
                    return bad(&format!(
                        r#"{{"error":{{"code":400,"message":"* GenerateContentRequest.tools[0].function_declarations[{n}].parameters.properties: should be non-empty for OBJECT type","status":"INVALID_ARGUMENT"}}}}"#
                    ));
                }
            }
            for call in with_calls().flat_map(|m| m["tool_calls"].as_array().cloned().unwrap_or_default()) {
                let id = call["id"].as_str().unwrap_or_default();
                if call["extra_content"]["google"]["thought_signature"].as_str() != Some(signature_of(id).as_str()) {
                    return bad(
                        r#"{"error":{"code":400,"message":"Function call is missing a thought_signature in functionCall parts. This is required for tools to work correctly.","status":"INVALID_ARGUMENT"}}"#,
                    );
                }
            }
            None
        }
        Vendor::Mistral => {
            if body.get("stream_options").is_some() {
                return Some((
                    StatusCode::UNPROCESSABLE_ENTITY,
                    r#"{"object":"error","message":{"detail":[{"type":"extra_forbidden","loc":["body","stream_options"],"msg":"Extra inputs are not permitted"}]},"type":"invalid_request_error"}"#.into(),
                ));
            }
            let after_tool = messages.windows(2).any(|w| w[0]["role"] == "tool" && w[1]["role"] == "user");
            if after_tool {
                return bad(
                    r#"{"object":"error","message":"Unexpected role 'user' after role 'tool'","type":"invalid_request_message_order","code":"3230"}"#,
                );
            }
            None
        }
        Vendor::DeepSeek => {
            if with_calls().any(|m| m["reasoning_content"].as_str() != Some(DEEPSEEK_REASONING)) {
                return bad(
                    r#"{"error":{"message":"Missing `reasoning_content` field in the assistant message at message index 2.","type":"invalid_request_error"}}"#,
                );
            }
            None
        }
        Vendor::OpenRouterAnthropic => {
            if messages.iter().any(|m| m["role"] == "assistant" && m["content"] == "") {
                return bad(
                    r#"{"error":{"message":"Provider returned error","code":400,"metadata":{"raw":"messages: text content blocks must be non-empty"}}}"#,
                );
            }
            None
        }
    }
}

async fn chat(State(s): State<Arc<Shared>>, headers: HeaderMap, body: Bytes) -> Response {
    let body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let vendor = *s.vendor.lock().unwrap();
    if let Some((status, words)) = vendor_refusal(vendor, &body) {
        record(&s, "/v1/chat/completions", &headers, body, true);
        return (status, format!("{words} {}", credentials(&headers))).into_response();
    }
    let (tools, usage) = (body.get("tools").is_some(), body["stream_options"]["include_usage"] == true);
    record(&s, "/v1/chat/completions", &headers, body, false);
    respond(&s, &headers, vendor, tools, usage)
}

async fn models(State(s): State<Arc<Shared>>, headers: HeaderMap) -> Response {
    record(&s, "/v1/models", &headers, Value::Null, false);
    let script = s.models.lock().unwrap().clone();
    let json = |value: Value| {
        Response::builder()
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(value.to_string()))
            .unwrap()
    };
    match script {
        Models::Ids(ids) => json(
            json!({"object": "list", "data": ids.iter().map(|id| json!({"id": id, "object": "model"})).collect::<Vec<_>>()}),
        ),
        Models::Status(code) => {
            let body = json!({"error": {"message": format!("Incorrect API key provided: {}", credentials(&headers))}});
            (StatusCode::from_u16(code).unwrap(), body.to_string()).into_response()
        }
        Models::Redirect(to) => Response::builder()
            .status(StatusCode::TEMPORARY_REDIRECT)
            .header(header::LOCATION, to)
            .body(Body::empty())
            .unwrap(),
        Models::Raw(value) => json(value),
        Models::Garbage => (StatusCode::OK, "this is not json").into_response(),
        Models::Huge => {
            (StatusCode::OK, format!("{{\"data\":[],\"pad\":\"{}\"}}", "x".repeat(2_200_000))).into_response()
        }
    }
}

/// One step of a stream: bytes to send, or a pause (`None` ends the stream).
enum Step {
    Send(Vec<u8>),
    Pause,
}

fn respond(s: &Shared, headers: &HeaderMap, vendor: Vendor, tools: bool, usage: bool) -> Response {
    let queued = s.queue.lock().unwrap().pop_front();
    let script = queued.unwrap_or_else(|| s.script.lock().unwrap().clone());
    let echoed = credentials(headers);
    let (pieces, tail) = match script {
        Script::Status(code) => {
            let body = json!({"error": {"message": format!("Incorrect API key provided: {echoed}")}});
            return (StatusCode::from_u16(code).unwrap(), body.to_string()).into_response();
        }
        Script::StatusBody(code, text) => {
            return (StatusCode::from_u16(code).unwrap(), format!("{text} {echoed}")).into_response();
        }
        Script::Redirect(to) => {
            return Response::builder()
                .status(StatusCode::TEMPORARY_REDIRECT)
                .header(header::LOCATION, to)
                .body(Body::empty())
                .unwrap();
        }
        Script::Whole(text) => {
            let body = json!({"choices": [{"index": 0, "message": {"role": "assistant", "content": text}, "finish_reason": "stop"}]});
            return Response::builder()
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap();
        }
        Script::Answer(pieces) => (pieces, Tail::Done),
        Script::Hang(pieces) => (pieces, Tail::Hang),
        Script::Cut(pieces) => (pieces, Tail::Cut),
        Script::StreamError(pieces) => (pieces, Tail::Error(echoed)),
        Script::Length(pieces) => (pieces, Tail::Length),
        Script::Tools { text, calls } => (text, Tail::Tools(calls)),
        Script::NoTools(_) if tools => {
            let message = format!("This model does not support tools. Credentials sent: {echoed}");
            let body = json!({"error": {"message": message, "type": "invalid_request_error"}});
            return (StatusCode::BAD_REQUEST, body.to_string()).into_response();
        }
        Script::NoTools(pieces) => (pieces, Tail::Done),
        Script::NoFunctions => {
            let message = format!("Functions are not supported by this model. Credentials sent: {echoed}");
            let body = json!({"error": {"message": message, "type": "invalid_request_error"}});
            return (StatusCode::BAD_REQUEST, body.to_string()).into_response();
        }
    };
    let events = events(&pieces, &tail, usage, vendor);
    let split = *s.split_events.lock().unwrap();
    let mut steps = Vec::new();
    for event in events {
        if split {
            let (first, second) = event.as_bytes().split_at(event.len() / 2);
            steps.push(Step::Send(first.to_vec()));
            steps.push(Step::Pause);
            steps.push(Step::Send(second.to_vec()));
        } else {
            steps.push(Step::Send(event.into_bytes()));
        }
        steps.push(Step::Pause);
    }
    if matches!(tail, Tail::Hang) {
        steps.push(Step::Pause);
    }
    let hang = matches!(tail, Tail::Hang);
    let body = Body::from_stream(stream::unfold((steps.into_iter(), hang), |(mut steps, hang)| async move {
        match steps.next() {
            Some(Step::Send(text)) => Some((Ok::<_, Infallible>(Bytes::from(text)), (steps, hang))),
            Some(Step::Pause) => {
                tokio::time::sleep(Duration::from_millis(5)).await;
                Some((Ok(Bytes::new()), (steps, hang)))
            }
            None if hang => {
                // Never ends, until the client lets go.
                std::future::pending::<()>().await;
                None
            }
            None => None,
        }
    }));
    Response::builder().header(header::CONTENT_TYPE, "text/event-stream").body(body).unwrap()
}

enum Tail {
    Done,
    Hang,
    Cut,
    Length,
    Error(String),
    Tools(Vec<MockCall>),
}

/// The arguments as the two fragments they are streamed in.
fn fragments(call: &MockCall) -> (String, String) {
    let whole = call.args.to_string();
    let mut cut = whole.len() / 2;
    while !whole.is_char_boundary(cut) {
        cut -= 1;
    }
    (whole[..cut].to_string(), whole[cut..].to_string())
}

fn data(value: Value) -> String {
    format!("data: {value}\n\n")
}

fn events(pieces: &[String], tail: &Tail, usage: bool, vendor: Vendor) -> Vec<String> {
    let reasoning = if vendor == Vendor::DeepSeek { DEEPSEEK_REASONING } else { "private reasoning" };
    let mut out = vec![
        data(json!({"choices": [{"index": 0, "delta": {"role": "assistant", "content": ""}}]})),
        data(json!({"choices": [{"index": 0, "delta": {"reasoning_content": reasoning}}]})),
    ];
    out.extend(pieces.iter().map(|p| data(json!({"choices": [{"index": 0, "delta": {"content": p}}]}))));
    match tail {
        Tail::Done => {
            out.push(data(json!({"choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]})));
            if usage {
                out.push(data(json!({"choices": [], "usage": {"prompt_tokens": USAGE_IN, "completion_tokens": USAGE_OUT, "total_tokens": USAGE_IN + USAGE_OUT}})));
            }
            out.push("data: [DONE]\n\n".into());
        }
        Tail::Length => {
            out.push(data(json!({"choices": [{"index": 0, "delta": {}, "finish_reason": "length"}]})));
            out.push("data: [DONE]\n\n".into());
        }
        Tail::Error(echoed) => out.push(data(json!({"error": {"message": format!("bad key {echoed}")}}))),
        Tail::Tools(calls) => {
            for (index, call) in calls.iter().enumerate() {
                let (first, second) = fragments(call);
                let mut start = json!({"index": index, "id": call.id, "type": "function",
                    "function": {"name": call.name, "arguments": first}});
                if vendor == Vendor::Gemini {
                    start["extra_content"] = json!({"google": {"thought_signature": signature_of(&call.id)}});
                }
                out.push(data(json!({"choices": [{"index": 0, "delta": {"tool_calls": [start]}}]})));
                out.push(data(json!({"choices": [{"index": 0, "delta": {"tool_calls": [{"index": index,
                    "function": {"arguments": second}}]}}]})));
            }
            out.push(data(json!({"choices": [{"index": 0, "delta": {}, "finish_reason": "tool_calls"}]})));
            if usage {
                out.push(data(
                    json!({"choices": [], "usage": {"prompt_tokens": USAGE_IN, "completion_tokens": USAGE_OUT}}),
                ));
            }
            out.push("data: [DONE]\n\n".into());
        }
        Tail::Hang | Tail::Cut => {}
    }
    out
}
