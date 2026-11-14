//! What the user's own calls do with the server for a message on its way out: hand it over to
//! be held back for the undo window, take it back, schedule it for a later time, list and cancel
//! the scheduled ones.
//!
//! They use the steps of a pass (`flush`): the copy that is stored is the copy that is
//! submitted, the row says what was started, and a send whose answer was lost is settled by what
//! the server holds. The caller holds `Store::send_lock`, so no pass works on the same rows
//! meanwhile, and nothing here holds the store's own lock across the network.

use std::collections::HashSet;

use super::flush::{accepted, held_for, make_copy, put_back, refused, send_one, standing, Standing};
use crate::{
    jmap::{is_unauthorized, Connection, CopyState, Hold, Undo},
    model::{Draft, ScheduledSend},
    store::{DraftRow, DraftState, Store},
    Error, Result,
};

/// The shortest time ahead a message can be scheduled, in seconds.
pub const SCHEDULE_MIN_SECS: i64 = 60;
/// The longest the server holds a message back, in seconds (seven days).
pub const SCHEDULE_MAX_SECS: i64 = crate::jmap::MAX_HOLD_SECS as i64;

/// Whether `send_at` is a time the server will hold a message until.
pub fn check_schedule(send_at: i64, now: i64) -> Result<()> {
    if send_at < now + SCHEDULE_MIN_SECS {
        return Err(Error::invalid("choose a time at least a minute from now"));
    }
    if send_at > now + SCHEDULE_MAX_SECS {
        return Err(Error::invalid("the server holds a message for seven days at most: choose an earlier time"));
    }
    Ok(())
}

fn too_late() -> Error {
    Error::invalid("this message cannot be taken back: it was sent, or is not being sent")
}

fn no_drafts_folder() -> Error {
    Error::temporary("the folders are not loaded yet: try again in a moment")
}

/// Hands a message that was just sent (in its undo window) to the server, which holds it for
/// what is left of the window. The row stays, so the send can be undone and still counts as
/// waiting. Does nothing for a row that is not just sent; whatever fails, the row is as the
/// pass knows how to go on with.
pub async fn hand_over(conn: &Connection, store: &Store, account_id: &str, draft_id: &str, now: i64) -> Result<()> {
    let Some(drafts) = store.mailbox_by_role(account_id, "drafts")? else { return Ok(()) };
    let Some(row) = store.draft_get(account_id, draft_id)? else { return Ok(()) };
    if row.state != DraftState::Sending {
        return Ok(());
    }
    send_one(conn, store, account_id, row, &drafts.id, now, &mut false).await
}

/// Takes a message back from the server and gives it back as a draft: the submission is
/// cancelled, and the message is put back in Drafts as a draft. Too late (it has left) is
/// `Error::Invalid`. When the cancel worked but the message could not be moved, the draft is
/// given back all the same and the next pass makes a new copy in Drafts (and removes the old
/// one from Sent): nothing is lost.
pub async fn take_back(conn: &Connection, store: &Store, account_id: &str, draft_id: &str) -> Result<Draft> {
    let Some(row) = store.draft_get(account_id, draft_id)? else { return Err(too_late()) };
    if !matches!(row.state, DraftState::Held | DraftState::Submitting) {
        return Err(too_late());
    }
    let Some(drafts) = store.mailbox_by_role(account_id, "drafts")? else { return Err(no_drafts_folder()) };
    let Some(copy) = row.draft.server_id.clone() else { return Err(too_late()) };
    let mut changed = false;
    match standing(&conn.submissions_of(&copy).await?) {
        Standing::Held(held) => match conn.cancel_submission(&held.id).await {
            Ok(()) => {}
            // It left in the meantime: the row has done its work.
            Err(e @ Error::Invalid { .. }) => {
                accepted(conn, store, account_id, &row, &mut changed).await?;
                return Err(e);
            }
            Err(e) => return Err(e),
        },
        Standing::Left => {
            accepted(conn, store, account_id, &row, &mut changed).await?;
            return Err(too_late());
        }
        // Cancelled already (here, or on another device): only the putting back is left.
        Standing::Cancelled => {}
        // The server has no submission. A send that never got there left the copy a draft: it
        // is the user's again. A held message without one has been dealt with elsewhere.
        Standing::None if row.state == DraftState::Submitting => {
            return match conn.copy_state(&copy).await? {
                CopyState::Draft => store.draft_back(account_id, draft_id, false)?.ok_or_else(too_late),
                CopyState::Gone => store.draft_back(account_id, draft_id, true)?.ok_or_else(too_late),
                CopyState::Sent => {
                    accepted(conn, store, account_id, &row, &mut changed).await?;
                    Err(too_late())
                }
            };
        }
        Standing::None => {
            accepted(conn, store, account_id, &row, &mut changed).await?;
            return Err(too_late());
        }
    }
    // Cancelled. Put back in Drafts; when that fails the message is in Sent, and the draft is
    // given back as one that needs a new copy: the next pass makes it in Drafts and removes the
    // old one, so nothing is lost either way.
    let moved = conn.restore_draft(&copy, &drafts.id).await.is_ok();
    store.draft_back(account_id, draft_id, !moved)?.ok_or_else(too_late)
}

