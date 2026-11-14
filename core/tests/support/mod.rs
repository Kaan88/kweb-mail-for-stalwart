#![allow(dead_code)]
//! An in-process mock of a Stalwart JMAP server for the integration tests: OAuth (PKCE,
//! dynamic registration, rotating refresh tokens), the JMAP session, the mail methods the core
//! uses, and an EventSource push stream. It speaks plain http on 127.0.0.1.
//!
//! The control methods on `MockServer` stand in for "another device" (they change mail and
//! cause push events) or for server trouble (expired tokens, a dead stream, an outage).

mod account;
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

use account::Account;
use calendar::Calendar;
use oauth::Oauth;
use state::{resolve_refs, Attachment, Mail, StreamMsg};

pub use state::{MailView, Refusal, Submission, ACCOUNT_ID, USERNAME};

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
    /// See [`MockServer::set_calendar_refused`].
    calendar_refused: AtomicBool,
    oauth: Mutex<Oauth>,
    /// Identities, address book and quotas.
    account: Mutex<Account>,
    /// The `maxSizeUpload` the session states, and the upload endpoint enforces.
    upload_limit: AtomicU64,
    /// See [`MockServer::lose_next_submission_answer`].
    lose_submission_answer: AtomicBool,
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
            calendar_refused: AtomicBool::new(false),
            oauth: Mutex::new(Oauth::new()),
            account: Mutex::new(Account::default()),
            upload_limit: AtomicU64::new(50_000_000),
            lose_submission_answer: AtomicBool::new(false),
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
            .route("/jmap/upload/{account}", post(upload))
            .route("/jmap/download/{account}/{blob}/{name}", get(download))
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

    /// Puts a keyword on a message, as another client or a half-done request would.
    pub fn add_keyword(&self, id: &str, keyword: &str) {
        self.edit(id, |e| {
            e.keywords.insert(keyword.to_string());
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

    /// Puts the messages in one thread. A message is its own thread until it is told otherwise.
    pub fn set_thread(&self, id: &str, thread_id: &str) {
        self.edit(id, |e| e.thread = Some(thread_id.to_string()));
    }

    /// The plain-text and HTML versions of a message; `None` leaves a version out.
    pub fn set_bodies(&self, id: &str, text: Option<&str>, html: Option<&str>) {
        self.edit(id, |e| {
            e.text = text.map(str::to_string);
            e.html = html.map(str::to_string);
        });
    }

    /// `(name, email)` pairs for the To and Cc headers.
    pub fn set_recipients(&self, id: &str, to: &[(&str, &str)], cc: &[(&str, &str)]) {
        let pairs = |l: &[(&str, &str)]| l.iter().map(|(n, e)| (n.to_string(), e.to_string())).collect();
        self.edit(id, |e| {
            e.to = pairs(to);
            e.cc = pairs(cc);
        });
    }

    /// Adds a file to a message and returns its blob id. An `inline` file with a `cid` is one
    /// the HTML shows: the server lists it with the body, not with the attachments.
    pub fn add_attachment(
        &self,
        id: &str,
        name: &str,
        mime_type: &str,
        bytes: &[u8],
        cid: Option<&str>,
        inline: bool,
    ) -> String {
        let mut blob_id = String::new();
        self.edit(id, |e| {
            blob_id = format!("blob-{id}-{}", e.attachments.len());
            e.attachments.push(Attachment {
                blob_id: blob_id.clone(),
                name: name.into(),
                mime_type: mime_type.into(),
                cid: cid.map(str::to_string),
                inline,
                bytes: bytes.to_vec(),
            });
        });
        blob_id
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

    /// An event as the server has it now.
    pub fn event(&self, id: &str) -> Option<Value> {
        self.shared.calendar.lock().unwrap().event(id)
    }

    /// Every event, in the order they were made.
    pub fn events(&self) -> Vec<Value> {
        self.shared.calendar.lock().unwrap().events()
    }

    /// Another calendar (the first, `cal1` "Personal", is there from the start). One that is
    /// not `visible` has `isVisible` false; one that is not `writable` has no write rights.
    pub fn add_calendar(&self, id: &str, name: &str, visible: bool, writable: bool) {
        self.shared.calendar.lock().unwrap().add_calendar(id, name, visible, writable);
    }

    /// The addresses `ParticipantIdentity/get` gives for the user (default: the sign-in name).
    pub fn set_calendar_addresses(&self, addresses: &[&str]) {
        self.shared.calendar.lock().unwrap().set_addresses(addresses);
    }

    /// The `maxExpandedQueryDuration` the session states (default `P52W1D`, which is what
    /// Stalwart says); `None` leaves it out.
    pub fn set_calendar_max_span(&self, span: Option<&str>) {
        self.shared.calendar.lock().unwrap().max_span = span.map(str::to_string);
    }

    /// The ids of the events a change asked the server to send scheduling messages for.
    pub fn scheduled_events(&self) -> Vec<String> {
        self.shared.calendar.lock().unwrap().scheduled.clone()
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

    /// Another client changes `property` of the event right after this client's next
    /// `CalendarEvent/get` (each call queues one change): its write then finds the state moved.
    pub fn change_event_after_next_get(&self, id: &str, property: &str, value: Value) {
        self.shared.calendar.lock().unwrap().interfere.push_back((id.into(), property.into(), value));
    }

    /// Makes `CalendarEvent/get` slow, to show that nothing else waits for it.
    pub fn set_calendar_delay(&self, delay: Duration) {
        self.shared.calendar_delay_ms.store(delay.as_millis() as u64, Ordering::SeqCst);
    }

    /// Makes `CalendarEvent/get` answer 401 while everything else keeps working. Expiring
    /// the tokens would do that too, but it also ends the session of any pass that happens to
    /// be running, and an alert pushed while the stream is being replaced is never seen.
    pub fn set_calendar_refused(&self, refused: bool) {
        self.shared.calendar_refused.store(refused, Ordering::SeqCst);
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

    /// While set, `Email/get` that asks for body values fails with a server error; everything
    /// else (headers, changes, actions) keeps working.
    pub fn set_bodies_fail(&self, fail: bool) {
        self.shared.mail().bodies_fail = fail;
    }

    /// With false, mutations stop producing push events: a stream that is open but dead.
    pub fn set_push_enabled(&self, enabled: bool) {
        self.shared.mail().push_enabled = enabled;
    }

    // ---- sending ---------------------------------------------------------------------

    /// Makes `EmailSubmission/set` refuse (`None` accepts again).
    pub fn set_submission_refusal(&self, refusal: Option<Refusal>) {
        self.shared.mail().refusal = refusal;
    }

    /// The next request that holds an `EmailSubmission/set` is carried out, and the answer is
    /// lost: the client gets a 500. The message is sent all the same.
    pub fn lose_next_submission_answer(&self) {
        self.shared.lose_submission_answer.store(true, Ordering::SeqCst);
    }

    /// The largest upload the server takes, in bytes (the session says so and the upload
    /// endpoint answers 413 above it). Set it before a client connects.
    pub fn set_upload_limit(&self, bytes: u64) {
        self.shared.upload_limit.store(bytes, Ordering::SeqCst);
    }

    /// Adds a JSContact card to the address book and returns its id.
    pub fn add_contact(&self, card: Value) -> String {
        self.shared.account.lock().unwrap().add_contact(card)
    }

    /// With false the server has no contacts (the session names no capability).
    pub fn set_contacts_enabled(&self, enabled: bool) {
        self.shared.account.lock().unwrap().contacts_enabled = enabled;
    }

    /// Adds an address book (the default one is `b`, writable).
    pub fn add_address_book(&self, id: &str, name: &str, may_write: bool) {
        self.shared.account.lock().unwrap().add_address_book(id, name, may_write);
    }

    /// The cards the server holds, in the order they were made.
    pub fn contact_cards(&self) -> Vec<Value> {
        self.shared.account.lock().unwrap().cards()
    }

    pub fn contact_card(&self, id: &str) -> Option<Value> {
        self.shared.account.lock().unwrap().card(id)
    }

    /// Another client changes a property of a card right after the next `ContactCard/get`: a
    /// write that was built from that read meets a changed card.
    pub fn interfere_with_contact(&self, id: &str, property: &str, value: Value) {
        self.shared.account.lock().unwrap().interfere.push_back((id.into(), property.into(), value));
    }

    /// Sets a property of a card at once, as another client would.
    pub fn set_contact_property(&self, id: &str, property: &str, value: Value) {
        self.shared.account.lock().unwrap().set_card_property(id, property, value);
    }

    /// Deletes a card at once, as another client would.
    pub fn remove_contact(&self, id: &str) {
        self.shared.account.lock().unwrap().remove_card(id);
    }

    /// The most ids a `ContactCard/query` gives, whatever its `limit` says.
    pub fn set_contact_query_limit(&self, limit: usize) {
        self.shared.account.lock().unwrap().query_limit = limit;
    }

    /// The `properties` every `ContactCard/get` asked for so far (`null` for all of them).
    pub fn contact_get_properties(&self) -> Vec<Value> {
        self.shared.account.lock().unwrap().card_get_properties.clone()
    }

    /// The away message as the server holds it.
    pub fn vacation(&self) -> Value {
        self.shared.account.lock().unwrap().vacation()
    }

    /// Another client changes properties of the away message.
    pub fn set_vacation(&self, properties: Value) {
        self.shared.account.lock().unwrap().set_vacation(properties);
    }

    /// With false the server has no away message (the session names no capability).
    pub fn set_vacation_enabled(&self, enabled: bool) {
        self.shared.account.lock().unwrap().vacation_enabled = enabled;
    }

    /// With false the server has no quotas (the session names no capability).
    pub fn set_quota_enabled(&self, enabled: bool) {
        self.shared.account.lock().unwrap().quota_enabled = enabled;
    }

    /// The storage quota: bytes used and the limit.
    pub fn set_quota(&self, used: u64, limit: Option<u64>) {
        self.shared.account.lock().unwrap().quota = (used, limit);
    }

    /// Replaces the identities: `(id, name, email, signature)`.
    pub fn set_identities(&self, identities: &[(&str, &str, &str, &str)]) {
        self.shared.account.lock().unwrap().identities = identities
            .iter()
            .map(|(id, name, email, signature)| account::IdentityRec {
                id: id.to_string(),
                name: name.to_string(),
                email: email.to_string(),
                signature: signature.to_string(),
            })
            .collect();
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

    /// What the server holds of a message.
    pub fn view(&self, id: &str) -> MailView {
        self.shared.mail().view(id).unwrap_or_else(|| panic!("no message '{id}'"))
    }

    /// Ids of the messages in the mailbox with this role, oldest arrival first.
    pub fn ids_in(&self, role: &str) -> Vec<String> {
        let mail = self.shared.mail();
        let mailbox = mail.mailbox_id_by_role(role).unwrap_or_else(|| panic!("no mailbox with role '{role}'"));
        let mut inside: Vec<_> = mail.emails.values().filter(|e| e.mailbox_ids.contains(&mailbox)).collect();
        inside.sort_by_key(|e| e.seq);
        inside.into_iter().map(|e| e.id.clone()).collect()
    }

    /// The messages the server has sent, oldest first.
    pub fn submissions(&self) -> Vec<Submission> {
        self.shared.mail().submissions.clone()
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
    let (contacts, quota, vacation) = {
        let account = s.account.lock().unwrap();
        (account.contacts_enabled, account.quota_enabled, account.vacation_enabled)
    };
    let upload_limit = s.upload_limit.load(Ordering::SeqCst);
    let mut capabilities = json!({ core: core_caps, mail: {}, submission: {}, calendars: {} });
    // Stalwart states the longest span it expands among the calendar account capabilities.
    let calendar_caps = match s.calendar.lock().unwrap().max_span.clone() {
        Some(span) => json!({ "maxExpandedQueryDuration": span, "maxCalendarsPerEvent": 10 }),
        None => json!({}),
    };
    let mut account_capabilities = json!({ mail: mail_caps, submission: submission_caps, calendars: calendar_caps });
    let mut primary = json!({ mail: ACCOUNT_ID, submission: ACCOUNT_ID, calendars: ACCOUNT_ID });
    for (urn, on) in [
        ("urn:ietf:params:jmap:contacts", contacts),
        ("urn:ietf:params:jmap:quota", quota),
        ("urn:ietf:params:jmap:vacationresponse", vacation),
    ] {
        if on {
            capabilities[urn] = json!({});
            account_capabilities[urn] = json!({});
            primary[urn] = json!(ACCOUNT_ID);
        }
    }
    capabilities[core]["maxSizeUpload"] = json!(upload_limit);
    json_response(
        StatusCode::OK,
        json!({
            "capabilities": capabilities,
            "accounts": {
                ACCOUNT_ID: {
                    "name": USERNAME,
                    "isPersonal": true,
                    "isReadOnly": false,
                    "accountCapabilities": account_capabilities,
                }
            },
            "primaryAccounts": primary,
            "username": USERNAME,
            "apiUrl": format!("{b}/jmap"),
            "downloadUrl": format!("{b}/jmap/download/{{accountId}}/{{blobId}}/{{name}}?accept={{type}}"),
            "uploadUrl": format!("{b}/jmap/upload/{{accountId}}"),
            "eventSourceUrl": format!("{b}/eventsource?types={{types}}&closeafter={{closeafter}}&ping={{ping}}"),
            "state": state::SESSION_STATE,
        }),
    )
}

/// A blob (an attachment) by the URL the session names.
async fn download(
    State(s): State<Arc<Shared>>,
    headers: HeaderMap,
    axum::extract::Path((account, blob, _name)): axum::extract::Path<(String, String, String)>,
) -> Response {
    if let Err(r) = check_bearer(&s, &headers) {
        return r;
    }
    let found = if account == ACCOUNT_ID { s.mail().blob(&blob) } else { None };
    match found {
        Some((mime_type, bytes)) => (StatusCode::OK, [(header::CONTENT_TYPE, mime_type)], bytes).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

/// The upload endpoint the session names: keeps the body as a blob.
async fn upload(
    State(s): State<Arc<Shared>>,
    headers: HeaderMap,
    axum::extract::Path(account): axum::extract::Path<String>,
    body: Bytes,
) -> Response {
    if let Err(r) = check_bearer(&s, &headers) {
        return r;
    }
    if account != ACCOUNT_ID {
        return StatusCode::NOT_FOUND.into_response();
    }
    if body.len() as u64 > s.upload_limit.load(Ordering::SeqCst) {
        return problem(StatusCode::PAYLOAD_TOO_LARGE, "limit", "the upload is larger than maxSizeUpload");
    }
    let mime_type =
        headers.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("application/octet-stream");
    let blob_id = s.mail().upload(mime_type, body.to_vec());
    json_response(
        StatusCode::CREATED,
        json!({ "accountId": ACCOUNT_ID, "blobId": blob_id, "type": mime_type, "size": body.len() }),
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
        if s.calendar_refused.load(Ordering::SeqCst) {
            return StatusCode::UNAUTHORIZED.into_response();
        }
    }
    let mut responses: Vec<(String, Value, String)> = Vec::new();
    for call in calls {
        let (Some(name), Some(args), Some(call_id)) = (call[0].as_str(), call.get(1), call[2].as_str()) else {
            return problem(StatusCode::BAD_REQUEST, "notRequest", "a method call must be [name, arguments, id]");
        };
        s.count(name);
        let result = resolve_refs(args, &responses).and_then(|args| call_method(&s, name, &args));
        match result {
            Ok(list) => responses.extend(list.into_iter().map(|(n, v)| (n, v, call_id.to_string()))),
            Err(e) => responses.push((
                "error".to_string(),
                json!({ "type": e.kind, "description": e.description }),
                call_id.to_string(),
            )),
        }
    }
    if calls.iter().any(|c| c[0] == "EmailSubmission/set") && s.lose_submission_answer.swap(false, Ordering::SeqCst) {
        // Carried out, but the client never hears of it.
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    json_response(
        StatusCode::OK,
        json!({
            "methodResponses": responses.into_iter().map(|(n, v, id)| json!([n, v, id])).collect::<Vec<_>>(),
            "sessionState": state::SESSION_STATE,
        }),
    )
}

/// Calendar methods are answered from the calendar, the account's own from the account,
/// everything else from the mail. A method may answer with more than one response.
fn call_method(s: &Shared, name: &str, args: &Value) -> Result<Vec<(String, Value)>, state::MethodError> {
    let one = |value: Value| vec![(name.to_string(), value)];
    match name {
        "Calendar/get"
        | "ParticipantIdentity/get"
        | "CalendarEvent/get"
        | "CalendarEvent/query"
        | "CalendarEvent/set" => s.calendar.lock().unwrap().call(name, args).map(one),
        "Identity/get"
        | "AddressBook/get"
        | "ContactCard/query"
        | "ContactCard/get"
        | "ContactCard/set"
        | "VacationResponse/get"
        | "VacationResponse/set"
        | "Quota/get" => s.account.lock().unwrap().call(name, args).map(one),
        "EmailSubmission/query" => {
            let wanted: Vec<&str> =
                args["filter"]["emailIds"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
            let mail = s.mail();
            let ids: Vec<&str> = mail
                .submissions
                .iter()
                .filter(|sub| wanted.is_empty() || wanted.contains(&sub.email_id.as_str()))
                .map(|sub| sub.id.as_str())
                .collect();
            Ok(one(json!({ "accountId": state::ACCOUNT_ID, "queryState": "q", "position": 0, "ids": ids })))
        }
        "EmailSubmission/set" => {
            let identities: Vec<String> = s.account.lock().unwrap().identities.iter().map(|i| i.id.clone()).collect();
            let mut mail = s.mail();
            let (answer, follow) = mail.submission_set(args, &identities)?;
            mail.flush();
            Ok(std::iter::once((name.to_string(), answer))
                .chain(follow.map(|f| ("Email/set".to_string(), f)))
                .collect())
        }
        _ => s.mail().call(name, args).map(one),
    }
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
