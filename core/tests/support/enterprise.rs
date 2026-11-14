//! What the mock knows of a licensed (Enterprise) server, and of an unlicensed one: `GET
//! /api/account` with the edition and the permissions of the key, the live metrics stream, and
//! the management methods `x:Metric`, `x:Trace`, `x:DataRetention`, `x:ArchivedItem`,
//! `x:Alert` and `x:Tenant`. It follows `dev/notes/stalwart-enterprise-observed.md` where
//! something was seen (rows, traces, the stream, the Enterprise refusal) and the documentation
//! where not (archived items, alerts).
//!
//! The state is kept apart from the rest of the mock, by the server's address, and is changed
//! through [`MockServer::enterprise`]. A server is unlicensed and takes no admin key until a
//! test says otherwise.

use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::Duration,
};

use axum::{
    body::Body,
    extract::{RawQuery, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use chrono::DateTime;
use serde_json::{json, Map, Value};

use super::{
    state::{now_secs, MethodError},
    MockServer, Shared,
};

/// The key the mock takes once a test has switched the admin side on.
pub const ADMIN_KEY: &str = "API_mock_admin_key_0123456789";

const ENTERPRISE_TEXT: &str = "This feature is only available in the Enterprise edition. Obtain your trial license at https://license.stalw.art/trial.";
const NOT_ALLOWED: &str = "You are not authorized to perform this action";

/// What the live stream does after its frames.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LiveEnd {
    /// Ends the stream.
    Close,
    /// Stays open and silent.
    Hold,
}

struct Live {
    frames: Vec<Value>,
    gap_ms: u64,
    end: LiveEnd,
    /// Waits this long before answering at all.
    stall_ms: u64,
    /// The query of each request, and whether it carried the right key.
    requests: Vec<(String, bool)>,
    open: Arc<AtomicUsize>,
}

struct State_ {
    licensed: bool,
    /// The key the server takes; `None`: it takes none (every key is refused).
    key: Option<String>,
    /// `None`: every permission.
    permissions: Option<Vec<String>>,
    next_id: u64,
    metrics: Vec<Value>,
    /// The most ids a query gives, whatever the limit asked.
    query_cap: usize,
    traces: Vec<Value>,
    retention: Value,
    archived: Vec<Value>,
    archived_sort: bool,
    restore_requests: Vec<String>,
    alerts: Vec<Value>,
    tenants: Vec<Value>,
    live: Live,
    /// How long `GET /api/account` takes to answer (a slow network).
    account_delay_ms: u64,
}

impl Default for State_ {
    fn default() -> Self {
        State_ {
            licensed: false,
            key: None,
            permissions: None,
            next_id: 0,
            metrics: Vec::new(),
            query_cap: 500,
            traces: Vec::new(),
            retention: Value::Null,
            archived: Vec::new(),
            archived_sort: true,
            restore_requests: Vec::new(),
            alerts: Vec::new(),
            tenants: Vec::new(),
            account_delay_ms: 0,
            live: Live {
                frames: Vec::new(),
                gap_ms: 0,
                end: LiveEnd::Close,
                stall_ms: 0,
                requests: Vec::new(),
                open: Arc::new(AtomicUsize::new(0)),
            },
        }
    }
}

type Registry = Mutex<HashMap<String, Arc<Mutex<State_>>>>;

/// The metrics the real server writes to its store (stalwart `telemetry/metrics/store.rs`), those
/// a test here may use. "delivery.delivered" is a trace event only, with no stored counter.
const PERSISTED_COUNTERS: [&str; 7] = [
    "queue.message-queued",
    "queue.authenticated-message-queued",
    "queue.dsn-queued",
    "queue.report-queued",
    "delivery.attempt-start",
    "message-ingest.ham",
    "message-ingest.spam",
];
const PERSISTED_HISTOGRAMS: [&str; 2] = ["delivery.total-time", "delivery.attempt-time"];

fn registry() -> &'static Registry {
    static REGISTRY: OnceLock<Registry> = OnceLock::new();
    REGISTRY.get_or_init(Default::default)
}

fn state_of(base: &str) -> Arc<Mutex<State_>> {
    registry().lock().unwrap().entry(base.to_string()).or_default().clone()
}

