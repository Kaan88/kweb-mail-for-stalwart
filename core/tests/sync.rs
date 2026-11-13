//! Sync against the mock JMAP server: first fill, incremental changes, full resync, outages
//! and expired tokens.

mod support;

use std::collections::BTreeSet;

use kweb_core::{
    jmap::{self, Connection},
    model::Account,
    store::Store,
    sync::{self, INBOX_WINDOW, OTHER_WINDOW},
};
use support::*;

const ACCOUNT: &str = ACCOUNT_ID;

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

async fn setup() -> (MockServer, Connection, Store) {
    let server = MockServer::start().await;
    let conn = connect(&server).await;
    let store = new_store(&server);
    (server, conn, store)
}

/// Ids of the messages the cache lists for a mailbox.
fn cached(store: &Store, mailbox: &str) -> BTreeSet<String> {
    store.list_messages(ACCOUNT, mailbox, 10_000, 0).unwrap().into_iter().map(|m| m.id).collect()
}

/// Ids the server has in a mailbox.
fn on_server(server: &MockServer, mailbox: &str) -> BTreeSet<String> {
    server.all_message_ids().into_iter().filter(|id| server.mailboxes_of(id).iter().any(|m| m == mailbox)).collect()
}

/// The cache equals the server in every mailbox, including read state.
fn assert_cache_matches(server: &MockServer, store: &Store) {
    for mailbox in [INBOX_ID, ARCHIVE_ID, TRASH_ID, DRAFTS_ID, SENT_ID, JUNK_ID] {
        assert_eq!(cached(store, mailbox), on_server(server, mailbox), "mailbox {mailbox}");
    }
    for id in server.all_message_ids() {
        let m = store.get_message(ACCOUNT, &id).unwrap().unwrap_or_else(|| panic!("{id} not cached"));
        assert_eq!(!m.is_unread, server.is_read(&id), "read state of {id}");
    }
}

#[tokio::test]
async fn first_sync_fills_mailboxes_and_messages() {
    let (server, conn, store) = setup().await;
    let out = sync::sync(&conn, &store, ACCOUNT).await.unwrap();
    assert!(out.changed && out.full_resync);

    let boxes = store.list_mailboxes(ACCOUNT).unwrap();
    assert_eq!(boxes.len(), 6);
    assert_eq!(store.mailbox_by_role(ACCOUNT, "inbox").unwrap().unwrap().id, INBOX_ID);
    assert_eq!(cached(&store, INBOX_ID).len(), 3);
    assert_cache_matches(&server, &store);
    assert!(store.get_state(ACCOUNT, "Mailbox").unwrap().is_some());
    assert_eq!(store.get_state(ACCOUNT, "Email").unwrap().unwrap(), server.email_state());
}

#[tokio::test]
async fn a_second_pass_with_nothing_new_changes_nothing() {
    let (_server, conn, store) = setup().await;
    sync::sync(&conn, &store, ACCOUNT).await.unwrap();
    let out = sync::sync(&conn, &store, ACCOUNT).await.unwrap();
    assert_eq!(out, sync::Outcome { changed: false, full_resync: false });
}

#[tokio::test]
async fn incremental_sync_follows_deliver_read_move_and_destroy() {
    let (server, conn, store) = setup().await;
    sync::sync(&conn, &store, ACCOUNT).await.unwrap();

    let new = server.deliver("Zed", "zed@example.test", "Hello", "Hi there");
    let out = sync::sync(&conn, &store, ACCOUNT).await.unwrap();
    assert!(out.changed && !out.full_resync);
    let m = store.get_message(ACCOUNT, &new).unwrap().unwrap();
    assert_eq!((m.subject.as_str(), m.is_unread, m.mailbox_ids.clone()), ("Hello", true, vec![INBOX_ID.to_string()]));

    server.mark_read(&new);
    assert!(sync::sync(&conn, &store, ACCOUNT).await.unwrap().changed);
    assert!(!store.get_message(ACCOUNT, &new).unwrap().unwrap().is_unread);

    server.move_to(&new, "archive");
    assert!(sync::sync(&conn, &store, ACCOUNT).await.unwrap().changed);
    assert_eq!(store.get_message(ACCOUNT, &new).unwrap().unwrap().mailbox_ids, vec![ARCHIVE_ID.to_string()]);

    server.destroy(&new);
    assert!(sync::sync(&conn, &store, ACCOUNT).await.unwrap().changed);
    assert!(store.get_message(ACCOUNT, &new).unwrap().is_none());
    assert_cache_matches(&server, &store);
}

#[tokio::test]
async fn a_message_created_and_destroyed_between_passes_leaves_no_trace() {
    let (server, conn, store) = setup().await;
    sync::sync(&conn, &store, ACCOUNT).await.unwrap();
    let id = server.deliver("Zed", "zed@example.test", "Gone", "Soon");
    server.destroy(&id);
    sync::sync(&conn, &store, ACCOUNT).await.unwrap();
    assert!(store.get_message(ACCOUNT, &id).unwrap().is_none());
    assert_cache_matches(&server, &store);
}

#[tokio::test]
async fn cannot_calculate_changes_triggers_a_full_resync() {
    let (server, conn, store) = setup().await;
    sync::sync(&conn, &store, ACCOUNT).await.unwrap();

    let new = server.deliver("Zed", "zed@example.test", "Hello", "Hi");
    server.mark_read("e1");
    server.destroy("e3");
    server.move_to("e2", "junk");
    server.forget_changes_before_now();

    let out = sync::sync(&conn, &store, ACCOUNT).await.unwrap();
    assert!(out.changed && out.full_resync);
    assert!(store.get_message(ACCOUNT, &new).unwrap().is_some());
    assert!(store.get_message(ACCOUNT, "e3").unwrap().is_none());
    assert_cache_matches(&server, &store);
    assert_eq!(store.get_state(ACCOUNT, "Email").unwrap().unwrap(), server.email_state());

    // And the pass after that is incremental again.
    assert!(!sync::sync(&conn, &store, ACCOUNT).await.unwrap().full_resync);
}