/// Makes the server hold the message until `send_at`. The draft must have been stored as
/// `Scheduling` (`Store::draft_schedule`). Steps: the copy, then the submission with the hold;
/// the row is gone when the server holds the message. When the answer to the submission is
/// lost the row says so (`ScheduleSubmitting`) and the server decides: a message it holds is
/// done, one it does not is a draft again, never sent.
pub async fn schedule(
    conn: &Connection,
    store: &Store,
    account_id: &str,
    draft_id: &str,
    send_at: i64,
) -> Result<ScheduledSend> {
    let Some(mut row) = store.draft_get(account_id, draft_id)? else {
        return Err(Error::invalid("this draft is gone: it was sent or thrown away"));
    };
    let revert = |e: Error| -> Error {
        match store.draft_revert(account_id, draft_id) {
            Ok(()) => e,
            Err(store_error) => store_error,
        }
    };
    let Some(drafts) = store.mailbox_by_role(account_id, "drafts")? else {
        return Err(if row.state == DraftState::Scheduling { revert(no_drafts_folder()) } else { no_drafts_folder() });
    };
    match row.state {
        DraftState::Scheduling => {}
        // A second try after a request whose answer was lost (a renewed sign-in runs this again).
        DraftState::ScheduleSubmitting => return confirm(conn, store, account_id, &row, &drafts.id, send_at).await,
        _ => return Err(Error::invalid("this message is being sent: undo the send to change it")),
    }
    if row.dirty || row.draft.server_id.is_none() {
        match make_copy(conn, store, account_id, &row, &drafts.id).await {
            Ok(copy) => row.draft.server_id = Some(copy),
            Err(Error::Invalid { msg }) => {
                store.draft_refused(account_id, draft_id, &msg)?;
                return Err(Error::invalid(msg));
            }
            // A 401 is handed back: the caller renews the token and runs this again.
            Err(e) if is_unauthorized(&e) => return Err(e),
            Err(e) => return Err(revert(e)),
        }
    }
    let Some(copy) = row.draft.server_id.clone() else { return Err(revert(Error::temporary("no copy was made"))) };
    // The very version that was copied, or nothing is submitted.
    if !store.draft_begin_schedule(account_id, draft_id, row.rev, &copy)? {
        return Err(revert(Error::invalid("this draft was changed or thrown away: it was not scheduled")));
    }
    let held = match held_for(store, account_id, &row, Hold::Until(send_at)) {
        Ok(held) => held,
        Err(e) => return Err(revert(e)),
    };
    let sent = store.mailbox_by_role(account_id, "sent")?;
    let sent = sent.as_ref().map(|m| m.id.as_str());
    match conn.submit_with(&row.draft.identity_id, &copy, &drafts.id, sent, Some(&held)).await {
        Ok(submission) => {
            accepted(conn, store, account_id, &row, &mut false).await?;
            Ok(scheduled(&row, &submission, send_at))
        }
        Err(Error::Invalid { msg }) => {
            refused(store, account_id, &row, &msg, &mut false)?;
            Err(Error::invalid(msg))
        }
        // A 401 is handed back: the caller renews the token and this runs again, from the state
        // the row is in now.
        Err(e) if is_unauthorized(&e) => Err(e),
        Err(e) => match confirm(conn, store, account_id, &row, &drafts.id, send_at).await {
            Ok(done) => Ok(done),
            // The server could not be asked either: the row stays, and the next pass settles it.
            Err(Error::Temporary { .. }) if store.draft_get(account_id, draft_id)?.is_some_and(|r| r.state == DraftState::ScheduleSubmitting) => {
                Err(Error::temporary(format!(
                    "the server did not answer ({e}): the message is held back or still a draft, and the next check settles which; it is never sent twice"
                )))
            }
            Err(other) => Err(other),
        },
    }
}

