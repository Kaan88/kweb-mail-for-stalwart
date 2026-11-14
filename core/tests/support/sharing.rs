//! What the mock knows about other users sharing mail folders, calendars and address books with
//! the signed-in user (see `dev/notes/stalwart-sharing.md`). Each of the accounts `sh1`, `sh2`
//! and `sh3` (the ones of `support/files.rs`) holds its own mail, calendar and address book,
//! independently of the others and of the user's own account: their ids repeat the own
//! account's on purpose (a message `e1`, a calendar `cal1`, an event `ev1`, a card `c1`), so
//! code that mixes accounts up gives a wrong answer instead of the right one.
//!
//! It behaves like Stalwart 1.1.0 where that matters to the core:
//!
//! - An account is in the session (`isPersonal: false`) only while it is "listed". An account
//!   that is not listed answers every method with `accountNotFound`. A listed account that
//!   shares no mail, calendar or address book answers that kind's methods with
//!   `accountNotSupportedByMethod`. A test may also make one kind answer with any error.
//! - `Mailbox/get` gives only the folders the user may read, with `myRights`; `Email/query` and
//!   `Email/get` give only messages in such folders (a message elsewhere is `notFound`).
//! - Every refused write is a `forbidden` set error with Stalwart's own text.
//! - The calendar and the contacts are the mock's own (`support/calendar.rs`, `account.rs`),
//!   one of each per account.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{json, Map, Value};

use super::{
    account::Account,
    calendar::Calendar,
    state::{now_secs, rfc3339, MethodError},
};

pub const MAIL_URN: &str = "urn:ietf:params:jmap:mail";
pub const CALENDARS_URN: &str = "urn:ietf:params:jmap:calendars";
pub const CONTACTS_URN: &str = "urn:ietf:params:jmap:contacts";

/// Every right a folder may have, for a share that gives all of them.
pub const ALL_MAIL_RIGHTS: [&str; 5] =
    ["mayReadItems", "mayAddItems", "mayRemoveItems", "maySetSeen", "maySetKeywords"];

struct Folder {
    id: String,
    name: String,
    parent: Option<String>,
    /// The rights the user has, by their JMAP names. Without `mayReadItems` the folder is not
    /// shared with the user at all.
    rights: BTreeSet<&'static str>,
}

struct Message {
    id: String,
    folders: BTreeSet<String>,
    keywords: BTreeSet<String>,
    from: (String, String),
    subject: String,
    received_at: i64,
    text: Option<String>,
    html: Option<String>,
    /// `(blob id, name, type, bytes)`
    files: Vec<(String, String, String, Vec<u8>)>,
}

#[derive(Default)]
struct SharedMail {
    folders: Vec<Folder>,
    messages: BTreeMap<String, Message>,
    version: u64,
}

struct Acct {
    name: &'static str,
    /// In the session, and answering at all.
    listed: bool,
    mail: Option<SharedMail>,
    calendar: Option<Calendar>,
    contacts: Option<Account>,
}

pub struct Sharing {
    accounts: BTreeMap<String, Acct>,
    /// `(account, area)` answers every method of the area (`mail`, `calendar`, `contacts`) with
    /// this error type.
    refusals: BTreeMap<(String, &'static str), &'static str>,
    /// Accounts whose rights were taken back after the user read them: every write to a
    /// calendar or address book of theirs is `forbidden`, whatever the rights read say.
    pub deny_writes: BTreeSet<String>,
    /// `(account, method)` of every call that reached a shared account, in order.
    pub calls: Vec<(String, String)>,
}

fn merr(kind: &'static str, description: impl Into<String>) -> MethodError {
    MethodError { kind, description: description.into() }
}

impl Default for Sharing {
    fn default() -> Self {
        let acct = |name| Acct { name, listed: false, mail: None, calendar: None, contacts: None };
        let accounts = [
            (super::SHARED_ACCOUNT, acct("shared@kweb.test")),
            (super::EMPTY_SHARE_ACCOUNT, acct("empty@kweb.test")),
            (super::MAIL_ONLY_ACCOUNT, acct("mail@kweb.test")),
        ];
        Sharing {
            accounts: accounts.into_iter().map(|(id, a)| (id.to_string(), a)).collect(),
            refusals: BTreeMap::new(),
            deny_writes: BTreeSet::new(),
            calls: Vec::new(),
        }
    }
}

impl Sharing {
    fn acct(&mut self, account: &str) -> &mut Acct {
        self.accounts.get_mut(account).unwrap_or_else(|| panic!("the mock has no account {account}"))
    }

