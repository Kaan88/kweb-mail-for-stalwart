//! Email/changes, Mailbox/changes, full resync when the server cannot give changes, and the
//! reads that go beyond that: older mail, a thread, search results, bodies kept ahead.
//!
//! Order of work in one pass: mailboxes, then messages, then (only if something was written)
//! the queued actions are put back on the cache. A state string is stored only after the data
//! it stands for is in the cache, so a failed pass is simply repeated by the next one.

use std::collections::HashSet;

use crate::{
    actions,
    jmap::{self, Changes, Connection},
    model::{Mailbox, MessageHeader},
    store::Store,
    Result,
};

/// Newest messages kept for the Inbox on a full fetch.
pub const INBOX_WINDOW: usize = 500;
/// Newest messages kept for every other mailbox on a full fetch.
pub const OTHER_WINDOW: usize = 100;

/// How many older messages one [`load_older`] fetches.
pub const PAGE: usize = 50;
/// How many of the newest Inbox messages [`prefetch_bodies`] keeps bodies for.
const PREFETCH_NEWEST: u32 = 100;
/// Bodies fetched per request by [`prefetch_bodies`]. Small, so that a prefetch that is cut
/// short has still stored the batches before.
const PREFETCH_BATCH: usize = 10;

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
    // Per mailbox: the ids its query returned, and whether that was a full page.
    let mut found: Vec<(String, Vec<String>, bool)> = Vec::new();
    for mailbox in store.list_mailboxes(account_id)? {
        let window = if mailbox.role.as_deref() == Some("inbox") { INBOX_WINDOW } else { OTHER_WINDOW };
        let page = conn.message_ids(&mailbox.id, 0, window).await?;
        ids.extend(page.iter().cloned());
        let full = page.len() == window;
        found.push((mailbox.id, page, full));
    }
    let (headers, _) = conn.messages(&dedup(ids)).await?;
    store.replace_messages(account_id, &headers)?;
    for (mailbox_id, page, full) in found {
        set_window(store, account_id, &mailbox_id, &page, full, &headers)?;
    }
    store.set_state(account_id, EMAIL, &state)?;
    Ok(Outcome { changed: true, full_resync: true })
}

