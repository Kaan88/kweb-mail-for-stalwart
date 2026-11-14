//! Mark read, flag, archive, move, delete. Applied to the cache at once and queued in the store, so they
//! also work offline.
//!
//! The cache is optimistic: after `submit` it already shows the result. That hides where the
//! message really is on the server, which matters for Delete (move to Trash, or destroy when
//! it is already there). `submit` therefore decides while the cache still shows the truth and
//! queues the decision itself: a Delete of a message in Trash is queued as `Action::Destroy`.
//!
//! Queued actions and sync: a sync pass can write a server copy of a message over the
//! optimistic one (the message changed elsewhere meanwhile). [`reapply_pending`] puts the
//! queued actions back on the cache, and `sync` calls it whenever it wrote something, so the
//! notification rule never sees the message unread again just because a sync ran first.

use crate::{
    jmap::{ActionContext, Connection},
    model::Action,
    store::Store,
    Error, Result,
};

/// What an action does to the cache.
#[derive(Debug, Clone, PartialEq)]
enum Edit {
    Unread { id: String, unread: bool },
    Flagged { id: String, flagged: bool },
    Mailboxes { id: String, mailboxes: Vec<String> },
    Remove { id: String },
}

/// The cache change for an action.
fn edit_for(action: &Action, archive: Option<&str>, trash: Option<&str>) -> Result<Edit> {
    Ok(match action {
        Action::MarkRead { message_id } => Edit::Unread { id: message_id.clone(), unread: false },
        Action::MarkUnread { message_id } => Edit::Unread { id: message_id.clone(), unread: true },
        Action::Archive { message_id } => {
            let archive = archive.ok_or_else(|| Error::invalid("the account has no Archive mailbox"))?;
            Edit::Mailboxes { id: message_id.clone(), mailboxes: vec![archive.to_string()] }
        }
        Action::Delete { message_id } => {
            let trash = trash.ok_or_else(|| Error::invalid("the account has no Trash mailbox"))?;
            Edit::Mailboxes { id: message_id.clone(), mailboxes: vec![trash.to_string()] }
        }
        Action::Destroy { message_id } => Edit::Remove { id: message_id.clone() },
        Action::Flag { message_id } => Edit::Flagged { id: message_id.clone(), flagged: true },
        Action::Unflag { message_id } => Edit::Flagged { id: message_id.clone(), flagged: false },
        Action::Move { message_id, mailbox_id } => {
            Edit::Mailboxes { id: message_id.clone(), mailboxes: vec![mailbox_id.clone()] }
        }
    })
}

fn message_id(action: &Action) -> &str {
    match action {
        Action::MarkRead { message_id }
        | Action::MarkUnread { message_id }
        | Action::Archive { message_id }
        | Action::Delete { message_id }
        | Action::Destroy { message_id }
        | Action::Flag { message_id }
        | Action::Unflag { message_id }
        | Action::Move { message_id, .. } => message_id,
    }
}

/// A Delete of a message the cache shows in Trash is a Destroy; everything else stays as is.
fn resolve(action: &Action, trash: Option<&str>, cached_mailboxes: Option<&[String]>) -> Action {
    match (action, trash, cached_mailboxes) {
        (Action::Delete { message_id }, Some(trash), Some(mailboxes)) if mailboxes.iter().any(|m| m == trash) => {
            Action::Destroy { message_id: message_id.clone() }
        }
        _ => action.clone(),
    }
}

fn role_id(store: &Store, account_id: &str, role: &str) -> Result<Option<String>> {
    Ok(store.mailbox_by_role(account_id, role)?.map(|m| m.id))
}

fn write_edit(store: &Store, account_id: &str, edit: &Edit) -> Result<()> {
    match edit {
        Edit::Unread { id, unread } => store.set_unread(account_id, id, *unread).map(drop),
        Edit::Flagged { id, flagged } => store.set_flagged(account_id, id, *flagged).map(drop),
        Edit::Mailboxes { id, mailboxes } => store.set_message_mailboxes(account_id, id, mailboxes).map(drop),
        Edit::Remove { id } => store.delete_messages(account_id, std::slice::from_ref(id)),
    }
}

/// Applies the action to the cache at once (so the list and the notification rule see it)
/// and queues it for the server. A Delete of a message that is in Trash is queued as
/// `Action::Destroy`. Archive or Delete without such a mailbox is `Error::Invalid` and queues
/// nothing.
pub fn submit(store: &Store, account_id: &str, action: &Action) -> Result<()> {
    let archive = role_id(store, account_id, "archive")?;
    let trash = role_id(store, account_id, "trash")?;
    let cached = store.get_message(account_id, message_id(action))?;
    let action = resolve(action, trash.as_deref(), cached.as_ref().map(|m| m.mailbox_ids.as_slice()));
    let edit = edit_for(&action, archive.as_deref(), trash.as_deref())?;
    store.outbox_enqueue(account_id, &action)?;
    write_edit(store, account_id, &edit)
}

/// Submits the actions one after the other (a swipe on a conversation). Stops at the first
/// error and returns it; the ones before it stay queued and applied. Each action is its own
/// cache transaction, so a list read in between can show some of them.
pub fn submit_all(store: &Store, account_id: &str, actions: &[Action]) -> Result<()> {
    actions.iter().try_for_each(|action| submit(store, account_id, action))
}

