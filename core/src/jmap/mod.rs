//! JMAP requests: jmap-client for mail, own types for calendars and contacts.
//!
//! This layer only talks to the server and converts the answers into the core's plain types.
//! It keeps no state and decides nothing: sync, push and actions build on it.
//!
//! Two failures are told apart from the rest with [`is_unauthorized`] (HTTP 401: the access
//! token expired, renew it) and [`is_cannot_calculate_changes`] (do a full resync). Both
//! arrive as `Error::Temporary`; every other network or server problem does too.

mod calendar;
mod calendars;
mod clock;
mod contacts;
mod convert;
mod errors;
mod files;
mod folders;
mod jscal;
mod jscontact;
mod recur;
mod send;
mod shared;
mod sharing;
mod sieve;
mod vacation;

use std::{io::Write, pin::Pin, time::Duration};

use futures_util::{Stream, StreamExt};
use jmap_client::{
    client::{Client, Credentials},
    core::{
        query::Filter,
        response::{EmailGetResponse, MailboxGetResponse, ThreadGetResponse},
    },
    email,
    event_source::PushNotification,
    DataType,
};

use crate::{
    auth::{secure_url, server_base},
    model::{Action, Mailbox, MessageBody, MessageHeader},
    Error, Result,
};
pub use contacts::is_no_such_contact;
use convert::{
    batch_size, body_part_properties, body_properties, header_properties, mailbox_properties, merge_pages,
    order_by_ids, to_body, to_header, to_mailbox, Page,
};
use errors::from_jmap;
pub use errors::{is_cannot_calculate_changes, is_unauthorized};
pub use files::UploadResume;
pub use jscal::{check_span, sort as sort_agenda};
pub use send::{CopyState, HeaderIds, Uploaded};
pub use sieve::{SieveLimits, SieveList, SieveScript, CHANGED as SIEVE_CHANGED};

const MAIL_CAPABILITY: &str = "urn:ietf:params:jmap:mail";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// jmap-client only bounds the connect step, so a server or proxy that accepts the connection
/// and never answers would otherwise hang the caller here for good.
const STREAM_OPEN_TIMEOUT: Duration = Duration::from_secs(30);
/// The server sends a ping this often (seconds) so a dead connection is noticed.
const STREAM_PING_SECS: u32 = 60;
/// The longest a blob download may stay silent before it is given up.
const DOWNLOAD_IDLE_TIMEOUT: Duration = Duration::from_secs(60);
const SEEN: &str = "$seen";
const FLAGGED: &str = "$flagged";
/// A text or HTML part longer than this (bytes) arrives cut, and the body says so.
const MAX_BODY_VALUE_BYTES: usize = 1_000_000;

/// The answer of a /changes call, followed to the end (`hasMoreChanges` loops are done inside).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Changes {
    pub created: Vec<String>,
    pub updated: Vec<String>,
    pub destroyed: Vec<String>,
    pub new_state: String,
}

/// What `apply` needs to know besides the action itself.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ActionContext {
    /// The mailboxes the message is in now.
    pub current_mailbox_ids: Vec<String>,
    pub archive_id: Option<String>,
    pub trash_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// Some of these data types changed on the server.
    StateChange {
        email: bool,
        mailbox: bool,
    },
    CalendarAlert {
        event_id: String,
        alert_id: String,
        recurrence_id: Option<String>,
    },
}

/// A boxed Stream of Result<Event>; unreadable events (parse errors, comments) are skipped inside, not surfaced.
/// After an `Err` the stream ends: open a new one.
pub type EventStream = Pin<Box<dyn Stream<Item = Result<Event>> + Send>>;

/// One authenticated JMAP session. Cheap to share behind an Arc.
pub struct Connection {
    client: Client,
    /// For the requests jmap-client has no methods for (calendars).
    http: reqwest::Client,
    /// For blob downloads, which may take long: see [`download_http`].
    downloads: reqwest::Client,
    account_id: String,
    /// The longest span the server expands, in seconds; read from the session when first needed.
    calendar_span: tokio::sync::OnceCell<u64>,
    /// The raw session, read once for the limits of the file storage.
    files_session: tokio::sync::OnceCell<serde_json::Value>,
}