/// The handle a test changes the licensed side through.
pub struct Enterprise(Arc<Mutex<State_>>);

impl MockServer {
    pub fn enterprise(&self) -> Enterprise {
        Enterprise(state_of(&self.shared.base))
    }
}

fn secs(text: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(text).ok().map(|t| t.timestamp())
}

pub fn iso(secs: i64) -> String {
    DateTime::from_timestamp(secs, 0).unwrap().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

impl Enterprise {
    fn with<T>(&self, f: impl FnOnce(&mut State_) -> T) -> T {
        f(&mut self.0.lock().unwrap())
    }

    /// Switches the licence on or off.
    pub fn licensed(&self, on: bool) {
        self.with(|s| s.licensed = on);
    }

    /// The server takes this key (and only this one).
    pub fn accept_key(&self, key: &str) {
        self.with(|s| s.key = Some(key.to_string()));
    }

    /// The key holds only these permissions.
    pub fn permissions(&self, names: &[&str]) {
        self.with(|s| s.permissions = Some(names.iter().map(|n| n.to_string()).collect()));
    }

    /// A licensed server with the admin key accepted and every permission held.
    pub fn switch_on(&self) {
        self.licensed(true);
        self.accept_key(ADMIN_KEY);
    }

    pub fn query_cap(&self, cap: usize) {
        self.with(|s| s.query_cap = cap);
    }

    fn new_id(s: &mut State_) -> String {
        s.next_id += 1;
        format!("i{}", s.next_id)
    }

    /// A counter row, `at` in Unix seconds. The real server stores only a fixed list of metrics
    /// (`PERSISTED_COUNTERS`); a name outside it never has a row, so a test cannot make one.
    pub fn counter(&self, metric: &str, at: i64, count: u64) {
        assert!(PERSISTED_COUNTERS.contains(&metric), "the server does not store the counter {metric}");
        self.with(|s| {
            let id = Self::new_id(s);
            s.metrics.push(json!({"@type": "Counter", "metric": metric, "count": count, "nodeId": 1, "timestamp": iso(at), "id": id}));
        });
    }

    pub fn histogram(&self, metric: &str, at: i64, count: u64, sum: u64) {
        assert!(PERSISTED_HISTOGRAMS.contains(&metric), "the server does not store the histogram {metric}");
        self.with(|s| {
            let id = Self::new_id(s);
            s.metrics.push(json!({"@type": "Histogram", "metric": metric, "count": count, "sum": sum, "nodeId": 1, "timestamp": iso(at), "id": id}));
        });
    }

    /// A trace as the server stores it. `events` are `(name, key values)`; a value is a String.
    pub fn trace(&self, id: &str, at: i64, from: &str, to: &str, size: u64, events: &[(&str, &[(&str, &str)])]) {
        let mut map = Map::new();
        for (i, (name, kvs)) in events.iter().enumerate() {
            let mut values = Map::new();
            for (j, (k, v)) in kvs.iter().enumerate() {
                values.insert(j.to_string(), json!({"key": k, "value": {"@type": "String", "value": v}}));
            }
            map.insert(i.to_string(), json!({"event": name, "timestamp": iso(at + i as i64), "keyValues": values}));
        }
        self.trace_json(json!({"id": id, "timestamp": iso(at), "from": from, "to": to, "size": size, "events": map}));
    }

    pub fn trace_json(&self, trace: Value) {
        self.with(|s| s.traces.push(trace));
    }

    /// `archiveDeletedItemsFor` in milliseconds; `None` is null.
    pub fn retention(&self, ms: Option<u64>) {
        self.with(|s| s.retention = ms.map_or(Value::Null, |m| json!(m)));
    }

    /// An archived item of the account `account`; `kind` is the variant ("Email", "FileNode",
    /// "CalendarEvent", "ContactCard", "SieveScript"), `title` goes into its title property.
    pub fn archived(&self, id: &str, account: &str, kind: &str, title: &str, archived_at: i64) {
        let mut item = json!({
            "id": id, "@type": kind, "accountId": account, "archivedAt": iso(archived_at),
            "archivedUntil": iso(archived_at + 86_400 * 30), "blobId": "b1", "status": "archived",
        });
        let property = match kind {
            "Email" => "subject",
            "CalendarEvent" => "title",
            _ => "name",
        };
        item[property] = json!(title);
        self.with(|s| s.archived.push(item));
    }

    /// Whether the server sorts archived items by `archivedAt` (it answers `unsupportedSort`
    /// when not).
    pub fn archived_sort(&self, on: bool) {
        self.with(|s| s.archived_sort = on);
    }

    /// The ids the server was asked to restore, in order.
    pub fn restore_requests(&self) -> Vec<String> {
        self.with(|s| s.restore_requests.clone())
    }

    pub fn alert(&self, alert: Value) {
        self.with(|s| s.alerts.push(alert));
    }

    pub fn alert_enabled(&self, id: &str) -> Option<bool> {
        self.with(|s| s.alerts.iter().find(|a| a["id"] == id).and_then(|a| a["enable"].as_bool()))
    }

    pub fn tenant(&self, tenant: Value) {
        self.with(|s| s.tenants.push(tenant));
    }

    /// The frames of the live stream: each is the list of rows of one `event: metrics`.
    pub fn live_frames(&self, frames: Vec<Value>, gap_ms: u64, end: LiveEnd) {
        self.with(|s| {
            s.live.frames = frames;
            s.live.gap_ms = gap_ms;
            s.live.end = end;
        });
    }

    /// The live stream's answer comes this late.
    /// `GET /api/account` answers only after `ms` milliseconds.
    pub fn account_delay_ms(&self, ms: u64) {
        self.with(|s| s.account_delay_ms = ms);
    }

    pub fn live_stall_ms(&self, ms: u64) {
        self.with(|s| s.live.stall_ms = ms);
    }

    /// The query string of each live request and whether it carried the accepted key.
    pub fn live_requests(&self) -> Vec<(String, bool)> {
        self.with(|s| s.live.requests.clone())
    }

    /// How many live streams are open now.
    pub fn live_open(&self) -> usize {
        self.with(|s| s.live.open.load(Ordering::SeqCst))
    }
}

// ---- the HTTP side ----

fn bearer(headers: &HeaderMap) -> &str {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("")
}

/// Whether `token` is the admin key the server takes. Used by the mock's bearer check.
pub fn key_accepted(s: &Shared, token: &str) -> bool {
    let state = state_of(&s.base);
    let state = state.lock().unwrap();
    state.key.as_deref() == Some(token) && !token.is_empty()
}

fn unauthorized() -> Response {
    (StatusCode::UNAUTHORIZED, [(header::CONTENT_TYPE, "application/json")], r#"{"status":401,"title":"Unauthorized"}"#)
        .into_response()
}

/// `GET /api/account`.
pub async fn account(State(s): State<Arc<Shared>>, headers: HeaderMap) -> Response {
    let delay = state_of(&s.base).lock().unwrap().account_delay_ms;
    if delay > 0 {
        tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
    }
    let state = state_of(&s.base);
    let state = state.lock().unwrap();
    if state.key.as_deref() != Some(bearer(&headers)) {
        return unauthorized();
    }
    let all = [
        "authenticate",
        "liveMetrics",
        "sysMetricQuery",
        "sysMetricGet",
        "sysTraceQuery",
        "sysTraceGet",
        "sysDataRetentionGet",
        "sysArchivedItemQuery",
        "sysArchivedItemGet",
        "sysArchivedItemUpdate",
        "sysAlertQuery",
        "sysAlertGet",
        "sysAlertUpdate",
        "sysTenantQuery",
        "sysTenantGet",
        "impersonate",
    ];
    let permissions: Vec<String> =
        state.permissions.clone().unwrap_or_else(|| all.iter().map(|p| p.to_string()).collect());
    let edition = if state.licensed { "enterprise" } else { "community" };
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json")],
        json!({"edition": edition, "locale": "en", "permissions": permissions}).to_string(),
    )
        .into_response()
}

struct OpenGuard(Arc<AtomicUsize>);

impl Drop for OpenGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

struct Stream {
    frames: std::vec::IntoIter<Value>,
    gap: Duration,
    end: LiveEnd,
    first: bool,
    _guard: OpenGuard,
}

/// `GET /api/live/metrics`.
pub async fn live(State(s): State<Arc<Shared>>, headers: HeaderMap, RawQuery(query): RawQuery) -> Response {
    let state = state_of(&s.base);
    let (stream, stall) = {
        let mut state = state.lock().unwrap();
        let key_ok = state.key.as_deref() == Some(bearer(&headers));
        state.live.requests.push((query.unwrap_or_default(), key_ok));
        if !key_ok {
            return unauthorized();
        }
        if !state.licensed {
            return (StatusCode::NOT_FOUND, "Enterprise feature").into_response();
        }
        state.live.open.fetch_add(1, Ordering::SeqCst);
        let stream = Stream {
            frames: state.live.frames.clone().into_iter(),
            gap: Duration::from_millis(state.live.gap_ms),
            end: state.live.end,
            first: true,
            _guard: OpenGuard(state.live.open.clone()),
        };
        (stream, state.live.stall_ms)
    };
    if stall > 0 {
        tokio::time::sleep(Duration::from_millis(stall)).await;
    }
    let body = Body::from_stream(futures_util::stream::unfold(stream, |mut st| async move {
        match st.frames.next() {
            Some(frame) => {
                if !std::mem::take(&mut st.first) {
                    tokio::time::sleep(st.gap).await;
                }
                let text = format!("event: metrics\ndata: {frame}\n\n");
                Some((Ok::<_, std::convert::Infallible>(text.into_bytes()), st))
            }
            None => match st.end {
                LiveEnd::Close => None,
                LiveEnd::Hold => futures_util::future::pending().await,
            },
        }
    }));
    Response::builder().header(header::CONTENT_TYPE, "text/event-stream").body(body).unwrap()
}

// ---- the management methods ----

fn merr(kind: &'static str, description: impl Into<String>) -> MethodError {
    MethodError { kind, description: description.into() }
}

fn page<'a>(args: &Value, rows: &'a [Value], cap: usize) -> (Vec<&'a Value>, usize) {
    let total = rows.len();
    let position = args["position"].as_u64().unwrap_or(0) as usize;
    let limit = args["limit"].as_u64().map_or(cap, |l| (l as usize).min(cap));
    (rows.iter().skip(position).take(limit).collect(), total)
}

