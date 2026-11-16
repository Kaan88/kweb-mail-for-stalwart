//! Sync against the mock JMAP server: first fill, incremental changes, full resync, outages
//! and expired tokens.

use std::collections::BTreeSet;

use crate::support::*;
use kweb_core::{
    jmap::{self, Connection},
    model::Account,
    store::Store,
    sync::{self, INBOX_WINDOW, OTHER_WINDOW},
};

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

use kweb_core::sync::window::{self, SyncWindow};
use std::collections::HashSet;

fn put_body(store: &Store, account: &str, id: &str) {
    let body = kweb_core::model::MessageBody {
        message_id: id.into(),
        blob_id: "b".into(),
        to: vec![],
        cc: vec![],
        bcc: vec![],
        reply_to: vec![],
        sent_at: None,
        html: None,
        text: Some("hello".into()),
        attachments: vec![],
        is_truncated: false,
    };
    assert!(store.put_body(account, &body).unwrap());
}

/// `n` messages in the Archive, received at 1_000_000 + i, all read.
fn archive_of(server: &MockServer, n: i64) -> Vec<String> {
    (0..n)
        .map(|i| {
            let id = server.deliver_at("Bulk", "bulk@example.test", &format!("a{i}"), "p", 1_000_000 + i);
            server.move_to(&id, "archive");
            server.mark_read(&id);
            id
        })
        .collect()
}

#[tokio::test]
async fn a_bigger_window_fetches_older_mail_on_the_next_sync() {
    let server = MockServer::start().await;
    archive_of(&server, OTHER_WINDOW as i64 + 40);
    let conn = connect(&server).await;
    let store = new_store(&server);
    sync::sync(&conn, &store, ACCOUNT).await.unwrap();
    assert_eq!(cached(&store, ARCHIVE_ID).len(), OTHER_WINDOW);
    assert!(store.get_window(ACCOUNT, ARCHIVE_ID).unwrap().is_some());

    assert!(window::set(&store, ACCOUNT, SyncWindow::Large).unwrap(), "the window grew");
    sync::sync(&conn, &store, ACCOUNT).await.unwrap();
    assert_eq!(cached(&store, ARCHIVE_ID).len(), OTHER_WINDOW + 40 + 1, "the seeded one too");
    // Everything is cached now: no window is left.
    assert_eq!(store.get_window(ACCOUNT, ARCHIVE_ID).unwrap(), None);
}

#[tokio::test]
async fn a_smaller_window_removes_only_what_may_go() {
    let server = MockServer::start().await;
    let ids = archive_of(&server, 70);
    // The oldest is starred, the next is unread, the next is named by a queued action.
    server.add_keyword(&ids[0], "$flagged");
    server.mark_unread(&ids[1]);
    let conn = connect(&server).await;
    let store = new_store(&server);
    sync::sync(&conn, &store, ACCOUNT).await.unwrap();
    assert_eq!(cached(&store, ARCHIVE_ID).len(), 71, "70 and the seeded one");
    // A body is cached for an old and for a new message.
    for id in [&ids[2], &ids[3], &ids[69]] {
        put_body(&store, ACCOUNT, id);
    }

    window::set(&store, ACCOUNT, SyncWindow::Light).unwrap();
    let keep: HashSet<String> = [ids[2].clone()].into();
    let gone = window::prune(&store, ACCOUNT, SyncWindow::Light, &keep).unwrap();

    // Light keeps the newest 50 of the Archive (the seeded one and ids[21..]); of the 21 older ones three stay.
    assert_eq!(gone, 18);
    let left = cached(&store, ARCHIVE_ID);
    assert_eq!(left.len(), 53);
    for kept in [&ids[0], &ids[1], &ids[2], &ids[21], &ids[69]] {
        assert!(left.contains(kept), "{kept} should stay");
    }
    for gone in [&ids[3], &ids[20]] {
        assert!(!left.contains(gone), "{gone} should be gone");
    }
    assert!(store.get_body(ACCOUNT, &ids[3]).unwrap().is_none(), "its body went with it");
    assert!(store.get_body(ACCOUNT, &ids[69]).unwrap().is_some());
    // The folder is cached back to the oldest of its newest 50, and "load older" can go on from there.
    assert!(store.get_window(ACCOUNT, ARCHIVE_ID).unwrap().is_some());
}

#[tokio::test]
async fn a_resync_replaces_one_accounts_cache_and_leaves_the_other_alone() {
    let (server, conn, store) = setup().await;
    sync::sync(&conn, &store, ACCOUNT).await.unwrap();
    let other = Account { id: "other".into(), server_url: server.url(), username: "o".into(), signed_in_at: 0 };
    store.save_account(&other).unwrap();
    let mail = store.get_message(ACCOUNT, "e1").unwrap().unwrap();
    let mut theirs = mail.clone();
    theirs.id = "theirs".into();
    store.upsert_messages("other", &[theirs]).unwrap();
    store.set_state("other", "Email", "s-other").unwrap();
    put_body(&store, "other", "theirs");
    put_body(&store, ACCOUNT, "e1");
    store.dismissed_add(ACCOUNT, "e1").unwrap();

    server.deliver("New", "new@example.test", "After", "p");
    sync::resync_messages(&conn, &store, ACCOUNT).await.unwrap();
    store.delete_bodies(ACCOUNT).unwrap();

    assert_eq!(cached(&store, INBOX_ID).len(), 4, "the cache was fetched again");
    assert!(store.get_body(ACCOUNT, "e1").unwrap().is_none());
    assert!(store.get_message("other", "theirs").unwrap().is_some());
    assert!(store.get_body("other", "theirs").unwrap().is_some());
    assert_eq!(store.get_state("other", "Email").unwrap().as_deref(), Some("s-other"));
    assert!(store.is_dismissed(ACCOUNT, "e1").unwrap(), "what the user swiped away stays swiped");
    assert_eq!(store.get_state(ACCOUNT, "Email").unwrap().unwrap(), server.email_state());
}
