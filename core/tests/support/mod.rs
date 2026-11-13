#![allow(dead_code)]
//! An in-process mock of a Stalwart JMAP server for the integration tests: OAuth (PKCE,
//! dynamic registration, rotating refresh tokens), the JMAP session, the mail methods the core
//! uses, and an EventSource push stream. It speaks plain http on 127.0.0.1.
//!
//! The control methods on `MockServer` stand in for "another device" (they change mail and
//! cause push events) or for server trouble (expired tokens, a dead stream, an outage).

mod calendar;
mod oauth;
mod state;

use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::extract::{Request, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use serde_json::{json, Value};
use tokio::sync::broadcast;
use tokio::task::JoinHandle;

use calendar::Calendar;
use oauth::Oauth;
use state::{resolve_refs, Mail, StreamMsg};

pub use state::{ACCOUNT_ID, USERNAME};

/// Ids of the seeded mailboxes.
pub const INBOX_ID: &str = "mb1";
pub const ARCHIVE_ID: &str = "mb2";
pub const TRASH_ID: &str = "mb3";
pub const DRAFTS_ID: &str = "mb4";
pub const SENT_ID: &str = "mb5";
pub const JUNK_ID: &str = "mb6";

struct Shared {
    base: String,
    mail: Mutex<Mail>,
    calendar: Mutex<Calendar>,
    /// How long `CalendarEvent/get` takes to answer, in milliseconds.
    calendar_delay_ms: AtomicU64,
    oauth: Mutex<Oauth>,
    down: AtomicBool,
    /// Requests by path ("/jmap") and, for JMAP calls, by method name ("Email/get").
    counts: Mutex<HashMap<String, usize>>,
    open_streams: AtomicUsize,
    tx: broadcast::Sender<StreamMsg>,
}

impl Shared {
    fn count(&self, key: &str) {
        *self.counts.lock().unwrap().entry(key.to_string()).or_default() += 1;
    }

    fn mail(&self) -> MutexGuard<'_, Mail> {
        self.mail.lock().unwrap()
    }
}

/// The handle. Dropping it stops the server and closes its streams.
pub struct MockServer {
    shared: Arc<Shared>,
    task: JoinHandle<()>,
}

impl Drop for MockServer {
    fn drop(&mut self) {
        let _ = self.shared.tx.send(StreamMsg::Drop);
        self.task.abort();
    }
}

impl MockServer {
    pub async fn start() -> MockServer {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind the mock server");
        let base = format!("http://{}", listener.local_addr().unwrap());
        let (tx, _) = broadcast::channel(256);
        let shared = Arc::new(Shared {
            base,
            mail: Mutex::new(Mail::new(tx.clone())),
            calendar: Mutex::new(Calendar::default()),
            calendar_delay_ms: AtomicU64::new(0),
            oauth: Mutex::new(Oauth::new()),
            down: AtomicBool::new(false),
            counts: Mutex::new(HashMap::new()),
            open_streams: AtomicUsize::new(0),
            tx,
        });
        let app = Router::new()
            .route("/.well-known/oauth-authorization-server", get(oauth_metadata))
            .route("/register", post(register))
            .route("/authorize", get(authorize_http))
            .route("/token", post(token))
            .route("/revoke", post(revoke))
            .route("/.well-known/jmap", get(session))
            .route("/jmap", post(jmap))
            .route("/eventsource", get(eventsource))
            .layer(middleware::from_fn_with_state(shared.clone(), gate))
            .with_state(shared.clone());
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        MockServer { shared, task }
    }

    /// `http://127.0.0.1:<port>`, no trailing slash.
    pub fn url(&self) -> String {
        self.shared.base.clone()
    }