/// A mailbox whose query filled its window may have older mail on the server: the cache holds
/// it without gaps back to the oldest message that query returned. Any other mailbox is
/// cached whole.
fn set_window(
    store: &Store,
    account_id: &str,
    mailbox_id: &str,
    page: &[String],
    full: bool,
    headers: &[MessageHeader],
) -> Result<()> {
    let in_page: HashSet<&str> = page.iter().map(String::as_str).collect();
    let oldest = headers.iter().filter(|h| in_page.contains(h.id.as_str())).map(|h| h.received_at).min();
    match oldest {
        Some(oldest) if full => store.set_window(account_id, mailbox_id, oldest),
        _ => store.clear_window(account_id, mailbox_id),
    }
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

/// The cached copy of each message, as `None` where there is none.
fn snapshot(store: &Store, account_id: &str, ids: &[String]) -> Result<Vec<Option<MessageHeader>>> {
    ids.iter().map(|id| store.get_message(account_id, id)).collect()
}

/// Writes headers that were fetched outside a sync pass (older mail, a thread, search results)
/// and puts the queued actions back on top.
///
/// The request ran without the pass lock, so that a slow answer to something the user asked for
/// never holds up a notification pass; the lock is taken only for this write. A pass may
/// therefore have run in between. `seen` is the Email state the cache was at before the
/// request ([`email_state`]). If it still is, no pass wrote meanwhile: the headers are at least
/// as new as the cache and are all written. If not, they may be older than what the pass
/// wrote, so only messages the cache does not have are added: the ones it has are kept
/// current by /changes anyway.
async fn store_fetched(store: &Store, account_id: &str, headers: &[MessageHeader], seen: Option<String>) -> Result<()> {
    let _one_at_a_time = store.pass_lock().lock().await;
    if store.get_state(account_id, EMAIL)? == seen {
        store.upsert_messages(account_id, headers)?;
    } else {
        let mut missing = Vec::new();
        for h in headers {
            if store.get_message(account_id, &h.id)?.is_none() {
                missing.push(h.clone());
            }
        }
        store.upsert_messages(account_id, &missing)?;
    }
    actions::reapply_pending(store, account_id)
}

/// The Email state the cache is at: taken before a request whose answer goes to [`store_fetched`].
fn email_state(store: &Store, account_id: &str) -> Result<Option<String>> {
    store.get_state(account_id, EMAIL)
}

/// Fetches the next older messages of a mailbox into the cache (`PAGE` of them, more only when
/// a whole page arrived in one second) and moves the mailbox's window back over them. Returns
/// whether the server may have still older mail. A mailbox without a window is cached whole:
/// nothing to do, no request.
pub async fn load_older(conn: &Connection, store: &Store, account_id: &str, mailbox_id: &str) -> Result<bool> {
    let Some(oldest) = store.get_window(account_id, mailbox_id)? else { return Ok(false) };
    // `before` is exclusive; one second more re-reads the mail of the second the window ends
    // on, which a page boundary may have cut in two.
    let before = oldest + 1;
    let seen = email_state(store, account_id)?;
    let mut headers: Vec<MessageHeader> = Vec::new();
    let mut position = 0;
    // Where the window ends afterwards; `None` when the server has nothing older.
    let reached = loop {
        let ids = conn.message_ids_before(mailbox_id, before, position, PAGE).await?;
        let page = if ids.is_empty() { Vec::new() } else { conn.messages(&ids).await?.0 };
        let low = page.iter().map(|h| h.received_at).min();
        headers.extend(page);
        if ids.len() < PAGE {
            break None;
        }
        match low {
            Some(low) if low < oldest => break Some(low),
            // A full page of mail from the window's own second: the next page of the same
            // query, until one reaches further back.
            _ => position += PAGE,
        }
    };
    store_fetched(store, account_id, &headers, seen).await?;
    // A full resync meanwhile set the window anew: it is the truth now.
    if store.get_window(account_id, mailbox_id)? != Some(oldest) {
        return Ok(store.get_window(account_id, mailbox_id)?.is_some());
    }
    match reached {
        Some(reached) => store.set_window(account_id, mailbox_id, reached)?,
        None => store.clear_window(account_id, mailbox_id)?,
    }
    Ok(reached.is_some())
}

/// Asks the server which messages a thread has and caches their headers. Returns whether the
/// cache changed.
pub async fn refresh_thread(conn: &Connection, store: &Store, account_id: &str, thread_id: &str) -> Result<bool> {
    if thread_id.is_empty() {
        return Ok(false);
    }
    let seen = email_state(store, account_id)?;
    let ids = conn.thread_email_ids(thread_id).await?;
    if ids.is_empty() {
        return Ok(false);
    }
    let (headers, _) = conn.messages(&ids).await?;
    let before = snapshot(store, account_id, &ids)?;
    store_fetched(store, account_id, &headers, seen).await?;
    Ok(before != snapshot(store, account_id, &ids)?)
}

/// What the server finds for `query`, as headers in the server's order. The headers go into the
/// cache first (so the list can act on them), then are read back from it, which shows queued
/// actions the server has not seen yet.
pub async fn search(
    conn: &Connection,
    store: &Store,
    account_id: &str,
    query: &str,
    mailbox_id: Option<&str>,
    limit: usize,
) -> Result<Vec<MessageHeader>> {
    let seen = email_state(store, account_id)?;
    let ids = conn.search_ids(query, mailbox_id, limit).await?;
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let (headers, _) = conn.messages(&ids).await?;
    store_fetched(store, account_id, &headers, seen).await?;
    Ok(snapshot(store, account_id, &ids)?.into_iter().flatten().collect())
}

/// Keeps the bodies of the newest Inbox mail in the cache, so it can be read offline and opens
/// at once. Returns how many bodies were stored. A failure part-way leaves the bodies already
/// stored in place.
pub async fn prefetch_bodies(conn: &Connection, store: &Store, account_id: &str) -> Result<usize> {
    let Some(inbox) = store.mailbox_by_role(account_id, "inbox")? else { return Ok(0) };
    let missing = store.ids_without_body(account_id, &inbox.id, PREFETCH_NEWEST)?;
    let mut stored = 0;
    for chunk in missing.chunks(PREFETCH_BATCH) {
        for body in conn.bodies(chunk).await? {
            stored += usize::from(store.put_body(account_id, &body)?);
        }
    }
    Ok(stored)
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
