//! What other users share with the signed-in user: mail folders, calendars and address books
//! in their accounts. What Stalwart 1.1.0 really does is in `dev/notes/stalwart-sharing.md`;
//! the points that shape this code:
//!
//! - A shared account is in the session with `isPersonal: false`, and only while something in
//!   it is shared with a read right. The session cannot say what: an account that shares only
//!   a folder answers `accountNotSupportedByMethod` to a calendar question, and one whose
//!   share was taken back `accountNotFound` to every question.
//! - Ids are only unique within one account, so every call here names the account, and nothing
//!   is remembered between calls or written to the cache.
//! - A forbidden write is a `forbidden` set error with the server's own text; the words the
//!   user reads are chosen from the action, not from that text.
//! - The sharee's marks (`$seen`, `$flagged`) are the owner's too: there is no reading state
//!   of one's own.
//!
//! The reading code is the one the user's own account uses (conversion of mail, the agenda with
//! its repeating events, contacts); it takes the account as an argument.

use jmap_client::{
    core::query::Filter,
    email::{self, query::Comparator},
};
use serde_json::{json, Value};

use super::{
    calendars::CALENDARS_CAPABILITY,
    contacts::CONTACTS_CAPABILITY,
    errors::{from_jmap, is_no_account},
    send::{response, set_error},
    Connection, MAIL_CAPABILITY,
};
use crate::{
    contacts::{select, Indexed},
    model::{
        Agenda, Calendar, ContactDetail, ContactRow, EventDetail, MessageHeader, SharedAccount, SharedAction,
        SharedMailbox, SharedMessage, SharedMessages,
    },
    Error, Result,
};
use chrono_tz::Tz;

const UNSHARED: &str = "this is no longer shared with you";
const MESSAGE_GONE: &str = "that message is no longer there";
const FOLDER_UNSHARED: &str = "that folder is not shared with you";
/// The most messages one page of a shared folder holds.
const MAX_PAGE: usize = 100;
/// Answers to "what do you share" that mean "nothing of this kind".
const NOT_SHARED: [&str; 4] = ["accountNotSupportedByMethod", "accountNotFound", "forbidden", "accountNoAccess"];

/// A call that failed because the account is not there for the user (any more) is `Invalid`.
fn unshared(e: Error) -> Error {
    if is_no_account(&e) {
        Error::invalid(UNSHARED)
    } else {
        e
    }
}

impl Connection {
    /// Whether the session this connection was made with lists `account` as someone else's.
    fn cached_shared(&self, account: &str) -> bool {
        self.client.session().account(account).is_some_and(|a| !a.is_personal())
    }

    /// Makes sure the user can name `account`: a non-personal account of the session. A share
    /// given after the connection was made is not in its session, so the server is asked once
    /// more; one taken back since is caught by the server's own answer (`unshared`).
    pub(super) async fn shared_check(&self, account: &str) -> Result<()> {
        if self.cached_shared(account) {
            return Ok(());
        }
        let session = self.session_json().await?;
        if is_shared_in(&session, account) {
            Ok(())
        } else {
            Err(Error::invalid(UNSHARED))
        }
    }

    /// The accounts of other users that share mail, calendars or contacts, by name. The session
    /// is read afresh, so a share given or taken back since the connection was made shows.
    pub async fn shared_accounts(&self) -> Result<Vec<SharedAccount>> {
        let session = self.session_json().await?;
        let mut found = Vec::new();
        for (id, name) in other_accounts(&session) {
            if id == self.account_id {
                continue;
            }
            let [has_mail, has_calendars, has_contacts] = self.shares(&id).await?;
            if has_mail || has_calendars || has_contacts {
                found.push(SharedAccount { account_id: id, name, has_mail, has_calendars, has_contacts });
            }
        }
        found.sort_by_cached_key(|a| (a.name.to_lowercase(), a.account_id.clone()));
        Ok(found)
    }

