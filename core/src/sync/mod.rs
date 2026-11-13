//! Email/changes, Mailbox/changes, full resync when the server cannot give changes.
//!
//! Order of work in one pass: mailboxes, then messages, then (only if something was written)
//! the queued actions are put back on the cache. A state string is stored only after the data
//! it stands for is in the cache, so a failed pass is simply repeated by the next one.

use std::collections::HashSet;

use crate::{
    actions,
    jmap::{self, Changes, Connection},
    model::Mailbox,
    store::Store,
    Result,
};

/// Newest messages kept for the Inbox on a full fetch.
pub const INBOX_WINDOW: usize = 500;
/// Newest messages kept for every other mailbox on a full fetch.
pub const OTHER_WINDOW: usize = 100;

const MAILBOX: &str = "Mailbox";
const EMAIL: &str = "Email";

/// What one sync pass did.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Outcome {
    /// Anything in the cache is different from before the pass.
    pub changed: bool,
    /// The server could not give changes (or there was no state yet): everything was fetched again.
    pub full_resync: bool,
}

/// Brings the cache of `account_id` up to date with the server.
pub async fn sync(conn: &Connection, store: &Store, account_id: &str) -> Result<Outcome> {
    let mailboxes = sync_mailboxes(conn, store, account_id).await?;
    let messages = sync_messages(conn, store, account_id).await?;
    let changed = mailboxes.changed || messages.changed;
    if changed {
        // Server data may have overwritten what the user already did offline.
        actions::reapply_pending(store, account_id)?;
    }
    Ok(Outcome { changed, full_resync: mailboxes.full_resync || messages.full_resync })
}

/// Ids in the order given, each once.
fn dedup(ids: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut seen = HashSet::new();
    ids.into_iter().filter(|id| seen.insert(id.clone())).collect()
}

/// Created and updated ids of a /changes answer.
fn to_fetch(changes: &Changes) -> Vec<String> {
    dedup(changes.created.iter().chain(&changes.updated).cloned())
}

/// Requested ids the server did not return: they are gone (destroyed after the changes call).
fn missing<'a>(requested: &'a [String], returned: impl Iterator<Item = &'a str>) -> Vec<String> {
    let got: HashSet<&str> = returned.collect();
    requested.iter().filter(|id| !got.contains(id.as_str())).cloned().collect()
}

async fn sync_mailboxes(conn: &Connection, store: &Store, account_id: &str) -> Result<Outcome> {
    let Some(since) = store.get_state(account_id, MAILBOX)? else {
        return full_mailboxes(conn, store, account_id).await;
    };
    match conn.mailbox_changes(&since).await {
        Ok(changes) => {
            let changed = apply_mailbox_changes(conn, store, account_id, &changes).await?;
            Ok(Outcome { changed, full_resync: false })
        }
        Err(e) if jmap::is_cannot_calculate_changes(&e) => full_mailboxes(conn, store, account_id).await,
        Err(e) => Err(e),
    }
}

async fn full_mailboxes(conn: &Connection, store: &Store, account_id: &str) -> Result<Outcome> {
    let (boxes, state) = conn.mailboxes().await?;
    store.replace_mailboxes(account_id, &boxes)?;
    store.set_state(account_id, MAILBOX, &state)?;
    Ok(Outcome { changed: true, full_resync: true })
}

async fn apply_mailbox_changes(conn: &Connection, store: &Store, account_id: &str, changes: &Changes) -> Result<bool> {
    let ids = to_fetch(changes);
    let fetched: Vec<Mailbox> = if ids.is_empty() { Vec::new() } else { conn.mailboxes_by_id(&ids).await? };
    let mut gone = changes.destroyed.clone();
    gone.extend(missing(&ids, fetched.iter().map(|m| m.id.as_str())));
    store.upsert_mailboxes(account_id, &fetched)?;
    store.delete_mailboxes(account_id, &gone)?;
    store.set_state(account_id, MAILBOX, &changes.new_state)?;
    Ok(!fetched.is_empty() || !gone.is_empty())
}

async fn sync_messages(conn: &Connection, store: &Store, account_id: &str) -> Result<Outcome> {
    let Some(since) = store.get_state(account_id, EMAIL)? else {
        return full_messages(conn, store, account_id).await;
    };
    match conn.email_changes(&since).await {
        Ok(changes) => {
            let changed = apply_message_changes(conn, store, account_id, &changes).await?;
            Ok(Outcome { changed, full_resync: false })
        }
        Err(e) if jmap::is_cannot_calculate_changes(&e) => full_messages(conn, store, account_id).await,
        Err(e) => Err(e),
    }
}

/// The newest messages of every mailbox. The state stored is read BEFORE the queries: mail
/// that arrives while we fetch is then reported again by the next /changes (and applying a
/// change twice is harmless), whereas a state read afterwards would hide it for good.
async fn full_messages(conn: &Connection, store: &Store, account_id: &str) -> Result<Outcome> {
    let (_, state) = conn.messages(&[]).await?;
    let mut ids = Vec::new();
    for mailbox in store.list_mailboxes(account_id)? {
        let window = if mailbox.role.as_deref() == Some("inbox") { INBOX_WINDOW } else { OTHER_WINDOW };
        ids.extend(conn.message_ids(&mailbox.id, 0, window).await?);
    }
    let (headers, _) = conn.messages(&dedup(ids)).await?;
    store.replace_messages(account_id, &headers)?;
    store.set_state(account_id, EMAIL, &state)?;
    Ok(Outcome { changed: true, full_resync: true })
}

async fn apply_message_changes(conn: &Connection, store: &Store, account_id: &str, changes: &Changes) -> Result<bool> {
    let ids = to_fetch(changes);
    let headers = if ids.is_empty() { Vec::new() } else { conn.messages(&ids).await?.0 };
    let mut gone = changes.destroyed.clone();
    gone.extend(missing(&ids, headers.iter().map(|h| h.id.as_str())));
    store.upsert_messages(account_id, &headers)?;
    store.delete_messages(account_id, &gone)?;
    store.set_state(account_id, EMAIL, &changes.new_state)?;
    Ok(!headers.is_empty() || !gone.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(ids: &[&str]) -> Vec<String> {
        ids.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn dedup_keeps_first_occurrence_order() {
        assert_eq!(dedup(v(&["b", "a", "b", "c", "a"])), v(&["b", "a", "c"]));
    }

    #[test]
    fn created_and_updated_are_fetched_once() {
        let c = Changes { created: v(&["a", "b"]), updated: v(&["b", "c"]), ..Default::default() };
        assert_eq!(to_fetch(&c), v(&["a", "b", "c"]));
    }

    #[test]
    fn missing_lists_what_the_server_did_not_return() {
        let asked = v(&["a", "b", "c"]);
        assert_eq!(missing(&asked, ["a", "c"].into_iter()), v(&["b"]));
        assert!(missing(&asked, ["a", "b", "c"].into_iter()).is_empty());
    }
}