    pub fn account_id(&self) -> &'static str {
        ACCOUNT_ID
    }

    pub fn username(&self) -> &'static str {
        USERNAME
    }

    // ---- OAuth helpers ---------------------------------------------------------------

    /// What the browser step does, without HTTP: takes the URL the app would open and returns
    /// the callback URL the browser would be sent to (code and state attached).
    pub fn authorize(&self, authorize_url: &str) -> Result<String, String> {
        let url = url::Url::parse(authorize_url).map_err(|e| e.to_string())?;
        let params: HashMap<String, String> = url.query_pairs().into_owned().collect();
        self.shared.oauth.lock().unwrap().authorize(&params)
    }

    /// An access token without an OAuth round trip, for tests about something else.
    pub fn issue_access_token(&self) -> String {
        self.shared.oauth.lock().unwrap().mint_access_token()
    }

    // ---- "another device": these change mail and push an event ----------------------

    /// New unread mail in the Inbox, received now. Returns the message id.
    pub fn deliver(&self, from_name: &str, from_email: &str, subject: &str, preview: &str) -> String {
        self.deliver_at(from_name, from_email, subject, preview, state::now_secs())
    }

    /// Like `deliver`, with `received_at` in Unix seconds.
    pub fn deliver_at(
        &self,
        from_name: &str,
        from_email: &str,
        subject: &str,
        preview: &str,
        received_at: i64,
    ) -> String {
        self.shared.mail().deliver(from_name, from_email, subject, preview, received_at)
    }

    pub fn mark_read(&self, id: &str) {
        self.edit(id, |e| {
            e.keywords.insert("$seen".into());
        });
    }

    pub fn mark_unread(&self, id: &str) {
        self.edit(id, |e| {
            e.keywords.remove("$seen");
        });
    }

    /// Moves the message so that it is only in the mailbox with this role
    /// ("inbox", "archive", "trash", "drafts", "sent", "junk").
    pub fn move_to(&self, id: &str, role: &str) {
        let mut mail = self.shared.mail();
        let target = mail.mailbox_id_by_role(role).unwrap_or_else(|| panic!("no mailbox with role '{role}'"));
        assert!(
            mail.update_email(id, |e| {
                e.mailbox_ids.clear();
                e.mailbox_ids.insert(target);
            }),
            "no message '{id}'"
        );
    }

    pub fn destroy(&self, id: &str) {
        assert!(self.shared.mail().destroy(id), "no message '{id}'");
    }

    fn edit(&self, id: &str, f: impl FnOnce(&mut state::Email)) {
        assert!(self.shared.mail().update_email(id, f), "no message '{id}'");
    }

    // ---- calendar ----------------------------------------------------------------------

    /// Stores a JSCalendar Event or Task (`@type` defaults to "Event") and returns its id.
    /// Nothing is pushed: the server only pushes when an alarm fires, see `fire_alert`.
    pub fn add_event(&self, event: Value) -> String {
        self.shared.calendar.lock().unwrap().add(event)
    }

    /// The event is deleted: `CalendarEvent/get` reports it in `notFound`.
    pub fn remove_event(&self, id: &str) {
        assert!(self.shared.calendar.lock().unwrap().remove(id), "no event '{id}'");
    }

    /// The alarm fires: a `CalendarAlert` goes out on every open stream that asked for that
    /// type. The event need not exist (it may have been deleted since).
    pub fn fire_alert(&self, event_id: &str, alert_id: &str, recurrence_id: Option<&str>) {
        let uid = self.shared.calendar.lock().unwrap().uid_of(event_id).unwrap_or_else(|| format!("uid-{event_id}"));
        let data = json!({
            "accountId": ACCOUNT_ID,
            "calendarEventId": event_id,
            "uid": uid,
            "recurrenceId": recurrence_id,
            "alertId": alert_id,
        });
        let _ = self.shared.tx.send(StreamMsg::CalendarAlert { data });
    }

    /// Makes `CalendarEvent/get` slow, to show that nothing else waits for it.
    pub fn set_calendar_delay(&self, delay: Duration) {
        self.shared.calendar_delay_ms.store(delay.as_millis() as u64, Ordering::SeqCst);
    }

    // ---- trouble ---------------------------------------------------------------------

    /// Lifetime of access tokens issued from now on, in seconds (default 3600).
    pub fn set_access_token_lifetime(&self, secs: u64) {
        self.shared.oauth.lock().unwrap().lifetime_secs = secs;
    }

    /// Every access token issued so far stops working now (the JMAP endpoints answer 401).
    pub fn expire_access_tokens(&self) {
        self.shared.oauth.lock().unwrap().expire_access_tokens();
    }

    /// The user revoked the app: every refresh token fails with `invalid_grant`. A new
    /// sign-in still works.
    pub fn revoke_grant(&self) {
        self.shared.oauth.lock().unwrap().refresh.clear();
    }

    /// Closes every open EventSource connection from the server side.
    pub fn drop_event_streams(&self) {
        let _ = self.shared.tx.send(StreamMsg::Drop);
    }

    /// While down, every request gets 503 and open streams are closed.
    pub fn set_down(&self, down: bool) {
        self.shared.down.store(down, Ordering::SeqCst);
        if down {
            self.drop_event_streams();
        }
    }

    /// States up to now become unknown to `/changes` (`cannotCalculateChanges`). The current
    /// state still works.
    pub fn forget_changes_before_now(&self) {
        self.shared.mail().forget_changes();
    }

    /// With false, mutations stop producing push events: a stream that is open but dead.
    pub fn set_push_enabled(&self, enabled: bool) {
        self.shared.mail().push_enabled = enabled;
    }

    // ---- inspection ------------------------------------------------------------------

    /// Requests received (also those answered 503), by path such as "/jmap" or "/token", or
    /// by JMAP method name such as "Email/changes" (counted when the call was executed).
    pub fn request_count(&self, path_or_method: &str) -> usize {
        self.shared.counts.lock().unwrap().get(path_or_method).copied().unwrap_or(0)
    }

    pub fn reset_request_counts(&self) {
        self.shared.counts.lock().unwrap().clear();
    }

    /// Requests to /token, successful or not.
    pub fn token_requests(&self) -> usize {
        self.request_count("/token")
    }

    pub fn open_event_streams(&self) -> usize {
        self.shared.open_streams.load(Ordering::SeqCst)
    }

    /// Waits until exactly `n` streams are open (panics after 5 s).
    pub async fn wait_for_event_streams(&self, n: usize) {
        for _ in 0..500 {
            if self.open_event_streams() == n {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("expected {n} open event streams, have {}", self.open_event_streams());
    }

    pub fn exists(&self, id: &str) -> bool {
        self.shared.mail().emails.contains_key(id)
    }

    pub fn is_read(&self, id: &str) -> bool {
        self.shared.mail().emails.get(id).unwrap_or_else(|| panic!("no message '{id}'")).keywords.contains("$seen")
    }

    pub fn keywords_of(&self, id: &str) -> Vec<String> {
        self.shared
            .mail()
            .emails
            .get(id)
            .unwrap_or_else(|| panic!("no message '{id}'"))
            .keywords
            .iter()
            .cloned()
            .collect()
    }

    /// Mailbox ids the message is in, sorted.
    pub fn mailboxes_of(&self, id: &str) -> Vec<String> {
        self.shared
            .mail()
            .emails
            .get(id)
            .unwrap_or_else(|| panic!("no message '{id}'"))
            .mailbox_ids
            .iter()
            .cloned()
            .collect()
    }

    /// Roles of the mailboxes the message is in, sorted.
    pub fn roles_of(&self, id: &str) -> Vec<String> {
        let mail = self.shared.mail();
        let e = mail.emails.get(id).unwrap_or_else(|| panic!("no message '{id}'"));
        let mut roles: Vec<String> = mail
            .mailboxes
            .iter()
            .filter(|m| e.mailbox_ids.contains(&m.id))
            .filter_map(|m| m.role.map(str::to_string))
            .collect();
        roles.sort();
        roles
    }

    /// Ids of all messages, oldest arrival first.
    pub fn all_message_ids(&self) -> Vec<String> {
        let mail = self.shared.mail();
        let mut all: Vec<_> = mail.emails.values().collect();
        all.sort_by_key(|e| e.seq);
        all.into_iter().map(|e| e.id.clone()).collect()
    }

    pub fn email_state(&self) -> String {
        self.shared.mail().email_state()
    }

    pub fn mailbox_state(&self) -> String {
        self.shared.mail().mailbox_state()
    }
}

// ---- HTTP -----------------------------------------------------------------------------

/// Counts every request and answers 503 while the server is "down".
async fn gate(State(s): State<Arc<Shared>>, req: Request, next: Next) -> Response {
    s.count(req.uri().path());
    if s.down.load(Ordering::SeqCst) {
        return (StatusCode::SERVICE_UNAVAILABLE, "the mock server is down").into_response();
    }
    next.run(req).await
}

fn json_response(status: StatusCode, v: Value) -> Response {
    (status, [(header::CONTENT_TYPE, "application/json"), (header::CACHE_CONTROL, "no-store")], v.to_string())
        .into_response()
}

fn oauth_error(status: StatusCode, error: &str, description: &str) -> Response {
    json_response(status, json!({ "error": error, "error_description": description }))
}

/// JMAP request-level errors use problem+json with exactly this content type; jmap-client
/// compares it byte for byte.
fn problem(status: StatusCode, kind: &str, detail: &str) -> Response {
    (
        status,
        [(header::CONTENT_TYPE, "application/problem+json")],
        json!({ "type": format!("urn:ietf:params:jmap:error:{kind}"), "status": status.as_u16(), "detail": detail })
            .to_string(),
    )
        .into_response()
}

fn form(bytes: &[u8]) -> HashMap<String, String> {
    url::form_urlencoded::parse(bytes).into_owned().collect()
}

fn unauthorized() -> Response {
    (StatusCode::UNAUTHORIZED, [(header::WWW_AUTHENTICATE, "Bearer")], "invalid or expired access token")
        .into_response()
}

#[allow(clippy::result_large_err)] // a test helper: the response is returned as it is
fn check_bearer(s: &Shared, headers: &HeaderMap) -> Result<(), Response> {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    if s.oauth.lock().unwrap().access_valid(token) {
        Ok(())
    } else {
        Err(unauthorized())
    }
}

async fn oauth_metadata(State(s): State<Arc<Shared>>) -> Response {
    let b = &s.base;
    json_response(
        StatusCode::OK,
        json!({
            "issuer": b,
            "authorization_endpoint": format!("{b}/authorize"),
            "token_endpoint": format!("{b}/token"),
            "registration_endpoint": format!("{b}/register"),
            "revocation_endpoint": format!("{b}/revoke"),
            "response_types_supported": ["code"],
            "grant_types_supported": ["authorization_code", "refresh_token"],
            "code_challenge_methods_supported": ["S256"],
            "token_endpoint_auth_methods_supported": ["none"],
            "scopes_supported": ["openid", "offline_access", "urn:ietf:params:oauth:scope:mail", "urn:ietf:params:oauth:scope:calendars"],
        }),
    )
}

async fn register(State(s): State<Arc<Shared>>, body: Bytes) -> Response {
    let Ok(v) = serde_json::from_slice::<Value>(&body) else {
        return oauth_error(StatusCode::BAD_REQUEST, "invalid_client_metadata", "body is not JSON");
    };
    let uris: Vec<String> = v["redirect_uris"]
        .as_array()
        .map(|a| a.iter().filter_map(|u| u.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    if uris.is_empty() {
        return oauth_error(StatusCode::BAD_REQUEST, "invalid_redirect_uri", "redirect_uris is required");
    }
    let client_id = s.oauth.lock().unwrap().register(uris.clone());
    json_response(
        StatusCode::CREATED,
        json!({
            "client_id": client_id,
            "client_id_issued_at": state::now_secs(),
            "client_name": v["client_name"],
            "redirect_uris": uris,
            "grant_types": ["authorization_code", "refresh_token"],
            "response_types": ["code"],
            "token_endpoint_auth_method": "none",
        }),
    )
}

/// Stands in for the browser step: no login page, it approves at once.
async fn authorize_http(State(s): State<Arc<Shared>>, axum::extract::RawQuery(q): axum::extract::RawQuery) -> Response {
    let params = form(q.unwrap_or_default().as_bytes());
    let result = s.oauth.lock().unwrap().authorize(&params);
    match result {
        Ok(location) => (StatusCode::FOUND, [(header::LOCATION, location)], "").into_response(),
        Err(why) => (StatusCode::BAD_REQUEST, why).into_response(),
    }
}

async fn token(State(s): State<Arc<Shared>>, body: Bytes) -> Response {
    let result = s.oauth.lock().unwrap().token(&form(&body));
    match result {
        Ok(t) => json_response(
            StatusCode::OK,
            json!({
                "access_token": t.access_token,
                "token_type": "Bearer",
                "expires_in": t.expires_in,
                "refresh_token": t.refresh_token,
                "scope": "openid offline_access urn:ietf:params:oauth:scope:mail urn:ietf:params:oauth:scope:calendars",
                "username": USERNAME,
            }),
        ),
        Err((error, description)) => oauth_error(StatusCode::BAD_REQUEST, error, &description),
    }
}

async fn revoke(State(s): State<Arc<Shared>>, body: Bytes) -> Response {
    if let Some(t) = form(&body).get("token") {
        s.oauth.lock().unwrap().revoke(t);
    }
    StatusCode::OK.into_response()
}

async fn session(State(s): State<Arc<Shared>>, headers: HeaderMap) -> Response {
    if let Err(r) = check_bearer(&s, &headers) {
        return r;
    }
    let b = &s.base;
    let mail_caps = json!({
        "maxMailboxesPerEmail": null, "maxMailboxDepth": 10, "maxSizeMailboxName": 255,
        "maxSizeAttachmentsPerEmail": 50_000_000, "emailQuerySortOptions": ["receivedAt", "sentAt", "subject"],
        "mayCreateTopLevelMailbox": true
    });
    let submission_caps = json!({ "maxDelayedSend": 0, "submissionExtensions": {} });
    // jmap-client wants every core field; the numbers are Stalwart-like.
    let core_caps = json!({
        "maxSizeUpload": 50_000_000, "maxConcurrentUpload": 4, "maxSizeRequest": 10_000_000,
        "maxConcurrentRequests": 4, "maxCallsInRequest": 16, "maxObjectsInGet": 500,
        "maxObjectsInSet": 500, "collationAlgorithms": ["i;ascii-casemap"]
    });
    let core = "urn:ietf:params:jmap:core";
    let mail = "urn:ietf:params:jmap:mail";
    let submission = "urn:ietf:params:jmap:submission";
    let calendars = "urn:ietf:params:jmap:calendars";
    json_response(
        StatusCode::OK,
        json!({
            "capabilities": {
                core: core_caps,
                mail: {},
                submission: {},
                calendars: {},
            },
            "accounts": {
                ACCOUNT_ID: {
                    "name": USERNAME,
                    "isPersonal": true,
                    "isReadOnly": false,
                    "accountCapabilities": {
                        mail: mail_caps,
                        submission: submission_caps,
                        calendars: {},
                    }
                }
            },
            "primaryAccounts": { mail: ACCOUNT_ID, submission: ACCOUNT_ID, calendars: ACCOUNT_ID },
            "username": USERNAME,
            "apiUrl": format!("{b}/jmap"),
            "downloadUrl": format!("{b}/jmap/download/{{accountId}}/{{blobId}}/{{name}}?accept={{type}}"),
            "uploadUrl": format!("{b}/jmap/upload/{{accountId}}"),
            "eventSourceUrl": format!("{b}/eventsource?types={{types}}&closeafter={{closeafter}}&ping={{ping}}"),
            "state": state::SESSION_STATE,
        }),
    )
}

async fn jmap(State(s): State<Arc<Shared>>, headers: HeaderMap, body: Bytes) -> Response {
    if let Err(r) = check_bearer(&s, &headers) {
        return r;
    }
    let Ok(req) = serde_json::from_slice::<Value>(&body) else {
        return problem(StatusCode::BAD_REQUEST, "notJSON", "the body is not JSON");
    };
    let (Some(_), Some(calls)) = (req["using"].as_array(), req["methodCalls"].as_array()) else {
        return problem(StatusCode::BAD_REQUEST, "notRequest", "using and methodCalls are required");
    };

    if calls.iter().any(|c| c[0] == "CalendarEvent/get") {
        let delay = s.calendar_delay_ms.load(Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(delay)).await;
    }
    let mut responses: Vec<(String, Value, String)> = Vec::new();
    for call in calls {
        let (Some(name), Some(args), Some(call_id)) = (call[0].as_str(), call.get(1), call[2].as_str()) else {
            return problem(StatusCode::BAD_REQUEST, "notRequest", "a method call must be [name, arguments, id]");
        };
        s.count(name);
        let result = resolve_refs(args, &responses).and_then(|args| call_method(&s, name, &args));
        responses.push(match result {
            Ok(v) => (name.to_string(), v, call_id.to_string()),
            Err(e) => {
                ("error".to_string(), json!({ "type": e.kind, "description": e.description }), call_id.to_string())
            }
        });
    }
    json_response(
        StatusCode::OK,
        json!({
            "methodResponses": responses.into_iter().map(|(n, v, id)| json!([n, v, id])).collect::<Vec<_>>(),
            "sessionState": state::SESSION_STATE,
        }),
    )
}

/// Calendar methods are answered from the calendar, everything else from the mail.
fn call_method(s: &Shared, name: &str, args: &Value) -> Result<Value, state::MethodError> {
    if name == "CalendarEvent/get" {
        return s.calendar.lock().unwrap().get(args);
    }
    s.mail().call(name, args)
}

// ---- EventSource ------------------------------------------------------------------------

/// Counts an open stream for as long as it lives, wherever it ends.
struct StreamGuard(Arc<Shared>);

impl Drop for StreamGuard {
    fn drop(&mut self) {
        self.0.open_streams.fetch_sub(1, Ordering::SeqCst);
    }
}

struct StreamState {
    rx: broadcast::Receiver<StreamMsg>,
    _guard: StreamGuard,
    ping: Option<tokio::time::Interval>,
    types: Option<Vec<String>>,
    close_after_state: bool,
    first: bool,
    done: bool,
}

async fn eventsource(
    State(s): State<Arc<Shared>>,
    headers: HeaderMap,
    axum::extract::RawQuery(q): axum::extract::RawQuery,
) -> Response {
    if let Err(r) = check_bearer(&s, &headers) {
        return r;
    }
    let params = form(q.unwrap_or_default().as_bytes());
    let types = match params.get("types").map(String::as_str) {
        None | Some("") | Some("*") => None,
        Some(list) => Some(list.split(',').map(str::to_string).collect()),
    };
    let ping_secs: u64 = params.get("ping").and_then(|p| p.parse().ok()).unwrap_or(0);
    let ping = (ping_secs > 0).then(|| {
        let period = Duration::from_secs(ping_secs);
        tokio::time::interval_at(tokio::time::Instant::now() + period, period)
    });
    // Subscribe before answering, so nothing that happens after the response is missed.
    let rx = s.tx.subscribe();
    s.open_streams.fetch_add(1, Ordering::SeqCst);
    let st = StreamState {
        rx,
        _guard: StreamGuard(s.clone()),
        ping,
        types,
        close_after_state: params.get("closeafter").map(String::as_str) == Some("state"),
        first: true,
        done: false,
    };
    let stream = futures_util::stream::unfold(st, stream_step);
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/event-stream"), (header::CACHE_CONTROL, "no-cache")],
        Body::from_stream(stream),
    )
        .into_response()
}

async fn stream_step(mut st: StreamState) -> Option<(Result<Bytes, Infallible>, StreamState)> {
    if st.done {
        return None;
    }
    if st.first {
        st.first = false;
        // A comment line (no blank line after it, which jmap-client would read as an empty
        // event). It makes the response headers reach the client at once.
        return Some((Ok(Bytes::from_static(b": connected\n")), st));
    }
    loop {
        let tick = async {
            match st.ping.as_mut() {
                Some(i) => {
                    i.tick().await;
                }
                None => std::future::pending::<()>().await,
            }
        };
        tokio::select! {
            msg = st.rx.recv() => match msg {
                Ok(StreamMsg::State { id, changed }) => {
                    let changed = filter_types(changed, st.types.as_deref());
                    if changed.as_object().is_none_or(|accounts| accounts.is_empty()) {
                        continue;
                    }
                    let data = json!({ "@type": "StateChange", "changed": changed });
                    let frame = format!("event: state\nid: {id}\ndata: {data}\n\n");
                    st.done = st.close_after_state;
                    return Some((Ok(Bytes::from(frame)), st));
                }
                Ok(StreamMsg::CalendarAlert { data }) => {
                    if st.types.as_ref().is_some_and(|t| !t.iter().any(|t| t == "CalendarAlert")) {
                        continue;
                    }
                    // jmap-client tells this event from a state change by its name.
                    return Some((Ok(Bytes::from(format!("event: calendarAlert
data: {data}

"))), st));
                }
                Ok(StreamMsg::Drop) | Err(broadcast::error::RecvError::Closed) => return None,
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
            },
            _ = tick => {
                // A real ping event: a bare comment plus blank line would parse as an empty
                // state event in jmap-client.
                return Some((Ok(Bytes::from_static(b"event: ping\ndata: {\"interval\":0}\n\n")), st));
            }
        }
    }
}

/// Keeps only the requested data types; accounts left with none are dropped.
fn filter_types(changed: Value, types: Option<&[String]>) -> Value {
    let Some(types) = types else { return changed };
    let mut out = serde_json::Map::new();
    if let Value::Object(accounts) = changed {
        for (account, per_type) in accounts {
            let kept: serde_json::Map<String, Value> = per_type
                .as_object()
                .into_iter()
                .flatten()
                .filter(|(t, _)| types.contains(t))
                .map(|(t, v)| (t.clone(), v.clone()))
                .collect();
            if !kept.is_empty() {
                out.insert(account, Value::Object(kept));
            }
        }
    }
    Value::Object(out)
}
