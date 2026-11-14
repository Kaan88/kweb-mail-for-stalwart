//! Actions against the mock JMAP server: optimistic cache, queue, flush, and the interplay
//! with sync.

mod support;

use kweb_core::{
    actions,
    jmap::{self, Connection},
    model::{Account, Action},
    store::Store,
    sync, Error,
};
use support::*;

const ACCOUNT: &str = ACCOUNT_ID;
/// Seeded: Maria's unread mail in the Inbox, and the unread Build Bot mail.
const E1: &str = "e1";
const E2: &str = "e2";
/// Seeded: Tom's read mail in the Inbox.
const E3: &str = "e3";

async fn connect(server: &MockServer) -> Connection {
    Connection::connect(&server.url(), &server.issue_access_token()).await.expect("connect")
}

fn new_store(server: &MockServer) -> Store {
    let store = Store::open_in_memory().unwrap();
    let account =
        Account { id: ACCOUNT.into(), server_url: server.url(), username: server.username().into(), signed_in_at: 0 };
    store.save_account(&account).unwrap();
    store
}

/// A server, a connection and a store that has done its first sync.
async fn setup() -> (MockServer, Connection, Store) {
    let server = MockServer::start().await;
    let conn = connect(&server).await;
    let store = new_store(&server);
    sync::sync(&conn, &store, ACCOUNT).await.unwrap();
    (server, conn, store)
}

fn read(id: &str) -> Action {
    Action::MarkRead { message_id: id.into() }
}
fn unread(id: &str) -> Action {
    Action::MarkUnread { message_id: id.into() }
}
fn archive(id: &str) -> Action {
    Action::Archive { message_id: id.into() }
}
fn delete(id: &str) -> Action {
    Action::Delete { message_id: id.into() }
}

fn notifiable_ids(store: &Store) -> Vec<String> {
    store.notifiable(ACCOUNT, INBOX_ID, 0).unwrap().into_iter().map(|m| m.id).collect()
}

fn pending(store: &Store) -> usize {
    store.outbox_pending(ACCOUNT).unwrap().len()
}

#[tokio::test]
async fn mark_read_and_unread_reach_the_server() {
    let (server, conn, store) = setup().await;
    actions::submit(&store, ACCOUNT, &read(E1)).unwrap();
    assert_eq!(actions::flush(&conn, &store, ACCOUNT).await.unwrap(), 1);
    assert!(server.is_read(E1));
    assert_eq!(pending(&store), 0);

    actions::submit(&store, ACCOUNT, &unread(E1)).unwrap();
    assert_eq!(actions::flush(&conn, &store, ACCOUNT).await.unwrap(), 1);
    assert!(!server.is_read(E1));
}

#[tokio::test]
async fn archive_reaches_the_server() {
    let (server, conn, store) = setup().await;
    actions::submit(&store, ACCOUNT, &archive(E1)).unwrap();
    assert_eq!(store.get_message(ACCOUNT, E1).unwrap().unwrap().mailbox_ids, vec![ARCHIVE_ID.to_string()]);
    actions::flush(&conn, &store, ACCOUNT).await.unwrap();
    assert_eq!(server.roles_of(E1), ["archive"]);
}

#[tokio::test]
async fn delete_moves_to_trash_and_a_second_delete_destroys() {
    let (server, conn, store) = setup().await;
    actions::submit(&store, ACCOUNT, &delete(E1)).unwrap();
    assert_eq!(store.get_message(ACCOUNT, E1).unwrap().unwrap().mailbox_ids, vec![TRASH_ID.to_string()]);
    actions::flush(&conn, &store, ACCOUNT).await.unwrap();
    assert_eq!(server.roles_of(E1), ["trash"]);

    // Deleting from Trash removes the message from the cache and destroys it on the server.
    actions::submit(&store, ACCOUNT, &delete(E1)).unwrap();
    assert!(store.get_message(ACCOUNT, E1).unwrap().is_none());
    actions::flush(&conn, &store, ACCOUNT).await.unwrap();
    assert!(!server.exists(E1));
}