    /// Whether the account shares mail, calendars and contacts, in that order: one request with
    /// the smallest question for each kind the server has. An answer that says "not here" is a
    /// no; any other failure is the caller's.
    async fn shares(&self, account: &str) -> Result<[bool; 3]> {
        let kinds = [
            (MAIL_CAPABILITY, "Mailbox/get"),
            (CALENDARS_CAPABILITY, "Calendar/get"),
            (CONTACTS_CAPABILITY, "AddressBook/get"),
        ];
        let asked: Vec<(usize, &str, &str)> = kinds
            .iter()
            .enumerate()
            .filter(|(_, (c, _))| self.has_capability(c))
            .map(|(i, (c, m))| (i, *c, *m))
            .collect();
        let capabilities: Vec<&str> = asked.iter().map(|(_, c, _)| *c).collect();
        let calls: Vec<(&str, Value)> =
            asked.iter().map(|(_, _, m)| (*m, json!({ "accountId": account, "properties": ["id"] }))).collect();
        let responses = self.call_keeping_errors(&capabilities, &calls).await?;
        let mut out = [false; 3];
        for ((kind, _, _), (name, answer)) in asked.iter().zip(&responses) {
            out[*kind] = if name == "error" {
                match answer["type"].as_str().unwrap_or("unknown") {
                    t if NOT_SHARED.contains(&t) => false,
                    t => return Err(Error::temporary(format!("asking {account} what it shares failed: {t}"))),
                }
            } else {
                answer["list"].as_array().is_some_and(|l| !l.is_empty())
            };
        }
        Ok(out)
    }

    // ---- mail ----

    /// The mail folders `account` shares, with what the user may do in each.
    pub async fn shared_mailboxes(&self, account: &str) -> Result<Vec<SharedMailbox>> {
        self.shared_check(account).await?;
        let args = json!({
            "accountId": account,
            "properties": ["name", "parentId", "totalEmails", "unreadEmails", "myRights"],
        });
        let responses = self.call(&[MAIL_CAPABILITY], &[("Mailbox/get", args)]).await.map_err(unshared)?;
        Ok(mailboxes_from(response(&responses, "Mailbox/get")?))
    }

    /// One page of a shared folder, newest first, and how many messages it has in all.
    pub async fn shared_messages(
        &self,
        account: &str,
        mailbox_id: &str,
        offset: u32,
        limit: u32,
    ) -> Result<SharedMessages> {
        self.shared_check(account).await?;
        let limit = (limit as usize).min(MAX_PAGE);
        let position = i32::try_from(offset).map_err(|_| Error::invalid("message position too large"))?;
        let mut req = self.client.build();
        req.query_email()
            .account_id(account)
            .filter(Filter::from(email::query::Filter::in_mailbox(mailbox_id)))
            .sort([Comparator::received_at().descending()])
            .position(position)
            .limit(limit)
            .calculate_total(true);
        let mut answer = req.send_query_email().await.map_err(from_jmap).map_err(unshared)?;
        let total = answer.total();
        let ids = answer.take_ids();
        let messages: Vec<MessageHeader> =
            if ids.is_empty() { Vec::new() } else { self.messages_in(account, &ids).await.map_err(unshared)?.0 };
        let total = total.unwrap_or(offset as usize + messages.len());
        Ok(SharedMessages { messages, total: u32::try_from(total).unwrap_or(u32::MAX) })
    }

    /// A shared message with its body, read as the user's own are, and left as it is: it is not
    /// marked read and nothing is kept.
    pub async fn shared_message(&self, account: &str, message_id: &str) -> Result<SharedMessage> {
        self.shared_check(account).await?;
        let ids = [message_id.to_string()];
        let header = self.messages_in(account, &ids).await.map_err(unshared)?.0.pop();
        let header = header.ok_or_else(|| Error::invalid(MESSAGE_GONE))?;
        let body = self.bodies_in(account, &ids).await.map_err(unshared)?.pop();
        Ok(SharedMessage { header, body: body.ok_or_else(|| Error::invalid(MESSAGE_GONE))? })
    }

    /// Does `action` to a message of the account now.
    pub async fn shared_act(&self, account: &str, message_id: &str, action: &SharedAction) -> Result<()> {
        self.shared_check(account).await?;
        if let SharedAction::Move { mailbox_id } = action {
            self.shared_folder_exists(account, mailbox_id).await?;
        }
        let mut args = json!({ "accountId": account });
        match action_patch(action) {
            Some(patch) => args["update"] = json!({ message_id: patch }),
            None => args["destroy"] = json!([message_id]),
        }
        let responses = self.call(&[MAIL_CAPABILITY], &[("Email/set", args)]).await.map_err(unshared)?;
        set_outcome(response(&responses, "Email/set")?, message_id, action)
    }

    /// Whether `mailbox_id` is a folder of the account that the user can see. A folder that is
    /// not shared (also one that has the id of a folder of the user's own) is `Invalid`.
    async fn shared_folder_exists(&self, account: &str, mailbox_id: &str) -> Result<()> {
        let args = json!({ "accountId": account, "ids": [mailbox_id], "properties": ["id"] });
        let responses = self.call(&[MAIL_CAPABILITY], &[("Mailbox/get", args)]).await.map_err(unshared)?;
        let list = &response(&responses, "Mailbox/get")?["list"];
        if list.as_array().is_some_and(|l| !l.is_empty()) {
            Ok(())
        } else {
            Err(Error::invalid(FOLDER_UNSHARED))
        }
    }