    pub fn knows(&self, account: &str) -> bool {
        self.accounts.contains_key(account)
    }

    /// Whether any account shares something: the mock then speaks of sharing in its session.
    pub fn any_listed(&self) -> bool {
        self.accounts.values().any(|a| a.listed)
    }

    /// Puts the account in the session or takes it out. Taken out, it answers `accountNotFound`.
    pub fn set_listed(&mut self, account: &str, listed: bool) {
        self.acct(account).listed = listed;
    }

    pub fn share_mail(&mut self, account: &str) -> &mut Self {
        let a = self.acct(account);
        a.listed = true;
        a.mail.get_or_insert_with(SharedMail::default);
        self
    }

    pub fn unshare_mail(&mut self, account: &str) {
        self.acct(account).mail = None;
    }

    /// The account shares calendars; the calendars of `Calendar::default()` are cleared so the
    /// test adds the ones it wants with [`Sharing::calendar`].
    pub fn share_calendars(&mut self, account: &str) -> &mut Self {
        let a = self.acct(account);
        a.listed = true;
        a.calendar.get_or_insert_with(|| {
            let mut calendar = Calendar::default();
            calendar.clear_calendars();
            calendar
        });
        self
    }

    pub fn unshare_calendars(&mut self, account: &str) {
        self.acct(account).calendar = None;
    }

    pub fn share_contacts(&mut self, account: &str) -> &mut Self {
        let a = self.acct(account);
        a.listed = true;
        a.contacts.get_or_insert_with(|| {
            let mut contacts = Account::default();
            contacts.clear_address_books();
            contacts
        });
        self
    }

    pub fn unshare_contacts(&mut self, account: &str) {
        self.acct(account).contacts = None;
    }

    /// The calendar the account shares, to add calendars and events to.
    pub fn calendar(&mut self, account: &str) -> &mut Calendar {
        self.acct(account).calendar.as_mut().unwrap_or_else(|| panic!("{account} shares no calendar"))
    }

    /// The contacts the account shares, to add address books and cards to.
    pub fn contacts(&mut self, account: &str) -> &mut Account {
        self.acct(account).contacts.as_mut().unwrap_or_else(|| panic!("{account} shares no contacts"))
    }

    /// Every method of the area (`mail`, `calendar` or `contacts`) of the account answers with
    /// this error type; `None` takes it back.
    pub fn refuse(&mut self, account: &str, area: &'static str, kind: Option<&'static str>) {
        match kind {
            Some(kind) => self.refusals.insert((account.to_string(), area), kind),
            None => self.refusals.remove(&(account.to_string(), area)),
        };
    }

    fn mail(&mut self, account: &str) -> &mut SharedMail {
        self.acct(account).mail.as_mut().unwrap_or_else(|| panic!("{account} shares no mail"))
    }