impl Connection {
    /// Fetches the session from `<server>/.well-known/jmap` with a bearer token. The token
    /// only goes over https (or to loopback): to the server, and to the API and push URLs
    /// its session names.
    pub async fn connect(server_url: &str, access_token: &str) -> Result<Connection> {
        let base = server_base(server_url).map_err(Error::invalid)?;
        secure_url(base.as_str()).map_err(|e| Error::invalid(format!("Mail server URL {e}")))?;
        let host = base.host_str().unwrap_or_default().to_string();
        let mut client = Client::new()
            .credentials(Credentials::bearer(access_token))
            .timeout(REQUEST_TIMEOUT)
            .follow_redirects([host])
            .connect(base.as_str().trim_end_matches('/'))
            .await
            .map_err(from_jmap)?;
        check_session_urls(&client)?;
        let account_id = mail_account(&client)?;
        // jmap-client defaults to an arbitrary primary account; pin it to the mail one.
        client.set_default_account_id(account_id.clone());
        let http = plain_http().map_err(|e| Error::temporary(format!("the HTTP client could not be built: {e}")))?;
        let downloads =
            download_http().map_err(|e| Error::temporary(format!("the HTTP client could not be built: {e}")))?;
        Ok(Connection {
            client,
            http,
            downloads,
            account_id,
            calendar_span: tokio::sync::OnceCell::new(),
            files_session: tokio::sync::OnceCell::new(),
        })
    }

    /// The primary mail account.
    pub fn account_id(&self) -> String {
        self.account_id.clone()
    }

    pub fn username(&self) -> String {
        self.client.session().username().to_string()
    }

    pub fn has_capability(&self, urn: &str) -> bool {
        self.client.session().has_capability(urn)
    }

    /// All mailboxes and the Mailbox state they belong to.
    pub async fn mailboxes(&self) -> Result<(Vec<Mailbox>, String)> {
        let mut req = self.client.build();
        req.get_mailbox().properties(mailbox_properties());
        let mut resp = req.send_single::<MailboxGetResponse>().await.map_err(from_jmap)?;
        let list = resp.take_list().iter().filter_map(to_mailbox).collect();
        Ok((list, resp.take_state()))
    }

    /// What changed in the mailboxes since `since_state`. See [`is_cannot_calculate_changes`].
    pub async fn mailbox_changes(&self, since_state: &str) -> Result<Changes> {
        let mut pages = Vec::new();
        let mut since = since_state.to_string();
        loop {
            let page = self.mailbox_changes_page(&since).await?;
            let more = page.has_more;
            since = page.new_state.clone();
            pages.push(page);
            if !more {
                return Ok(merge_pages(pages));
            }
        }
    }

    async fn mailbox_changes_page(&self, since: &str) -> Result<Page> {
        let mut req = self.client.build();
        req.changes_mailbox(since);
        let mut r = req.send_changes_mailbox().await.map_err(from_jmap)?;
        Ok(Page {
            created: r.take_created(),
            updated: r.take_updated(),
            destroyed: r.take_destroyed(),
            has_more: r.has_more_changes(),
            new_state: r.take_new_state(),
        })
    }

    /// The mailboxes with these ids, in the order asked. Unknown ids are left out.
    pub async fn mailboxes_by_id(&self, ids: &[String]) -> Result<Vec<Mailbox>> {
        let mut out = Vec::with_capacity(ids.len());
        for chunk in ids.chunks(self.get_batch()) {
            let mut req = self.client.build();
            req.get_mailbox().ids(chunk.iter().cloned()).properties(mailbox_properties());
            let mut resp = req.send_single::<MailboxGetResponse>().await.map_err(from_jmap)?;
            out.extend(resp.take_list().iter().filter_map(to_mailbox));
        }
        Ok(order_by_ids(ids, out, |m| m.id.as_str()))
    }