    // ---- calendars ----

    pub async fn shared_calendars(&self, account: &str) -> Result<Vec<Calendar>> {
        self.shared_check(account).await?;
        self.calendars_in(account).await.map_err(unshared)
    }

    pub async fn shared_agenda(&self, account: &str, from: i64, to: i64, zone: Tz) -> Result<Agenda> {
        self.shared_check(account).await?;
        let (_, entries) = self.agenda_in(account, from, to, zone).await.map_err(unshared)?;
        Ok(Agenda { entries, from_cache: false })
    }

    /// An event of a shared calendar. This stage only shows it, so the user may not change it.
    pub async fn shared_event(
        &self,
        account: &str,
        event_id: &str,
        recurrence_id: Option<&str>,
    ) -> Result<EventDetail> {
        self.shared_check(account).await?;
        let mut detail = self.event_detail_in(account, event_id, recurrence_id).await.map_err(unshared)?;
        detail.may_write = false;
        Ok(detail)
    }

    // ---- contacts ----

    /// The contacts of the account's shared address books that `query` finds, by name.
    pub async fn shared_contacts(&self, account: &str, query: &str) -> Result<Vec<ContactRow>> {
        self.shared_check(account).await?;
        let mut cards = self.contact_cards_in(account).await.map_err(unshared)?;
        cards.sort_by(|a, b| (&a.sort_key, &a.id).cmp(&(&b.sort_key, &b.id)));
        let index = cards.into_iter().map(|e| Indexed { row: e.row, search: e.search, digits: e.digits }).collect();
        Ok(select(index, query))
    }

    /// A contact of a shared address book. Shown only.
    pub async fn shared_contact(&self, account: &str, contact_id: &str) -> Result<ContactDetail> {
        self.shared_check(account).await?;
        let entry = self.contact_card_in(account, contact_id).await.map_err(unshared)?;
        Ok(ContactDetail { id: entry.id, contact: entry.contact, may_write: false })
    }
}

/// The accounts of a session that belong to other users, as `(id, name)`.
fn other_accounts(session: &Value) -> Vec<(String, String)> {
    let accounts = session["accounts"].as_object().into_iter().flatten();
    accounts
        .filter(|(_, a)| a["isPersonal"].as_bool() == Some(false))
        .map(|(id, a)| (id.clone(), a["name"].as_str().unwrap_or_default().to_string()))
        .collect()
}

fn is_shared_in(session: &Value, account: &str) -> bool {
    session["accounts"][account]["isPersonal"].as_bool() == Some(false)
}

/// The folders of a `Mailbox/get` answer, by name. A parent that is not in the answer is not
/// named: the user cannot see it.
fn mailboxes_from(get: &Value) -> Vec<SharedMailbox> {
    let list: Vec<&Value> = get["list"].as_array().into_iter().flatten().collect();
    let known: Vec<&str> = list.iter().filter_map(|m| m["id"].as_str()).collect();
    let count = |m: &Value, key: &str| u32::try_from(m[key].as_u64().unwrap_or(0)).unwrap_or(u32::MAX);
    let mut out: Vec<SharedMailbox> = list
        .iter()
        .filter_map(|m| {
            let rights = |name: &str| m["myRights"][name].as_bool().unwrap_or(false);
            Some(SharedMailbox {
                id: m["id"].as_str()?.to_string(),
                name: m["name"].as_str().unwrap_or_default().to_string(),
                parent_id: m["parentId"].as_str().filter(|p| known.contains(p)).map(str::to_string),
                total: count(m, "totalEmails"),
                unread: count(m, "unreadEmails"),
                may_mark: rights("maySetSeen") && rights("maySetKeywords"),
                may_add: rights("mayAddItems"),
                may_remove: rights("mayRemoveItems"),
            })
        })
        .collect();
    out.sort_by_cached_key(|m| (m.name.to_lowercase(), m.id.clone()));
    out
}

/// The patch of an `Email/set` update for the action; `None` for Delete, which is a destroy.
fn action_patch(action: &SharedAction) -> Option<Value> {
    Some(match action {
        SharedAction::MarkRead => json!({ "keywords/$seen": true }),
        SharedAction::MarkUnread => json!({ "keywords/$seen": null }),
        SharedAction::Flag => json!({ "keywords/$flagged": true }),
        SharedAction::Unflag => json!({ "keywords/$flagged": null }),
        SharedAction::Move { mailbox_id } => json!({ "mailboxIds": { mailbox_id: true } }),
        SharedAction::Delete => return None,
    })
}