    pub fn add_folder(&mut self, account: &str, id: &str, name: &str, parent: Option<&str>, rights: &[&'static str]) {
        self.mail(account).folders.push(Folder {
            id: id.into(),
            name: name.into(),
            parent: parent.map(str::to_string),
            rights: rights.iter().copied().collect(),
        });
    }

    pub fn set_folder_rights(&mut self, account: &str, id: &str, rights: &[&'static str]) {
        let mail = self.mail(account);
        if let Some(folder) = mail.folders.iter_mut().find(|f| f.id == id) {
            folder.rights = rights.iter().copied().collect();
        }
    }

    /// A message in a folder, `age` seconds old, unread, with a plain text body.
    pub fn add_message(&mut self, account: &str, id: &str, folder: &str, subject: &str, age: i64) {
        let message = Message {
            id: id.into(),
            folders: [folder.to_string()].into(),
            keywords: BTreeSet::new(),
            from: ("Owner".into(), "owner@shared.test".into()),
            subject: subject.into(),
            received_at: now_secs() - age,
            text: Some(format!("Body of {subject}")),
            html: None,
            files: Vec::new(),
        };
        let mail = self.mail(account);
        mail.version += 1;
        mail.messages.insert(id.into(), message);
    }

    pub fn set_message_bodies(&mut self, account: &str, id: &str, text: Option<&str>, html: Option<&str>) {
        if let Some(m) = self.mail(account).messages.get_mut(id) {
            (m.text, m.html) = (text.map(str::to_string), html.map(str::to_string));
        }
    }

    /// Attaches a file to a message; the blob is downloadable from the account.
    pub fn add_message_file(&mut self, account: &str, id: &str, name: &str, media_type: &str, bytes: &[u8]) -> String {
        let mail = self.mail(account);
        mail.version += 1;
        let blob = format!("sb-{account}-{id}-{}", mail.version);
        if let Some(m) = mail.messages.get_mut(id) {
            m.files.push((blob.clone(), name.into(), media_type.into(), bytes.to_vec()));
        }
        blob
    }

    pub fn mark_message(&mut self, account: &str, id: &str, keyword: &str, set: bool) {
        if let Some(m) = self.mail(account).messages.get_mut(id) {
            if set {
                m.keywords.insert(keyword.into());
            } else {
                m.keywords.remove(keyword);
            }
        }
    }

    pub fn remove_message(&mut self, account: &str, id: &str) {
        self.mail(account).messages.remove(id);
    }

    pub fn keywords_of(&mut self, account: &str, id: &str) -> Option<Vec<String>> {
        self.mail(account).messages.get(id).map(|m| m.keywords.iter().cloned().collect())
    }

    pub fn folders_of(&mut self, account: &str, id: &str) -> Option<Vec<String>> {
        self.mail(account).messages.get(id).map(|m| m.folders.iter().cloned().collect())
    }

    pub fn message_exists(&mut self, account: &str, id: &str) -> bool {
        self.mail(account).messages.contains_key(id)
    }

    /// A blob of an attachment of the account's messages.
    pub fn blob(&self, account: &str, blob: &str) -> Option<(String, Vec<u8>)> {
        let mail = self.accounts.get(account)?.mail.as_ref()?;
        let files = mail.messages.values().flat_map(|m| m.files.iter());
        files.into_iter().find(|f| f.0 == blob).map(|f| (f.2.clone(), f.3.clone()))
    }

    /// The accounts the session lists, as `(id, account object)`.
    pub fn session_accounts(&self) -> Vec<(String, Value)> {
        self.accounts
            .iter()
            .filter(|(_, a)| a.listed)
            .map(|(id, a)| {
                let caps = json!({ MAIL_URN: {}, CALENDARS_URN: {}, CONTACTS_URN: {} });
                (
                    id.clone(),
                    json!({ "name": a.name, "isPersonal": false, "isReadOnly": false, "accountCapabilities": caps }),
                )
            })
            .collect()
    }

    /// Answers a method that names the account `account` (not the user's own).
    pub fn call(&mut self, account: &str, name: &str, args: &Value) -> Result<Value, MethodError> {
        self.calls.push((account.to_string(), name.to_string()));
        let refusals = self.refusals.clone();
        let denied = self.deny_writes.contains(account);
        let Some(a) = self.accounts.get_mut(account) else {
            return Err(merr("accountNotFound", "The accountId does not correspond to a valid account"));
        };
        if !a.listed {
            return Err(merr("accountNotFound", "The accountId does not correspond to a valid account"));
        }
        let not_supported = || {
            merr(
                "accountNotSupportedByMethod",
                "The accountId given corresponds to a valid account, but the account does not support this method or data type.",
            )
        };
        let area = match name {
            "Mailbox/get" | "Email/get" | "Email/query" | "Email/set" => "mail",
            "Calendar/get" | "CalendarEvent/get" | "CalendarEvent/query" | "CalendarEvent/set" => "calendar",
            "AddressBook/get" | "ContactCard/query" | "ContactCard/get" | "ContactCard/set" => "contacts",
            other => return Err(merr("unknownMethod", format!("the mock does not implement '{other}'"))),
        };
        if let Some(kind) = refusals.get(&(account.to_string(), area)) {
            return Err(merr(kind, "the mock was told to refuse this"));
        }
        match area {
            "mail" => a.mail.as_mut().ok_or_else(not_supported)?.call(account, name, args),
            "calendar" => {
                let calendar = a.calendar.as_mut().ok_or_else(not_supported)?;
                if name == "CalendarEvent/set" {
                    // The rights are the calendar's: an event is written only where it is.
                    let (allowed, refused) = guard_set(
                        args,
                        "calendarIds",
                        &|id| calendar.event(id),
                        &|c| !denied && calendar.writable(c),
                        ["add calendar events to calendar", "modify this event", "remove events from calendar"],
                    );
                    return merge_refused(calendar.call(name, &allowed)?, refused);
                }
                calendar.call(name, args)
            }
            _ => {
                let contacts = a.contacts.as_mut().ok_or_else(not_supported)?;
                if name == "ContactCard/set" {
                    let (allowed, refused) = guard_set(
                        args,
                        "addressBookIds",
                        &|id| contacts.card(id),
                        &|b| !denied && contacts.book_writable(b),
                        ["add contacts to address book", "modify address book", "remove contacts from address book"],
                    );
                    return merge_refused(contacts.call(name, &allowed)?, refused);
                }
                contacts.call(name, args)
            }
        }
    }
}

/// What a `/set` of events or cards may do given where its objects are: the request without
/// what is refused, and the set errors of what was (`notCreated`, `notUpdated`,
/// `notDestroyed`). `container` is the property that names the calendars or address books,
/// `words` the end of the refusal texts for create, update and destroy.
fn guard_set(
    args: &Value,
    container: &str,
    object: &dyn Fn(&str) -> Option<Value>,
    writable: &dyn Fn(&str) -> bool,
    words: [&str; 3],
) -> (Value, [Map<String, Value>; 3]) {
    let forbidden = |what: &str, place: &str| json!({ "type": "forbidden", "description": format!("You are not allowed to {what} {place}.") });
    let first_closed = |holder: &Value| {
        holder[container].as_object().into_iter().flatten().map(|(c, _)| c.clone()).find(|c| !writable(c))
    };
    let mut allowed = args.clone();
    let mut refused = [Map::new(), Map::new(), Map::new()];
    let mut keep = Map::new();
    for (cid, spec) in args["create"].as_object().into_iter().flatten() {
        match first_closed(spec) {
            Some(place) => {
                refused[0].insert(cid.clone(), forbidden(words[0], &place));
            }
            None => {
                keep.insert(cid.clone(), spec.clone());
            }
        }
    }
    allowed["create"] = Value::Object(keep);
    let mut keep = Map::new();
    for (id, patch) in args["update"].as_object().into_iter().flatten() {
        let closed = object(id).and_then(|o| first_closed(&o));
        if closed.is_some() {
            refused[1].insert(id.clone(), forbidden(words[1], ""));
        } else {
            keep.insert(id.clone(), patch.clone());
        }
    }
    allowed["update"] = Value::Object(keep);
    let mut keep = Vec::new();
    for id in args["destroy"].as_array().into_iter().flatten().filter_map(Value::as_str) {
        match object(id).and_then(|o| first_closed(&o)) {
            Some(place) => {
                refused[2].insert(id.to_string(), forbidden(words[2], &place));
            }
            None => keep.push(json!(id)),
        }
    }
    allowed["destroy"] = Value::Array(keep);
    (allowed, refused)
}

/// The answer of a `/set` with the refusals put in.
fn merge_refused(mut answer: Value, refused: [Map<String, Value>; 3]) -> Result<Value, MethodError> {
    for (key, errors) in ["notCreated", "notUpdated", "notDestroyed"].into_iter().zip(refused) {
        if errors.is_empty() {
            continue;
        }
        if !answer[key].is_object() {
            answer[key] = json!({});
        }
        for (id, e) in errors {
            answer[key][id] = e;
        }
    }
    Ok(answer)
}

fn strings(v: &Value) -> Option<Vec<String>> {
    v.as_array().map(|a| a.iter().filter_map(|s| s.as_str().map(str::to_string)).collect())
}

fn picked(all: Map<String, Value>, props: &Option<Vec<String>>) -> Value {
    let Some(props) = props else { return Value::Object(all) };
    let mut out = Map::new();
    out.insert("id".into(), all["id"].clone());
    for (key, value) in all {
        if props.contains(&key) {
            out.insert(key, value);
        }
    }
    Value::Object(out)
}

impl SharedMail {
    fn state(&self) -> String {
        format!("sm{}", self.version)
    }

    fn may(&self, folder: &str, right: &str) -> bool {
        self.folders.iter().find(|f| f.id == folder).is_some_and(|f| f.rights.contains(right))
    }

    fn readable(&self, m: &Message) -> bool {
        m.folders.iter().any(|f| self.may(f, "mayReadItems"))
    }

    fn counts(&self, folder: &str) -> (usize, usize) {
        let inside: Vec<&Message> = self.messages.values().filter(|m| m.folders.contains(folder)).collect();
        (inside.len(), inside.iter().filter(|m| !m.keywords.contains("$seen")).count())
    }

    fn call(&mut self, account: &str, name: &str, args: &Value) -> Result<Value, MethodError> {
        match name {
            "Mailbox/get" => Ok(self.mailbox_get(account, args)),
            "Email/query" => Ok(self.email_query(account, args)),
            "Email/get" => Ok(self.email_get(account, args)),
            _ => Ok(self.email_set(account, args)),
        }
    }

    fn mailbox_get(&self, account: &str, args: &Value) -> Value {
        let props = strings(&args["properties"]);
        let wanted = strings(&args["ids"]);
        let (mut list, mut not_found) = (Vec::new(), Vec::new());
        let shown = self.folders.iter().filter(|f| f.rights.contains("mayReadItems"));
        let mut shown: Vec<&Folder> = shown.collect();
        if let Some(ids) = &wanted {
            for id in ids {
                if !shown.iter().any(|f| f.id == *id) {
                    not_found.push(id.clone());
                }
            }
            shown.retain(|f| ids.contains(&f.id));
        }
        for f in shown {
            let (total, unread) = self.counts(&f.id);
            let rights: Map<String, Value> = [
                "mayReadItems",
                "mayAddItems",
                "mayRemoveItems",
                "maySetSeen",
                "maySetKeywords",
                "mayCreateChild",
                "mayRename",
                "maySubmit",
                "mayDelete",
                "mayShare",
                "maySetMetadata",
            ]
            .iter()
            .map(|r| (r.to_string(), json!(f.rights.contains(r))))
            .collect();
            let mut all = Map::new();
            all.insert("id".into(), json!(f.id));
            all.insert("name".into(), json!(f.name));
            all.insert("parentId".into(), json!(f.parent));
            all.insert("role".into(), Value::Null);
            all.insert("totalEmails".into(), json!(total));
            all.insert("unreadEmails".into(), json!(unread));
            all.insert("isSubscribed".into(), json!(false));
            all.insert("myRights".into(), Value::Object(rights));
            list.push(picked(all, &props));
        }
        json!({ "accountId": account, "state": self.state(), "list": list, "notFound": not_found })
    }

    fn email_query(&self, account: &str, args: &Value) -> Value {
        let folder = args["filter"]["inMailbox"].as_str();
        let mut found: Vec<&Message> = self
            .messages
            .values()
            .filter(|m| self.readable(m) && folder.is_none_or(|f| m.folders.contains(f) && self.may(f, "mayReadItems")))
            .collect();
        found.sort_by(|a, b| b.received_at.cmp(&a.received_at).then(b.id.cmp(&a.id)));
        let position = args["position"].as_u64().unwrap_or(0) as usize;
        let limit = args["limit"].as_u64().map_or(usize::MAX, |l| l as usize);
        let ids: Vec<&str> = found.iter().skip(position).take(limit).map(|m| m.id.as_str()).collect();
        let mut answer = json!({
            "accountId": account, "queryState": self.state(), "canCalculateChanges": false,
            "position": position, "ids": ids,
        });
        if args["calculateTotal"] == true {
            answer["total"] = json!(found.len());
        }
        answer
    }

    fn email_get(&self, account: &str, args: &Value) -> Value {
        let props = strings(&args["properties"]);
        let (text, html) = (args["fetchTextBodyValues"] == true, args["fetchHTMLBodyValues"] == true);
        let (mut list, mut not_found) = (Vec::new(), Vec::new());
        for id in strings(&args["ids"]).unwrap_or_default() {
            match self.messages.get(&id).filter(|m| self.readable(m)) {
                Some(m) => list.push(picked(self.message_json(m), &props).then_values(m, text, html)),
                None => not_found.push(id),
            }
        }
        json!({ "accountId": account, "state": self.state(), "list": list, "notFound": not_found })
    }

    fn message_json(&self, m: &Message) -> Map<String, Value> {
        let address = |(name, email): &(String, String)| json!([{ "name": name, "email": email }]);
        let mut parts = Vec::new();
        let part = |id: &str, kind: &str, size: usize| json!({ "partId": id, "blobId": format!("body-{}-{id}", m.id), "size": size, "type": kind });
        let text = m.text.as_ref().map(|t| part("1", "text/plain", t.len()));
        let html = m.html.as_ref().map(|t| part("2", "text/html", t.len()));
        let files: Vec<Value> = m
            .files
            .iter()
            .enumerate()
            .map(|(i, (blob, name, kind, bytes))| {
                json!({ "partId": format!("{}", 3 + i), "blobId": blob, "size": bytes.len(), "name": name,
                        "type": kind, "disposition": "attachment" })
            })
            .collect();
        parts.extend(text.iter().chain(html.iter()).cloned());
        let text_body: Vec<Value> = text.clone().or_else(|| html.clone()).into_iter().collect();
        let html_body: Vec<Value> = html.clone().or_else(|| text.clone()).into_iter().collect();
        let keywords: Map<String, Value> = m.keywords.iter().map(|k| (k.clone(), json!(true))).collect();
        let folders: Map<String, Value> = m.folders.iter().map(|f| (f.clone(), json!(true))).collect();
        let mut all = Map::new();
        for (key, value) in [
            ("id", json!(m.id)),
            ("blobId", json!(format!("whole-{}", m.id))),
            ("threadId", json!(format!("t-{}", m.id))),
            ("mailboxIds", Value::Object(folders)),
            ("keywords", Value::Object(keywords)),
            ("from", address(&m.from)),
            ("to", json!([{ "name": "Me", "email": "me@kweb.test" }])),
            ("cc", json!([])),
            ("bcc", json!([])),
            ("replyTo", json!([])),
            ("subject", json!(m.subject)),
            ("messageId", json!([format!("m-{}@shared.test", m.id)])),
            ("inReplyTo", json!([])),
            ("references", json!([format!("root-{}@shared.test", m.id)])),
            ("preview", json!(format!("Preview of {}", m.subject))),
            ("receivedAt", json!(rfc3339(m.received_at))),
            ("sentAt", json!(rfc3339(m.received_at - 60))),
            ("hasAttachment", json!(!files.is_empty())),
            ("size", json!(500 + m.subject.len())),
            ("textBody", json!(text_body)),
            ("htmlBody", json!(html_body)),
            ("attachments", json!(files)),
        ] {
            all.insert(key.to_string(), value);
        }
        all
    }

    fn email_set(&mut self, account: &str, args: &Value) -> Value {
        let old = self.state();
        let (mut updated, mut not_updated) = (Map::new(), Map::new());
        for (id, patch) in args["update"].as_object().into_iter().flatten() {
            match self.update(id, patch) {
                Ok(()) => {
                    updated.insert(id.clone(), Value::Null);
                }
                Err(e) => {
                    not_updated.insert(id.clone(), e);
                }
            }
        }
        let (mut destroyed, mut not_destroyed) = (Vec::new(), Map::new());
        for id in strings(&args["destroy"]).unwrap_or_default() {
            match self.destroy(&id) {
                Ok(()) => destroyed.push(id),
                Err(e) => {
                    not_destroyed.insert(id, e);
                }
            }
        }
        let mut answer = json!({ "accountId": account, "oldState": old, "newState": self.state() });
        for (key, map) in [("updated", updated), ("notUpdated", not_updated), ("notDestroyed", not_destroyed)] {
            if !map.is_empty() {
                answer[key] = Value::Object(map);
            }
        }
        if !destroyed.is_empty() {
            answer["destroyed"] = json!(destroyed);
        }
        answer
    }

    fn visible(&self, id: &str) -> Result<&Message, Value> {
        self.messages.get(id).filter(|m| self.readable(m)).ok_or_else(|| json!({ "type": "notFound" }))
    }

    fn forbidden(text: String) -> Value {
        json!({ "type": "forbidden", "description": text })
    }

    fn update(&mut self, id: &str, patch: &Value) -> Result<(), Value> {
        let message = self.visible(id)?;
        let mut keywords = message.keywords.clone();
        let mut folders = message.folders.clone();
        let current = message.folders.clone();
        for (path, value) in patch.as_object().into_iter().flatten() {
            if let Some(keyword) = path.strip_prefix("keywords/") {
                let right = if keyword == "$seen" { "maySetSeen" } else { "maySetKeywords" };
                if !current.iter().all(|f| self.may(f, right)) {
                    return Err(Self::forbidden("You are not allowed to modify keywords.".into()));
                }
                if value.is_null() {
                    keywords.remove(keyword);
                } else {
                    keywords.insert(keyword.to_string());
                }
            } else if path == "mailboxIds" {
                let target: BTreeSet<String> = value
                    .as_object()
                    .into_iter()
                    .flatten()
                    .filter(|(_, v)| **v == true)
                    .map(|(k, _)| k.clone())
                    .collect();
                for gone in current.difference(&target) {
                    if !self.may(gone, "mayRemoveItems") {
                        return Err(Self::forbidden(format!(
                            "You are not allowed to delete messages from mailbox {gone}"
                        )));
                    }
                }
                for added in target.difference(&current) {
                    if !self.folders.iter().any(|f| f.id == *added && f.rights.contains("mayReadItems")) {
                        return Err(
                            json!({ "type": "invalidProperties", "description": format!("Mailbox {added} does not exist."), "properties": ["mailboxIds"] }),
                        );
                    }
                    if !self.may(added, "mayAddItems") {
                        return Err(Self::forbidden(format!(
                            "You are not allowed to add messages to mailbox {added}."
                        )));
                    }
                }
                folders = target;
            } else {
                return Err(
                    json!({ "type": "invalidPatch", "description": "Unsupported patch.", "properties": [path] }),
                );
            }
        }
        if let Some(m) = self.messages.get_mut(id) {
            m.keywords = keywords;
            m.folders = folders;
        }
        self.version += 1;
        Ok(())
    }

    fn destroy(&mut self, id: &str) -> Result<(), Value> {
        let message = self.visible(id)?;
        if !message.folders.iter().all(|f| self.may(f, "mayRemoveItems")) {
            return Err(Self::forbidden("You are not allowed to delete this message.".into()));
        }
        self.messages.remove(id);
        self.version += 1;
        Ok(())
    }
}

/// Fills in the body values of a message JSON that was cut to the properties asked for.
trait ThenValues {
    fn then_values(self, m: &Message, text: bool, html: bool) -> Value;
}

impl ThenValues for Value {
    fn then_values(mut self, m: &Message, text: bool, html: bool) -> Value {
        let mut values = Map::new();
        if let (true, Some(t)) = (text, &m.text) {
            values.insert("1".into(), json!({ "value": t }));
        }
        if let (true, Some(t)) = (html, &m.html) {
            values.insert("2".into(), json!({ "value": t }));
        }
        let wants_values = self.get("textBody").is_some() || self.get("htmlBody").is_some();
        if wants_values && (text || html) {
            self["bodyValues"] = Value::Object(values);
        }
        self
    }
}