#[tokio::test]
async fn delete_in_trash_queues_a_destroy_and_elsewhere_a_move() {
    let (server, conn, store) = setup().await;
    server.move_to(E2, "trash");
    sync::sync(&conn, &store, ACCOUNT).await.unwrap();

    actions::submit(&store, ACCOUNT, &delete(E2)).unwrap();
    actions::submit(&store, ACCOUNT, &delete(E3)).unwrap();
    let queued: Vec<Action> = store.outbox_pending(ACCOUNT).unwrap().into_iter().map(|r| r.1).collect();
    assert_eq!(queued, [Action::Destroy { message_id: E2.into() }, delete(E3)]);

    assert_eq!(actions::flush(&conn, &store, ACCOUNT).await.unwrap(), 2);
    assert!(!server.exists(E2), "destroyed");
    assert_eq!(server.roles_of(E3), ["trash"], "moved");
}

#[tokio::test]
async fn destroy_submitted_directly_removes_the_message() {
    let (server, conn, store) = setup().await;
    actions::submit(&store, ACCOUNT, &Action::Destroy { message_id: E1.into() }).unwrap();
    assert!(store.get_message(ACCOUNT, E1).unwrap().is_none());
    assert_eq!(actions::flush(&conn, &store, ACCOUNT).await.unwrap(), 1);
    assert!(!server.exists(E1));
}

#[tokio::test]
async fn delete_of_a_message_trashed_elsewhere_destroys_it() {
    let (server, conn, store) = setup().await;
    server.move_to(E2, "trash");
    sync::sync(&conn, &store, ACCOUNT).await.unwrap();
    actions::submit(&store, ACCOUNT, &delete(E2)).unwrap();
    actions::flush(&conn, &store, ACCOUNT).await.unwrap();
    assert!(!server.exists(E2));
}

#[tokio::test]
async fn a_queued_destroy_survives_a_sync_that_brings_the_message_back() {
    let (server, conn, store) = setup().await;
    server.move_to(E2, "trash");
    sync::sync(&conn, &store, ACCOUNT).await.unwrap();

    server.set_down(true);
    actions::submit(&store, ACCOUNT, &delete(E2)).unwrap();
    server.set_down(false);
    // Another change to the same message makes sync fetch it again, over the optimistic removal.
    server.mark_read(E2);
    sync::sync(&conn, &store, ACCOUNT).await.unwrap();
    assert!(store.get_message(ACCOUNT, E2).unwrap().is_none(), "the pending delete is shown again");

    actions::flush(&conn, &store, ACCOUNT).await.unwrap();
    assert!(!server.exists(E2), "still a destroy, not a second move to Trash");
}

#[tokio::test]
async fn submit_without_the_needed_mailbox_is_invalid_and_queues_nothing() {
    let server = MockServer::start().await;
    let store = new_store(&server); // no mailboxes cached
    for action in [archive(E1), delete(E1)] {
        let err = actions::submit(&store, ACCOUNT, &action).unwrap_err();
        assert!(matches!(err, Error::Invalid { .. }), "got {err:?}");
    }
    assert_eq!(pending(&store), 0);
}

#[tokio::test]
async fn submit_removes_the_notification_at_once_even_offline() {
    let (server, _conn, store) = setup().await;
    server.set_down(true);
    assert_eq!(notifiable_ids(&store), [E1, E2]);

    actions::submit(&store, ACCOUNT, &read(E1)).unwrap();
    assert_eq!(notifiable_ids(&store), [E2]);
    actions::submit(&store, ACCOUNT, &archive(E2)).unwrap();
    assert!(notifiable_ids(&store).is_empty());

    // Marking it unread again is a new reason to notify.
    actions::submit(&store, ACCOUNT, &unread(E1)).unwrap();
    assert_eq!(notifiable_ids(&store), [E1]);
    actions::submit(&store, ACCOUNT, &delete(E1)).unwrap();
    assert!(notifiable_ids(&store).is_empty());
}