/// What the user reads when the server says `forbidden`. The server's text names a message or
/// a folder by an id; these words say what could not be done.
fn forbidden_words(action: &SharedAction, e: &Value) -> &'static str {
    match action {
        SharedAction::Delete => "you are not allowed to delete messages in this folder",
        SharedAction::Move { .. } => {
            let text = e["description"].as_str().unwrap_or_default().to_lowercase();
            if text.contains("add messages") {
                "you are not allowed to move messages into that folder"
            } else {
                "you are not allowed to move messages out of this folder"
            }
        }
        _ => "you are not allowed to change messages in this folder",
    }
}

/// Whether an `Email/set` did what was asked. A message that is gone is as good as deleted for
/// Delete and `Invalid` otherwise.
fn set_outcome(set: &Value, id: &str, action: &SharedAction) -> Result<()> {
    let deleting = matches!(action, SharedAction::Delete);
    let refused = if deleting { &set["notDestroyed"][id] } else { &set["notUpdated"][id] };
    if !refused.is_null() {
        return match refused["type"].as_str() {
            Some("notFound") if deleting => Ok(()),
            Some("notFound") => Err(Error::invalid(MESSAGE_GONE)),
            Some("forbidden") => Err(Error::invalid(forbidden_words(action, refused))),
            _ => Err(set_error(refused)),
        };
    }
    let done = if deleting {
        set["destroyed"].as_array().is_some_and(|ids| ids.iter().any(|d| d == id))
    } else {
        set["updated"].get(id).is_some()
    };
    if done {
        Ok(())
    } else {
        Err(Error::temporary("the server's answer does not say whether it was done"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders_by_name_with_parents_only_when_shared_too() {
        let get = json!({"list": [
            {"id": "z", "name": "zeta", "parentId": null, "totalEmails": 3, "unreadEmails": 1,
             "myRights": {"mayReadItems": true, "maySetSeen": true, "maySetKeywords": true}},
            {"id": "c", "name": "Child", "parentId": "z", "myRights": {"mayAddItems": true, "mayRemoveItems": true}},
            {"id": "o", "name": "alpha", "parentId": "unseen", "myRights": {"maySetSeen": true}},
        ]});
        let found = mailboxes_from(&get);
        let names: Vec<&str> = found.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["alpha", "Child", "zeta"]);
        assert_eq!(found[0].parent_id, None, "the parent is not shared");
        assert!(!found[0].may_mark, "seen without keywords is not enough");
        assert_eq!(found[1].parent_id.as_deref(), Some("z"));
        assert!(found[1].may_add && found[1].may_remove);
        assert_eq!((found[2].total, found[2].unread, found[2].may_mark), (3, 1, true));
    }

    #[test]
    fn a_refusal_is_in_words_for_the_user() {
        let forbidden = json!({"notUpdated": {"m": {"type": "forbidden", "description": "You are not allowed to modify keywords."}}});
        let e = set_outcome(&forbidden, "m", &SharedAction::MarkRead).unwrap_err();
        assert_eq!(e, Error::invalid("you are not allowed to change messages in this folder"));
        let gone = json!({"notUpdated": {"m": {"type": "notFound"}}});
        assert_eq!(set_outcome(&gone, "m", &SharedAction::Flag).unwrap_err(), Error::invalid(MESSAGE_GONE));
        let gone = json!({"notDestroyed": {"m": {"type": "notFound"}}});
        assert!(set_outcome(&gone, "m", &SharedAction::Delete).is_ok());
        let add = json!({"notUpdated": {"m": {"type": "forbidden", "description": "You are not allowed to add messages to mailbox i."}}});
        let to = SharedAction::Move { mailbox_id: "i".into() };
        assert_eq!(
            set_outcome(&add, "m", &to).unwrap_err(),
            Error::invalid("you are not allowed to move messages into that folder")
        );
        let nothing = json!({});
        assert!(matches!(set_outcome(&nothing, "m", &SharedAction::Delete), Err(Error::Temporary { .. })));
    }

    #[test]
    fn only_accounts_of_others_count() {
        let session = json!({"accounts": {
            "a": {"name": "me", "isPersonal": true},
            "b": {"name": "owner@x.test", "isPersonal": false},
        }});
        assert_eq!(other_accounts(&session), [("b".to_string(), "owner@x.test".to_string())]);
        assert!(is_shared_in(&session, "b") && !is_shared_in(&session, "a") && !is_shared_in(&session, "c"));
    }
}
