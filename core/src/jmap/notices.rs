//! The server's record of what other people did that concerns the user: `CalendarEventNotification`
//! (an invitation, a change or a cancellation of an event, or a write into one of the user's calendars)
//! and `ShareNotification` (something was shared, changed or unshared). Checked on Stalwart 1.1
//! (`dev/notes/stalwart-notices.md`):
//!
//! - Both live in the user's own account. A `CalendarEventNotification` has `type` `created`, `updated`
//!   or `destroyed`, `changedBy` (name, email, principalId), `calendarEventId`, `event` (a snapshot with
//!   title and start) and, for an update, `eventPatch`. A `ShareNotification` has `objectType`,
//!   `objectAccountId`, `oldRights` and `newRights`; its `name` is empty and its `changedBy` is unreliable
//!   for a mailbox (it names the administrator), so the person is the account that gives the share.
//! - The server only makes one when someone else acted; the user's own writes make none.
//! - Both are listed with `query` and `get` and taken away with `set` `destroy` (the acknowledgement).
//! - The push types are `CalendarEventNotification` and `ShareNotification` (`StateChange`).
//! - `ShareNotification` needs the principals capability, the other the calendars one.

use serde_json::{json, Value};

use super::{calendars::CALENDARS_CAPABILITY, Connection};
use crate::{
    model::{Notice, NoticeKind, NoticeThing},
    Result,
};

const PRINCIPALS_CAPABILITY: &str = "urn:ietf:params:jmap:principals";
const EVENT_PREFIX: &str = "ce:";
const SHARE_PREFIX: &str = "sh:";
/// The newest this many of each kind are read.
const READ_LIMIT: usize = 50;

impl Connection {
    /// The notices the server holds for the user, oldest first. Those made by the user, and
    /// those of a kind the server does not offer, are left out. A method the server refuses gives
    /// nothing for that kind (a user without the permission has none to see).
    pub async fn notices(&self) -> Result<Vec<Notice>> {
        let mut calls: Vec<(&str, Value)> = Vec::new();
        let mut capabilities: Vec<&str> = Vec::new();
        if self.has_capability(CALENDARS_CAPABILITY) {
            capabilities.push(CALENDARS_CAPABILITY);
            let first = calls.len();
            calls.extend(listing("CalendarEventNotification", &self.account_for(CALENDARS_CAPABILITY), first));
        }
        if self.has_capability(PRINCIPALS_CAPABILITY) {
            capabilities.push(PRINCIPALS_CAPABILITY);
            let first = calls.len();
            calls.extend(listing("ShareNotification", &self.account_for(PRINCIPALS_CAPABILITY), first));
        }
        if calls.is_empty() {
            return Ok(Vec::new());
        }
        let responses = self.call_keeping_errors(&capabilities, &calls).await?;
        let own = self.account_id();
        let mut out = Vec::new();
        for (name, answer) in &responses {
            match name.as_str() {
                "CalendarEventNotification/get" => {
                    out.extend(answer["list"].as_array().into_iter().flatten().filter_map(|n| event_notice(n, &own)))
                }
                "ShareNotification/get" => out.extend(
                    answer["list"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|n| share_notice(n, &own, |account| self.account_name(account))),
                ),
                _ => {}
            }
        }
        out.sort_by(|a, b| a.at.cmp(&b.at).then_with(|| a.id.cmp(&b.id)));
        Ok(out)
    }

    /// Tells the server the notices were shown: each is destroyed there. An id that is already gone
    /// is as good as destroyed. `ids` are [`Notice::id`]s.
    pub async fn acknowledge_notices(&self, ids: &[String]) -> Result<()> {
        let split = |prefix: &str| -> Vec<&str> { ids.iter().filter_map(|i| i.strip_prefix(prefix)).collect() };
        let (events, shares) = (split(EVENT_PREFIX), split(SHARE_PREFIX));
        let mut calls: Vec<(&str, Value)> = Vec::new();
        let mut capabilities: Vec<&str> = Vec::new();
        if !events.is_empty() {
            capabilities.push(CALENDARS_CAPABILITY);
            let args = json!({ "accountId": self.account_for(CALENDARS_CAPABILITY), "destroy": events });
            calls.push(("CalendarEventNotification/set", args));
        }
        if !shares.is_empty() {
            capabilities.push(PRINCIPALS_CAPABILITY);
            let args = json!({ "accountId": self.account_for(PRINCIPALS_CAPABILITY), "destroy": shares });
            calls.push(("ShareNotification/set", args));
        }
        if calls.is_empty() {
            return Ok(());
        }
        // A refusal of one kind must not keep the other from being acknowledged.
        self.call_keeping_errors(&capabilities, &calls).await.map(|_| ())
    }