fn query_answer(args: &Value, ids: Vec<&Value>, total: usize) -> Value {
    let mut answer = json!({
        "accountId": args["accountId"], "queryState": "n", "canCalculateChanges": false,
        "position": args["position"].as_u64().unwrap_or(0),
        "ids": ids.iter().map(|r| r["id"].clone()).collect::<Vec<_>>(),
    });
    if args["calculateTotal"].as_bool() == Some(true) {
        answer["total"] = json!(total);
    }
    answer
}

fn get_answer(args: &Value, rows: &[Value]) -> Result<Value, MethodError> {
    let wanted: Vec<&str> = match args["ids"].as_array() {
        Some(ids) => {
            if ids.len() > 500 {
                return Err(merr("requestTooLarge", "too many ids"));
            }
            ids.iter().filter_map(Value::as_str).collect()
        }
        None => rows.iter().filter_map(|r| r["id"].as_str()).collect(),
    };
    let list: Vec<&Value> = wanted.iter().filter_map(|id| rows.iter().find(|r| r["id"] == *id)).collect();
    let missing: Vec<&&str> = wanted.iter().filter(|id| !rows.iter().any(|r| r["id"] == **id)).collect();
    Ok(json!({"accountId": args["accountId"], "state": "n", "list": list, "notFound": missing}))
}