    /// Ids of the newest `limit` messages in a mailbox (receivedAt descending), from `position`.
    pub async fn message_ids(&self, mailbox_id: &str, position: usize, limit: usize) -> Result<Vec<String>> {
        let position = i32::try_from(position).map_err(|_| Error::invalid("message position too large"))?;
        let mut req = self.client.build();
        req.query_email()
            .filter(email::query::Filter::in_mailbox(mailbox_id))
            .sort([email::query::Comparator::received_at().descending()])
            .position(position)
            .limit(limit);
        let mut resp = req.send_query_email().await.map_err(from_jmap)?;
        Ok(resp.take_ids())
    }

    /// Headers for these ids (batched so one request never exceeds the server's maxObjectsInGet),
    /// in the order asked, and the Email state. The state is the first batch's: if mail arrives
    /// between batches, the next `email_changes` from it reports that mail again rather than
    /// losing it. Ids the server does not know are left out.
    pub async fn messages(&self, ids: &[String]) -> Result<(Vec<MessageHeader>, String)> {
        self.messages_in(&self.account_id, ids).await
    }

    /// Like [`Connection::messages`], in the account `account` (another user's, when a folder of
    /// it is shared). Ids are only unique within one account.
    pub(super) async fn messages_in(&self, account: &str, ids: &[String]) -> Result<(Vec<MessageHeader>, String)> {
        let mut state = None;
        let mut out = Vec::with_capacity(ids.len());
        // An empty list still asks once: the caller wants the current state.
        let chunks: Vec<&[String]> = if ids.is_empty() { vec![ids] } else { ids.chunks(self.get_batch()).collect() };
        for chunk in chunks {
            let mut req = self.client.build();
            req.get_email().account_id(account).ids(chunk.iter().cloned()).properties(header_properties());
            let mut resp = req.send_single::<EmailGetResponse>().await.map_err(from_jmap)?;
            out.extend(resp.take_list().iter().filter_map(to_header));
            state.get_or_insert_with(|| resp.take_state());
        }
        let out = order_by_ids(ids, out, |h| h.id.as_str());
        Ok((out, state.unwrap_or_default()))
    }

    /// Ids of up to `limit` messages of a mailbox that were received before `before_secs`
    /// (exclusive), newest first, from `position` in that list.
    pub async fn message_ids_before(
        &self,
        mailbox_id: &str,
        before_secs: i64,
        position: usize,
        limit: usize,
    ) -> Result<Vec<String>> {
        let filter =
            Filter::and([email::query::Filter::in_mailbox(mailbox_id), email::query::Filter::before(before_secs)]);
        self.query_ids(filter, position, limit).await
    }

    /// Ids of the messages the server finds for `query` (it searches headers and bodies), in one
    /// mailbox or in all, newest first.
    pub async fn search_ids(&self, query: &str, mailbox_id: Option<&str>, limit: usize) -> Result<Vec<String>> {
        let text = email::query::Filter::text(query);
        let filter = match mailbox_id {
            Some(mailbox) => Filter::and([text, email::query::Filter::in_mailbox(mailbox)]),
            None => text.into(),
        };
        self.query_ids(filter, 0, limit).await
    }

    async fn query_ids(
        &self,
        filter: Filter<email::query::Filter>,
        position: usize,
        limit: usize,
    ) -> Result<Vec<String>> {
        let position = i32::try_from(position).map_err(|_| Error::invalid("message position too large"))?;
        let mut req = self.client.build();
        req.query_email()
            .filter(filter)
            .sort([email::query::Comparator::received_at().descending()])
            .position(position)
            .limit(limit);
        let mut resp = req.send_query_email().await.map_err(from_jmap)?;
        Ok(resp.take_ids())
    }

    /// The bodies of these messages (recipients, text, HTML, files), batched like `messages`, in
    /// the order asked. Ids the server does not know are left out.
    pub async fn bodies(&self, ids: &[String]) -> Result<Vec<MessageBody>> {
        self.bodies_in(&self.account_id, ids).await
    }