#[tokio::test]
async fn actions_queued_while_down_are_flushed_in_order() {
    let (server, conn, store) = setup().await;
    server.set_down(true);
    actions::submit(&store, ACCOUNT, &read(E2)).unwrap();
    actions::submit(&store, ACCOUNT, &unread(E2)).unwrap();
    actions::submit(&store, ACCOUNT, &archive(E1)).unwrap();
    actions::submit(&store, ACCOUNT, &read(E1)).unwrap();

    let err = actions::flush(&conn, &store, ACCOUNT).await.unwrap_err();
    assert!(matches!(err, Error::Temporary { .. }));
    assert!(!jmap::is_unauthorized(&err));
    let rows = store.outbox_pending(ACCOUNT).unwrap();
    assert_eq!(rows.len(), 4, "nothing is lost");
    assert_eq!(rows.iter().map(|r| r.2).collect::<Vec<_>>(), [1, 0, 0, 0], "it stopped at the first row");
    assert!(store.outbox_last_error(rows[0].0).unwrap().is_some());

    server.set_down(false);
    assert_eq!(actions::flush(&conn, &store, ACCOUNT).await.unwrap(), 4);
    assert_eq!(pending(&store), 0);
    // Read then unread: the later action won, so order was kept.
    assert!(!server.is_read(E2));
    assert!(server.is_read(E1));
    assert_eq!(server.roles_of(E1), ["archive"]);
}

#[tokio::test]
async fn an_expired_token_stops_the_flush_with_an_unauthorized_error() {
    let (server, conn, store) = setup().await;
    actions::submit(&store, ACCOUNT, &read(E1)).unwrap();
    server.expire_access_tokens();
    let err = actions::flush(&conn, &store, ACCOUNT).await.unwrap_err();
    assert!(jmap::is_unauthorized(&err), "got {err:?}");
    assert_eq!(pending(&store), 1);

    let conn = connect(&server).await;
    assert_eq!(actions::flush(&conn, &store, ACCOUNT).await.unwrap(), 1);
    assert!(server.is_read(E1));
}

#[tokio::test]
async fn an_action_on_a_message_destroyed_elsewhere_is_dropped_and_does_not_block() {
    let (server, conn, store) = setup().await;
    server.destroy(E1);
    actions::submit(&store, ACCOUNT, &read(E1)).unwrap();
    actions::submit(&store, ACCOUNT, &archive(E1)).unwrap();
    actions::submit(&store, ACCOUNT, &read(E2)).unwrap();

    assert_eq!(actions::flush(&conn, &store, ACCOUNT).await.unwrap(), 1);
    assert_eq!(pending(&store), 0);
    assert!(server.is_read(E2));
}

#[tokio::test]
async fn submit_offline_other_device_changes_sync_then_flush() {
    let (server, conn, store) = setup().await;
    server.set_down(true);
    actions::submit(&store, ACCOUNT, &read(E1)).unwrap();
    actions::submit(&store, ACCOUNT, &archive(E2)).unwrap();
    server.set_down(false);

    // Meanwhile on another device: new mail, E1 marked unread again, E2 moved to Junk.
    let new = server.deliver("Zed", "zed@example.test", "Hello", "Hi");
    server.mark_unread(E1);
    server.move_to(E2, "junk");

    let out = sync::sync(&conn, &store, ACCOUNT).await.unwrap();
    assert!(out.changed);
    // The new mail is there; the two handled mails did not come back as notifications.
    assert_eq!(notifiable_ids(&store), std::slice::from_ref(&new));
    assert!(!store.get_message(ACCOUNT, E1).unwrap().unwrap().is_unread);
    assert_eq!(store.get_message(ACCOUNT, E2).unwrap().unwrap().mailbox_ids, vec![ARCHIVE_ID.to_string()]);

    assert_eq!(actions::flush(&conn, &store, ACCOUNT).await.unwrap(), 2);
    assert!(server.is_read(E1));
    assert_eq!(server.roles_of(E2), ["archive"]);
    assert!(!server.is_read(&new));

    // A pass after the flush agrees with the cache and changes nothing visible.
    sync::sync(&conn, &store, ACCOUNT).await.unwrap();
    assert_eq!(notifiable_ids(&store), [new]);
    assert!(store.get_message(ACCOUNT, E3).unwrap().is_some());
}

fn flag(id: &str) -> Action {
    Action::Flag { message_id: id.into() }
}
fn unflag(id: &str) -> Action {
    Action::Unflag { message_id: id.into() }
}
fn move_to(id: &str, mailbox: &str) -> Action {
    Action::Move { message_id: id.into(), mailbox_id: mailbox.into() }
}

