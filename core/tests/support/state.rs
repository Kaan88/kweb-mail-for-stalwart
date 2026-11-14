//! The mail data of the mock and the JMAP methods that read and change it.
//!
//! State strings are `es<n>` for Email and `ms<n>` for Mailbox. Every mutation bumps the
//! counter and is written to a change log, which is what `/changes` answers from.

use std::collections::{BTreeMap, BTreeSet};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Map, Value};
use tokio::sync::broadcast;

pub const ACCOUNT_ID: &str = "a1";
pub const USERNAME: &str = "alice@example.test";
pub const SESSION_STATE: &str = "ss1";

/// What the EventSource connections are told.
#[derive(Clone, Debug)]
pub enum StreamMsg {
    State {
        id: u64,
        changed: Value,
    },
    /// A `CalendarAlert` push object: accountId, calendarEventId, uid, recurrenceId, alertId.
    CalendarAlert {
        data: Value,
    },
    /// Close the connection from the server side.
    Drop,
}

pub struct MethodError {
    pub kind: &'static str,
    pub description: String,
}

fn merr(kind: &'static str, description: impl Into<String>) -> MethodError {
    MethodError { kind, description: description.into() }
}

type MResult = Result<Value, MethodError>;

pub fn now_secs() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

pub fn rfc3339(secs: i64) -> String {
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

/// Reads the `YYYY-MM-DDTHH:MM:SS` start of an RFC 3339 UTC timestamp.
fn parse_rfc3339(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 19 {
        return None;
    }
    let n = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, m, d) = (n(0..4)?, n(5..7)?, n(8..10)?);
    let (hh, mm, ss) = (n(11..13)?, n(14..16)?, n(17..19)?);
    let y2 = if m <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2.rem_euclid(400);
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Some(days * 86400 + hh * 3600 + mm * 60 + ss)
}

#[derive(Clone, Debug)]
pub struct MailboxRec {
    pub id: String,
    pub name: String,
    pub role: Option<&'static str>,
    pub sort_order: u32,
    pub parent_id: Option<String>,
}

/// A file in a message. `inline` ones sit in the body lists (a picture the HTML shows), the
/// others in `attachments`, as a server sorts them.
#[derive(Clone, Debug, PartialEq)]
pub struct Attachment {
    pub blob_id: String,
    pub name: String,
    pub mime_type: String,
    pub cid: Option<String>,
    pub inline: bool,
    pub bytes: Vec<u8>,
}

/// What `Email/get` was asked about the body.
#[derive(Default)]
struct BodyOpts {
    /// `bodyProperties`; all when absent.
    part_props: Option<BTreeSet<String>>,
    fetch_text: bool,
    fetch_html: bool,
    max_bytes: Option<usize>,
}

impl BodyOpts {
    fn from_args(args: &Value) -> Result<BodyOpts, MethodError> {
        let flag = |k: &str| args.get(k).and_then(Value::as_bool) == Some(true);
        Ok(BodyOpts {
            part_props: opt_strings(args.get("bodyProperties"))?.map(|v| v.into_iter().collect()),
            fetch_text: flag("fetchTextBodyValues") || flag("fetchAllBodyValues"),
            fetch_html: flag("fetchHTMLBodyValues") || flag("fetchAllBodyValues"),
            max_bytes: args.get("maxBodyValueBytes").and_then(Value::as_u64).map(|n| n as usize),
        })
    }
}

/// One body part as `Email/get` shows it.
#[derive(Clone)]
struct Part {
    part_id: String,
    blob_id: String,
    size: usize,
    mime_type: String,
    name: Option<String>,
    cid: Option<String>,
    disposition: Option<&'static str>,
    /// The text of a text part.
    text: Option<String>,
}