fn scheduled(row: &DraftRow, submission: &str, send_at: i64) -> ScheduledSend {
    let mut to: Vec<String> = Vec::new();
    for a in row.draft.to.iter().chain(&row.draft.cc).chain(&row.draft.bcc) {
        if !to.iter().any(|t| t.eq_ignore_ascii_case(a.email.trim())) {
            to.push(a.email.trim().to_string());
        }
    }
    ScheduledSend { id: submission.to_string(), subject: row.draft.subject.clone(), to, send_at }
}

/// Asks the server what became of a scheduling whose answer was lost.
async fn confirm(
    conn: &Connection,
    store: &Store,
    account_id: &str,
    row: &DraftRow,
    drafts_id: &str,
    send_at: i64,
) -> Result<ScheduledSend> {
    let Some(copy) = &row.draft.server_id else { return Err(Error::temporary("no copy was made")) };
    let submissions = conn.submissions_of(copy).await?;
    if let Some(held) = submissions.iter().find(|s| s.undo != Undo::Canceled) {
        accepted(conn, store, account_id, row, &mut false).await?;
        return Ok(scheduled(row, &held.id, if held.send_at > 0 { held.send_at } else { send_at }));
    }
    let nothing = || Error::temporary("the server did not hold the message: it is a draft again");
    match conn.copy_state(copy).await? {
        CopyState::Draft => {
            store.draft_revert(account_id, &row.draft.id)?;
        }
        CopyState::Gone => {
            store.draft_copy_lost(account_id, &row.draft.id)?;
            store.draft_revert(account_id, &row.draft.id)?;
        }
        // Cancelled, and not a draft any more: put back.
        CopyState::Sent => {
            put_back(conn, store, account_id, row, drafts_id, &mut false).await?;
        }
    }
    Err(nothing())
}

/// The messages the server holds back, soonest first. The ones that wait out the undo window of
/// a send are not scheduled messages and are left out.
pub async fn scheduled_list(conn: &Connection, store: &Store, account_id: &str) -> Result<Vec<ScheduledSend>> {
    let skip: HashSet<String> = store.held_copy_ids(account_id)?.into_iter().collect();
    let pending: Vec<_> =
        conn.pending_submissions().await?.into_iter().filter(|s| !skip.contains(&s.email_id)).collect();
    let ids: Vec<String> = pending.iter().map(|s| s.email_id.clone()).collect();
    let summaries = conn.email_summaries(&ids).await?;
    let mut out: Vec<ScheduledSend> = pending
        .iter()
        .map(|s| {
            let found = summaries.iter().find(|m| m.id == s.email_id);
            ScheduledSend {
                id: s.id.clone(),
                subject: found.map(|m| m.subject.clone()).unwrap_or_default(),
                to: found.map(|m| m.recipients.clone()).unwrap_or_default(),
                send_at: s.send_at,
            }
        })
        .collect();
    out.sort_by(|a, b| (a.send_at, &a.id).cmp(&(b.send_at, &b.id)));
    Ok(out)
}

/// The key (followed by the message's id on the server) of a note that a cancelled scheduled
/// message may still have to be put back in Drafts: the cancel and the putting back are two
/// requests, and the message sits in Sent Items, looking sent, until the second is done.
const RESTORE_OWED: &str = "restore_owed:";