#[tokio::test]
async fn many_changes_are_all_picked_up() {
    let (server, conn, store) = setup().await;
    sync::sync(&conn, &store, ACCOUNT).await.unwrap();
    // More than one get batch (the mock allows 500 objects per get).
    let ids: Vec<String> = (0..620)
        .map(|i| server.deliver_at("Bulk", "bulk@example.test", &format!("n{i}"), "p", 2_000_000_000 + i))
        .collect();
    for id in ids.iter().step_by(2) {
        server.mark_read(id);
    }
    let out = sync::sync(&conn, &store, ACCOUNT).await.unwrap();
    assert!(out.changed && !out.full_resync);
    assert_eq!(cached(&store, INBOX_ID).len(), 623);
    assert_cache_matches(&server, &store);
}

#[tokio::test]
async fn the_first_fetch_is_limited_to_the_newest_messages() {
    let server = MockServer::start().await;
    for i in 0..(INBOX_WINDOW as i64 + 20) {
        server.deliver_at("Bulk", "bulk@example.test", &format!("n{i}"), "p", 1_000_000 + i);
    }
    let conn = connect(&server).await;
    let store = new_store(&server);
    sync::sync(&conn, &store, ACCOUNT).await.unwrap();

    assert_eq!(cached(&store, INBOX_ID).len(), INBOX_WINDOW);
    // Of the newest 500: the three seeded messages (received just now) and the 497 newest bulk ones.
    let newest = server.all_message_ids().into_iter().rfind(|id| server.mailboxes_of(id) == [INBOX_ID]).unwrap();
    assert!(cached(&store, INBOX_ID).contains(&newest));
    assert!(store.get_message(ACCOUNT, "e1").unwrap().is_some());
}

#[tokio::test]
async fn other_mailboxes_get_a_smaller_window() {
    let server = MockServer::start().await;
    for i in 0..(OTHER_WINDOW as i64 + 30) {
        let id = server.deliver_at("Bulk", "bulk@example.test", &format!("a{i}"), "p", 1_000_000 + i);
        server.move_to(&id, "archive");
    }
    let conn = connect(&server).await;
    let store = new_store(&server);
    sync::sync(&conn, &store, ACCOUNT).await.unwrap();
    assert_eq!(cached(&store, ARCHIVE_ID).len(), OTHER_WINDOW);
}

#[tokio::test]
async fn a_failed_first_sync_leaves_no_state_and_the_next_pass_recovers() {
    let (server, conn, store) = setup().await;
    server.set_down(true);
    let err = sync::sync(&conn, &store, ACCOUNT).await.unwrap_err();
    assert!(!jmap::is_unauthorized(&err));
    assert!(store.get_state(ACCOUNT, "Mailbox").unwrap().is_none());
    assert!(store.get_state(ACCOUNT, "Email").unwrap().is_none());

    server.set_down(false);
    sync::sync(&conn, &store, ACCOUNT).await.unwrap();
    assert_cache_matches(&server, &store);
}

#[tokio::test]
async fn an_outage_between_passes_loses_nothing() {
    let (server, conn, store) = setup().await;
    sync::sync(&conn, &store, ACCOUNT).await.unwrap();
    let state = store.get_state(ACCOUNT, "Email").unwrap();

    let new = server.deliver("Zed", "zed@example.test", "During the outage", "x");
    server.mark_read("e1");
    server.set_down(true);
    assert!(sync::sync(&conn, &store, ACCOUNT).await.is_err());
    assert_eq!(store.get_state(ACCOUNT, "Email").unwrap(), state, "a failed pass must not move the state");

    server.set_down(false);
    let out = sync::sync(&conn, &store, ACCOUNT).await.unwrap();
    assert!(out.changed && !out.full_resync);
    assert!(store.get_message(ACCOUNT, &new).unwrap().is_some());
    assert_cache_matches(&server, &store);
}

#[tokio::test]
async fn an_expired_token_is_reported_as_unauthorized() {
    let (server, conn, store) = setup().await;
    sync::sync(&conn, &store, ACCOUNT).await.unwrap();
    server.expire_access_tokens();
    server.deliver("Zed", "zed@example.test", "Hello", "Hi");

    let err = sync::sync(&conn, &store, ACCOUNT).await.unwrap_err();
    assert!(jmap::is_unauthorized(&err), "got {err:?}");

    // With a fresh token the same store catches up.
    let conn = connect(&server).await;
    assert!(sync::sync(&conn, &store, ACCOUNT).await.unwrap().changed);
    assert_cache_matches(&server, &store);
}

#[tokio::test]
async fn mailbox_counts_follow_incremental_passes() {
    // Mailbox state moves when mail arrives; the counts must follow.
    let (server, conn, store) = setup().await;
    sync::sync(&conn, &store, ACCOUNT).await.unwrap();
    server.deliver("Zed", "zed@example.test", "Hello", "Hi");
    sync::sync(&conn, &store, ACCOUNT).await.unwrap();
    assert_eq!(store.list_mailboxes(ACCOUNT).unwrap().len(), 6);
    let inbox = store.get_mailbox(ACCOUNT, INBOX_ID).unwrap().unwrap();
    assert_eq!(inbox.total_emails, 4);
}