impl Part {
    fn json(&self, wanted: Option<&BTreeSet<String>>) -> Value {
        let all: [(&str, Value); 8] = [
            ("partId", json!(self.part_id)),
            ("blobId", json!(self.blob_id)),
            ("size", json!(self.size)),
            ("type", json!(self.mime_type)),
            ("charset", json!(self.text.as_ref().map(|_| "utf-8"))),
            ("name", json!(self.name)),
            ("disposition", json!(self.disposition)),
            ("cid", json!(self.cid)),
        ];
        let mut m = Map::new();
        for (k, v) in all {
            if wanted.is_none_or(|w| w.contains(k)) {
                m.insert(k.into(), v);
            }
        }
        Value::Object(m)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Email {
    pub id: String,
    /// Arrival order, the tie-breaker when two messages have the same `receivedAt`.
    pub seq: u64,
    pub mailbox_ids: BTreeSet<String>,
    pub keywords: BTreeSet<String>,
    pub from_name: String,
    pub from_email: String,
    /// `(name, email)` pairs.
    pub to: Vec<(String, String)>,
    pub cc: Vec<(String, String)>,
    pub bcc: Vec<(String, String)>,
    /// Message-IDs without angle brackets.
    pub in_reply_to: Vec<String>,
    pub references: Vec<String>,
    /// `None`: the message is its own thread.
    pub thread: Option<String>,
    pub text: Option<String>,
    pub html: Option<String>,
    pub attachments: Vec<Attachment>,
    pub subject: String,
    pub preview: String,
    pub received_at: i64,
}

const DEFAULT_EMAIL_PROPS: &[&str] = &[
    "id",
    "blobId",
    "threadId",
    "mailboxIds",
    "keywords",
    "size",
    "receivedAt",
    "sentAt",
    "from",
    "to",
    "subject",
    "preview",
    "hasAttachment",
    "messageId",
];

fn addresses(list: &[(String, String)]) -> Value {
    if list.is_empty() {
        return Value::Null;
    }
    list.iter().map(|(name, email)| json!({ "name": name, "email": email })).collect::<Vec<_>>().into()
}

impl Email {
    fn is_read(&self) -> bool {
        self.keywords.contains("$seen")
    }

    pub fn thread_id(&self) -> String {
        self.thread.clone().unwrap_or_else(|| format!("t{}", self.id))
    }

    /// The text and HTML parts and the files, as `(text_body, html_body, attachments)`. A message
    /// with only one kind of text offers it in both lists, and the pictures its HTML shows are in
    /// both lists too, as RFC 8621 says.
    fn parts(&self) -> (Vec<Part>, Vec<Part>, Vec<Part>) {
        let text = self.text.as_ref().map(|t| Part {
            part_id: "1".into(),
            blob_id: format!("bt{}", self.id),
            size: t.len(),
            mime_type: "text/plain".into(),
            name: None,
            cid: None,
            disposition: None,
            text: Some(t.clone()),
        });
        let html = self.html.as_ref().map(|h| Part {
            part_id: "2".into(),
            blob_id: format!("bh{}", self.id),
            size: h.len(),
            mime_type: "text/html".into(),
            name: None,
            cid: None,
            disposition: None,
            text: Some(h.clone()),
        });
        let mut text_body: Vec<Part> = text.clone().or_else(|| html.clone()).into_iter().collect();
        let mut html_body: Vec<Part> = html.or(text).into_iter().collect();
        let mut files = Vec::new();
        for (n, a) in self.attachments.iter().enumerate() {
            let part = Part {
                part_id: format!("a{n}"),
                blob_id: a.blob_id.clone(),
                size: a.bytes.len(),
                mime_type: a.mime_type.clone(),
                name: Some(a.name.clone()).filter(|n| !n.is_empty()),
                cid: a.cid.clone(),
                disposition: Some(if a.inline { "inline" } else { "attachment" }),
                text: None,
            };
            if a.inline {
                text_body.push(part.clone());
                html_body.push(part);
            } else {
                files.push(part);
            }
        }
        (text_body, html_body, files)
    }

    fn has_attachment(&self) -> bool {
        self.attachments.iter().any(|a| !a.inline)
    }

    fn json(&self, props: Option<&BTreeSet<String>>, body: &BodyOpts) -> Value {
        let mut m = Map::new();
        m.insert("id".into(), json!(self.id));
        let mut put = |k: &str, v: Value| {
            let wanted = match props {
                Some(p) => p.contains(k),
                None => DEFAULT_EMAIL_PROPS.contains(&k),
            };
            if wanted {
                m.insert(k.to_string(), v);
            }
        };
        let flags = |s: &BTreeSet<String>| s.iter().map(|k| (k.clone(), Value::Bool(true))).collect::<Map<_, _>>();
        put("blobId", json!(format!("b{}", self.id)));
        put("threadId", json!(self.thread_id()));
        put("mailboxIds", Value::Object(flags(&self.mailbox_ids)));
        put("keywords", Value::Object(flags(&self.keywords)));
        put("size", json!(1200 + self.subject.len() + self.preview.len()));
        put("receivedAt", json!(rfc3339(self.received_at)));
        put("sentAt", json!(rfc3339(self.received_at - 5)));
        put("from", json!([{ "name": self.from_name, "email": self.from_email }]));
        put("to", addresses(&self.to));
        put("cc", addresses(&self.cc));
        put("bcc", addresses(&self.bcc));
        put("inReplyTo", if self.in_reply_to.is_empty() { Value::Null } else { json!(self.in_reply_to) });
        put("references", if self.references.is_empty() { Value::Null } else { json!(self.references) });
        put("replyTo", Value::Null);
        put("subject", json!(self.subject));
        put("preview", json!(self.preview));
        put("hasAttachment", json!(self.has_attachment()));
        put("messageId", json!([format!("{}@mock.example.test", self.id)]));

        let (text_body, html_body, files) = self.parts();
        let part_list = |parts: &[Part]| parts.iter().map(|p| p.json(body.part_props.as_ref())).collect::<Vec<_>>();
        put("textBody", json!(part_list(&text_body)));
        put("htmlBody", json!(part_list(&html_body)));
        put("attachments", json!(part_list(&files)));
        // Values of the text parts of the lists asked for, cut to `maxBodyValueBytes`.
        let mut values = Map::new();
        for (wanted, parts) in [(body.fetch_text, &text_body), (body.fetch_html, &html_body)] {
            for part in parts.iter().filter(|p| wanted && p.text.is_some()) {
                let text = part.text.as_deref().unwrap_or_default();
                let mut end = body.max_bytes.map_or(text.len(), |max| max.min(text.len()));
                while !text.is_char_boundary(end) {
                    end -= 1;
                }
                let value =
                    json!({ "value": &text[..end], "isEncodingProblem": false, "isTruncated": end < text.len() });
                values.insert(part.part_id.clone(), value);
            }
        }
        put("bodyValues", Value::Object(values));
        Value::Object(m)
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    Created,
    Updated,
    Destroyed,
}

struct Log {
    state: u64,
    /// States older than this were "forgotten": asking for changes since them is an error.
    floor: u64,
    entries: Vec<(u64, String, Kind)>,
}

impl Log {
    fn new() -> Log {
        Log { state: 1, floor: 1, entries: Vec::new() }
    }

    fn bump(&mut self) -> u64 {
        self.state += 1;
        self.state
    }
}

struct ChangesOut {
    old_state: String,
    new_state: String,
    has_more: bool,
    created: Vec<String>,
    updated: Vec<String>,
    destroyed: Vec<String>,
}

pub struct Mail {
    pub emails: BTreeMap<String, Email>,
    pub mailboxes: Vec<MailboxRec>,
    email_log: Log,
    mailbox_log: Log,
    next_seq: u64,
    event_id: u64,
    dirty_email: bool,
    dirty_mailbox: bool,
    pub push_enabled: bool,
    /// The id the next folder made through `Mailbox/set` gets.
    next_mailbox: u64,
    /// How many more message changes `Email/set` carries out before it answers `serverFail` to
    /// the rest (`None`: no limit). For the tests of a failure part-way.
    pub email_set_budget: Option<usize>,
    /// `Email/get` that asks for body values fails (see `MockServer::set_bodies_fail`).
    pub bodies_fail: bool,
    /// The List-Id header of some messages, by message id.
    pub list_ids: BTreeMap<String, String>,
    /// The raw List-Unsubscribe and List-Unsubscribe-Post headers of some messages, by message id.
    pub unsubscribe: BTreeMap<String, (Option<String>, Option<String>)>,
    /// The most ids a get or a snippet request takes, when the test sets one.
    pub get_max: Option<usize>,
    /// Files put on the server through the upload endpoint: blob id to type and bytes.
    uploads: BTreeMap<String, (String, Vec<u8>)>,
    /// Every submission the server accepted, oldest first.
    pub submissions: Vec<Submission>,
    /// What `EmailSubmission/set` does instead of accepting.
    pub refusal: Option<Refusal>,
    tx: broadcast::Sender<StreamMsg>,
}

/// A message the server was asked to send: sent, or held back until its time.
#[derive(Clone, Debug, PartialEq)]
pub struct Submission {
    pub id: String,
    pub identity_id: String,
    pub email_id: String,
    /// The addresses it was sent to (To, Cc and Bcc, or the envelope's own list).
    pub recipients: Vec<String>,
    /// The envelope parameter that held it back, as asked: `("HOLDFOR", "10")`.
    pub hold: Option<(String, String)>,
    /// When it leaves, in milliseconds since the epoch (the creation time when not held).
    pub release_at_ms: u64,
    /// Cancelled: it will never leave.
    pub cancelled: bool,
    /// Destroyed: gone from the server's lists, but destroying does not stop a message.
    pub destroyed: bool,
}

impl Submission {
    /// `pending` until its time, then `final`; `canceled` when taken back.
    pub fn undo_status(&self, now_ms: u64) -> &'static str {
        if self.cancelled {
            "canceled"
        } else if now_ms < self.release_at_ms {
            "pending"
        } else {
            "final"
        }
    }

    /// It has left, whatever happened to its record.
    pub fn has_left(&self, now_ms: u64) -> bool {
        self.undo_status(now_ms) == "final"
    }
}

pub fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// How the mock refuses a submission.
#[derive(Clone, Debug, PartialEq)]
pub enum Refusal {
    /// A SetError of this type (`invalidEmail`, `tooLarge`, `forbiddenFrom`, ...): it will be
    /// refused again.
    Permanent(&'static str),
    /// The method fails as a whole with `serverFail`: a retry may work.
    Temporary,
}

/// A message as a test looks at it: what the server holds.
#[derive(Clone, Debug, PartialEq)]
pub struct MailView {
    pub id: String,
    pub from: (String, String),
    pub to: Vec<(String, String)>,
    pub cc: Vec<(String, String)>,
    pub bcc: Vec<(String, String)>,
    pub subject: String,
    pub text: Option<String>,
    pub in_reply_to: Vec<String>,
    pub references: Vec<String>,
    /// Sorted.
    pub keywords: Vec<String>,
    /// Roles of the mailboxes it is in, sorted.
    pub roles: Vec<String>,
    /// `(name, type, bytes)`.
    pub files: Vec<(String, String, Vec<u8>)>,
}

fn set_of(ids: &[&str]) -> BTreeSet<String> {
    ids.iter().map(|s| s.to_string()).collect()
}

impl Mail {
    pub fn new(tx: broadcast::Sender<StreamMsg>) -> Mail {
        let mailboxes = [
            ("mb1", "Inbox", "inbox"),
            ("mb2", "Archive", "archive"),
            ("mb3", "Trash", "trash"),
            ("mb4", "Drafts", "drafts"),
            ("mb5", "Sent", "sent"),
            ("mb6", "Junk", "junk"),
        ]
        .iter()
        .enumerate()
        .map(|(i, (id, name, role))| MailboxRec {
            id: id.to_string(),
            name: name.to_string(),
            role: Some(*role),
            sort_order: (i as u32 + 1) * 10,
            parent_id: None,
        })
        .collect();
        let mut mail = Mail {
            emails: BTreeMap::new(),
            mailboxes,
            email_log: Log::new(),
            mailbox_log: Log::new(),
            next_seq: 1,
            event_id: 0,
            dirty_email: false,
            dirty_mailbox: false,
            push_enabled: true,
            next_mailbox: 100,
            email_set_budget: None,
            bodies_fail: false,
            list_ids: BTreeMap::new(),
            unsubscribe: BTreeMap::new(),
            get_max: None,
            uploads: BTreeMap::new(),
            submissions: Vec::new(),
            refusal: None,
            tx,
        };
        let now = now_secs();
        // Seeded without touching the change log: the starting state is es1 / ms1.
        let seeds: [(&str, &str, &str, &str, &str, bool, i64); 5] = [
            (
                "mb1",
                "Maria Keller",
                "maria@example.test",
                "Lunch on Friday?",
                "Are you free at noon? The new place on the corner has a table.",
                false,
                2 * 3600,
            ),
            (
                "mb1",
                "Build Bot",
                "ci@example.test",
                "Nightly build passed",
                "All 212 tests passed on main in 4 minutes.",
                false,
                5 * 3600,
            ),
            (
                "mb1",
                "Tom Fischer",
                "tom@example.test",
                "Invoice 2026-114",
                "The invoice for October is attached. Payment is due in 14 days.",
                true,
                26 * 3600,
            ),
            (
                "mb2",
                "Anna Roth",
                "anna@example.test",
                "Conference notes",
                "Here are my notes from the talk on sync protocols.",
                true,
                3 * 86400,
            ),
            ("mb5", "Alice Test", USERNAME, "Re: Lunch on Friday?", "Noon works for me.", true, 100 * 60),
        ];
        for (mailbox, name, email, subject, preview, read, age) in seeds {
            let seq = mail.next_seq;
            mail.next_seq += 1;
            let (from_name, from_email, to_name, to_email) = if mailbox == "mb5" {
                (name, email, "Maria Keller", "maria@example.test")
            } else {
                (name, email, "Alice Test", USERNAME)
            };
            let id = format!("e{seq}");
            mail.emails.insert(
                id.clone(),
                Email {
                    id,
                    seq,
                    mailbox_ids: set_of(&[mailbox]),
                    keywords: if read { set_of(&["$seen"]) } else { BTreeSet::new() },
                    from_name: from_name.into(),
                    from_email: from_email.into(),
                    to: vec![(to_name.into(), to_email.into())],
                    cc: Vec::new(),
                    bcc: Vec::new(),
                    in_reply_to: Vec::new(),
                    references: Vec::new(),
                    thread: None,
                    text: None,
                    html: None,
                    attachments: Vec::new(),
                    subject: subject.into(),
                    preview: preview.into(),
                    received_at: now - age,
                },
            );
        }
        mail
    }

    // ---- state strings -------------------------------------------------------------

    pub fn email_state(&self) -> String {
        format!("es{}", self.email_log.state)
    }

    pub fn mailbox_state(&self) -> String {
        format!("ms{}", self.mailbox_log.state)
    }

    pub fn forget_changes(&mut self) {
        self.email_log.floor = self.email_log.state;
        self.email_log.entries.clear();
        self.mailbox_log.floor = self.mailbox_log.state;
        self.mailbox_log.entries.clear();
    }

    /// The type and bytes of an attachment, by blob id.
    pub fn blob(&self, blob_id: &str) -> Option<(String, Vec<u8>)> {
        let all = self.emails.values().flat_map(|e| e.attachments.iter());
        let attached = all.into_iter().find(|a| a.blob_id == blob_id).map(|a| (a.mime_type.clone(), a.bytes.clone()));
        attached.or_else(|| self.uploads.get(blob_id).cloned())
    }

    /// Keeps an uploaded file and returns its blob id.
    pub fn upload(&mut self, mime_type: &str, bytes: Vec<u8>) -> String {
        let id = format!("up{}", self.uploads.len() + 1);
        self.uploads.insert(id.clone(), (mime_type.to_string(), bytes));
        id
    }

    /// What the server holds of a message.
    pub fn view(&self, id: &str) -> Option<MailView> {
        let e = self.emails.get(id)?;
        let mut roles: Vec<String> = self
            .mailboxes
            .iter()
            .filter(|m| e.mailbox_ids.contains(&m.id))
            .filter_map(|m| m.role.map(str::to_string))
            .collect();
        roles.sort();
        Some(MailView {
            id: e.id.clone(),
            from: (e.from_name.clone(), e.from_email.clone()),
            to: e.to.clone(),
            cc: e.cc.clone(),
            bcc: e.bcc.clone(),
            subject: e.subject.clone(),
            text: e.text.clone(),
            in_reply_to: e.in_reply_to.clone(),
            references: e.references.clone(),
            keywords: e.keywords.iter().cloned().collect(),
            roles,
            files: e.attachments.iter().map(|a| (a.name.clone(), a.mime_type.clone(), a.bytes.clone())).collect(),
        })
    }

    /// A folder made, renamed or removed by another client; one Mailbox state change.
    pub fn add_mailbox(&mut self, id: &str, name: &str, parent: Option<&str>) {
        let sort_order = (self.mailboxes.len() as u32 + 1) * 10;
        let parent_id = parent.map(str::to_string);
        self.mailboxes.push(MailboxRec { id: id.into(), name: name.into(), role: None, sort_order, parent_id });
        self.touch_mailboxes(set_of(&[id]));
    }

    pub fn rename_mailbox(&mut self, id: &str, name: &str) {
        if let Some(m) = self.mailboxes.iter_mut().find(|m| m.id == id) {
            m.name = name.to_string();
        }
        self.touch_mailboxes(set_of(&[id]));
    }

    pub fn remove_mailbox(&mut self, id: &str) {
        self.mailboxes.retain(|m| m.id != id);
        self.touch_mailboxes(set_of(&[id]));
    }

    pub fn mailbox_id_by_role(&self, role: &str) -> Option<String> {
        self.mailboxes.iter().find(|m| m.role == Some(role)).map(|m| m.id.clone())
    }

    fn counts(&self, mailbox_id: &str) -> (usize, usize) {
        let inside = self.emails.values().filter(|e| e.mailbox_ids.contains(mailbox_id));
        let total = inside.clone().count();
        let unread = inside.filter(|e| !e.is_read()).count();
        (total, unread)
    }

    // ---- mutations (shared by the test controls and Email/set) ---------------------

    fn log_email(&mut self, id: &str, kind: Kind) {
        let s = self.email_log.bump();
        self.email_log.entries.push((s, id.to_string(), kind));
        self.dirty_email = true;
    }

    /// Mailbox counts changed for these mailboxes: one new Mailbox state for the lot.
    fn touch_mailboxes(&mut self, ids: BTreeSet<String>) {
        if ids.is_empty() {
            return;
        }
        let s = self.mailbox_log.bump();
        for id in ids {
            self.mailbox_log.entries.push((s, id, Kind::Updated));
        }
        self.dirty_mailbox = true;
    }

    /// Sends one StateChange for everything that changed since the last flush.
    pub fn flush(&mut self) {
        let (e, m) = (std::mem::take(&mut self.dirty_email), std::mem::take(&mut self.dirty_mailbox));
        if !(e || m) || !self.push_enabled {
            return;
        }
        let mut changed = Map::new();
        if e {
            changed.insert("Email".into(), json!(self.email_state()));
        }
        if m {
            changed.insert("Mailbox".into(), json!(self.mailbox_state()));
        }
        self.event_id += 1;
        let _ = self.tx.send(StreamMsg::State { id: self.event_id, changed: json!({ ACCOUNT_ID: changed }) });
    }

    pub fn deliver(
        &mut self,
        from_name: &str,
        from_email: &str,
        subject: &str,
        preview: &str,
        received_at: i64,
    ) -> String {
        let inbox = self.mailbox_id_by_role("inbox").expect("seeded");
        self.deliver_in(&inbox, from_name, from_email, subject, preview, received_at)
    }

    /// Like `deliver`, into the mailbox `inbox` instead of the Inbox (a filter put it there).
    pub fn deliver_in(
        &mut self,
        inbox: &str,
        from_name: &str,
        from_email: &str,
        subject: &str,
        preview: &str,
        received_at: i64,
    ) -> String {
        let seq = self.next_seq;
        self.next_seq += 1;
        let id = format!("e{seq}");
        self.emails.insert(
            id.clone(),
            Email {
                id: id.clone(),
                seq,
                mailbox_ids: set_of(&[inbox]),
                keywords: BTreeSet::new(),
                from_name: from_name.into(),
                from_email: from_email.into(),
                to: vec![("Alice Test".into(), USERNAME.into())],
                cc: Vec::new(),
                bcc: Vec::new(),
                in_reply_to: Vec::new(),
                references: Vec::new(),
                thread: None,
                text: None,
                html: None,
                attachments: Vec::new(),
                subject: subject.into(),
                preview: preview.into(),
                received_at,
            },
        );
        self.log_email(&id, Kind::Created);
        self.touch_mailboxes(set_of(&[inbox]));
        self.flush();
        id
    }

    /// Stores a changed copy of a message and logs what the change means. Returns whether
    /// anything differed.
    fn commit(&mut self, new: Email) -> bool {
        let old = match self.emails.get(&new.id) {
            Some(old) if *old == new => return false,
            Some(old) => old.clone(),
            None => return false,
        };
        let counts_changed = old.mailbox_ids != new.mailbox_ids || old.is_read() != new.is_read();
        let affected: BTreeSet<String> = old.mailbox_ids.union(&new.mailbox_ids).cloned().collect();
        let id = new.id.clone();
        self.emails.insert(id.clone(), new);
        self.log_email(&id, Kind::Updated);
        if counts_changed {
            self.touch_mailboxes(affected);
        }
        true
    }

    /// Test-control edit of one message. Returns false when there is no such message.
    pub fn update_email(&mut self, id: &str, f: impl FnOnce(&mut Email)) -> bool {
        let Some(mut e) = self.emails.get(id).cloned() else { return false };
        f(&mut e);
        self.commit(e);
        self.flush();
        true
    }

    fn destroy_internal(&mut self, id: &str) -> bool {
        let Some(old) = self.emails.remove(id) else { return false };
        self.log_email(id, Kind::Destroyed);
        self.touch_mailboxes(old.mailbox_ids);
        true
    }

    pub fn destroy(&mut self, id: &str) -> bool {
        let found = self.destroy_internal(id);
        self.flush();
        found
    }

    // ---- JMAP methods --------------------------------------------------------------

    pub fn call(&mut self, name: &str, args: &Value) -> MResult {
        if !args.is_object() {
            return Err(merr("invalidArguments", "arguments must be an object"));
        }
        if let Some(a) = args.get("accountId").and_then(Value::as_str) {
            if a != ACCOUNT_ID {
                return Err(merr("accountNotFound", format!("no account '{a}'")));
            }
        }
        let result = match name {
            "Mailbox/get" => self.mailbox_get(args),
            "Mailbox/query" => self.mailbox_query(args),
            "Mailbox/changes" => self.mailbox_changes(args),
            "Mailbox/set" => self.mailbox_set(args),
            "Email/get" => self.email_get(args),
            "Email/query" => self.email_query(args),
            "Email/changes" => self.email_changes(args),
            "Email/set" => self.email_set(args),
            "Thread/get" => self.thread_get(args),
            "SearchSnippet/get" => self.snippet_get(args),
            other => Err(merr("unknownMethod", format!("the mock does not implement '{other}'"))),
        };
        self.flush();
        result
    }

    fn mailbox_json(&self, m: &MailboxRec, props: Option<&BTreeSet<String>>) -> Value {
        let (total, unread) = self.counts(&m.id);
        let all: [(&str, Value); 10] = [
            ("name", json!(m.name)),
            ("parentId", json!(m.parent_id)),
            ("role", json!(m.role)),
            ("sortOrder", json!(m.sort_order)),
            ("totalEmails", json!(total)),
            ("unreadEmails", json!(unread)),
            // Every message is its own thread in the mock.
            ("totalThreads", json!(total)),
            ("unreadThreads", json!(unread)),
            (
                "myRights",
                json!({
                    "mayReadItems": true, "mayAddItems": true, "mayRemoveItems": true,
                    "maySetSeen": true, "maySetKeywords": true, "mayCreateChild": true,
                    "mayRename": false, "mayDelete": false, "maySubmit": true
                }),
            ),
            ("isSubscribed", json!(true)),
        ];
        let mut out = Map::new();
        out.insert("id".into(), json!(m.id));
        for (k, v) in all {
            if props.is_none_or(|p| p.contains(k)) {
                out.insert(k.into(), v);
            }
        }
        Value::Object(out)
    }

    fn mailbox_get(&self, args: &Value) -> MResult {
        let ids = opt_strings(args.get("ids"))?;
        let props = prop_set(args)?;
        let mut list = Vec::new();
        let mut not_found = Vec::new();
        match ids {
            None => list.extend(self.mailboxes.iter().map(|m| self.mailbox_json(m, props.as_ref()))),
            Some(ids) => {
                for id in ids {
                    match self.mailboxes.iter().find(|m| m.id == id) {
                        Some(m) => list.push(self.mailbox_json(m, props.as_ref())),
                        None => not_found.push(id),
                    }
                }
            }
        }
        Ok(json!({ "accountId": ACCOUNT_ID, "state": self.mailbox_state(), "list": list, "notFound": not_found }))
    }

    fn mailbox_query(&self, args: &Value) -> MResult {
        let filter = args.get("filter").filter(|f| !f.is_null());
        let mut found: Vec<&MailboxRec> = Vec::new();
        for m in &self.mailboxes {
            let ok = match filter {
                None => true,
                Some(f) => mailbox_filter(m, f)?,
            };
            if ok {
                found.push(m);
            }
        }
        found.sort_by_key(|m| m.sort_order);
        let ids: Vec<&str> = found.iter().map(|m| m.id.as_str()).collect();
        let (position, page) = page_of(&ids, args)?;
        let mut out = json!({
            "accountId": ACCOUNT_ID, "queryState": self.mailbox_state(), "canCalculateChanges": false,
            "position": position, "ids": page,
        });
        if args.get("calculateTotal").and_then(Value::as_bool) == Some(true) {
            out["total"] = json!(ids.len());
        }
        Ok(out)
    }

    fn mailbox_changes(&self, args: &Value) -> MResult {
        let c = changes(&self.mailbox_log, "ms", args)?;
        // Only counts ever change on a mailbox here, and saying so lets a client skip the
        // fields it already has.
        let props = if c.created.is_empty() && c.destroyed.is_empty() {
            json!(["totalEmails", "unreadEmails", "totalThreads", "unreadThreads"])
        } else {
            Value::Null
        };
        let mut out = changes_json(c);
        out["updatedProperties"] = props;
        Ok(out)
    }

    /// One message as `Email/get` gives it, with the header the message type does not know.
    fn email_value(&self, e: &Email, props: Option<&BTreeSet<String>>, body: &BodyOpts) -> Value {
        let mut value = e.json(props, body);
        for property in props.into_iter().flatten().filter(|p| p.to_ascii_lowercase().starts_with("header:")) {
            let lower = property.to_ascii_lowercase();
            let (unsubscribe, post) = self.unsubscribe.get(&e.id).cloned().unwrap_or_default();
            value[property.as_str()] = match lower.as_str() {
                "header:list-id:astext" => json!(self.list_ids.get(&e.id)),
                "header:list-unsubscribe:asurls" => json!(unsubscribe.as_deref().map(header_urls)),
                "header:list-unsubscribe-post:astext" => json!(post.as_deref().map(str::trim)),
                _ => Value::Null,
            };
        }
        value
    }

    fn email_get(&self, args: &Value) -> MResult {
        let ids = opt_strings(args.get("ids"))?;
        let props = prop_set(args)?;
        let body = BodyOpts::from_args(args)?;
        if self.bodies_fail && (body.fetch_text || body.fetch_html) {
            return Err(merr("serverFail", "the mock was told to fail body requests"));
        }
        // As Stalwart 1.1.0: this form is not known, and one unknown property fails the whole request.
        if let Some(p) = props.iter().flatten().find(|p| p.eq_ignore_ascii_case("header:list-unsubscribe:asText")) {
            return Err(merr("invalidArguments", format!("Invalid property {p}")));
        }
        let mut list = Vec::new();
        let mut not_found = Vec::new();
        match ids {
            None => {
                let mut all: Vec<&Email> = self.emails.values().collect();
                all.sort_by_key(|e| e.seq);
                list.extend(all.iter().map(|e| self.email_value(e, props.as_ref(), &body)));
            }
            Some(ids) => {
                for id in ids {
                    match self.emails.get(&id) {
                        Some(e) => list.push(self.email_value(e, props.as_ref(), &body)),
                        None => not_found.push(id),
                    }
                }
            }
        }
        Ok(json!({ "accountId": ACCOUNT_ID, "state": self.email_state(), "list": list, "notFound": not_found }))
    }

    fn thread_get(&self, args: &Value) -> MResult {
        let ids = opt_strings(args.get("ids"))?.unwrap_or_default();
        let mut list = Vec::new();
        let mut not_found = Vec::new();
        for id in ids {
            let mut members: Vec<&Email> = self.emails.values().filter(|e| e.thread_id() == id).collect();
            members.sort_by_key(|e| (e.received_at, e.seq));
            if members.is_empty() {
                not_found.push(id);
            } else {
                let email_ids: Vec<&str> = members.iter().map(|e| e.id.as_str()).collect();
                list.push(json!({ "id": id, "emailIds": email_ids }));
            }
        }
        Ok(json!({ "accountId": ACCOUNT_ID, "state": self.email_state(), "list": list, "notFound": not_found }))
    }

    fn email_matches(&self, e: &Email, filter: &Value) -> Result<bool, MethodError> {
        let obj = filter.as_object().ok_or_else(|| merr("unsupportedFilter", "a filter must be an object"))?;
        if let Some(op) = obj.get("operator").and_then(Value::as_str) {
            let conditions = obj
                .get("conditions")
                .and_then(Value::as_array)
                .ok_or_else(|| merr("invalidArguments", "operator without conditions"))?;
            let mut results = Vec::new();
            for c in conditions {
                results.push(self.email_matches(e, c)?);
            }
            return match op {
                "AND" => Ok(results.iter().all(|r| *r)),
                "OR" => Ok(results.iter().any(|r| *r)),
                "NOT" => Ok(!results.iter().any(|r| *r)),
                other => Err(merr("unsupportedFilter", format!("operator '{other}'"))),
            };
        }
        for (key, value) in obj {
            let s = value.as_str().unwrap_or("");
            let ok = match key.as_str() {
                "inMailbox" => e.mailbox_ids.contains(s),
                "inMailboxOtherThan" => {
                    let others = opt_strings(Some(value))?.unwrap_or_default();
                    e.mailbox_ids.iter().any(|m| !others.contains(m))
                }
                "hasKeyword" => e.keywords.contains(&s.to_lowercase()),
                "notKeyword" => !e.keywords.contains(&s.to_lowercase()),
                "before" => parse_rfc3339(s).is_some_and(|t| e.received_at < t),
                "after" => parse_rfc3339(s).is_some_and(|t| e.received_at >= t),
                "subject" => e.subject.to_lowercase().contains(&s.to_lowercase()),
                // Everything a person would look for: headers and both bodies.
                "text" => {
                    let s = s.to_lowercase();
                    let has = |t: &str| t.to_lowercase().contains(&s);
                    has(&e.subject)
                        || has(&e.from_name)
                        || has(&e.from_email)
                        || e.to.iter().any(|(name, email)| has(name) || has(email))
                        || e.text.as_deref().is_some_and(has)
                        || e.html.as_deref().is_some_and(has)
                }
                "from" => {
                    let s = s.to_lowercase();
                    e.from_name.to_lowercase().contains(&s) || e.from_email.to_lowercase().contains(&s)
                }
                other => return Err(merr("unsupportedFilter", format!("filter condition '{other}'"))),
            };
            if !ok {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn email_query(&self, args: &Value) -> MResult {
        let filter = args.get("filter").filter(|f| !f.is_null());
        let mut found: Vec<&Email> = Vec::new();
        for e in self.emails.values() {
            let ok = match filter {
                None => true,
                Some(f) => self.email_matches(e, f)?,
            };
            if ok {
                found.push(e);
            }
        }
        // (property, ascending) pairs; the default is newest first.
        let mut sorts: Vec<(String, bool)> = Vec::new();
        if let Some(Value::Array(list)) = args.get("sort") {
            for s in list {
                let prop = s.get("property").and_then(Value::as_str).unwrap_or("").to_string();
                let asc = s.get("isAscending").and_then(Value::as_bool).unwrap_or(true);
                if !["receivedAt", "sentAt", "subject", "from", "size"].contains(&prop.as_str()) {
                    return Err(merr("unsupportedSort", format!("sort property '{prop}'")));
                }
                sorts.push((prop, asc));
            }
        }
        if sorts.is_empty() {
            sorts.push(("receivedAt".into(), false));
        }
        found.sort_by(|a, b| {
            for (prop, asc) in &sorts {
                let ord = match prop.as_str() {
                    "subject" => a.subject.to_lowercase().cmp(&b.subject.to_lowercase()),
                    "from" => a.from_email.cmp(&b.from_email),
                    "size" => a.subject.len().cmp(&b.subject.len()),
                    _ => a.received_at.cmp(&b.received_at),
                };
                let ord = if *asc { ord } else { ord.reverse() };
                if ord.is_ne() {
                    return ord;
                }
            }
            // Equal on every key: arrival order, in the direction of the first key.
            let ord = a.seq.cmp(&b.seq);
            if sorts[0].1 {
                ord
            } else {
                ord.reverse()
            }
        });
        let ids: Vec<&str> = found.iter().map(|e| e.id.as_str()).collect();
        let (position, page) = page_of(&ids, args)?;
        let mut out = json!({
            "accountId": ACCOUNT_ID, "queryState": self.email_state(), "canCalculateChanges": false,
            "position": position, "ids": page,
        });
        if args.get("calculateTotal").and_then(Value::as_bool) == Some(true) {
            out["total"] = json!(ids.len());
        }
        Ok(out)
    }

    /// `SearchSnippet/get` as Stalwart 1.1.0 answers it (section 4 of the notes): only a text,
    /// subject or body condition marks anything; the words are marked wherever they are found,
    /// without regard to case; subject and preview are HTML-escaped with `<mark>` tags; the
    /// preview keeps a trailing space; a message with no hit has two nulls; unknown ids are
    /// in `notFound`.
    fn snippet_get(&self, args: &Value) -> MResult {
        let ids = opt_strings(args.get("emailIds"))?.unwrap_or_default();
        if self.get_max.is_some_and(|max| ids.len() > max) {
            return Err(merr("requestTooLarge", "too many emailIds"));
        }
        let mut words = Vec::new();
        if let Some(filter) = args.get("filter") {
            filter_words(filter, &mut words);
        }
        let mut list = Vec::new();
        let mut not_found = Vec::new();
        for id in ids {
            let Some(e) = self.emails.get(&id) else {
                not_found.push(id);
                continue;
            };
            let text = e.text.clone().unwrap_or_else(|| e.preview.clone());
            let preview: String = text.chars().take(255).collect();
            let subject = marked_html(&e.subject, &words);
            let preview = marked_html(&preview, &words).map(|p| format!("{p} "));
            list.push(json!({ "emailId": id, "subject": subject, "preview": preview }));
        }
        Ok(
            json!({ "accountId": ACCOUNT_ID, "list": list, "notFound": if not_found.is_empty() { Value::Null } else { json!(not_found) } }),
        )
    }

    fn email_changes(&self, args: &Value) -> MResult {
        Ok(changes_json(changes(&self.email_log, "es", args)?))
    }

    /// `Mailbox/set` as Stalwart 1.1.0 answers it (`dev/notes/stalwart-mail-features.md`): names
    /// are unique among siblings without regard to case, at most 255 bytes, nesting stops at ten
    /// levels, Inbox, Trash and Junk cannot be destroyed (Drafts and the rest can, when empty),
    /// roles cannot be changed, and role folders can be renamed and moved.
    fn mailbox_set(&mut self, args: &Value) -> MResult {
        let old_state = self.mailbox_state();
        if let Some(s) = args.get("ifInState").and_then(Value::as_str) {
            if s != old_state {
                return Err(merr("stateMismatch", "ifInState does not match"));
            }
        }
        let mut out = Map::new();
        out.insert("accountId".into(), json!(ACCOUNT_ID));
        out.insert("oldState".into(), json!(old_state));
        if let Some(Value::Object(create)) = args.get("create") {
            let (mut created, mut not_created) = (Map::new(), Map::new());
            for (key, object) in create {
                match self.create_mailbox(object) {
                    Ok(id) => {
                        created.insert(key.clone(), json!({"id": id, "totalEmails": 0, "unreadEmails": 0}));
                    }
                    Err(e) => {
                        not_created.insert(key.clone(), e);
                    }
                }
            }
            out.insert("created".into(), Value::Object(created));
            if !not_created.is_empty() {
                out.insert("notCreated".into(), Value::Object(not_created));
            }
        }
        if let Some(Value::Object(update)) = args.get("update") {
            let (mut updated, mut not_updated) = (Map::new(), Map::new());
            for (id, patch) in update {
                match self.update_mailbox(id, patch) {
                    Ok(()) => {
                        updated.insert(id.clone(), Value::Null);
                    }
                    Err(e) => {
                        not_updated.insert(id.clone(), e);
                    }
                }
            }
            out.insert("updated".into(), Value::Object(updated));
            if !not_updated.is_empty() {
                out.insert("notUpdated".into(), Value::Object(not_updated));
            }
        }
        if let Some(ids) = opt_strings(args.get("destroy"))? {
            let remove_emails = args.get("onDestroyRemoveEmails").and_then(Value::as_bool) == Some(true);
            let (mut destroyed, mut not_destroyed) = (Vec::new(), Map::new());
            for id in ids {
                match self.destroy_mailbox(&id, remove_emails) {
                    Ok(()) => destroyed.push(id),
                    Err(e) => {
                        not_destroyed.insert(id, e);
                    }
                }
            }
            out.insert("destroyed".into(), json!(destroyed));
            if !not_destroyed.is_empty() {
                out.insert("notDestroyed".into(), Value::Object(not_destroyed));
            }
        }
        out.insert("newState".into(), json!(self.mailbox_state()));
        Ok(Value::Object(out))
    }

    fn mailbox_depth(&self, id: &str) -> usize {
        let mut depth = 0;
        let mut at = Some(id.to_string());
        while let Some(current) = at {
            depth += 1;
            if depth > 100 {
                break;
            }
            at = self.mailboxes.iter().find(|m| m.id == current).and_then(|m| m.parent_id.clone());
        }
        depth
    }

    fn mailbox_height(&self, id: &str) -> usize {
        let below = self.mailboxes.iter().filter(|m| m.parent_id.as_deref() == Some(id));
        below.map(|m| 1 + self.mailbox_height(&m.id)).max().unwrap_or(0)
    }

    /// The sibling with this name (any case) other than `except`, as the SetError the server gives.
    fn name_taken(&self, parent: Option<&str>, name: &str, except: Option<&str>) -> Option<Value> {
        let wanted = name.to_lowercase();
        let found = self.mailboxes.iter().find(|m| {
            m.parent_id.as_deref() == parent && Some(m.id.as_str()) != except && m.name.to_lowercase() == wanted
        })?;
        Some(json!({
            "type": "alreadyExists",
            "description": format!("A mailbox with name '{name}' already exists."),
            "existingId": found.id,
        }))
    }

    fn check_mailbox_name(name: &str) -> Result<(), Value> {
        let bad = |why: &str| json!({"type": "invalidProperties", "description": why, "properties": ["name"]});
        if name.trim().is_empty() {
            return Err(bad("Mailbox name cannot be empty."));
        }
        if name.len() > 255 {
            return Err(bad("Mailbox name is too long."));
        }
        Ok(())
    }

    fn create_mailbox(&mut self, object: &Value) -> Result<String, Value> {
        let name = object.get("name").and_then(Value::as_str).unwrap_or_default().to_string();
        Self::check_mailbox_name(&name)?;
        let parent = object.get("parentId").and_then(Value::as_str).map(str::to_string);
        if let Some(p) = &parent {
            if !self.mailboxes.iter().any(|m| &m.id == p) {
                return Err(json!({"type": "invalidProperties", "description": "Parent ID does not exist."}));
            }
            if self.mailbox_depth(p) + 1 > 10 {
                return Err(json!({
                    "type": "invalidProperties",
                    "description": "Mailbox parent-child relationship is too deep.",
                    "properties": ["parentId"],
                }));
            }
        }
        if let Some(e) = self.name_taken(parent.as_deref(), &name, None) {
            return Err(e);
        }
        self.next_mailbox += 1;
        let id = format!("mb{}", self.next_mailbox);
        let sort_order = object.get("sortOrder").and_then(Value::as_u64).unwrap_or(0) as u32;
        self.mailboxes.push(MailboxRec { id: id.clone(), name, role: None, sort_order, parent_id: parent });
        self.log_mailbox(&id, Kind::Created);
        Ok(id)
    }

    fn update_mailbox(&mut self, id: &str, patch: &Value) -> Result<(), Value> {
        let Some(mut folder) = self.mailboxes.iter().find(|m| m.id == id).cloned() else {
            return Err(json!({"type": "notFound"}));
        };
        let patch = patch.as_object().ok_or(json!({"type": "invalidPatch"}))?;
        for (key, value) in patch {
            match key.as_str() {
                "name" => {
                    let name = value.as_str().ok_or(json!({
                        "type": "invalidProperties", "description": "Invalid property or value.", "properties": ["name"]
                    }))?;
                    Self::check_mailbox_name(name)?;
                    folder.name = name.to_string();
                }
                "parentId" => {
                    let parent = value.as_str().map(str::to_string);
                    if let Some(p) = &parent {
                        let bad = |why: &str| json!({"type": "invalidProperties", "description": why, "properties": ["parentId"]});
                        if p == id {
                            return Err(bad("Mailbox cannot be a parent of itself."));
                        }
                        if !self.mailboxes.iter().any(|m| &m.id == p) {
                            return Err(
                                json!({"type": "invalidProperties", "description": "Parent ID does not exist."}),
                            );
                        }
                        let mut up = Some(p.clone());
                        while let Some(current) = up {
                            if current == id {
                                return Err(bad("Mailbox cannot be a parent of itself."));
                            }
                            up = self.mailboxes.iter().find(|m| m.id == current).and_then(|m| m.parent_id.clone());
                        }
                        if self.mailbox_depth(p) + 1 + self.mailbox_height(id) > 10 {
                            return Err(bad("Mailbox parent-child relationship is too deep."));
                        }
                    }
                    folder.parent_id = parent;
                }
                "sortOrder" | "isSubscribed" => {}
                "role" => {
                    return Err(json!({
                        "type": "invalidProperties",
                        "description": "You are not allowed to change the role of Inbox, Junk or Trash folders.",
                        "properties": ["role"],
                    }))
                }
                other => {
                    return Err(
                        json!({"type": "invalidProperties", "description": "Invalid property or value.", "properties": [other]}),
                    )
                }
            }
        }
        if let Some(e) = self.name_taken(folder.parent_id.as_deref(), &folder.name, Some(id)) {
            return Err(e);
        }
        if let Some(slot) = self.mailboxes.iter_mut().find(|m| m.id == id) {
            *slot = folder;
        }
        self.log_mailbox(id, Kind::Updated);
        Ok(())
    }

    fn destroy_mailbox(&mut self, id: &str, remove_emails: bool) -> Result<(), Value> {
        let Some(folder) = self.mailboxes.iter().find(|m| m.id == id).cloned() else {
            return Err(json!({"type": "notFound"}));
        };
        if matches!(folder.role, Some("inbox" | "trash" | "junk")) {
            return Err(
                json!({"type": "forbidden", "description": "You are not allowed to delete Inbox, Junk or Trash folders."}),
            );
        }
        if self.mailboxes.iter().any(|m| m.parent_id.as_deref() == Some(id)) {
            return Err(json!({"type": "mailboxHasChild", "description": "Mailbox has at least one children."}));
        }
        let inside: Vec<String> =
            self.emails.values().filter(|e| e.mailbox_ids.contains(id)).map(|e| e.id.clone()).collect();
        if !inside.is_empty() && !remove_emails {
            return Err(json!({"type": "mailboxHasEmail", "description": "Mailbox is not empty."}));
        }
        for email in inside {
            let Some(mut e) = self.emails.get(&email).cloned() else { continue };
            e.mailbox_ids.remove(id);
            if e.mailbox_ids.is_empty() {
                self.destroy_internal(&email);
            } else {
                self.commit(e);
            }
        }
        self.mailboxes.retain(|m| m.id != id);
        self.log_mailbox(id, Kind::Destroyed);
        Ok(())
    }

    fn log_mailbox(&mut self, id: &str, kind: Kind) {
        let s = self.mailbox_log.bump();
        self.mailbox_log.entries.push((s, id.to_string(), kind));
        self.dirty_mailbox = true;
    }

    fn email_set(&mut self, args: &Value) -> MResult {
        let old_state = self.email_state();
        if let Some(s) = args.get("ifInState").and_then(Value::as_str) {
            if s != old_state {
                return Err(merr("stateMismatch", "ifInState does not match"));
            }
        }
        let mut out = Map::new();
        out.insert("accountId".into(), json!(ACCOUNT_ID));
        out.insert("oldState".into(), json!(old_state));

        if let Some(Value::Object(create)) = args.get("create") {
            let mut created = Map::new();
            let mut not_created = Map::new();
            for (key, object) in create {
                match self.create_email(object) {
                    Ok(made) => {
                        created.insert(key.clone(), made);
                    }
                    Err(e) => {
                        not_created.insert(key.clone(), e);
                    }
                }
            }
            out.insert("created".into(), Value::Object(created));
            if !not_created.is_empty() {
                out.insert("notCreated".into(), Value::Object(not_created));
            }
        }
        if let Some(Value::Object(update)) = args.get("update") {
            let mut updated = Map::new();
            let mut not_updated = Map::new();
            for (id, patch) in update {
                if let Some(left) = self.email_set_budget.as_mut() {
                    if *left == 0 {
                        not_updated
                            .insert(id.clone(), json!({"type": "serverFail", "description": "mock: out of budget"}));
                        continue;
                    }
                    *left -= 1;
                }
                match self.apply_patch(id, patch) {
                    Ok(()) => {
                        updated.insert(id.clone(), Value::Null);
                    }
                    Err(e) => {
                        not_updated.insert(id.clone(), e);
                    }
                }
            }
            out.insert("updated".into(), Value::Object(updated));
            if !not_updated.is_empty() {
                out.insert("notUpdated".into(), Value::Object(not_updated));
            }
        }
        if let Some(ids) = opt_strings(args.get("destroy"))? {
            let mut destroyed = Vec::new();
            let mut not_destroyed = Map::new();
            for id in ids {
                if self.destroy_internal(&id) {
                    destroyed.push(id);
                } else {
                    not_destroyed.insert(id, json!({"type": "notFound"}));
                }
            }
            out.insert("destroyed".into(), json!(destroyed));
            if !not_destroyed.is_empty() {
                out.insert("notDestroyed".into(), Value::Object(not_destroyed));
            }
        }
        out.insert("newState".into(), json!(self.email_state()));
        Ok(Value::Object(out))
    }

    /// One entry of `create`: a message built from header properties, a body structure and
    /// body values, the way a client writes a draft. The error is a JMAP SetError object.
    fn create_email(&mut self, object: &Value) -> Result<Value, Value> {
        let invalid =
            |prop: &str, why: &str| json!({"type": "invalidProperties", "properties": [prop], "description": why});
        let mailbox_ids = object.get("mailboxIds").and_then(flag_set).filter(|m| !m.is_empty());
        let mailbox_ids = mailbox_ids.ok_or_else(|| invalid("mailboxIds", "a message must be in a mailbox"))?;
        if let Some(bad) = mailbox_ids.iter().find(|m| !self.mailboxes.iter().any(|b| &b.id == *m)) {
            return Err(invalid("mailboxIds", &format!("no mailbox '{bad}'")));
        }
        let keywords = match object.get("keywords") {
            None | Some(Value::Null) => BTreeSet::new(),
            Some(v) => flag_set(v).ok_or_else(|| invalid("keywords", "expected an object of true values"))?,
        };
        let list = |key: &str| -> Result<Vec<(String, String)>, Value> {
            match object.get(key) {
                None | Some(Value::Null) => Ok(Vec::new()),
                Some(Value::Array(a)) => a
                    .iter()
                    .map(|x| {
                        let email = x["email"].as_str().ok_or_else(|| invalid(key, "an address needs an email"))?;
                        Ok((x["name"].as_str().unwrap_or_default().to_string(), email.to_string()))
                    })
                    .collect(),
                Some(_) => Err(invalid(key, "expected a list of addresses")),
            }
        };
        let ids = |key: &str| -> Vec<String> {
            object[key].as_array().into_iter().flatten().filter_map(|s| s.as_str().map(str::to_string)).collect()
        };
        let mut from = list("from")?;
        let (mut text, mut html, mut attachments) = (None, None, Vec::new());
        let values = object.get("bodyValues").cloned().unwrap_or(Value::Null);
        let mut parts = vec![object.get("bodyStructure").cloned().unwrap_or(Value::Null)];
        while let Some(part) = parts.pop() {
            parts.extend(part["subParts"].as_array().into_iter().flatten().rev().cloned());
            let mime = part["type"].as_str().unwrap_or("text/plain");
            if let Some(blob_id) = part["blobId"].as_str() {
                let Some((_, bytes)) = self.blob(blob_id) else {
                    return Err(json!({"type": "blobNotFound", "notFound": [blob_id]}));
                };
                attachments.push(Attachment {
                    blob_id: blob_id.to_string(),
                    name: part["name"].as_str().unwrap_or_default().to_string(),
                    mime_type: mime.to_string(),
                    cid: None,
                    inline: false,
                    bytes,
                });
            } else if let Some(value) = part["partId"].as_str().and_then(|p| values[p]["value"].as_str()) {
                match mime {
                    "text/html" => html = html.or(Some(value.to_string())),
                    _ => text = text.or(Some(value.to_string())),
                }
            }
        }
        let (from_name, from_email) = if from.is_empty() { Default::default() } else { from.remove(0) };
        let seq = self.next_seq;
        self.next_seq += 1;
        let id = format!("e{seq}");
        let in_reply_to = ids("inReplyTo");
        // An answer joins the thread of the message it answers.
        let thread = in_reply_to.first().and_then(|mid| {
            self.emails.values().find(|e| format!("{}@mock.example.test", e.id) == *mid).map(|e| e.thread_id())
        });
        let preview: String = text.iter().flat_map(|t| t.chars()).take(100).collect();
        let email = Email {
            id: id.clone(),
            seq,
            mailbox_ids: mailbox_ids.clone(),
            keywords,
            from_name,
            from_email,
            to: list("to")?,
            cc: list("cc")?,
            bcc: list("bcc")?,
            in_reply_to,
            references: ids("references"),
            thread,
            text,
            html,
            attachments,
            subject: object["subject"].as_str().unwrap_or_default().to_string(),
            preview,
            received_at: now_secs(),
        };
        let (thread_id, size) = (email.thread_id(), 1200 + email.subject.len() + email.preview.len());
        self.emails.insert(id.clone(), email);
        self.log_email(&id, Kind::Created);
        self.touch_mailboxes(mailbox_ids);
        Ok(json!({ "id": id, "blobId": format!("b{id}"), "threadId": thread_id, "size": size }))
    }

    /// `EmailSubmission/set`: the answer, and the `Email/set` answer for `onSuccessUpdateEmail`
    /// that a server adds after it. `identities` are the ids a message may be sent as.
    pub fn submission_set(
        &mut self,
        args: &Value,
        identities: &[String],
    ) -> Result<(Value, Option<Value>), MethodError> {
        if let Some(a) = args.get("accountId").and_then(Value::as_str) {
            if a != ACCOUNT_ID {
                return Err(merr("accountNotFound", format!("no account '{a}'")));
            }
        }
        if self.refusal == Some(Refusal::Temporary) {
            return Err(merr("serverFail", "the mock was told to fail submissions"));
        }
        let old_state = self.email_state();
        let mut created = Map::new();
        let mut not_created = Map::new();
        let mut updated = Map::new();
        let mut not_updated = Map::new();
        let mut destroyed = Vec::new();
        let mut not_destroyed = Map::new();
        for (key, object) in args.get("create").and_then(Value::as_object).into_iter().flatten() {
            match self.accept_submission(object, identities) {
                Ok(submission) => {
                    let now = now_ms();
                    created.insert(
                        key.clone(),
                        json!({
                            "id": submission.id,
                            "undoStatus": submission.undo_status(now),
                            "sendAt": rfc3339((submission.release_at_ms / 1000) as i64),
                        }),
                    );
                    let patch = &args["onSuccessUpdateEmail"][format!("#{key}")];
                    if !patch.is_null() {
                        // Done when the submission is made, held or not, as the real server does.
                        match self.apply_patch(&submission.email_id, patch) {
                            Ok(()) => {
                                updated.insert(submission.email_id.clone(), Value::Null);
                            }
                            Err(e) => return Err(merr("serverFail", format!("onSuccessUpdateEmail failed: {e}"))),
                        }
                    }
                    self.submissions.push(submission);
                }
                Err(e) => {
                    not_created.insert(key.clone(), e);
                }
            }
        }
        let now = now_ms();
        for (id, patch) in args.get("update").and_then(Value::as_object).into_iter().flatten() {
            let found = self.submissions.iter_mut().find(|s| s.id == *id && !s.destroyed);
            let Some(sub) = found else {
                not_updated.insert(id.clone(), json!({"type": "notFound"}));
                continue;
            };
            if patch.as_object().is_none_or(|p| p.len() != 1 || p.get("undoStatus") != Some(&json!("canceled"))) {
                let why = "Email submissions can only be cancelled.";
                not_updated.insert(
                    id.clone(),
                    json!({"type": "invalidProperties", "properties": ["undoStatus"], "description": why}),
                );
            } else if sub.undo_status(now) != "pending" {
                let why = "The requested message is no longer in the queue.";
                not_updated.insert(id.clone(), json!({"type": "cannotUnsend", "description": why}));
            } else {
                sub.cancelled = true;
                updated.insert(id.clone(), Value::Null);
            }
        }
        for id in args.get("destroy").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
            match self.submissions.iter_mut().find(|s| s.id == id && !s.destroyed) {
                Some(sub) => {
                    // Gone from the lists; a held message is still sent at its time.
                    sub.destroyed = true;
                    destroyed.push(json!(id));
                }
                None => {
                    not_destroyed.insert(id.to_string(), json!({"type": "notFound"}));
                }
            }
        }
        // Only the email answer follows from the emails that onSuccessUpdateEmail changed; the
        // submission answer's own `updated` holds the cancelled ones.
        let email_updated: Map<String, Value> = updated
            .iter()
            .filter(|(id, _)| self.emails.contains_key(*id))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let mut out = json!({
            "accountId": ACCOUNT_ID, "oldState": "ss0", "newState": "ss1", "created": created,
        });
        for (key, map) in [
            ("notCreated", not_created),
            ("updated", updated.clone()),
            ("notUpdated", not_updated),
            ("notDestroyed", not_destroyed),
        ] {
            if !map.is_empty() {
                out[key] = Value::Object(map);
            }
        }
        if !destroyed.is_empty() {
            out["destroyed"] = Value::Array(destroyed);
        }
        let follow = (!email_updated.is_empty()).then(|| {
            json!({ "accountId": ACCOUNT_ID, "oldState": old_state, "newState": self.email_state(), "updated": email_updated })
        });
        Ok((out, follow))
    }

    fn accept_submission(&self, object: &Value, identities: &[String]) -> Result<Submission, Value> {
        if let Some(Refusal::Permanent(kind)) = &self.refusal {
            return Err(json!({"type": kind, "description": format!("the mock refuses submissions: {kind}")}));
        }
        let invalid =
            |prop: &str, why: &str| json!({"type": "invalidProperties", "properties": [prop], "description": why});
        let identity_id = object["identityId"].as_str().unwrap_or_default();
        if !identities.iter().any(|i| i == identity_id) {
            return Err(invalid("identityId", "no such identity"));
        }
        let email_id = object["emailId"].as_str().unwrap_or_default();
        let email = self.emails.get(email_id).ok_or_else(|| invalid("emailId", "no such message"))?;
        let all = [&email.to, &email.cc, &email.bcc];
        let mut recipients: Vec<String> = all.into_iter().flatten().map(|(_, e)| e.clone()).collect();
        let now = now_ms();
        let mut hold = None;
        let mut release_at_ms = now;
        let envelope = &object["envelope"];
        if !envelope.is_null() {
            let rcpt: Vec<String> = envelope["rcptTo"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|r| r["email"].as_str().map(str::to_string))
                .collect();
            if !rcpt.is_empty() {
                recipients = rcpt;
            }
            let forbidden = |why: &str| json!({"type": "forbiddenMailFrom", "description": format!("Server rejected MAIL-FROM: 501 5.5.4 {why}")});
            let params = envelope["mailFrom"]["parameters"].as_object().cloned().unwrap_or_default();
            if params.contains_key("HOLDFOR") && params.contains_key("HOLDUNTIL") {
                return Err(forbidden("Only one of HOLDFOR or HOLDUNTIL may be specified."));
            }
            for (name, value) in &params {
                let Some(text) = value.as_str() else {
                    let why = format!("Failed to parse mailFrom parameters: Unsupported parameter: {name}.");
                    return Err(invalid("envelope", &why));
                };
                let bad = || invalid("envelope", &format!("Invalid parameter: {name}."));
                match name.as_str() {
                    "HOLDFOR" => {
                        let secs: u64 = text.parse().ok().filter(|s| *s > 0).ok_or_else(bad)?;
                        if secs > 604_800 {
                            return Err(forbidden("Requested hold time exceeds maximum of 604800 seconds."));
                        }
                        release_at_ms = now + secs * 1000;
                    }
                    "HOLDUNTIL" => {
                        let at = chrono::DateTime::parse_from_rfc3339(text).map_err(|_| bad())?.timestamp();
                        let at_ms = u64::try_from(at).map_err(|_| bad())? * 1000;
                        if at_ms <= now {
                            return Err(forbidden("HOLDUNTIL must be a date and time in the future."));
                        }
                        if at_ms > now + 604_800_000 {
                            return Err(forbidden("Requested release time exceeds maximum."));
                        }
                        release_at_ms = at_ms;
                    }
                    other => {
                        return Err(invalid("envelope", &format!("Unsupported parameter: {other}={text}.")));
                    }
                }
                hold = Some((name.clone(), text.to_string()));
            }
        }
        if recipients.is_empty() {
            return Err(json!({"type": "noRecipients"}));
        }
        Ok(Submission {
            id: format!("s{}", self.submissions.len() + 1),
            identity_id: identity_id.to_string(),
            email_id: email_id.to_string(),
            recipients,
            hold,
            release_at_ms,
            cancelled: false,
            destroyed: false,
        })
    }

    /// `EmailSubmission/query`: the filters of the notes (`emailIds`, `undoStatus`, `identityIds`);
    /// a sort by `sendAt` is not supported, as on the real server.
    pub fn submission_query(&self, args: &Value) -> MResult {
        for sort in args["sort"].as_array().into_iter().flatten() {
            let property = sort["property"].as_str().unwrap_or_default();
            if !["emailId", "threadId", "sentAt"].contains(&property) {
                return Err(merr("unsupportedSort", property.to_string()));
            }
        }
        let filter = args["filter"].as_object().cloned().unwrap_or_default();
        let strings = |key: &str| -> Option<Vec<String>> {
            filter
                .get(key)
                .map(|v| v.as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect())
        };
        if let Some(bad) =
            filter.keys().find(|k| !["emailIds", "undoStatus", "identityIds", "threadIds"].contains(&k.as_str()))
        {
            return Err(merr("unsupportedFilter", bad.clone()));
        }
        let (emails, identities) = (strings("emailIds"), strings("identityIds"));
        let status = filter.get("undoStatus").and_then(Value::as_str);
        let now = now_ms();
        let ids: Vec<&str> = self
            .submissions
            .iter()
            .filter(|s| !s.destroyed)
            .filter(|s| emails.as_ref().is_none_or(|e| e.contains(&s.email_id)))
            .filter(|s| identities.as_ref().is_none_or(|i| i.contains(&s.identity_id)))
            .filter(|s| status.is_none_or(|st| s.undo_status(now) == st))
            .map(|s| s.id.as_str())
            .collect();
        Ok(
            json!({ "accountId": ACCOUNT_ID, "queryState": "q", "canCalculateChanges": true, "position": 0, "ids": ids }),
        )
    }

    /// `EmailSubmission/get`.
    pub fn submission_get(&self, args: &Value) -> MResult {
        let wanted: Option<Vec<&str>> =
            args["ids"].as_array().map(|ids| ids.iter().filter_map(Value::as_str).collect());
        let now = now_ms();
        let mut list = Vec::new();
        let mut not_found = Vec::new();
        let visible = |s: &&Submission| !s.destroyed;
        match &wanted {
            None => {
                for s in self.submissions.iter().filter(visible) {
                    list.push(self.submission_json(s, now));
                }
            }
            Some(ids) => {
                for id in ids {
                    match self.submissions.iter().filter(visible).find(|s| s.id == *id) {
                        Some(s) => list.push(self.submission_json(s, now)),
                        None => not_found.push(json!(id)),
                    }
                }
            }
        }
        Ok(json!({ "accountId": ACCOUNT_ID, "state": "ss1", "list": list, "notFound": not_found }))
    }

    fn submission_json(&self, s: &Submission, now: u64) -> Value {
        let recipients: Vec<Value> = s.recipients.iter().map(|e| json!({ "email": e })).collect();
        json!({
            "id": s.id, "identityId": s.identity_id, "emailId": s.email_id,
            "threadId": self.emails.get(&s.email_id).map(|e| e.thread_id()),
            "envelope": { "mailFrom": { "email": USERNAME, "parameters": null }, "rcptTo": recipients },
            "sendAt": rfc3339((s.release_at_ms / 1000) as i64),
            "undoStatus": s.undo_status(now),
        })
    }

    /// One entry of `update`: whole-property replacement and `a/b` patch paths. The error is
    /// a JMAP SetError object.
    fn apply_patch(&mut self, id: &str, patch: &Value) -> Result<(), Value> {
        let invalid =
            |prop: &str, why: &str| json!({"type": "invalidProperties", "properties": [prop], "description": why});
        let mut e = self.emails.get(id).cloned().ok_or(json!({"type": "notFound"}))?;
        let patch = patch.as_object().ok_or(json!({"type": "invalidPatch"}))?;
        for (key, value) in patch {
            match key.as_str() {
                "keywords" => {
                    let all =
                        flag_set(value).ok_or_else(|| invalid("keywords", "expected an object of true values"))?;
                    e.keywords = BTreeSet::new();
                    for keyword in all {
                        add_keyword(&mut e.keywords, &keyword, &invalid)?;
                    }
                }
                "mailboxIds" => {
                    e.mailbox_ids =
                        flag_set(value).ok_or_else(|| invalid("mailboxIds", "expected an object of true values"))?
                }
                k => {
                    let (target, name) = if let Some(n) = k.strip_prefix("keywords/") {
                        (&mut e.keywords, n)
                    } else if let Some(n) = k.strip_prefix("mailboxIds/") {
                        (&mut e.mailbox_ids, n)
                    } else {
                        return Err(invalid(k, "the mock cannot change this property"));
                    };
                    let name = name.replace("~1", "/").replace("~0", "~");
                    match value {
                        Value::Bool(true) if k.starts_with("keywords/") => {
                            add_keyword(target, &name, &invalid)?;
                        }
                        Value::Bool(true) => {
                            target.insert(name);
                        }
                        // The spec says null removes; jmap-client sends false.
                        Value::Bool(false) | Value::Null => {
                            target.remove(&name.to_lowercase());
                            target.remove(&name);
                        }
                        _ => return Err(invalid(k, "expected true or null")),
                    }
                }
            }
        }
        if e.mailbox_ids.is_empty() {
            return Err(invalid("mailboxIds", "a message must be in at least one mailbox"));
        }
        if let Some(bad) = e.mailbox_ids.iter().find(|m| !self.mailboxes.iter().any(|b| &b.id == *m)) {
            return Err(invalid("mailboxIds", &format!("no mailbox '{bad}'")));
        }
        self.commit(e);
        Ok(())
    }
}

/// The keywords of a message as Stalwart 1.1.0 takes them (`dev/notes/stalwart-mail-features.md`,
/// section 3): ASCII only and no space, tab, `( ) { % * " \\ ]`, at most 128 bytes, lower-cased,
/// at most 100 on a message.
fn add_keyword(
    keywords: &mut BTreeSet<String>,
    keyword: &str,
    invalid: &dyn Fn(&str, &str) -> Value,
) -> Result<(), Value> {
    let bad_char = |c: char| !c.is_ascii() || c.is_ascii_control() || " (){%*\"\\]".contains(c);
    if keyword.is_empty() || keyword.chars().any(bad_char) {
        return Err(invalid("keywords", "Invalid keyword."));
    }
    if keyword.len() > 128 {
        return Err(invalid("keywords", "Keywords cannot be longer than 128 bytes."));
    }
    keywords.insert(keyword.to_lowercase());
    if keywords.len() > 100 {
        return Err(json!({"type": "tooManyKeywords", "description": "An email cannot have more than 100 keywords.",
            "properties": ["keywords"]}));
    }
    Ok(())
}

/// The addresses in the brackets of a List-Unsubscribe header, comments left out.
fn header_urls(raw: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = raw;
    while let Some(open) = rest.find('<') {
        let Some(close) = rest[open..].find('>') else { break };
        out.push(rest[open + 1..open + close].trim().to_string());
        rest = &rest[open + close + 1..];
    }
    out
}

/// The words a snippet filter looks for: those of its `text`, `subject` and `body` conditions
/// (quotes dropped), inside AND and OR, not inside NOT.
fn filter_words(filter: &Value, words: &mut Vec<String>) {
    match filter["operator"].as_str() {
        Some("NOT") => {}
        Some(_) => filter["conditions"].as_array().into_iter().flatten().for_each(|c| filter_words(c, words)),
        None => {
            for key in ["text", "subject", "body"] {
                if let Some(text) = filter[key].as_str() {
                    words.extend(text.replace('"', " ").split_whitespace().map(str::to_lowercase));
                }
            }
        }
    }
}

/// `text` HTML-escaped with `<mark>` around each word (matched without regard to case); `None`
/// when no word is in it.
fn marked_html(text: &str, words: &[String]) -> Option<String> {
    let lower: Vec<char> = text.chars().flat_map(char::to_lowercase).collect();
    let chars: Vec<char> = text.chars().collect();
    // Only text whose letters keep their count when lower-cased can be marked by position.
    if lower.len() != chars.len() {
        return None;
    }
    let mut hit = vec![false; chars.len()];
    for word in words.iter().filter(|w| !w.is_empty()) {
        let word: Vec<char> = word.chars().collect();
        for start in 0..chars.len().saturating_sub(word.len() - 1) {
            if lower[start..start + word.len()] == word[..] {
                hit[start..start + word.len()].iter_mut().for_each(|h| *h = true);
            }
        }
    }
    if !hit.iter().any(|h| *h) {
        return None;
    }
    let mut out = String::new();
    let mut inside = false;
    for (c, h) in chars.iter().zip(&hit) {
        if *h != inside {
            out.push_str(if *h { "<mark>" } else { "</mark>" });
            inside = *h;
        }
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(*c),
        }
    }
    if inside {
        out.push_str("</mark>");
    }
    Some(out)
}

fn flag_set(v: &Value) -> Option<BTreeSet<String>> {
    let mut out = BTreeSet::new();
    for (k, v) in v.as_object()? {
        match v {
            Value::Bool(true) => {
                out.insert(k.clone());
            }
            Value::Bool(false) | Value::Null => {}
            _ => return None,
        }
    }
    Some(out)
}

fn opt_strings(v: Option<&Value>) -> Result<Option<Vec<String>>, MethodError> {
    match v {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Array(a)) => a
            .iter()
            .map(|x| {
                x.as_str().map(str::to_string).ok_or_else(|| merr("invalidArguments", "expected an array of strings"))
            })
            .collect::<Result<Vec<_>, _>>()
            .map(Some),
        Some(_) => Err(merr("invalidArguments", "expected an array of strings")),
    }
}

fn prop_set(args: &Value) -> Result<Option<BTreeSet<String>>, MethodError> {
    Ok(opt_strings(args.get("properties"))?.map(|v| v.into_iter().collect()))
}

fn mailbox_filter(m: &MailboxRec, f: &Value) -> Result<bool, MethodError> {
    let obj = f.as_object().ok_or_else(|| merr("unsupportedFilter", "a filter must be an object"))?;
    if obj.contains_key("operator") {
        return Err(merr("unsupportedFilter", "the mock only supports plain Mailbox filters"));
    }
    for (key, value) in obj {
        let ok = match key.as_str() {
            "role" => match value {
                Value::Null => m.role.is_none(),
                v => v.as_str() == m.role,
            },
            "hasAnyRole" => value.as_bool() == Some(m.role.is_some()),
            "name" => value.as_str() == Some(m.name.as_str()),
            "parentId" => value.is_null(),
            other => return Err(merr("unsupportedFilter", format!("Mailbox filter '{other}'"))),
        };
        if !ok {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Applies `position` and `limit` to a full result list.
fn page_of(ids: &[&str], args: &Value) -> Result<(usize, Vec<String>), MethodError> {
    if args.get("anchor").is_some_and(|a| !a.is_null()) {
        return Err(merr("invalidArguments", "the mock does not support anchor"));
    }
    let len = ids.len() as i64;
    let pos = args.get("position").and_then(Value::as_i64).unwrap_or(0);
    let start = if pos < 0 { (len + pos).max(0) } else { pos.min(len) } as usize;
    let limit = args.get("limit").and_then(Value::as_u64).map(|l| l as usize).unwrap_or(usize::MAX);
    let page = ids.iter().skip(start).take(limit).map(|s| s.to_string()).collect();
    Ok((start, page))
}

fn changes(log: &Log, prefix: &str, args: &Value) -> Result<ChangesOut, MethodError> {
    let since_str = args
        .get("sinceState")
        .and_then(Value::as_str)
        .ok_or_else(|| merr("invalidArguments", "sinceState is required"))?;
    let max = match args.get("maxChanges") {
        None | Some(Value::Null) => None,
        Some(v) => match v.as_u64() {
            Some(n) if n > 0 => Some(n as usize),
            _ => return Err(merr("invalidArguments", "maxChanges must be a positive integer")),
        },
    };
    let since = since_str
        .strip_prefix(prefix)
        .and_then(|n| n.parse::<u64>().ok())
        .filter(|s| *s >= log.floor && *s <= log.state)
        .ok_or_else(|| merr("cannotCalculateChanges", format!("state '{since_str}' is unknown or forgotten")))?;

    // Per id, in order of first appearance, with the kinds folded together.
    let mut folded: Vec<(String, Kind)> = Vec::new();
    let mut new_state = since;
    let mut has_more = false;
    let mut i = log.entries.iter().position(|(s, _, _)| *s > since).unwrap_or(log.entries.len());
    while i < log.entries.len() {
        let state = log.entries[i].0;
        let mut j = i;
        while j < log.entries.len() && log.entries[j].0 == state {
            j += 1;
        }
        let group = &log.entries[i..j];
        if let Some(max) = max {
            let fresh = group.iter().filter(|(_, id, _)| !folded.iter().any(|(f, _)| f == id)).count();
            if !folded.is_empty() && folded.len() + fresh > max {
                has_more = true;
                break;
            }
        }
        for (_, id, kind) in group {
            match folded.iter().position(|(f, _)| f == id) {
                None => folded.push((id.clone(), *kind)),
                Some(p) => {
                    let merged = match (folded[p].1, *kind) {
                        (Kind::Created, Kind::Updated) => Some(Kind::Created),
                        (Kind::Created, Kind::Destroyed) => None,
                        (Kind::Updated, Kind::Destroyed) => Some(Kind::Destroyed),
                        (Kind::Destroyed, Kind::Created) => Some(Kind::Updated),
                        (_, k) => Some(k),
                    };
                    match merged {
                        Some(k) => folded[p].1 = k,
                        None => {
                            folded.remove(p);
                        }
                    }
                }
            }
        }
        new_state = state;
        i = j;
    }
    if !has_more {
        new_state = log.state;
    }
    let pick = |k: Kind| folded.iter().filter(|(_, kind)| *kind == k).map(|(id, _)| id.clone()).collect();
    Ok(ChangesOut {
        old_state: since_str.to_string(),
        new_state: format!("{prefix}{new_state}"),
        has_more,
        created: pick(Kind::Created),
        updated: pick(Kind::Updated),
        destroyed: pick(Kind::Destroyed),
    })
}

fn changes_json(c: ChangesOut) -> Value {
    json!({
        "accountId": ACCOUNT_ID,
        "oldState": c.old_state,
        "newState": c.new_state,
        "hasMoreChanges": c.has_more,
        "created": c.created,
        "updated": c.updated,
        "destroyed": c.destroyed,
    })
}

// ---- result references -------------------------------------------------------------

/// Replaces every `#name: {resultOf, name, path}` argument with the value it points at in an
/// earlier response of the same request.
pub fn resolve_refs(args: &Value, prior: &[(String, Value, String)]) -> Result<Value, MethodError> {
    let Some(obj) = args.as_object() else { return Ok(args.clone()) };
    let mut out = Map::new();
    for (k, v) in obj {
        let Some(name) = k.strip_prefix('#') else {
            out.insert(k.clone(), v.clone());
            continue;
        };
        let bad = |why: &str| merr("invalidResultReference", why.to_string());
        let (of, method, path) = (
            v.get("resultOf").and_then(Value::as_str).ok_or_else(|| bad("missing resultOf"))?,
            v.get("name").and_then(Value::as_str).ok_or_else(|| bad("missing name"))?,
            v.get("path").and_then(Value::as_str).ok_or_else(|| bad("missing path"))?,
        );
        let target = prior
            .iter()
            .find(|(m, _, id)| id == of && m == method)
            .ok_or_else(|| bad("no earlier call with that id and name"))?;
        if out.contains_key(name) || obj.contains_key(name) {
            return Err(merr("invalidArguments", format!("'{name}' given both directly and as a reference")));
        }
        out.insert(name.to_string(), eval_path(&target.1, path).ok_or_else(|| bad("path not found"))?);
    }
    Ok(Value::Object(out))
}

fn eval_path(root: &Value, path: &str) -> Option<Value> {
    fn walk(cur: &Value, segs: &[&str], out: &mut Vec<Value>) -> bool {
        match segs.split_first() {
            None => {
                out.push(cur.clone());
                true
            }
            Some((&"*", rest)) => match cur {
                Value::Array(a) => {
                    for x in a {
                        walk(x, rest, out);
                    }
                    true
                }
                _ => false,
            },
            Some((seg, rest)) => {
                let seg = seg.replace("~1", "/").replace("~0", "~");
                match cur {
                    Value::Object(m) => m.get(&seg).is_some_and(|x| walk(x, rest, out)),
                    Value::Array(a) => {
                        seg.parse::<usize>().ok().and_then(|i| a.get(i)).is_some_and(|x| walk(x, rest, out))
                    }
                    _ => false,
                }
            }
        }
    }
    let segs: Vec<&str> = path.strip_prefix('/')?.split('/').collect();
    let wildcard = segs.contains(&"*");
    let mut out = Vec::new();
    if !walk(root, &segs, &mut out) {
        return None;
    }
    if wildcard {
        // A wildcard flattens the arrays it finds, so `/list/*/mailboxIds` style paths work.
        let flat = out
            .into_iter()
            .flat_map(|v| match v {
                Value::Array(a) => a,
                other => vec![other],
            })
            .collect();
        Some(Value::Array(flat))
    } else {
        out.into_iter().next()
    }
}