    /// Like [`Connection::bodies`], in the account `account`.
    pub(super) async fn bodies_in(&self, account: &str, ids: &[String]) -> Result<Vec<MessageBody>> {
        let mut out = Vec::with_capacity(ids.len());
        for chunk in ids.chunks(self.get_batch()) {
            let mut req = self.client.build();
            let get = req.get_email().account_id(account).ids(chunk.iter().cloned()).properties(body_properties());
            get.arguments()
                .body_properties(body_part_properties())
                .fetch_text_body_values(true)
                .fetch_html_body_values(true)
                .max_body_value_bytes(MAX_BODY_VALUE_BYTES);
            let mut resp = req.send_single::<EmailGetResponse>().await.map_err(from_jmap)?;
            out.extend(resp.take_list().iter().filter_map(to_body));
        }
        Ok(order_by_ids(ids, out, |b| b.message_id.as_str()))
    }

    /// Ids of the messages of a thread, in the server's order. An unknown thread has none.
    pub async fn thread_email_ids(&self, thread_id: &str) -> Result<Vec<String>> {
        let mut req = self.client.build();
        req.get_thread().ids([thread_id]);
        let mut resp = req.send_single::<ThreadGetResponse>().await.map_err(from_jmap)?;
        Ok(resp.take_list().pop().map(|t| t.email_ids().to_vec()).unwrap_or_default())
    }

    /// Writes a blob (an attachment, an inline picture) to `file`, piece by piece as it
    /// arrives, so a large attachment is never held in memory. There is no limit on the whole
    /// transfer, only on a silence in it. On an error `file` is left as far as it got: the
    /// caller owns it.
    pub async fn download_to(&self, blob_id: &str, file: &mut dyn Write) -> Result<()> {
        self.download_from(&self.account_id, blob_id, file).await
    }

    /// Like [`Connection::download_to`], for a blob of the account `account` (the one the URL
    /// names: a file in a shared space).
    pub async fn download_from(&self, account: &str, blob_id: &str, file: &mut dyn Write) -> Result<()> {
        let failed = |e: &dyn std::fmt::Display| Error::temporary(format!("download: {e}"));
        let mut headers = self.client.headers().clone();
        headers.remove(reqwest::header::CONTENT_TYPE);
        let mut response = self
            .downloads
            .get(self.download_url(account, blob_id))
            .headers(headers)
            .send()
            .await
            .map_err(|e| failed(&e))?;
        match response.status().as_u16() {
            200..=299 => {}
            401 => return Err(errors::unauthorized("download")),
            404 => return Err(Error::invalid("the server does not have this file")),
            status => return Err(failed(&format!("the server answered {status}"))),
        }
        while let Some(chunk) = response.chunk().await.map_err(|e| failed(&e))? {
            file.write_all(&chunk).map_err(|e| Error::store(format!("saving the file: {e}")))?;
        }
        Ok(())
    }

    /// The session's download URL with its placeholders filled in, as jmap-client does it.
    fn download_url(&self, account: &str, blob_id: &str) -> String {
        use jmap_client::{blob::URLParameter, core::session::URLPart};
        let mut url = String::new();
        for part in self.client.download_url() {
            match part {
                URLPart::Value(text) => url.push_str(text),
                URLPart::Parameter(URLParameter::AccountId) => url.push_str(account),
                URLPart::Parameter(URLParameter::BlobId) => url.push_str(blob_id),
                URLPart::Parameter(URLParameter::Name) => url.push_str("none"),
                URLPart::Parameter(URLParameter::Type) => url.push_str("application/octet-stream"),
            }
        }
        url
    }

    /// What changed in Email since `since_state`. See [`is_cannot_calculate_changes`].
    pub async fn email_changes(&self, since_state: &str) -> Result<Changes> {
        let mut pages = Vec::new();
        let mut since = since_state.to_string();
        loop {
            let page = self.email_changes_page(&since).await?;
            let more = page.has_more;
            since = page.new_state.clone();
            pages.push(page);
            if !more {
                return Ok(merge_pages(pages));
            }
        }
    }

    async fn email_changes_page(&self, since: &str) -> Result<Page> {
        let mut req = self.client.build();
        req.changes_email(since);
        let mut r = req.send_changes_email().await.map_err(from_jmap)?;
        Ok(Page {
            created: r.take_created(),
            updated: r.take_updated(),
            destroyed: r.take_destroyed(),
            has_more: r.has_more_changes(),
            new_state: r.take_new_state(),
        })
    }

