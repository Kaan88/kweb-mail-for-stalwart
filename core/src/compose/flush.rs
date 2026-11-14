//! The sending half of a pass: the server's copies of the drafts, the sends that are due, and
//! the identities and address book.
//!
//! The rules it keeps:
//!
//! - It never makes a pass fail. Only what the caller has to act on (a 401, a lost grant) is
//!   handed back; every other failure ends the work for this pass and the next one goes on.
//! - It never holds anything across the network but the pass lock the caller already has: a row
//!   is read, the server is asked, and the row is written back conditionally, so an edit made
//!   meanwhile is not lost.
//! - A message is sent from a server copy that is current, and the send is recorded before it is
//!   made (`submitting`). A send whose answer was lost is settled by asking the server whether
//!   the copy is still a draft, so it is never sent twice. Once the server has accepted a
//!   message its row is gone, whatever fails after.

use std::time::Instant;

use crate::{
    compose::answer_threading,
    jmap::{is_unauthorized, Connection, CopyState},
    store::{DraftRow, DraftState, Store, Threading},
    Error, Result,
};

/// What [`flush`] did.
#[derive(Debug, Default)]
pub struct Flushed {
    /// A message was sent, or came back as a draft: what the lists show is different now.
    pub changed: bool,
    /// Set only when the caller has to act: the token was refused or the grant is gone.
    pub error: Option<Error>,
}

/// Brings the server up to date with the drafts and sends what is due.
pub async fn flush(conn: &Connection, store: &Store, account_id: &str, now: i64) -> Flushed {
    let mut changed = false;
    let result = run(conn, store, account_id, now, &mut changed).await;
    let error = result.err().filter(|e| matches!(e, Error::SignInNeeded { .. }) || is_unauthorized(e));
    Flushed { changed, error }
}

/// After this many failures in a row the server is taken to be out of reach and the rest waits
/// for the next pass. One draft the server keeps choking on must not hold up the others.
const MISSES_IN_A_ROW: u32 = 2;

/// Counts a failure. `Err` when the work has to stop: the caller must act (a 401, a lost
/// grant), or the server seems out of reach.
fn missed(e: Error, misses: &mut u32) -> Result<()> {
    if matches!(e, Error::SignInNeeded { .. }) || is_unauthorized(&e) {
        return Err(e);
    }
    *misses += 1;
    if *misses >= MISSES_IN_A_ROW {
        return Err(e);
    }
    Ok(())
}

async fn run(conn: &Connection, store: &Store, account_id: &str, now: i64, changed: &mut bool) -> Result<()> {
    let mut misses = 0;
    for (id, server_id) in store.drafts_discarded(account_id)? {
        if let Some(server_id) = server_id {
            match conn.destroy_email(&server_id).await {
                // Gone, or not ours to remove: the row goes either way.
                Ok(()) | Err(Error::Invalid { .. }) => misses = 0,
                Err(e) => {
                    missed(e, &mut misses)?;
                    continue;
                }
            }
        }
        store.draft_delete(account_id, &id)?;
    }
    // Without a Drafts mailbox in the cache there is nothing to save into yet; the sync of
    // this pass brings it and the next pass goes on.
    let Some(drafts) = store.mailbox_by_role(account_id, "drafts")? else { return Ok(()) };
    for row in store.drafts_to_save(account_id)? {
        match make_copy(conn, store, account_id, &row, &drafts.id).await {
            Ok(_) => misses = 0,
            // A copy the server will never take (a file that is gone from it): the text is
            // safe here, and the next edit tries again.
            Err(Error::Invalid { .. }) => store.draft_give_up(account_id, &row.draft.id, row.rev)?,
            Err(e) => missed(e, &mut misses)?,
        }
    }
    for row in store.drafts_to_send(account_id, now)? {
        match send_one(conn, store, account_id, row, &drafts.id, changed).await {
            Ok(()) => misses = 0,
            Err(e) => missed(e, &mut misses)?,
        }
    }
    Ok(())
}

/// Makes the server's copy of the draft current and remembers it. Returns the copy's id.
async fn make_copy(
    conn: &Connection,
    store: &Store,
    account_id: &str,
    row: &DraftRow,
    drafts_id: &str,
) -> Result<String> {
    let identities = store.identities_list(account_id)?;
    let from = identities.iter().find(|i| i.id == row.draft.identity_id);
    let threading = threading_of(conn, store, account_id, row).await?;
    let replaces = row.draft.server_id.as_deref();
    let headers = (threading.in_reply_to.as_slice(), threading.references.as_slice());
    let copy = conn.save_draft_copy(&row.draft, from, headers, drafts_id, replaces).await?;
    store.draft_saved(account_id, &row.draft.id, &copy, row.rev)?;
    Ok(copy)
}