/// Puts every queued action back on the cache. Called by `sync` after it wrote server data.
/// Actions that cannot be applied (the mailbox is gone) are skipped: `flush` will drop them.
pub(crate) fn reapply_pending(store: &Store, account_id: &str) -> Result<()> {
    let archive = role_id(store, account_id, "archive")?;
    let trash = role_id(store, account_id, "trash")?;
    for (_, action, _) in store.outbox_pending(account_id)? {
        if let Ok(edit) = edit_for(&action, archive.as_deref(), trash.as_deref()) {
            write_edit(store, account_id, &edit)?;
        }
    }
    Ok(())
}

/// What `Connection::apply` needs.
fn context(store: &Store, account_id: &str) -> Result<ActionContext> {
    Ok(ActionContext {
        current_mailbox_ids: Vec::new(),
        archive_id: role_id(store, account_id, "archive")?,
        trash_id: role_id(store, account_id, "trash")?,
    })
}

/// Sends queued actions to the server, oldest first. Returns how many were applied.
///
/// `Error::Invalid` (the message is gone, or the action can never work) drops the row so it
/// cannot block the queue, and the message is fetched again so the cache stops showing a
/// change that did not happen. Any other error keeps the row, counts the try and stops here with
/// that error: later actions must not overtake an earlier one. The caller renews the token or
/// retries later.
pub async fn flush(conn: &Connection, store: &Store, account_id: &str) -> Result<usize> {
    let mut applied = 0;
    for (row, action, _) in store.outbox_pending(account_id)? {
        let ctx = context(store, account_id)?;
        match conn.apply(&action, &ctx).await {
            Ok(()) => {
                store.outbox_done(row)?;
                applied += 1;
            }
            Err(Error::Invalid { .. }) => {
                store.outbox_done(row)?;
                undo_in_cache(conn, store, account_id, message_id(&action)).await;
            }
            Err(e) => {
                store.outbox_failed(row, &e.to_string())?;
                return Err(e);
            }
        }
    }
    Ok(applied)
}

/// Replaces the optimistic copy of a message by the server's, after an action on it was
/// dropped; later queued actions are put back on top. Best effort: if the server cannot be
/// asked now, the next change to the message corrects the cache.
async fn undo_in_cache(conn: &Connection, store: &Store, account_id: &str, message_id: &str) {
    let ids = [message_id.to_string()];
    let Ok((headers, _)) = conn.messages(&ids).await else { return };
    let written = if headers.is_empty() {
        store.delete_messages(account_id, &ids)
    } else {
        store.upsert_messages(account_id, &headers)
    };
    if written.is_ok() {
        let _ = reapply_pending(store, account_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(s: &str) -> String {
        s.to_string()
    }
    fn del() -> Action {
        Action::Delete { message_id: id("m") }
    }
    fn destroy() -> Action {
        Action::Destroy { message_id: id("m") }
    }

    #[test]
    fn read_and_unread_flip_the_flag() {
        let e = edit_for(&Action::MarkRead { message_id: id("m") }, None, None).unwrap();
        assert_eq!(e, Edit::Unread { id: id("m"), unread: false });
        let e = edit_for(&Action::MarkUnread { message_id: id("m") }, None, None).unwrap();
        assert_eq!(e, Edit::Unread { id: id("m"), unread: true });
    }

    #[test]
    fn archive_needs_an_archive_mailbox() {
        let a = Action::Archive { message_id: id("m") };
        let e = edit_for(&a, Some("arch"), None).unwrap();
        assert_eq!(e, Edit::Mailboxes { id: id("m"), mailboxes: vec![id("arch")] });
        assert!(matches!(edit_for(&a, None, Some("t")), Err(Error::Invalid { .. })));
    }

    #[test]
    fn delete_moves_to_trash_and_destroy_removes() {
        let e = edit_for(&del(), None, Some("t")).unwrap();
        assert_eq!(e, Edit::Mailboxes { id: id("m"), mailboxes: vec![id("t")] });
        assert_eq!(edit_for(&destroy(), None, None).unwrap(), Edit::Remove { id: id("m") });
    }

    #[test]
    fn flag_and_unflag_flip_the_flag() {
        let e = edit_for(&Action::Flag { message_id: id("m") }, None, None).unwrap();
        assert_eq!(e, Edit::Flagged { id: id("m"), flagged: true });
        let e = edit_for(&Action::Unflag { message_id: id("m") }, None, None).unwrap();
        assert_eq!(e, Edit::Flagged { id: id("m"), flagged: false });
    }

    #[test]
    fn move_puts_the_message_in_just_the_target() {
        let m = Action::Move { message_id: id("m"), mailbox_id: id("junk") };
        let e = edit_for(&m, None, None).unwrap();
        assert_eq!(e, Edit::Mailboxes { id: id("m"), mailboxes: vec![id("junk")] });
    }

    #[test]
    fn delete_needs_a_trash_mailbox() {
        assert!(matches!(edit_for(&del(), Some("a"), None), Err(Error::Invalid { .. })));
    }

    #[test]
    fn resolve_turns_delete_in_trash_into_destroy() {
        let in_trash = [id("t")];
        let elsewhere = [id("inbox")];
        assert_eq!(resolve(&del(), Some("t"), Some(&in_trash)), destroy());
        assert_eq!(resolve(&del(), Some("t"), Some(&elsewhere)), del());
        // Not cached, or no Trash mailbox: nothing says it is in Trash, so it stays a move.
        assert_eq!(resolve(&del(), Some("t"), None), del());
        assert_eq!(resolve(&del(), None, Some(&in_trash)), del());
        assert_eq!(resolve(&destroy(), Some("t"), Some(&elsewhere)), destroy());
    }
}
