//! An in-process mock of an AI service speaking both protocols over plain http on 127.0.0.1:
//! OpenAI-style `POST /v1/chat/completions` and Anthropic-style `POST /v1/messages` (streaming,
//! or one JSON document for `Script::Whole`) and `GET /v1/models` in either style. It records what it received, so a test can assert exactly what left the device,
//! and it echoes whatever credentials it got into its error answers, so a test can check that
//! the key never comes back out through an error message.

use std::{
    collections::HashMap,
    convert::Infallible,
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{
    body::{Body, Bytes},
    extract::{Request, State},
    http::{header, HeaderMap, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::{get, post},
    Router,
};
use futures_util::stream;
use serde_json::{json, Value};
use tokio::task::JoinHandle;

/// What the mock does with the next requests.
#[derive(Clone, Debug)]
pub enum Script {
    /// A normal answer, one event per piece, ended properly.
    Answer(Vec<String>),
    /// An error status; the body echoes the credentials it was sent.
    Status(u16),
    /// A redirect to this address.
    Redirect(String),
    /// The pieces, then silence for ever: the connection stays open.
    Hang(Vec<String>),
    /// The pieces, then the connection closes without saying the answer is complete.
    Cut(Vec<String>),
    /// The pieces, then an error event that echoes the credentials.
    StreamError(Vec<String>),
    /// An OpenAI-style service that ignores `stream` and answers with this text as one JSON
    /// document.
    Whole(String),
    /// The pieces, then the service's own output limit: `finish_reason: length` /
    /// `stop_reason: max_tokens`.
    Length(Vec<String>),
}

/// What `GET /v1/models` answers.
#[derive(Clone, Debug)]
pub enum Models {
    /// OpenAI style: `{"data":[{"id":..}]}`, in this order.
    Ids(Vec<String>),
    /// Anthropic style, one inner list per page, paged by `after_id`.
    Pages(Vec<Vec<ModelSpec>>),
    /// An error status; the body echoes the credentials it was sent.
    Status(u16),
    /// A redirect to this address.
    Redirect(String),
    /// A 200 that is not JSON.
    Garbage,
    /// A JSON document longer than any list should be.
    Huge,
}

/// One model of the Anthropic-style list.
#[derive(Clone, Debug)]
pub struct ModelSpec {
    pub id: String,
    pub name: String,
    /// The effort levels it supports ("low".."max"); empty: `capabilities.effort.supported` is false.
    pub efforts: Vec<String>,
}

impl ModelSpec {
    pub fn new(id: &str, name: &str, efforts: &[&str]) -> ModelSpec {
        ModelSpec { id: id.into(), name: name.into(), efforts: efforts.iter().map(|e| e.to_string()).collect() }
    }
}

/// One request as it arrived.
#[derive(Clone, Debug)]
pub struct Seen {
    pub path: String,
    /// The query string, empty when there is none.
    pub query: String,
    pub authorization: Option<String>,
    pub api_key: Option<String>,
    pub version: Option<String>,
    pub body: Value,
}

impl Seen {
    /// The system prompt, in either protocol.
    pub fn system(&self) -> String {
        match self.body["system"].as_str() {
            Some(system) => system.to_string(),
            None => self.body["messages"][0]["content"].as_str().unwrap_or_default().to_string(),
        }
    }

    /// The user's message, in either protocol.
    pub fn user(&self) -> String {
        let last = self.body["messages"].as_array().and_then(|m| m.last()).cloned().unwrap_or_default();
        last["content"].as_str().unwrap_or_default().to_string()
    }
}

struct Shared {
    script: Mutex<Script>,
    models: Mutex<Models>,
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
            models: Mutex::new(Models::Ids(Vec::new())),
            split_events: Mutex::new(false),
            seen: Mutex::default(),
            hits: Mutex::default(),
        });
        let app = Router::new()
            .route("/v1/chat/completions", post(openai))
            .route("/v1/messages", post(anthropic))
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

async fn record(s: &Shared, path: &str, query: &str, headers: &HeaderMap, body: Bytes) {
    *s.hits.lock().unwrap().entry(path.to_string()).or_default() += 1;
    let get = |n: &str| headers.get(n).and_then(|v| v.to_str().ok()).map(str::to_string);
    s.seen.lock().unwrap().push(Seen {
        path: path.to_string(),
        query: query.to_string(),
        authorization: get("authorization"),
        api_key: get("x-api-key"),
        version: get("anthropic-version"),
        body: serde_json::from_slice(&body).unwrap_or(Value::Null),
    });
}

async fn openai(State(s): State<Arc<Shared>>, headers: HeaderMap, body: Bytes) -> Response {
    record(&s, "/v1/chat/completions", "", &headers, body).await;
    respond(&s, &headers, false)
}

async fn anthropic(State(s): State<Arc<Shared>>, headers: HeaderMap, body: Bytes) -> Response {
    record(&s, "/v1/messages", "", &headers, body).await;
    respond(&s, &headers, true)
}

async fn models(State(s): State<Arc<Shared>>, uri: Uri, headers: HeaderMap) -> Response {
    record(&s, "/v1/models", uri.query().unwrap_or_default(), &headers, Bytes::new()).await;
    let after = uri.query().and_then(|q| {
        url::form_urlencoded::parse(q.as_bytes()).find(|(k, _)| k == "after_id").map(|(_, v)| v.to_string())
    });
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
        Models::Pages(pages) => {
            let at = match &after {
                None => 0,
                Some(last) => {
                    pages.iter().position(|p| p.last().is_some_and(|m| &m.id == last)).map_or(pages.len(), |i| i + 1)
                }
            };
            let Some(page) = pages.get(at) else { return StatusCode::BAD_REQUEST.into_response() };
            let level = |on: bool| json!({"supported": on});
            let data: Vec<Value> = page
                .iter()
                .map(|m| {
                    let has = |l: &str| m.efforts.iter().any(|e| e == l);
                    json!({"type": "model", "id": m.id, "display_name": m.name, "capabilities": {"effort": {
                        "supported": !m.efforts.is_empty(), "low": level(has("low")), "medium": level(has("medium")),
                        "high": level(has("high")), "xhigh": level(has("xhigh")), "max": level(has("max"))}}})
                })
                .collect();
            let last = page.last().map(|m| m.id.clone());
            json(
                json!({"data": data, "has_more": at + 1 < pages.len(), "first_id": page.first().map(|m| m.id.clone()), "last_id": last}),
            )
        }
        Models::Status(code) => {
            let body = json!({"error": {"message": format!("Incorrect API key provided: {}", credentials(&headers))}});
            (StatusCode::from_u16(code).unwrap(), body.to_string()).into_response()
        }
        Models::Redirect(to) => Response::builder()
            .status(StatusCode::TEMPORARY_REDIRECT)
            .header(header::LOCATION, to)
            .body(Body::empty())
            .unwrap(),
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

fn respond(s: &Shared, headers: &HeaderMap, anthropic: bool) -> Response {
    let script = s.script.lock().unwrap().clone();
    let echoed = credentials(headers);
    let (pieces, tail) = match script {
        Script::Status(code) => {
            let body = json!({"error": {"message": format!("Incorrect API key provided: {echoed}")}});
            return (StatusCode::from_u16(code).unwrap(), body.to_string()).into_response();
        }
        Script::Redirect(to) => {
            return Response::builder()
                .status(StatusCode::TEMPORARY_REDIRECT)
                .header(header::LOCATION, to)
                .body(Body::empty())
                .unwrap();
        }
        Script::Whole(text) => {
            let body = if anthropic {
                json!({"type": "message", "role": "assistant", "content": [{"type": "thinking", "thinking": "private"}, {"type": "text", "text": text}], "stop_reason": "end_turn"})
            } else {
                json!({"choices": [{"index": 0, "message": {"role": "assistant", "content": text}, "finish_reason": "stop"}]})
            };
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
    };
    let events = if anthropic { anthropic_events(&pieces, &tail) } else { openai_events(&pieces, &tail) };
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
}

fn data(value: Value) -> String {
    format!("data: {value}\n\n")
}

fn openai_events(pieces: &[String], tail: &Tail) -> Vec<String> {
    let mut out = vec![
        data(json!({"choices": [{"index": 0, "delta": {"role": "assistant", "content": ""}}]})),
        data(json!({"choices": [{"index": 0, "delta": {"reasoning_content": "private reasoning"}}]})),
    ];
    out.extend(pieces.iter().map(|p| data(json!({"choices": [{"index": 0, "delta": {"content": p}}]}))));
    match tail {
        Tail::Done => {
            out.push(data(json!({"choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]})));
            out.push("data: [DONE]\n\n".into());
        }
        Tail::Length => {
            out.push(data(json!({"choices": [{"index": 0, "delta": {}, "finish_reason": "length"}]})));
            out.push("data: [DONE]\n\n".into());
        }
        Tail::Error(echoed) => out.push(data(json!({"error": {"message": format!("bad key {echoed}")}}))),
        Tail::Hang | Tail::Cut => {}
    }
    out
}

fn anthropic_events(pieces: &[String], tail: &Tail) -> Vec<String> {
    let event = |name: &str, value: Value| format!("event: {name}\n{}", data(value));
    let mut out = vec![
        event("message_start", json!({"type": "message_start", "message": {"id": "msg_1", "role": "assistant"}})),
        event(
            "content_block_start",
            json!({"type": "content_block_start", "index": 0, "content_block": {"type": "thinking", "thinking": ""}}),
        ),
        event(
            "content_block_delta",
            json!({"type": "content_block_delta", "index": 0, "delta": {"type": "thinking_delta", "thinking": "private reasoning"}}),
        ),
        event(
            "content_block_start",
            json!({"type": "content_block_start", "index": 1, "content_block": {"type": "text", "text": ""}}),
        ),
        event("ping", json!({"type": "ping"})),
    ];
    out.extend(pieces.iter().map(|p| {
        event(
            "content_block_delta",
            json!({"type": "content_block_delta", "index": 1, "delta": {"type": "text_delta", "text": p}}),
        )
    }));
    match tail {
        Tail::Done => {
            out.push(event("content_block_stop", json!({"type": "content_block_stop", "index": 1})));
            out.push(event("message_delta", json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"}})));
            out.push(event("message_stop", json!({"type": "message_stop"})));
        }
        Tail::Length => {
            out.push(event("message_delta", json!({"type": "message_delta", "delta": {"stop_reason": "max_tokens"}})));
            out.push(event("message_stop", json!({"type": "message_stop"})));
        }
        Tail::Error(echoed) => out.push(event(
            "error",
            json!({"type": "error", "error": {"type": "authentication_error", "message": format!("bad key {echoed}")}}),
        )),
        Tail::Hang | Tail::Cut => {}
    }
    out
}