/// Finishes what `cancel_scheduled` left half done, for the notes it keeps: a message whose
/// submission is all cancelled and which is not a draft is put back in Drafts. A note about a
/// message that is held again, has left, or is a draft or gone is dropped. Called by a pass
/// that holds `Store::send_lock`. `true` when a message was put back.
pub async fn finish_restores(conn: &Connection, store: &Store, account_id: &str, drafts_id: &str) -> Result<bool> {
    let mut restored = false;
    for key in store.kv_keys_with_prefix(account_id, RESTORE_OWED)? {
        let email_id = &key[RESTORE_OWED.len()..];
        let standing = standing(&conn.submissions_of(email_id).await?);
        if matches!(standing, Standing::Cancelled) && matches!(conn.copy_state(email_id).await?, CopyState::Sent) {
            match conn.restore_draft(email_id, drafts_id).await {
                Ok(()) => restored = true,
                Err(Error::Invalid { .. }) => {}
                Err(e) => return Err(e),
            }
        }
        // Not cancelled (the cancel did not happen) or done: nothing is owed any more.
        store.kv_delete(account_id, &key)?;
    }
    Ok(restored)
}

/// Cancels a held message and makes it a draft on the server. Returns the message's id there.
/// Too late (it has left) is `Error::Invalid`. When the cancel worked but the message could not
/// be moved, the error says so and where the message is; calling this again finishes the move.
pub async fn cancel_scheduled(conn: &Connection, store: &Store, account_id: &str, id: &str) -> Result<String> {
    let Some(drafts) = store.mailbox_by_role(account_id, "drafts")? else { return Err(no_drafts_folder()) };
    let Some(submission) = conn.submission(id).await? else {
        return Err(Error::invalid("the server holds no such message"));
    };
    let owed = format!("{RESTORE_OWED}{}", submission.email_id);
    match submission.undo {
        Undo::Pending => {
            // Written before the cancel: whatever happens after it (a lost answer, a dead
            // process), the passes know the message may need putting back.
            store.kv_set(account_id, &owed, id)?;
            match conn.cancel_submission(id).await {
                Ok(()) => {}
                Err(e @ Error::Invalid { .. }) => {
                    store.kv_delete(account_id, &owed)?;
                    return Err(e);
                }
                // Not known whether it was cancelled: the record stays and a pass finds out.
                Err(e) => return Err(e),
            }
        }
        Undo::Final => return Err(Error::invalid("too late: the server has already sent it")),
        // Cancelled by an earlier try: only the putting back is left, unless the message was
        // scheduled again since.
        Undo::Canceled => {
            if conn.submissions_of(&submission.email_id).await?.iter().any(|s| s.undo != Undo::Canceled) {
                return Err(Error::invalid("this message was scheduled again: cancel that one"));
            }
            store.kv_set(account_id, &owed, id)?;
        }
    }
    match conn.restore_draft(&submission.email_id, &drafts.id).await {
        Ok(()) => {
            store.kv_delete(account_id, &owed)?;
            Ok(submission.email_id)
        }
        Err(Error::Invalid { .. }) => {
            store.kv_delete(account_id, &owed)?;
            Err(Error::invalid(
                "the send is cancelled, but the message is gone from the server: there is nothing to get back",
            ))
        }
        Err(e) => Err(Error::temporary(format!(
            "the send is cancelled and nothing will be sent, but the message could not be moved back to Drafts ({e}): \
             it is in Sent Items for now; try again to get it back as a draft"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_message_is_scheduled_between_a_minute_and_seven_days_ahead() {
        let now = 1_000_000;
        assert!(check_schedule(now + 60, now).is_ok());
        assert!(check_schedule(now + 7 * 86_400, now).is_ok());
        for bad in [now - 5, now, now + 59, now + 7 * 86_400 + 1] {
            assert!(matches!(check_schedule(bad, now), Err(Error::Invalid { .. })), "{bad}");
        }
    }
}
