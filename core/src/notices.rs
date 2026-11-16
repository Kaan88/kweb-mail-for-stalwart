//! Notices from the server ("invited", "changed", "cancelled", "shared with you"): what the server
//! recorded as `CalendarEventNotification` and `ShareNotification` is handed to the host to show, and
//! destroyed on the server once the host says it has.
//!
//! The rules, so that nothing is shown twice, nothing old floods the phone and nothing is lost:
//! - A notice is shown only if the server made it after this account's baseline (the first look at
//!   it, less a few minutes), so the notices a server has kept for months are not shown on the first
//!   pass; those are left alone on the server, neither shown nor destroyed.
//! - The ids shown are remembered (the newest [`KEEP_SEEN`]), so a notice whose destroy failed is not
//!   shown again; it is destroyed on a later pass.
//! - The host answers `true` when it handled them (posted, or the user turned the channel off); on
//!   `false` they are not marked and the next pass offers them again.
//! - The user can turn the whole thing off per account ([`set_enabled`]); nothing is then read.
//! - Only the user's own account is read; a shared account's notices are not.
//!
//! What is in a notice is a name and a title written by another person: it is shown as text only.

use std::collections::VecDeque;

use crate::{jmap::Connection, model::Notice, push::Listener, store::Store, Result};

const OFF_KEY: &str = "notices_off";
const SINCE_KEY: &str = "notices_since";
const SEEN_KEY: &str = "notices_seen";
/// How many shown ids are remembered.
const KEEP_SEEN: usize = 200;
/// A notice made this long before the first look still counts as new (the server's clock, a slow first pass).
const SLACK_SECS: i64 = 300;

/// Whether the user wants these notices for the account (on unless switched off).
pub fn enabled(store: &Store, account_id: &str) -> Result<bool> {
    Ok(store.kv_get(account_id, OFF_KEY)?.as_deref() != Some("1"))
}

pub fn set_enabled(store: &Store, account_id: &str, on: bool) -> Result<()> {
    if on {
        store.kv_delete(account_id, OFF_KEY)
    } else {
        store.kv_set(account_id, OFF_KEY, "1")
    }
}

fn seen(store: &Store, account_id: &str) -> VecDeque<String> {
    let text = store.kv_get(account_id, SEEN_KEY).ok().flatten().unwrap_or_default();
    serde_json::from_str::<Vec<String>>(&text).unwrap_or_default().into()
}

fn baseline(store: &Store, account_id: &str, now: i64) -> Result<i64> {
    if let Some(at) = store.kv_get(account_id, SINCE_KEY)?.and_then(|t| t.parse::<i64>().ok()) {
        return Ok(at);
    }
    let first = now - SLACK_SECS;
    store.kv_set(account_id, SINCE_KEY, &first.to_string())?;
    Ok(first)
}

/// What to do with what the server holds: `show` are the notices to give the host, `again` the ids
/// that were shown before and are still on the server (their destroy is tried again). Pure.
pub fn sort(held: &[Notice], seen: &VecDeque<String>, since: i64) -> (Vec<Notice>, Vec<String>) {
    let show = held.iter().filter(|n| n.at >= since && !seen.contains(&n.id)).cloned().collect();
    let again = held.iter().filter(|n| seen.contains(&n.id)).map(|n| n.id.clone()).collect();
    (show, again)
}

/// One look at the server's notices, for the live loop and for the one-off checks. Best effort: a
/// failure (offline, a refusal) is not the pass's, and the next look tries again. Returns how
/// many notices were handed to the host.
pub async fn deliver(conn: &Connection, store: &Store, account_id: &str, listener: &dyn Listener, now: i64) -> usize {
    look(conn, store, account_id, listener, now).await.unwrap_or_default()
}

async fn look(conn: &Connection, store: &Store, account_id: &str, listener: &dyn Listener, now: i64) -> Result<usize> {
    if !enabled(store, account_id)? {
        return Ok(0);
    }
    let since = baseline(store, account_id, now)?;
    let held = conn.notices().await?;
    let mut known = seen(store, account_id);
    let (show, again) = sort(&held, &known, since);
    let mut shown = 0;
    let mut destroy = again;
    if !show.is_empty() && listener.notices(show.clone()) {
        shown = show.len();
        for notice in &show {
            known.push_back(notice.id.clone());
            destroy.push(notice.id.clone());
        }
        while known.len() > KEEP_SEEN {
            known.pop_front();
        }
        // Remembered before the server is asked: a destroy that fails must not show them twice.
        let list: Vec<&String> = known.iter().collect();
        store.kv_set(account_id, SEEN_KEY, &serde_json::to_string(&list).unwrap_or_default())?;
    }
    if !destroy.is_empty() {
        let _ = conn.acknowledge_notices(&destroy).await;
    }
    Ok(shown)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{NoticeKind, NoticeThing};

    fn notice(id: &str, at: i64) -> Notice {
        Notice {
            id: id.into(),
            kind: NoticeKind::EventAdded,
            thing: NoticeThing::Event,
            who: "Ann".into(),
            title: "Lunch".into(),
            local_start: None,
            time_zone: None,
            all_day: false,
            at,
        }
    }

    #[test]
    fn old_notices_and_those_already_shown_are_not_shown_and_the_shown_ones_are_destroyed_again() {
        let held = [notice("ce:old", 100), notice("ce:new", 1_000), notice("ce:shown", 1_001), notice("sh:new", 1_002)];
        let known: VecDeque<String> = ["ce:shown".to_string()].into();
        let (show, again) = sort(&held, &known, 500);
        assert_eq!(show.iter().map(|n| n.id.as_str()).collect::<Vec<_>>(), ["ce:new", "sh:new"]);
        assert_eq!(again, ["ce:shown"]);
    }
}