/// Sorts by a property. The sort is an array of `{property, isAscending}`; a property not in
/// `known` is `unsupportedSort`.
fn sort_by(rows: &mut [&Value], args: &Value, known: &[&str]) -> Result<(), MethodError> {
    let Some(sort) = args["sort"].as_array().and_then(|s| s.first()) else { return Ok(()) };
    let property = sort["property"].as_str().unwrap_or_default();
    if !known.contains(&property) {
        return Err(merr("unsupportedSort", format!("Property {property} is not supported for sorting")));
    }
    let ascending = sort["isAscending"].as_bool().unwrap_or(true);
    rows.sort_by(|a, b| {
        let order = a[property].as_str().cmp(&b[property].as_str());
        if ascending {
            order
        } else {
            order.reverse()
        }
    });
    Ok(())
}

/// The keywords the real server indexes for a trace: the whole values of `from`, `to` (each
/// recipient), `domain`, `hostname` and `remoteIp`, never the queue id and never part of an address.
fn keywords(trace: &Value) -> Vec<String> {
    let mut words = vec![trace["from"].as_str().unwrap_or_default().to_string()];
    for to in trace["to"].as_str().unwrap_or_default().split([';', ',']) {
        words.push(to.trim().to_string());
    }
    for event in trace["events"].as_object().into_iter().flat_map(|m| m.values()) {
        for kv in event["keyValues"].as_object().into_iter().flat_map(|m| m.values()) {
            if matches!(kv["key"].as_str(), Some("domain" | "hostname" | "remoteIp")) {
                if let Some(v) = kv["value"]["value"].as_str() {
                    words.push(v.to_string());
                }
            }
        }
    }
    words.into_iter().map(|w| w.to_lowercase()).filter(|w| !w.is_empty()).collect()
}