/// The Message-IDs an answer carries, looked up once (when the server can be asked) and kept.
/// A forward, a new message and an opened server draft have what they have.
async fn threading_of(conn: &Connection, store: &Store, account_id: &str, row: &DraftRow) -> Result<Threading> {
    if let Some(known) = &row.threading {
        return Ok(known.clone());
    }
    let found = match (&row.draft.source_message_id, row.draft.is_forward) {
        (Some(source), false) => conn.header_ids(source).await?.map(|ids| answer_threading(&ids)),
        _ => None,
    };
    let threading = found.unwrap_or_default();
    store.draft_set_threading(account_id, &row.draft.id, &threading)?;
    Ok(threading)
}

/// How a send that was started but not seen through ended.
enum Settled {
    /// The server has sent it.
    Sent,
    /// The server has no copy any more.
    Lost,
    /// The copy is a draft still: the send did not happen.
    NotSent,
}

/// The server is asked two things. Whether it holds a submission of the copy: that is the
/// proof of a send, whatever became of the copy afterwards (the step that moves it to Sent can
/// fail on its own, and another program can delete it). Only without one does the copy's own
/// state decide.
async fn settle(conn: &Connection, row: &DraftRow) -> Result<Settled> {
    let Some(copy) = &row.draft.server_id else { return Ok(Settled::Lost) };
    if conn.was_submitted(copy).await? {
        return Ok(Settled::Sent);
    }
    Ok(match conn.copy_state(copy).await? {
        CopyState::Sent => Settled::Sent,
        CopyState::Gone => Settled::Lost,
        CopyState::Draft => Settled::NotSent,
    })
}

/// Sends one message that is due. A refusal the server will repeat turns it back into a draft
/// with the reason; a failure that may pass leaves it for the next pass.
async fn send_one(
    conn: &Connection,
    store: &Store,
    account_id: &str,
    mut row: DraftRow,
    drafts_id: &str,
    changed: &mut bool,
) -> Result<()> {
    if row.state == DraftState::Submitting {
        match settle(conn, &row).await? {
            Settled::Sent => return accepted(conn, store, account_id, &row, changed).await,
            Settled::NotSent => {}
            Settled::Lost => {
                store.draft_copy_lost(account_id, &row.draft.id)?;
                match store.draft_get(account_id, &row.draft.id)? {
                    Some(again) => row = again,
                    None => return Ok(()),
                }
            }
        }
    }
    if row.state == DraftState::Sending {
        if row.dirty || row.draft.server_id.is_none() {
            match make_copy(conn, store, account_id, &row, drafts_id).await {
                Ok(copy) => row.draft.server_id = Some(copy),
                Err(Error::Invalid { msg }) => return refused(store, account_id, &row, &msg, changed),
                Err(e) => return Err(e),
            }
        }
        // Taken back, changed or thrown away while the copy was made: nothing is sent now.
        let Some(copy) = row.draft.server_id.as_deref() else { return Ok(()) };
        if !store.draft_begin_submit(account_id, &row.draft.id, row.rev, copy)? {
            return Ok(());
        }
    }
    let Some(copy) = row.draft.server_id.as_deref() else { return Ok(()) };
    let sent = store.mailbox_by_role(account_id, "sent")?;
    match conn.submit(&row.draft.identity_id, copy, drafts_id, sent.as_ref().map(|m| m.id.as_str())).await {
        Ok(()) => accepted(conn, store, account_id, &row, changed).await,
        Err(Error::Invalid { msg }) => refused(store, account_id, &row, &msg, changed),
        // The row stays `submitting`: the next pass asks the server how it went.
        Err(e) => Err(e),
    }
}

/// The server has the message and has sent it. The row goes first, so nothing that fails after
/// can make it go out again.
async fn accepted(
    conn: &Connection,
    store: &Store,
    account_id: &str,
    row: &DraftRow,
    changed: &mut bool,
) -> Result<()> {
    store.draft_delete(account_id, &row.draft.id)?;
    *changed = true;
    if let Some(source) = &row.draft.source_message_id {
        // Best effort: the message is sent whether or not the source can be marked.
        let _ = conn.mark_source(source, row.draft.is_forward).await;
    }
    Ok(())
}

fn refused(store: &Store, account_id: &str, row: &DraftRow, why: &str, changed: &mut bool) -> Result<()> {
    store.draft_refused(account_id, &row.draft.id, why)?;
    *changed = true;
    Ok(())
}

/// Fetches the identities and the address book when they are due (the first pass of the
/// process, then once an hour) and keeps them in the cache. Silent about failures. It is not
/// part of a pass: the live loop runs it beside itself and the one-off check within its
/// budget, so it can neither hold up nor fail a notification.
pub async fn refresh_directory(conn: &Connection, store: &Store, account_id: &str) {
    let now = Instant::now();
    if !store.directory_due(now) {
        return;
    }
    // The identities first and on their own: without them nothing can be sent, so a slow or
    // broken address book must not keep them out of the cache.
    let Ok(identities) = conn.identities().await else { return };
    if store.identities_replace(account_id, &identities).is_err() {
        return;
    }
    if let Ok(contacts) = conn.contacts().await {
        let _ = store.contacts_replace(account_id, &contacts.unwrap_or_default());
    }
    store.directory_done(now);
}