    /// The name the session gives an account (for a share, the account that shares).
    fn account_name(&self, account: &str) -> Option<String> {
        let session = self.client.session();
        session.account(account).map(|a| a.name().to_string()).filter(|n| !n.is_empty())
    }
}

/// A query for the newest notices of a kind and the get of what it found. `first` is the number of
/// calls before these in the request: the get takes the ids of the query by its call id.
fn listing(object: &str, account: &str, first: usize) -> [(&'static str, Value); 2] {
    let (query, get): (&'static str, &'static str) = match object {
        "CalendarEventNotification" => ("CalendarEventNotification/query", "CalendarEventNotification/get"),
        _ => ("ShareNotification/query", "ShareNotification/get"),
    };
    [
        (
            query,
            json!({ "accountId": account, "sort": [{ "property": "created", "isAscending": false }], "limit": READ_LIMIT }),
        ),
        (
            get,
            json!({ "accountId": account, "#ids": { "resultOf": format!("c{first}"), "name": query, "path": "/ids" } }),
        ),
    ]
}

fn text(v: &Value) -> String {
    v.as_str().unwrap_or_default().trim().to_string()
}

/// Whether the person who did it is the user (the server makes no such record, but a client that
/// writes for the user through a shared path could).
fn is_own(changed_by: &Value, own: &str) -> bool {
    changed_by["principalId"].as_str().is_some_and(|p| p == own)
}

fn who_of(changed_by: &Value) -> String {
    let name = text(&changed_by["name"]);
    if !name.is_empty() {
        return name;
    }
    let email = text(&changed_by["email"]);
    if email.is_empty() {
        "Someone".to_string()
    } else {
        email
    }
}

fn created_at(n: &Value) -> i64 {
    chrono::DateTime::parse_from_rfc3339(n["created"].as_str().unwrap_or_default()).map_or(0, |at| at.timestamp())
}

fn event_notice(n: &Value, own: &str) -> Option<Notice> {
    let id = n["id"].as_str().filter(|i| !i.is_empty())?;
    if is_own(&n["changedBy"], own) {
        return None;
    }
    let event = &n["event"];
    let guests = event["participants"].as_object().is_some_and(|p| !p.is_empty());
    let kind = match n["type"].as_str()? {
        "created" if guests => NoticeKind::EventInvited,
        "created" => NoticeKind::EventAdded,
        "updated" => NoticeKind::EventChanged,
        "destroyed" => NoticeKind::EventCancelled,
        _ => return None,
    };
    // An update gives the new title in its patch when it changed it.
    let title = [&n["eventPatch"]["title"], &event["title"]].into_iter().map(text).find(|t| !t.is_empty());
    let patch_start = text(&n["eventPatch"]["start"]);
    let start = if patch_start.is_empty() { text(&event["start"]) } else { patch_start };
    let zone = text(&n["eventPatch"]["timeZone"]);
    let zone = if zone.is_empty() { text(&event["timeZone"]) } else { zone };
    Some(Notice {
        id: format!("{EVENT_PREFIX}{id}"),
        kind,
        thing: NoticeThing::Event,
        who: who_of(&n["changedBy"]),
        title: title.unwrap_or_default(),
        local_start: Some(start).filter(|s| !s.is_empty()),
        time_zone: Some(zone).filter(|z| !z.is_empty()),
        all_day: event["showWithoutTime"] == true,
        at: created_at(n),
    })
}

fn any_right(rights: &Value) -> bool {
    rights.as_object().is_some_and(|r| r.values().any(|v| *v == true))
}

fn share_notice(n: &Value, own: &str, account_name: impl Fn(&str) -> Option<String>) -> Option<Notice> {
    let id = n["id"].as_str().filter(|i| !i.is_empty())?;
    if is_own(&n["changedBy"], own) {
        return None;
    }
    let thing = match n["objectType"].as_str()? {
        "Mailbox" => NoticeThing::Mailbox,
        "Calendar" => NoticeThing::Calendar,
        "AddressBook" => NoticeThing::AddressBook,
        _ => NoticeThing::Other,
    };
    let (before, after) = (any_right(&n["oldRights"]), any_right(&n["newRights"]));
    let kind = match (before, after) {
        (false, true) => NoticeKind::ShareNew,
        (true, true) => NoticeKind::ShareChanged,
        (true, false) => NoticeKind::ShareEnded,
        // Nothing before and nothing after: nothing happened to tell.
        (false, false) => return None,
    };
    // The account that gives the share is the person; `changedBy` of a mailbox names the administrator.
    let giver = n["objectAccountId"].as_str().and_then(account_name);
    Some(Notice {
        id: format!("{SHARE_PREFIX}{id}"),
        kind,
        thing,
        who: giver.unwrap_or_else(|| who_of(&n["changedBy"])),
        title: String::new(),
        local_start: None,
        time_zone: None,
        all_day: false,
        at: created_at(n),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name_of(account: &str) -> Option<String> {
        (account == "c").then(|| "tester@kweb.test".to_string())
    }

    #[test]
    fn what_the_server_recorded_becomes_a_notice() {
        let created = json!({
            "id": "b", "created": "2026-11-16T13:45:24Z", "type": "created", "calendarEventId": "bc",
            "changedBy": { "name": "Second test user", "email": "second@kweb.test", "principalId": "d" },
            "event": { "start": "2026-11-26T10:00:00", "title": "Lunch", "timeZone": "Etc/UTC", "duration": "PT1H" },
        });
        let n = event_notice(&created, "c").unwrap();
        assert_eq!(
            (n.id.as_str(), n.kind, n.who.as_str(), n.title.as_str()),
            ("ce:b", NoticeKind::EventAdded, "Second test user", "Lunch")
        );
        assert_eq!((n.local_start.as_deref(), n.time_zone.as_deref()), (Some("2026-11-26T10:00:00"), Some("Etc/UTC")));
        assert_eq!(n.at, 1_794_836_724);

        // With guests it is an invitation; a change takes the new title and time from the patch; a
        // cancellation names the old.
        let mut invited = created.clone();
        invited["event"]["participants"] = json!({ "p1": { "email": "tester@kweb.test" } });
        assert_eq!(event_notice(&invited, "c").unwrap().kind, NoticeKind::EventInvited);
        let mut changed = created.clone();
        changed["type"] = json!("updated");
        changed["eventPatch"] = json!({ "title": "Late lunch", "start": "2026-11-26T12:00:00" });
        let n = event_notice(&changed, "c").unwrap();
        assert_eq!(
            (n.kind, n.title.as_str(), n.local_start.as_deref()),
            (NoticeKind::EventChanged, "Late lunch", Some("2026-11-26T12:00:00"))
        );
        let mut gone = created.clone();
        gone["type"] = json!("destroyed");
        assert_eq!(event_notice(&gone, "c").unwrap().kind, NoticeKind::EventCancelled);
        // The user's own, a kind not known and one with no id are not notices.
        let mut mine = created.clone();
        mine["changedBy"]["principalId"] = json!("c");
        assert!(event_notice(&mine, "c").is_none());
        let mut odd = created.clone();
        odd["type"] = json!("moved");
        assert!(event_notice(&odd, "c").is_none());
        assert!(event_notice(&json!({ "type": "created" }), "c").is_none());
    }

    #[test]
    fn a_share_is_new_changed_or_ended_and_the_giver_is_the_account() {
        let share = |old: Value, new: Value| {
            json!({ "id": "x", "created": "2026-11-16T13:45:24Z", "objectType": "Mailbox", "objectAccountId": "c",
                    "oldRights": old, "newRights": new,
                    "changedBy": { "principalId": "d333333", "name": "Recovery admin account", "email": "admin" } })
        };
        let none = json!({ "mayReadItems": false, "mayAddItems": false });
        let read = json!({ "mayReadItems": true, "mayAddItems": false });
        let write = json!({ "mayReadItems": true, "mayAddItems": true });
        let kind = |n: Value| share_notice(&n, "d", name_of).map(|n| (n.kind, n.thing, n.who));
        assert_eq!(
            kind(share(none.clone(), read.clone())),
            Some((NoticeKind::ShareNew, NoticeThing::Mailbox, "tester@kweb.test".into()))
        );
        assert_eq!(kind(share(read.clone(), write)).map(|k| k.0), Some(NoticeKind::ShareChanged));
        assert_eq!(kind(share(read, none.clone())).map(|k| k.0), Some(NoticeKind::ShareEnded));
        assert_eq!(kind(share(none.clone(), none)), None);
        // An account the session does not name falls back to the person the server names.
        let mut other = share(json!({}), json!({ "mayReadItems": true }));
        other["objectAccountId"] = json!("z");
        assert_eq!(share_notice(&other, "d", name_of).unwrap().who, "Recovery admin account");
        other["objectType"] = json!("Directory");
        assert_eq!(share_notice(&other, "d", name_of).unwrap().thing, NoticeThing::Other);
    }

    #[test]
    fn the_second_listing_refers_to_its_own_query() {
        let both: Vec<_> = listing("ShareNotification", "d", 2).into_iter().collect();
        assert_eq!(both[1].1["#ids"]["resultOf"], "c2");
        assert_eq!(both[1].1["#ids"]["name"], "ShareNotification/query");
    }
}