    /// Applies one `model::Action`: MarkRead/MarkUnread set or clear `$seen`; Flag/Unflag set or
    /// clear `$flagged`; Archive moves the message to the archive mailbox (out of whatever it is
    /// in); Move to the mailbox the action names; Delete moves it to Trash; Destroy removes it
    /// for good. A message the server no longer has gives
    /// `Error::Invalid`.
    pub async fn apply(&self, action: &Action, ctx: &ActionContext) -> Result<()> {
        let (id, plan) = plan(action, ctx)?;
        match plan {
            Plan::Nothing => Ok(()),
            Plan::Seen(set) => self.client.email_set_keyword(&id, SEEN, set).await.map(drop),
            Plan::Flagged(set) => self.client.email_set_keyword(&id, FLAGGED, set).await.map(drop),
            Plan::MoveTo(mailbox) => self.client.email_set_mailboxes(&id, [mailbox]).await.map(drop),
            Plan::Destroy => self.client.email_destroy(&id).await,
        }
        .map_err(from_jmap)
    }

    /// Opens the EventSource stream for Email, Mailbox and CalendarAlert changes (ping 60 s),
    /// with a 30 s open timeout.
    pub async fn events(&self) -> Result<EventStream> {
        let open = self.client.event_source(
            Some([DataType::Email, DataType::Mailbox, DataType::CalendarAlert]),
            false,
            Some(STREAM_PING_SECS),
            None,
        );
        let stream = tokio::time::timeout(STREAM_OPEN_TIMEOUT, open)
            .await
            .map_err(|_| Error::temporary("the push stream did not open within 30 s"))?
            .map_err(from_jmap)?;
        let account = self.account_id.clone();
        Ok(Box::pin(stream.filter_map(move |item| {
            let account = account.clone();
            async move { read_event(item, &account) }
        })))
    }

    fn get_batch(&self) -> usize {
        let max = self.client.session().core_capabilities().map_or(0, |c| c.max_objects_in_get());
        batch_size(max)
    }
}

/// Like jmap-client's own client, but it follows no redirects at all: the bearer token must
/// not be sent anywhere the session did not name.
fn plain_http() -> std::result::Result<reqwest::Client, reqwest::Error> {
    reqwest::Client::builder().timeout(REQUEST_TIMEOUT).redirect(reqwest::redirect::Policy::none()).build()
}

/// Like [`plain_http`], but without a limit on the whole request: an attachment on a slow
/// line takes as long as it takes. What ends a download is a server that stops sending.
fn download_http() -> std::result::Result<reqwest::Client, reqwest::Error> {
    reqwest::Client::builder()
        .connect_timeout(REQUEST_TIMEOUT)
        .read_timeout(DOWNLOAD_IDLE_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()
}

/// The API and push URLs come from the server's own answer; the bearer token must not be sent
/// to a plain-http address just because the session says so.
fn check_session_urls(client: &Client) -> Result<()> {
    let session = client.session();
    let urls = [
        ("API", session.api_url()),
        ("push", session.event_source_url()),
        ("download", session.download_url()),
        ("upload", session.upload_url()),
    ];
    for (what, url) in urls {
        secure_url(url).map_err(|e| Error::temporary(format!("the server's JMAP {what} URL {e}")))?;
    }
    Ok(())
}

fn mail_account(client: &Client) -> Result<String> {
    client
        .session()
        .primary_accounts()
        .find(|(capability, _)| capability.as_str() == MAIL_CAPABILITY)
        .map(|(_, account)| account.clone())
        .ok_or_else(|| Error::invalid("the server has no primary mail account for this user"))
}

/// Maps one stream item to an event. Comments and events jmap-client cannot parse are
/// dropped so one odd line does not end the stream; real transport errors are passed on.
fn read_event(item: jmap_client::Result<PushNotification>, account: &str) -> Option<Result<Event>> {
    use jmap_client::Error as E;
    match item {
        Ok(PushNotification::StateChange(changes)) => {
            let types = changes.changes(account).into_iter().flatten().map(|(t, _)| t);
            let (email, mailbox) = changed_types(types);
            (email || mailbox).then_some(Ok(Event::StateChange { email, mailbox }))
        }
        Ok(PushNotification::CalendarAlert(a)) => Some(Ok(Event::CalendarAlert {
            event_id: a.calendar_event_id,
            alert_id: a.alert_id,
            recurrence_id: a.recurrence_id,
        })),
        Err(E::Parse(_) | E::Internal(_)) => None,
        Err(e) => Some(Err(from_jmap(e))),
    }
}

/// Whether Email and/or Mailbox are among the changed types. Other types (Thread, ...) are ignored.
fn changed_types<'a>(types: impl Iterator<Item = &'a DataType>) -> (bool, bool) {
    types.fold((false, false), |(e, m), t| (e || *t == DataType::Email, m || *t == DataType::Mailbox))
}

