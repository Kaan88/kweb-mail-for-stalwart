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
        put("bcc", Value::Null);
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
    /// `Email/get` that asks for body values fails (see `MockServer::set_bodies_fail`).
    pub bodies_fail: bool,
    tx: broadcast::Sender<StreamMsg>,
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
            bodies_fail: false,
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
        all.into_iter().find(|a| a.blob_id == blob_id).map(|a| (a.mime_type.clone(), a.bytes.clone()))
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
        let seq = self.next_seq;
        self.next_seq += 1;
        let id = format!("e{seq}");
        let inbox = self.mailbox_id_by_role("inbox").expect("seeded");
        self.emails.insert(
            id.clone(),
            Email {
                id: id.clone(),
                seq,
                mailbox_ids: set_of(&[&inbox]),
                keywords: BTreeSet::new(),
                from_name: from_name.into(),
                from_email: from_email.into(),
                to: vec![("Alice Test".into(), USERNAME.into())],
                cc: Vec::new(),
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
        self.touch_mailboxes(set_of(&[&inbox]));
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
            "Email/get" => self.email_get(args),
            "Email/query" => self.email_query(args),
            "Email/changes" => self.email_changes(args),
            "Email/set" => self.email_set(args),
            "Thread/get" => self.thread_get(args),
            other => Err(merr("unknownMethod", format!("the mock does not implement '{other}'"))),
        };
        self.flush();
        result
    }

    fn mailbox_json(&self, m: &MailboxRec, props: Option<&BTreeSet<String>>) -> Value {
        let (total, unread) = self.counts(&m.id);
        let all: [(&str, Value); 10] = [
            ("name", json!(m.name)),
            ("parentId", Value::Null),
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

    fn email_get(&self, args: &Value) -> MResult {
        let ids = opt_strings(args.get("ids"))?;
        let props = prop_set(args)?;
        let body = BodyOpts::from_args(args)?;
        if self.bodies_fail && (body.fetch_text || body.fetch_html) {
            return Err(merr("serverFail", "the mock was told to fail body requests"));
        }
        let mut list = Vec::new();
        let mut not_found = Vec::new();
        match ids {
            None => {
                let mut all: Vec<&Email> = self.emails.values().collect();
                all.sort_by_key(|e| e.seq);
                list.extend(all.iter().map(|e| e.json(props.as_ref(), &body)));
            }
            Some(ids) => {
                for id in ids {
                    match self.emails.get(&id) {
                        Some(e) => list.push(e.json(props.as_ref(), &body)),
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
                "hasKeyword" => e.keywords.contains(s),
                "notKeyword" => !e.keywords.contains(s),
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

    fn email_changes(&self, args: &Value) -> MResult {
        Ok(changes_json(changes(&self.email_log, "es", args)?))
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
            let not_created: Map<String, Value> = create
                .keys()
                .map(|k| {
                    (
                        k.clone(),
                        json!({"type": "forbidden", "description": "the mock does not create mail through Email/set"}),
                    )
                })
                .collect();
            if !not_created.is_empty() {
                out.insert("notCreated".into(), Value::Object(not_created));
            }
        }
        if let Some(Value::Object(update)) = args.get("update") {
            let mut updated = Map::new();
            let mut not_updated = Map::new();
            for (id, patch) in update {
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
                    e.keywords =
                        flag_set(value).ok_or_else(|| invalid("keywords", "expected an object of true values"))?
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
                        Value::Bool(true) => {
                            target.insert(name);
                        }
                        // The spec says null removes; jmap-client sends false.
                        Value::Bool(false) | Value::Null => {
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