#[tokio::test]
async fn flag_and_unflag_reach_the_server() {
    let (server, conn, store) = setup().await;
    actions::submit(&store, ACCOUNT, &flag(E1)).unwrap();
    assert!(store.get_message(ACCOUNT, E1).unwrap().unwrap().is_flagged);
    actions::flush(&conn, &store, ACCOUNT).await.unwrap();
    assert!(server.keywords_of(E1).contains(&"$flagged".to_string()));

    actions::submit(&store, ACCOUNT, &unflag(E1)).unwrap();
    assert!(!store.get_message(ACCOUNT, E1).unwrap().unwrap().is_flagged);
    actions::flush(&conn, &store, ACCOUNT).await.unwrap();
    assert!(!server.keywords_of(E1).contains(&"$flagged".to_string()));
    assert_eq!(pending(&store), 0);
}

#[tokio::test]
async fn move_reaches_the_server_and_leaves_the_other_mailboxes() {
    let (server, conn, store) = setup().await;
    actions::submit(&store, ACCOUNT, &move_to(E1, JUNK_ID)).unwrap();
    assert_eq!(store.get_message(ACCOUNT, E1).unwrap().unwrap().mailbox_ids, vec![JUNK_ID.to_string()]);
    actions::flush(&conn, &store, ACCOUNT).await.unwrap();
    assert_eq!(server.mailboxes_of(E1), [JUNK_ID]);

    // Back out of Junk (and Archive and Delete are undone the same way).
    actions::submit(&store, ACCOUNT, &move_to(E1, INBOX_ID)).unwrap();
    actions::flush(&conn, &store, ACCOUNT).await.unwrap();
    assert_eq!(server.mailboxes_of(E1), [INBOX_ID]);
}

#[tokio::test]
async fn queued_flag_and_move_survive_a_sync_that_overwrites_the_cache() {
    let (server, conn, store) = setup().await;
    server.set_down(true);
    actions::submit(&store, ACCOUNT, &flag(E2)).unwrap();
    actions::submit(&store, ACCOUNT, &move_to(E1, JUNK_ID)).unwrap();
    server.set_down(false);
    // Changes elsewhere make sync fetch both messages again, over the optimistic copies.
    server.mark_read(E1);
    server.mark_read(E2);
    sync::sync(&conn, &store, ACCOUNT).await.unwrap();
    assert!(store.get_message(ACCOUNT, E2).unwrap().unwrap().is_flagged);
    assert_eq!(store.get_message(ACCOUNT, E1).unwrap().unwrap().mailbox_ids, vec![JUNK_ID.to_string()]);

    assert_eq!(actions::flush(&conn, &store, ACCOUNT).await.unwrap(), 2);
    assert!(server.keywords_of(E2).contains(&"$flagged".to_string()));
    assert_eq!(server.mailboxes_of(E1), [JUNK_ID]);
}

#[tokio::test]
async fn submit_all_applies_in_order_and_stops_at_the_first_error() {
    let (server, conn, store) = setup().await;
    actions::submit_all(&store, ACCOUNT, &[read(E1), flag(E1), archive(E2)]).unwrap();
    assert_eq!(pending(&store), 3);
    actions::flush(&conn, &store, ACCOUNT).await.unwrap();
    assert!(server.is_read(E1));
    assert_eq!(server.roles_of(E2), ["archive"]);

    // No Archive mailbox is cached: the second action fails, the first stays queued, the
    // third is never tried.
    let bare = new_store(&server);
    bare.upsert_messages(ACCOUNT, &[store.get_message(ACCOUNT, E3).unwrap().unwrap()]).unwrap();
    let err = actions::submit_all(&bare, ACCOUNT, &[read(E3), archive(E3), flag(E3)]).unwrap_err();
    assert!(matches!(err, Error::Invalid { .. }), "got {err:?}");
    assert_eq!(bare.outbox_pending(ACCOUNT).unwrap().len(), 1);
    assert!(!bare.get_message(ACCOUNT, E3).unwrap().unwrap().is_flagged);
    assert!(actions::submit_all(&bare, ACCOUNT, &[]).is_ok());
}