/// What an action turns into on the server.
#[derive(Debug, PartialEq)]
enum Plan {
    Seen(bool),
    Flagged(bool),
    MoveTo(String),
    Destroy,
    /// Already as wanted.
    Nothing,
}

fn plan(action: &Action, ctx: &ActionContext) -> Result<(String, Plan)> {
    Ok(match action {
        Action::MarkRead { message_id } => (message_id.clone(), Plan::Seen(true)),
        Action::MarkUnread { message_id } => (message_id.clone(), Plan::Seen(false)),
        Action::Archive { message_id } => {
            let archive = ctx.archive_id.clone().ok_or_else(|| Error::invalid("the account has no Archive mailbox"))?;
            let already = ctx.current_mailbox_ids == [archive.clone()];
            (message_id.clone(), if already { Plan::Nothing } else { Plan::MoveTo(archive) })
        }
        Action::Delete { message_id } => {
            let trash = ctx.trash_id.clone().ok_or_else(|| Error::invalid("the account has no Trash mailbox"))?;
            (message_id.clone(), Plan::MoveTo(trash))
        }
        Action::Destroy { message_id } => (message_id.clone(), Plan::Destroy),
        Action::Flag { message_id } => (message_id.clone(), Plan::Flagged(true)),
        Action::Unflag { message_id } => (message_id.clone(), Plan::Flagged(false)),
        Action::Move { message_id, mailbox_id } => {
            let already = ctx.current_mailbox_ids == [mailbox_id.clone()];
            (message_id.clone(), if already { Plan::Nothing } else { Plan::MoveTo(mailbox_id.clone()) })
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(current: &[&str], archive: Option<&str>, trash: Option<&str>) -> ActionContext {
        ActionContext {
            current_mailbox_ids: current.iter().map(|s| s.to_string()).collect(),
            archive_id: archive.map(String::from),
            trash_id: trash.map(String::from),
        }
    }

    fn id(s: &str) -> String {
        s.to_string()
    }

    #[test]
    fn read_and_unread_toggle_seen() {
        let c = ctx(&["inbox"], None, None);
        assert_eq!(plan(&Action::MarkRead { message_id: id("m") }, &c).unwrap(), (id("m"), Plan::Seen(true)));
        assert_eq!(plan(&Action::MarkUnread { message_id: id("m") }, &c).unwrap(), (id("m"), Plan::Seen(false)));
    }

    #[test]
    fn archive_moves_unless_already_there() {
        let a = Action::Archive { message_id: id("m") };
        let (_, p) = plan(&a, &ctx(&["inbox"], Some("arch"), None)).unwrap();
        assert_eq!(p, Plan::MoveTo(id("arch")));
        let (_, p) = plan(&a, &ctx(&["arch"], Some("arch"), None)).unwrap();
        assert_eq!(p, Plan::Nothing);
        let (_, p) = plan(&a, &ctx(&["arch", "inbox"], Some("arch"), None)).unwrap();
        assert_eq!(p, Plan::MoveTo(id("arch")));
        assert!(matches!(plan(&a, &ctx(&["inbox"], None, None)), Err(Error::Invalid { .. })));
    }

    #[test]
    fn delete_always_moves_to_trash() {
        let d = Action::Delete { message_id: id("m") };
        let (_, p) = plan(&d, &ctx(&["inbox"], None, Some("trash"))).unwrap();
        assert_eq!(p, Plan::MoveTo(id("trash")));
        let (_, p) = plan(&d, &ctx(&["trash"], None, Some("trash"))).unwrap();
        assert_eq!(p, Plan::MoveTo(id("trash")));
        assert!(matches!(plan(&d, &ctx(&["inbox"], None, None)), Err(Error::Invalid { .. })));
    }

    #[test]
    fn flag_and_unflag_toggle_flagged() {
        let c = ctx(&["inbox"], None, None);
        assert_eq!(plan(&Action::Flag { message_id: id("m") }, &c).unwrap(), (id("m"), Plan::Flagged(true)));
        assert_eq!(plan(&Action::Unflag { message_id: id("m") }, &c).unwrap(), (id("m"), Plan::Flagged(false)));
    }

    #[test]
    fn move_sets_just_the_target_unless_it_is_already_exactly_there() {
        let m = Action::Move { message_id: id("m"), mailbox_id: id("junk") };
        let (_, p) = plan(&m, &ctx(&["inbox"], None, None)).unwrap();
        assert_eq!(p, Plan::MoveTo(id("junk")));
        let (_, p) = plan(&m, &ctx(&["junk"], None, None)).unwrap();
        assert_eq!(p, Plan::Nothing);
        let (_, p) = plan(&m, &ctx(&["junk", "inbox"], None, None)).unwrap();
        assert_eq!(p, Plan::MoveTo(id("junk")));
    }

    #[test]
    fn destroy_removes_for_good() {
        let d = Action::Destroy { message_id: id("m") };
        let (m, p) = plan(&d, &ctx(&[], None, None)).unwrap();
        assert_eq!((m, p), (id("m"), Plan::Destroy));
    }

    fn state_change(types: &[DataType], account: &str) -> jmap_client::Result<PushNotification> {
        // The map types are jmap-client's own (ahash); collect() lets inference name them.
        let changes =
            std::iter::once((account.to_string(), types.iter().map(|t| (t.clone(), "s".to_string())).collect()))
                .collect();
        Ok(PushNotification::StateChange(jmap_client::event_source::Changes::new(None, changes)))
    }

    #[test]
    fn state_change_flags() {
        let flags = |types: &[DataType]| match read_event(state_change(types, "a1"), "a1") {
            Some(Ok(Event::StateChange { email, mailbox })) => Some((email, mailbox)),
            _ => None,
        };
        assert_eq!(flags(&[DataType::Email]), Some((true, false)));
        assert_eq!(flags(&[DataType::Mailbox, DataType::Thread]), Some((false, true)));
        assert_eq!(flags(&[DataType::Email, DataType::Mailbox]), Some((true, true)));
        // Only types we do not use: nothing to tell the caller.
        assert_eq!(flags(&[DataType::Thread]), None);
    }

    #[test]
    fn other_accounts_are_ignored() {
        assert!(read_event(state_change(&[DataType::Email], "other"), "a1").is_none());
    }

    #[test]
    fn unreadable_events_are_skipped_and_real_errors_pass() {
        let parse = serde_json::from_str::<u8>("x").unwrap_err();
        assert!(read_event(Err(jmap_client::Error::Parse(parse)), "a1").is_none());
        assert!(read_event(Err(jmap_client::Error::Internal("odd".into())), "a1").is_none());
        let r = read_event(Err(jmap_client::Error::Server("401 Unauthorized".into())), "a1");
        assert!(is_unauthorized(&r.unwrap().unwrap_err()));
    }

    #[test]
    fn calendar_alert_is_passed_on() {
        let a = jmap_client::CalendarAlert {
            account_id: "a1".into(),
            calendar_event_id: "ev".into(),
            uid: "u".into(),
            recurrence_id: Some("r".into()),
            alert_id: "al".into(),
        };
        let got = read_event(Ok(PushNotification::CalendarAlert(a)), "a1").unwrap().unwrap();
        assert_eq!(
            got,
            Event::CalendarAlert { event_id: "ev".into(), alert_id: "al".into(), recurrence_id: Some("r".into()) }
        );
    }
}
