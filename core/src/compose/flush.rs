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
//! - A message the server holds back (a send with an undo window, or a scheduled one) is
//!   proved the same way: by a submission that was not cancelled. Until it is due its row
//!   stays (`held`), so the user can take it back; a message being scheduled that the server
//!   does not hold is a draft again and is never sent.
//! - It does not run while a call of the user is talking to the server about a message on its
//!   way out (`Store::send_lock`): that call finishes first, and the pass leaves the outgoing
//!   mail to the next one.

use std::time::Instant;

use crate::{
    compose::answer_threading,
    jmap::{is_unauthorized, Connection, CopyState, Held, Hold, SubmissionInfo, Undo, MAX_HOLD_SECS},
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
    // Someone is talking to the server about a message on its way out: this pass leaves the
    // outgoing mail alone, and never waits for it (it would hold up the next pass).
    let Ok(_sending) = store.send_lock().try_lock() else { return Flushed::default() };
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
pub(super) fn missed(e: Error, misses: &mut u32) -> Result<()> {
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
    for row in store.drafts_discarded_rows(account_id)? {
        let id = row.draft.id.clone();
        if let Some(server_id) = &row.draft.server_id {
            // A message the server holds back is taken back before its copy is removed. One that
            // has left is not: it stays in Sent, and only the row goes.
            if row.state == DraftState::Held {
                match cancel_held(conn, server_id).await {
                    Ok(true) => {}
                    Ok(false) | Err(Error::Invalid { .. }) => {
                        store.draft_delete(account_id, &id)?;
                        continue;
                    }
                    Err(e) => {
                        missed(e, &mut misses)?;
                        continue;
                    }
                }
            }
            match conn.destroy_email(server_id).await {
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
    // Messages whose cancel was accepted but whose putting back did not finish.
    match super::later::finish_restores(conn, store, account_id, &drafts.id).await {
        Ok(true) => *changed = true,
        Ok(false) => {}
        Err(e) => missed(e, &mut misses)?,
    }
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
        match send_one(conn, store, account_id, row, &drafts.id, now, changed).await {
            Ok(()) => misses = 0,
            Err(e) => missed(e, &mut misses)?,
        }
    }
    Ok(())
}

/// Makes the server's copy of the draft current and remembers it. Returns the copy's id.
pub(super) async fn make_copy(
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
pub(super) enum Settled {
    /// The server has sent it.
    Sent,
    /// The server holds it back and will send it.
    Held,
    /// The submission was cancelled (here earlier, or on another device): the message is not
    /// sent and sits in Sent.
    Cancelled,
    /// The server has no copy any more.
    Lost,
    /// The copy is a draft still: the send did not happen.
    NotSent,
}

/// What a message's submissions add up to.
pub(super) enum Standing {
    /// One is pending (the first of them).
    Held(SubmissionInfo),
    /// One has left.
    Left,
    /// There are some, all cancelled.
    Cancelled,
    /// There are none.
    None,
}

pub(super) fn standing(submissions: &[SubmissionInfo]) -> Standing {
    if submissions.iter().any(|s| s.undo == Undo::Final) {
        return Standing::Left;
    }
    match submissions.iter().find(|s| s.undo == Undo::Pending) {
        Some(held) => Standing::Held(held.clone()),
        None if submissions.is_empty() => Standing::None,
        None => Standing::Cancelled,
    }
}

/// The server is asked two things. Whether it holds a submission of the copy that was not
/// cancelled: that is the proof of a send, whatever became of the copy afterwards (the step
/// that moves it to Sent can fail on its own, and another program can delete it). Only without
/// one does the copy's own state decide.
pub(super) async fn settle(conn: &Connection, row: &DraftRow) -> Result<Settled> {
    let Some(copy) = &row.draft.server_id else { return Ok(Settled::Lost) };
    let standing = standing(&conn.submissions_of(copy).await?);
    match standing {
        Standing::Left => return Ok(Settled::Sent),
        Standing::Held(_) => return Ok(Settled::Held),
        _ => {}
    }
    Ok(match (conn.copy_state(copy).await?, standing) {
        (CopyState::Gone, _) => Settled::Lost,
        // Cancelled and put back (a take-back that was cut short before the row was updated):
        // the user took it back, so it is not submitted again. The safe direction when a send
        // of the same copy follows an undo and its answer is lost: it is a draft, not sent.
        (CopyState::Draft, Standing::Cancelled) => Settled::Cancelled,
        (CopyState::Draft, _) => Settled::NotSent,
        (CopyState::Sent, Standing::Cancelled) => Settled::Cancelled,
        (CopyState::Sent, _) => Settled::Sent,
    })
}

/// Takes a held message back for a row that was thrown away. `true`: it is cancelled (or was)
/// and its copy can go; `false`: it has left.
async fn cancel_held(conn: &Connection, copy: &str) -> Result<bool> {
    match standing(&conn.submissions_of(copy).await?) {
        Standing::Held(held) => match conn.cancel_submission(&held.id).await {
            Ok(()) => Ok(true),
            Err(Error::Invalid { .. }) => Ok(false),
            Err(e) => Err(e),
        },
        Standing::Left => Ok(false),
        Standing::Cancelled | Standing::None => Ok(true),
    }
}

/// How long the server is asked to hold a message that goes out now: what is left of the
/// time the user was given (a send whose first try failed is not held longer than that).
fn hold_of(row: &DraftRow, now: i64) -> Option<Hold> {
    let left = row.send_at.filter(|at| *at > now)? - now;
    Some(Hold::For(u32::try_from(left).unwrap_or(MAX_HOLD_SECS).min(MAX_HOLD_SECS)))
}

/// The envelope a held message needs: the address of its sender and every recipient, once.
pub(super) fn held_for(store: &Store, account_id: &str, row: &DraftRow, hold: Hold) -> Result<Held> {
    let identities = store.identities_list(account_id)?;
    let Some(from) = identities.iter().find(|i| i.id == row.draft.identity_id) else {
        return Err(Error::invalid("choose an address to send from"));
    };
    let mut to: Vec<String> = Vec::new();
    for a in row.draft.to.iter().chain(&row.draft.cc).chain(&row.draft.bcc) {
        let email = a.email.trim();
        if !to.iter().any(|t| t.eq_ignore_ascii_case(email)) {
            to.push(email.to_string());
        }
    }
    Ok(Held { from: from.email.clone(), to, hold })
}

/// The cancelled message is a draft again on the server and here.
pub(super) async fn put_back(
    conn: &Connection,
    store: &Store,
    account_id: &str,
    row: &DraftRow,
    drafts_id: &str,
    changed: &mut bool,
) -> Result<()> {
    let Some(copy) = &row.draft.server_id else { return Ok(()) };
    match conn.restore_draft(copy, drafts_id).await {
        Ok(()) => drop(store.draft_back(account_id, &row.draft.id, false)?),
        // The copy is gone: the text is here, and the next save makes a new one.
        Err(Error::Invalid { .. }) => {
            store.draft_back(account_id, &row.draft.id, true)?;
            store.draft_copy_lost(account_id, &row.draft.id)?;
        }
        Err(e) => return Err(e),
    };
    *changed = true;
    Ok(())
}

/// Sends one message that is due, or that the user's call hands over now to be held back (see
/// `compose::later`). A refusal the server will repeat turns it back into a draft with the
/// reason; a failure that may pass leaves it for the next pass.
pub(super) async fn send_one(
    conn: &Connection,
    store: &Store,
    account_id: &str,
    mut row: DraftRow,
    drafts_id: &str,
    now: i64,
    changed: &mut bool,
) -> Result<()> {
    match row.state {
        // The server holds it; once its time has come, the submission says how it went.
        DraftState::Held => {
            return match settle(conn, &row).await? {
                Settled::Held => Ok(()),
                Settled::Cancelled => put_back(conn, store, account_id, &row, drafts_id, changed).await,
                // Gone or sent or not a draft any more: it was handed over, and a message the
                // server may still be sending is not sent again.
                _ => accepted(conn, store, account_id, &row, changed).await,
            };
        }
        // Nobody is working on it (a pass runs only while no call of the user does): the call
        // was cut short before it asked the server for anything.
        DraftState::Scheduling => {
            store.draft_revert(account_id, &row.draft.id)?;
            *changed = true;
            return Ok(());
        }
        DraftState::ScheduleSubmitting => {
            return settle_scheduled(conn, store, account_id, &row, drafts_id, changed).await
        }
        _ => {}
    }
    if row.state == DraftState::Submitting {
        match settle(conn, &row).await? {
            Settled::Sent => return accepted(conn, store, account_id, &row, changed).await,
            Settled::Held => return store.draft_held(account_id, &row.draft.id),
            Settled::Cancelled => return put_back(conn, store, account_id, &row, drafts_id, changed).await,
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
    let sent = sent.as_ref().map(|m| m.id.as_str());
    let held = match hold_of(&row, now) {
        Some(hold) => match held_for(store, account_id, &row, hold) {
            Ok(held) => Some(held),
            Err(Error::Invalid { msg }) => return refused(store, account_id, &row, &msg, changed),
            Err(e) => return Err(e),
        },
        None => None,
    };
    match conn.submit_with(&row.draft.identity_id, copy, drafts_id, sent, held.as_ref()).await {
        // Held by the server: the row stays until its time, for the undo.
        Ok(_) if held.is_some() => store.draft_held(account_id, &row.draft.id),
        Ok(_) => accepted(conn, store, account_id, &row, changed).await,
        Err(Error::Invalid { msg }) => refused(store, account_id, &row, &msg, changed),
        // The row stays `submitting`: the next pass asks the server how it went.
        Err(e) => Err(e),
    }
}

/// A scheduling that was not seen through: the server holds the message (the row is done), or
/// it does not (a draft again, never sent).
async fn settle_scheduled(
    conn: &Connection,
    store: &Store,
    account_id: &str,
    row: &DraftRow,
    drafts_id: &str,
    changed: &mut bool,
) -> Result<()> {
    match settle(conn, row).await? {
        Settled::Sent | Settled::Held => accepted(conn, store, account_id, row, changed).await,
        Settled::Cancelled => put_back(conn, store, account_id, row, drafts_id, changed).await,
        Settled::NotSent => {
            store.draft_revert(account_id, &row.draft.id)?;
            *changed = true;
            Ok(())
        }
        Settled::Lost => {
            store.draft_copy_lost(account_id, &row.draft.id)?;
            store.draft_revert(account_id, &row.draft.id)?;
            *changed = true;
            Ok(())
        }
    }
}

/// The server has the message and has sent it. The row goes first, so nothing that fails after
/// can make it go out again.
pub(super) async fn accepted(
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

pub(super) fn refused(store: &Store, account_id: &str, row: &DraftRow, why: &str, changed: &mut bool) -> Result<()> {
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
    // A card the user saved while the listing was on its way is newer than it: not replaced then.
    let generation = store.contact_generation();
    if let Ok(cards) = conn.contact_cards().await {
        let _ = store.contact_cards_replace(account_id, &cards, generation);
    }
    store.directory_done(now);
}