/// `text` is split at white space and every word must be a whole keyword.
fn includes_text(trace: &Value, text: &str) -> bool {
    let words = keywords(trace);
    text.split_whitespace().all(|t| words.contains(&t.to_lowercase()))
}

/// The `queueId` the events carry.
fn has_queue_id(trace: &Value, id: &str) -> bool {
    trace["events"].as_object().into_iter().flat_map(|m| m.values()).any(|event| {
        event["keyValues"]
            .as_object()
            .into_iter()
            .flat_map(|m| m.values())
            .any(|kv| kv["key"] == "queueId" && kv["value"]["value"].as_str() == Some(id))
    })
}

/// Whether an `x:` method belongs to the Enterprise side (the rest is the user's own account).
pub fn owns(name: &str) -> bool {
    let object = name.trim_start_matches("x:").split('/').next().unwrap_or_default();
    matches!(object, "Metric" | "Trace" | "DataRetention" | "ArchivedItem" | "Alert" | "Tenant")
}

/// The management methods of the Enterprise side.
pub fn call(s: &Shared, name: &str, args: &Value) -> Result<Value, MethodError> {
    let state = state_of(&s.base);
    let mut state = state.lock().unwrap();
    let (object, method) = name.trim_start_matches("x:").split_once('/').ok_or_else(|| merr("unknownMethod", name))?;
    let enterprise_object = matches!(object, "Metric" | "Trace" | "ArchivedItem");
    if enterprise_object && !state.licensed {
        return Err(merr("forbidden", ENTERPRISE_TEXT));
    }
    let permission = format!("sys{object}{}", method[..1].to_uppercase() + &method[1..]);
    if state.permissions.as_ref().is_some_and(|p| !p.contains(&permission)) {
        return Err(merr("forbidden", NOT_ALLOWED));
    }
    let cap = state.query_cap;
    let filter = &args["filter"];
    match (object, method) {
        ("Metric", "query") => {
            let from = filter["timestampIsGreaterThanOrEqual"].as_str().and_then(secs);
            let to = filter["timestampIsLessThanOrEqual"].as_str().and_then(secs);
            let names: Option<Vec<&str>> =
                filter["metric"].as_array().map(|m| m.iter().filter_map(Value::as_str).collect());
            let rows: Vec<Value> = state
                .metrics
                .iter()
                .filter(|r| {
                    let at = r["timestamp"].as_str().and_then(secs).unwrap_or(0);
                    from.is_none_or(|f| at >= f)
                        && to.is_none_or(|t| at <= t)
                        && names.as_ref().is_none_or(|n| r["metric"].as_str().is_some_and(|m| n.contains(&m)))
                })
                .cloned()
                .collect();
            let (ids, total) = page(args, &rows, cap);
            Ok(query_answer(args, ids, total))
        }
        ("Metric", "get") => get_answer(args, &state.metrics),
        ("Trace", "query") => {
            let text = filter["text"].as_str();
            let event = filter["event"].as_str();
            let queue_id = filter["queueId"].as_str();
            let mut from = filter["timestampIsGreaterThanOrEqual"].as_str().and_then(secs);
            // The real server: without a text, a queue id or a time the query covers one day only.
            if text.is_none() && queue_id.is_none() && from.is_none() {
                from = Some(now_secs() - 86_400);
            }
            let mut rows: Vec<&Value> = state
                .traces
                .iter()
                .filter(|t| {
                    event.is_none_or(|e| t["events"].as_object().is_some_and(|m| m.values().any(|v| v["event"] == e)))
                        && text.is_none_or(|x| includes_text(t, x))
                        && queue_id.is_none_or(|q| has_queue_id(t, q))
                        && from.is_none_or(|f| t["timestamp"].as_str().and_then(secs).unwrap_or(0) >= f)
                })
                .collect();
            sort_by(&mut rows, args, &["timestamp", "id"])?;
            let total = rows.len();
            let position = args["position"].as_u64().unwrap_or(0) as usize;
            let limit = args["limit"].as_u64().map_or(cap, |l| (l as usize).min(cap));
            Ok(query_answer(args, rows.into_iter().skip(position).take(limit).collect(), total))
        }
        ("Trace", "get") => get_answer(args, &state.traces),
        ("DataRetention", "get") => {
            let ids = args["ids"].as_array().map(|i| i.iter().filter_map(Value::as_str).collect::<Vec<_>>());
            if ids.as_deref() != Some(&["singleton"]) {
                return Err(merr("invalidArguments", "only the singleton"));
            }
            let list = json!([{"id": "singleton", "archiveDeletedItemsFor": state.retention, "archiveDeletedAccountsFor": null}]);
            Ok(json!({"accountId": args["accountId"], "state": "n", "list": list, "notFound": []}))
        }
        ("ArchivedItem", "query") => {
            let account = filter["accountId"].as_str();
            let mut rows: Vec<&Value> =
                state.archived.iter().filter(|i| account.is_none_or(|a| i["accountId"] == a)).collect();
            if !state.archived_sort && args["sort"].is_array() {
                return Err(merr("unsupportedSort", "Property archivedAt is not supported for sorting"));
            }
            sort_by(&mut rows, args, &["archivedAt"])?;
            let total = rows.len();
            let position = args["position"].as_u64().unwrap_or(0) as usize;
            let limit = args["limit"].as_u64().map_or(cap, |l| (l as usize).min(cap));
            Ok(query_answer(args, rows.into_iter().skip(position).take(limit).collect(), total))
        }
        ("ArchivedItem", "get") => get_answer(args, &state.archived),
        ("ArchivedItem", "set") => {
            let mut updated = Map::new();
            let mut not_updated = Map::new();
            for (id, patch) in args["update"].as_object().into_iter().flatten() {
                let found = state.archived.iter().any(|i| i["id"] == id.as_str());
                if !found {
                    not_updated.insert(id.clone(), json!({"type": "notFound"}));
                } else if patch["status"] == "requestRestore" {
                    state.restore_requests.push(id.clone());
                    updated.insert(id.clone(), Value::Null);
                } else {
                    not_updated.insert(id.clone(), json!({"type": "invalidPatch"}));
                }
            }
            Ok(
                json!({"accountId": args["accountId"], "oldState": "n", "newState": "n", "updated": updated, "notUpdated": not_updated}),
            )
        }
        ("Alert", "get") => get_answer(args, &state.alerts),
        ("Alert", "set") => {
            let mut updated = Map::new();
            let mut not_updated = Map::new();
            for (id, patch) in args["update"].as_object().into_iter().flatten() {
                match state.alerts.iter_mut().find(|a| a["id"] == id.as_str()) {
                    Some(alert) => {
                        if let Some(enable) = patch["enable"].as_bool() {
                            alert["enable"] = json!(enable);
                        }
                        updated.insert(id.clone(), Value::Null);
                    }
                    None => {
                        not_updated.insert(id.clone(), json!({"type": "notFound"}));
                    }
                }
            }
            Ok(
                json!({"accountId": args["accountId"], "oldState": "n", "newState": "n", "updated": updated, "notUpdated": not_updated}),
            )
        }
        ("Tenant", "get") => get_answer(args, &state.tenants),
        _ => Err(merr("unknownMethod", format!("x:{object}/{method}"))),
    }
}
