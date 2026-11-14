//! M3 against the mock server: bodies, threads, older mail, search, downloads and the actions
//! the list offers. The first half drives the sync layer, the second the `Core` object.

mod support;

use std::{fs, path::Path, sync::Arc};

use kweb_core::{
    ffi::{Core, Shade},
    jmap::{self, Connection},
    model::{Account, Action, ListFilter},
    secrets::{MemorySecrets, SecretStore},
    store::Store,
    sync, Error,
};
use support::*;
use tokio::runtime::Runtime;

const ACCOUNT: &str = ACCOUNT_ID;
/// Seeded: Maria's unread mail, Build Bot's, and Tom's read one (all Inbox).
const E1: &str = "e1";
const E2: &str = "e2";
const E3: &str = "e3";
/// A time well before the seeded mail, so the mail made here is older than all of it.
const LONG_AGO: i64 = 1_700_000_000;

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

/// `n` messages in Junk, one second apart (or all in the same second), the newest last.
fn fill_junk(server: &MockServer, n: usize, same_second: bool) -> Vec<String> {
    (0..n)
        .map(|i| {
            let at = if same_second { LONG_AGO } else { LONG_AGO + i as i64 };
            let id = server.deliver_at("Sam", "sam@example.test", &format!("junk {i}"), "spam", at);
            server.move_to(&id, "junk");
            id
        })
        .collect()
}

fn thread_count(store: &Store, mailbox: &str) -> usize {
    store.list_threads(ACCOUNT, mailbox, ListFilter::All, 10_000, 0).unwrap().len()
}

// ---- sync layer -------------------------------------------------------------------------

#[tokio::test]
async fn a_full_sync_sets_a_window_only_for_a_mailbox_that_filled_its_page() {
    let server = MockServer::start().await;
    fill_junk(&server, sync::OTHER_WINDOW + 5, false);
    let conn = connect(&server).await;
    let store = new_store(&server);
    sync::sync(&conn, &store, ACCOUNT).await.unwrap();

    // The newest 100 of 105 are cached; the oldest of those is the window.
    assert_eq!(store.get_window(ACCOUNT, JUNK_ID).unwrap(), Some(LONG_AGO + 5));
    assert_eq!(thread_count(&store, JUNK_ID), sync::OTHER_WINDOW);
    for mailbox in [INBOX_ID, ARCHIVE_ID, SENT_ID, TRASH_ID] {
        assert_eq!(store.get_window(ACCOUNT, mailbox).unwrap(), None, "{mailbox} is cached whole");
    }
}

#[tokio::test]
async fn load_older_walks_to_the_end_of_a_mailbox() {
    let server = MockServer::start().await;
    fill_junk(&server, 160, false);
    let conn = connect(&server).await;
    let store = new_store(&server);
    sync::sync(&conn, &store, ACCOUNT).await.unwrap();
    assert_eq!(thread_count(&store, JUNK_ID), 100);

    // The page starts at the window's own second (one message is read again), so 49 are new.
    assert!(sync::load_older(&conn, &store, ACCOUNT, JUNK_ID).await.unwrap());
    assert_eq!(thread_count(&store, JUNK_ID), 149);
    assert_eq!(store.get_window(ACCOUNT, JUNK_ID).unwrap(), Some(LONG_AGO + 11));

    // 11 are left: a short page ends the walk and drops the window.
    assert!(!sync::load_older(&conn, &store, ACCOUNT, JUNK_ID).await.unwrap());
    assert_eq!(thread_count(&store, JUNK_ID), 160);
    assert_eq!(store.get_window(ACCOUNT, JUNK_ID).unwrap(), None);

    // Nothing more to do, and no request is made for it.
    server.reset_request_counts();
    assert!(!sync::load_older(&conn, &store, ACCOUNT, JUNK_ID).await.unwrap());
    assert_eq!(server.request_count("/jmap"), 0);
}

#[tokio::test]
async fn load_older_misses_nothing_when_many_messages_share_a_second() {
    let server = MockServer::start().await;
    fill_junk(&server, 120, true);
    let conn = connect(&server).await;
    let store = new_store(&server);
    sync::sync(&conn, &store, ACCOUNT).await.unwrap();
    assert_eq!(store.get_window(ACCOUNT, JUNK_ID).unwrap(), Some(LONG_AGO));

    // A page that all sits in the window's own second cannot move the window back, so the
    // call reads on, page after page, until the second is done: here that is the whole
    // mailbox, and none of the 120 is skipped.
    assert!(!sync::load_older(&conn, &store, ACCOUNT, JUNK_ID).await.unwrap());
    assert_eq!(store.get_window(ACCOUNT, JUNK_ID).unwrap(), None);
    assert_eq!(store.list_messages(ACCOUNT, JUNK_ID, 500, 0).unwrap().len(), 120);
}

#[tokio::test]
async fn load_older_puts_queued_actions_back_on_what_it_fetched() {
    let server = MockServer::start().await;
    let ids = fill_junk(&server, 120, false);
    let conn = connect(&server).await;
    let store = new_store(&server);
    sync::sync(&conn, &store, ACCOUNT).await.unwrap();
    // ids[0] is the oldest and is not cached yet; a queued action on it is not lost when it is.
    store.outbox_enqueue(ACCOUNT, &Action::MarkRead { message_id: ids[0].clone() }).unwrap();
    sync::load_older(&conn, &store, ACCOUNT, JUNK_ID).await.unwrap();
    assert!(!store.get_message(ACCOUNT, &ids[0]).unwrap().unwrap().is_unread);
}

#[tokio::test]
async fn a_resync_resets_the_window() {
    let server = MockServer::start().await;
    let ids = fill_junk(&server, 120, false);
    let conn = connect(&server).await;
    let store = new_store(&server);
    sync::sync(&conn, &store, ACCOUNT).await.unwrap();
    sync::load_older(&conn, &store, ACCOUNT, JUNK_ID).await.unwrap();
    assert_eq!(store.get_window(ACCOUNT, JUNK_ID).unwrap(), None);

    // The mailbox is trimmed on the server, and the server forgets the changes: a full resync.
    server.destroy(&ids[119]);
    server.forget_changes_before_now();
    let outcome = sync::sync(&conn, &store, ACCOUNT).await.unwrap();
    assert!(outcome.full_resync);
    assert_eq!(thread_count(&store, JUNK_ID), sync::OTHER_WINDOW);
    assert_eq!(store.get_window(ACCOUNT, JUNK_ID).unwrap(), Some(LONG_AGO + 19));
}

#[tokio::test]
async fn refresh_thread_pulls_a_reply_from_another_mailbox_into_the_cache() {
    let (server, conn, store) = setup().await;
    server.set_thread(E1, "conversation");
    sync::sync(&conn, &store, ACCOUNT).await.unwrap();
    assert_eq!(store.thread_messages(ACCOUNT, "conversation").unwrap().len(), 1);

    // The user answered from another device: the reply is in Sent, and not in the cache.
    let reply = server.deliver("Alice Test", USERNAME, "Re: Lunch on Friday?", "Noon works.");
    server.move_to(&reply, "sent");
    server.set_thread(&reply, "conversation");
    assert!(store.get_message(ACCOUNT, &reply).unwrap().is_none());

    assert!(sync::refresh_thread(&conn, &store, ACCOUNT, "conversation").await.unwrap());
    let thread = store.thread_messages(ACCOUNT, "conversation").unwrap();
    let ids: Vec<_> = thread.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, [E1, reply.as_str()], "oldest first");
    assert_eq!(thread[1].mailbox_ids, vec![SENT_ID.to_string()]);
    assert_eq!(thread[1].to[0].email, "alice@example.test", "the To header comes with the header");

    // Nothing new the second time; a thread the server does not know is nothing either.
    assert!(!sync::refresh_thread(&conn, &store, ACCOUNT, "conversation").await.unwrap());
    assert!(!sync::refresh_thread(&conn, &store, ACCOUNT, "no such thread").await.unwrap());
    assert!(!sync::refresh_thread(&conn, &store, ACCOUNT, "").await.unwrap());
}

#[tokio::test]
async fn refresh_thread_reports_a_change_to_a_cached_header() {
    let (server, conn, store) = setup().await;
    // The thread of Maria's mail is just that mail. Another device reads it.
    server.mark_read(E1);
    assert!(sync::refresh_thread(&conn, &store, ACCOUNT, &format!("t{E1}")).await.unwrap());
    assert!(!store.get_message(ACCOUNT, E1).unwrap().unwrap().is_unread);
    // A queued action that the server copy overwrote is put back, so it counts as no change.
    store.outbox_enqueue(ACCOUNT, &Action::MarkUnread { message_id: E1.into() }).unwrap();
    store.set_unread(ACCOUNT, E1, true).unwrap();
    assert!(!sync::refresh_thread(&conn, &store, ACCOUNT, &format!("t{E1}")).await.unwrap());
    assert!(store.get_message(ACCOUNT, E1).unwrap().unwrap().is_unread);
}

#[tokio::test]
async fn prefetch_stores_the_newest_inbox_bodies_in_batches() {
    let (server, conn, store) = setup().await;
    server.set_bodies(E1, Some("Are you free at noon?"), Some("<p>Are you free at noon?</p>"));
    for i in 0..30 {
        server.deliver_at("Pat", "pat@example.test", &format!("n{i}"), "p", LONG_AGO + i);
    }
    sync::sync(&conn, &store, ACCOUNT).await.unwrap();

    assert_eq!(sync::prefetch_bodies(&conn, &store, ACCOUNT).await.unwrap(), 33);
    let body = store.get_body(ACCOUNT, E1).unwrap().unwrap();
    assert_eq!(body.text.as_deref(), Some("Are you free at noon?"));
    assert_eq!(body.html.as_deref(), Some("<p>Are you free at noon?</p>"));
    // A message with no body at all still gets a (empty) body: it is fetched once.
    assert_eq!(store.get_body(ACCOUNT, E2).unwrap().unwrap().text, None);

    // Done: nothing to fetch, no request.
    server.reset_request_counts();
    assert_eq!(sync::prefetch_bodies(&conn, &store, ACCOUNT).await.unwrap(), 0);
    assert_eq!(server.request_count("/jmap"), 0);
    // Only Inbox mail is prefetched.
    let archived = store.mailbox_by_role(ACCOUNT, "archive").unwrap().unwrap();
    assert_eq!(store.ids_without_body(ACCOUNT, &archived.id, 10).unwrap().len(), 1);
}

#[tokio::test]
async fn prefetch_stops_at_the_newest_hundred() {
    let (server, conn, store) = setup().await;
    for i in 0..120 {
        server.deliver_at("Pat", "pat@example.test", &format!("n{i}"), "p", LONG_AGO + i);
    }
    sync::sync(&conn, &store, ACCOUNT).await.unwrap();
    assert_eq!(sync::prefetch_bodies(&conn, &store, ACCOUNT).await.unwrap(), 100);
    assert_eq!(sync::prefetch_bodies(&conn, &store, ACCOUNT).await.unwrap(), 0);
}

#[tokio::test]
async fn bodies_carry_text_html_recipients_and_files() {
    let (server, conn, _store) = setup().await;
    server.set_bodies(E1, Some("plain"), Some("<p>html <img src=\"cid:logo\"></p>"));
    server.set_recipients(
        E1,
        &[("Alice", "alice@example.test"), ("Zed", "zed@example.test")],
        &[("Cy", "cy@example.test")],
    );
    let doc = server.add_attachment(E1, "report.pdf", "application/pdf", b"%PDF-1.7", None, false);
    let logo = server.add_attachment(E1, "logo.png", "image/png", b"PNG", Some("<logo>"), true);

    let bodies = conn.bodies(&[E2.to_string(), E1.to_string(), "nope".to_string()]).await.unwrap();
    let ids: Vec<_> = bodies.iter().map(|b| b.message_id.as_str()).collect();
    assert_eq!(ids, [E2, E1], "the order asked, unknown ids left out");
    let b = &bodies[1];
    assert_eq!((b.text.as_deref(), b.blob_id.as_str()), (Some("plain"), "be1"));
    assert!(b.html.as_deref().unwrap().contains("cid:logo"));
    assert_eq!(b.to.len(), 2);
    assert_eq!((b.cc[0].name.as_str(), b.cc[0].email.as_str()), ("Cy", "cy@example.test"));
    assert!(b.bcc.is_empty() && b.reply_to.is_empty());
    assert!(b.sent_at.is_some());
    assert!(!b.is_truncated);
    let files: Vec<_> = b.attachments.iter().map(|a| (a.blob_id.as_str(), a.name.as_str(), a.is_inline)).collect();
    assert_eq!(files, [(doc.as_str(), "report.pdf", false), (logo.as_str(), "logo.png", true)]);
    assert_eq!(b.attachments[0].size, 8);
    assert_eq!(b.attachments[1].content_id.as_deref(), Some("logo"));

    // The mail with no body parts at all.
    assert_eq!((bodies[0].text.clone(), bodies[0].html.clone()), (None, None));
}

#[tokio::test]
async fn a_text_only_message_has_no_html_and_an_html_only_one_no_text() {
    let (server, conn, _store) = setup().await;
    server.set_bodies(E1, Some("only text"), None);
    server.set_bodies(E2, None, Some("<b>only html</b>"));
    let bodies = conn.bodies(&[E1.to_string(), E2.to_string()]).await.unwrap();
    assert_eq!((bodies[0].text.as_deref(), bodies[0].html.as_deref()), (Some("only text"), None));
    assert_eq!((bodies[1].text.as_deref(), bodies[1].html.as_deref()), (None, Some("<b>only html</b>")));
}

#[tokio::test]
async fn a_body_over_the_limit_arrives_cut_and_says_so() {
    let (server, conn, _store) = setup().await;
    server.set_bodies(E1, Some(&"x".repeat(1_200_000)), None);
    let body = conn.bodies(&[E1.to_string()]).await.unwrap().remove(0);
    assert_eq!(body.text.unwrap().len(), 1_000_000);
    assert!(body.is_truncated);
}

#[tokio::test]
async fn search_ids_use_the_servers_text_search_and_the_mailbox() {
    let (server, conn, _store) = setup().await;
    server.set_bodies(E2, Some("the needle is in the haystack"), None);
    server.set_bodies(E3, None, Some("<p>another NEEDLE</p>"));
    assert_eq!(conn.search_ids("needle", None, 10).await.unwrap(), [E2, E3]);
    assert_eq!(conn.search_ids("needle", Some(ARCHIVE_ID), 10).await.unwrap(), Vec::<String>::new());
    assert_eq!(conn.search_ids("needle", Some(INBOX_ID), 1).await.unwrap().len(), 1);
    // The sender and the recipients, as well: Maria wrote e1 and got e5.
    assert_eq!(conn.search_ids("maria", None, 10).await.unwrap(), ["e5", E1]);
}

#[tokio::test]
async fn searching_caches_what_it_finds_and_reapplies_queued_actions() {
    let (server, conn, store) = setup().await;
    // Not cached: it arrived after the last sync.
    let late = server.deliver("Nora", "nora@example.test", "Needle news", "p");
    assert!(store.get_message(ACCOUNT, &late).unwrap().is_none());
    store.outbox_enqueue(ACCOUNT, &Action::MarkRead { message_id: late.clone() }).unwrap();

    let found = sync::search(&conn, &store, ACCOUNT, "needle", None, 10).await.unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].id, late);
    assert!(!found[0].is_unread, "the queued action is shown");
    assert!(store.get_message(ACCOUNT, &late).unwrap().is_some());
    assert!(sync::search(&conn, &store, ACCOUNT, "no such words anywhere", None, 10).await.unwrap().is_empty());
}

#[tokio::test]
async fn download_returns_the_blob_and_refuses_what_it_does_not_have() {
    let (server, conn, _store) = setup().await;
    let blob = server.add_attachment(E1, "a.bin", "application/octet-stream", &[0, 1, 2, 254, 255], None, false);
    let mut bytes = Vec::new();
    conn.download_to(&blob, &mut bytes).await.unwrap();
    assert_eq!(bytes, vec![0, 1, 2, 254, 255]);
    let missing = conn.download_to("no-such-blob", &mut Vec::new()).await.unwrap_err();
    assert!(matches!(missing, Error::Invalid { .. }), "got {missing:?}");

    server.expire_access_tokens();
    let err = conn.download_to(&blob, &mut Vec::new()).await.unwrap_err();
    assert!(jmap::is_unauthorized(&err), "got {err:?}");
}

#[tokio::test]
async fn thread_ids_come_from_the_server_in_time_order() {
    let (server, conn, _store) = setup().await;
    server.set_thread(E1, "t-shared");
    server.set_thread(E3, "t-shared");
    assert_eq!(conn.thread_email_ids("t-shared").await.unwrap(), [E3, E1]);
    assert!(conn.thread_email_ids("unknown").await.unwrap().is_empty());
}

// ---- the Core object --------------------------------------------------------------------

struct Rig {
    // Field order is drop order: the core goes first, the mock and its runtime last.
    core: Arc<Core>,
    server: MockServer,
    _rt: Runtime,
    dir: tempfile::TempDir,
}

impl Rig {
    fn new(server_setup: impl FnOnce(&MockServer)) -> Rig {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        let server = rt.block_on(MockServer::start());
        server_setup(&server);
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("cache.db").to_string_lossy().into_owned();
        let secrets: Arc<dyn SecretStore> = Arc::new(MemorySecrets::default());
        let core = Core::new(db_path, secrets).unwrap();
        // The browser step: open the authorize URL, follow the redirect to the loopback socket.
        let url = core.begin_sign_in(server.url()).unwrap();
        let browser = rt.spawn(async move { reqwest::get(url).await.map(|r| r.status()) });
        core.finish_sign_in().unwrap();
        assert!(rt.block_on(browser).unwrap().unwrap().is_success());
        Rig { core, server, _rt: rt, dir }
    }

    /// Signed in, with the first pass done.
    fn synced(server_setup: impl FnOnce(&MockServer)) -> Rig {
        let rig = Rig::new(server_setup);
        rig.core.check_once(Shade::Unknown).unwrap();
        rig
    }

    fn inbox_ids(&self) -> Vec<String> {
        self.core.messages(INBOX_ID.into(), 1000, 0).unwrap().into_iter().map(|m| m.id).collect()
    }

    fn path(&self, name: &str) -> String {
        self.dir.path().join(name).to_string_lossy().into_owned()
    }
}

fn leftovers(dir: &Path) -> Vec<String> {
    let mut names: Vec<_> =
        fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    names.retain(|n| n != "cache.db" && !n.starts_with("cache.db-"));
    names.sort();
    names
}

#[test]
fn a_body_is_fetched_once_and_then_read_without_the_server() {
    let rig = Rig::synced(|s| s.set_bodies(E3, Some("the invoice"), None));
    // The pass already prefetched it, so take a message the pass cannot have: one that is not
    // in the Inbox window of a prefetch. Archive mail is not prefetched.
    let archived = rig.core.messages(ARCHIVE_ID.into(), 10, 0).unwrap().remove(0).id;
    rig.server.set_bodies(&archived, Some("conference notes"), None);
    rig.server.reset_request_counts();

    let body = rig.core.message_body(archived.clone()).unwrap();
    assert_eq!(body.text.as_deref(), Some("conference notes"));
    assert_eq!(rig.server.request_count("Email/get"), 1);

    rig.server.set_down(true);
    assert_eq!(rig.core.message_body(archived).unwrap(), body);
    assert_eq!(rig.server.request_count("Email/get"), 1, "no request for a cached body");
}

#[test]
fn a_message_the_server_does_not_have_is_invalid_and_offline_is_temporary() {
    let rig = Rig::synced(|_| {});
    let err = rig.core.message_body("no-such-message".into()).unwrap_err();
    assert!(matches!(err, Error::Invalid { .. }), "got {err:?}");

    let archived = rig.core.messages(ARCHIVE_ID.into(), 10, 0).unwrap().remove(0).id;
    rig.server.set_down(true);
    let err = rig.core.message_body(archived).unwrap_err();
    assert!(matches!(err, Error::Temporary { .. }), "got {err:?}");
}

#[test]
fn a_pass_prefetches_inbox_bodies_for_offline_reading() {
    let rig = Rig::synced(|s| s.set_bodies(E1, Some("Are you free at noon?"), None));
    rig.server.set_down(true);
    for id in rig.inbox_ids() {
        rig.core.message_body(id).unwrap();
    }
    assert_eq!(rig.core.message_body(E1.into()).unwrap().text.as_deref(), Some("Are you free at noon?"));
}

#[test]
fn a_failing_body_fetch_never_fails_the_pass_or_the_notification() {
    let rig = Rig::new(|s| s.set_bodies_fail(true));
    // The first pass announces nothing (the mail was there before sign-in); a later mail is.
    rig.core.check_once(Shade::Unknown).unwrap();
    let fresh = rig.server.deliver("Dora", "dora@example.test", "Fresh", "just now");
    let plan = rig.core.check_once(Shade::Unknown).unwrap();
    assert!(plan.post.iter().any(|n| n.message_id == fresh), "the notification went out");
    assert!(rig.core.messages(INBOX_ID.into(), 10, 0).unwrap().iter().any(|m| m.id == fresh));

    // Nothing was prefetched, so offline there is nothing to read; once the server answers,
    // the next pass fills the cache.
    rig.server.set_bodies_fail(false);
    rig.server.mark_read(&fresh);
    rig.core.check_once(Shade::Unknown).unwrap();
    rig.server.set_down(true);
    assert!(rig.core.message_body(fresh).is_ok());
}

#[test]
fn reading_calls_share_one_connection() {
    let rig = Rig::synced(|_| {});
    // Mail the pass did not prefetch (it only does the Inbox).
    let archived = rig.core.messages(ARCHIVE_ID.into(), 10, 0).unwrap().remove(0).id;
    let sent = rig.core.messages(SENT_ID.into(), 10, 0).unwrap().remove(0).id;
    rig.server.reset_request_counts();
    rig.core.message_body(archived).unwrap();
    rig.core.message_body(sent).unwrap();
    assert_eq!(rig.server.request_count("Email/get"), 2);
    assert_eq!(rig.server.request_count("/.well-known/jmap"), 1, "one session for both calls");
}

#[test]
fn a_401_during_a_body_fetch_is_renewed_and_retried_once() {
    let rig = Rig::synced(|_| {});
    let archived = rig.core.messages(ARCHIVE_ID.into(), 10, 0).unwrap().remove(0).id;
    rig.server.set_bodies(&archived, Some("conference notes"), None);
    rig.core.message_body(E1.into()).unwrap(); // makes the shared connection
    let tokens = rig.server.token_requests();
    rig.server.expire_access_tokens();

    let body = rig.core.message_body(archived).unwrap();
    assert_eq!(body.text.as_deref(), Some("conference notes"));
    assert_eq!(rig.server.token_requests(), tokens + 1);
}

#[test]
fn search_asks_the_server_and_falls_back_to_the_cache_when_it_is_down() {
    let rig = Rig::synced(|s| s.set_bodies(E3, Some("an obscure phrase"), None));
    let found = rig.core.search("obscure".into(), None, 20).unwrap();
    assert!(!found.from_cache);
    assert_eq!(found.messages.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), [E3]);

    // The server finds words in the body; the cache only has sender, subject and preview.
    rig.server.set_down(true);
    let offline = rig.core.search("obscure".into(), None, 20).unwrap();
    assert!(offline.from_cache && offline.messages.is_empty());
    let offline = rig.core.search("INVOICE".into(), Some(INBOX_ID.into()), 20).unwrap();
    assert!(offline.from_cache);
    assert_eq!(offline.messages.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), [E3]);
    let elsewhere = rig.core.search("invoice".into(), Some(ARCHIVE_ID.into()), 20).unwrap();
    assert!(elsewhere.messages.is_empty());
}

#[test]
fn a_blank_search_finds_nothing_and_asks_nothing() {
    let rig = Rig::synced(|_| {});
    rig.server.reset_request_counts();
    for blank in ["", "   ", "\t\n"] {
        let r = rig.core.search(blank.into(), None, 20).unwrap();
        assert!(r.messages.is_empty() && !r.from_cache);
    }
    assert_eq!(rig.server.request_count("/jmap"), 0);
    assert_eq!(rig.server.request_count("/.well-known/jmap"), 0);
}

#[test]
fn a_search_that_the_server_refuses_to_authorize_is_not_hidden_by_the_cache() {
    let rig = Rig::synced(|_| {});
    rig.server.revoke_grant();
    rig.server.expire_access_tokens();
    let err = rig.core.search("invoice".into(), None, 20).unwrap_err();
    assert!(matches!(err, Error::SignInNeeded { .. }), "got {err:?}");
}

#[test]
fn download_blob_writes_the_file_whole() {
    let rig = Rig::synced(|_| {});
    let blob = rig.server.add_attachment(E1, "a.bin", "application/octet-stream", &[7; 5000], None, false);
    let dest = rig.path("saved.bin");
    rig.core.download_blob(blob, dest.clone()).unwrap();
    assert_eq!(fs::read(&dest).unwrap(), vec![7; 5000]);
    assert_eq!(leftovers(rig.dir.path()), ["saved.bin"], "no .part file is left");
}

#[test]
fn a_failed_download_leaves_no_file_and_no_part_file() {
    let rig = Rig::synced(|_| {});
    let dest = rig.path("never.bin");
    assert!(rig.core.download_blob("no-such-blob".into(), dest).is_err());
    assert!(leftovers(rig.dir.path()).is_empty());

    // The data arrives but cannot be written.
    let blob = rig.server.add_attachment(E1, "a.bin", "application/octet-stream", b"abc", None, false);
    let err = rig.core.download_blob(blob.clone(), rig.path("missing-dir/x.bin")).unwrap_err();
    assert!(matches!(err, Error::Store { .. }), "got {err:?}");
    // The target is a directory: the rename fails after the part file was written.
    fs::create_dir(rig.path("taken")).unwrap();
    assert!(rig.core.download_blob(blob, rig.path("taken")).is_err());
    assert_eq!(leftovers(rig.dir.path()), ["taken"]);
}

#[test]
fn flag_and_move_from_a_swipe_reach_the_server() {
    let rig = Rig::synced(|_| {});
    rig.core
        .submit_all(vec![
            Action::Flag { message_id: E1.into() },
            Action::Move { message_id: E2.into(), mailbox_id: JUNK_ID.into() },
        ])
        .unwrap();
    let row = rig.core.threads(INBOX_ID.into(), ListFilter::Flagged, 10, 0).unwrap();
    assert_eq!(row.iter().map(|r| r.newest.id.as_str()).collect::<Vec<_>>(), [E1]);
    assert!(!rig.inbox_ids().contains(&E2.to_string()));

    rig.core.check_once(Shade::Unknown).unwrap();
    assert!(rig.server.keywords_of(E1).contains(&"$flagged".to_string()));
    assert_eq!(rig.server.mailboxes_of(E2), [JUNK_ID]);
}

#[test]
fn submit_all_applies_everything_in_order_and_needs_an_account() {
    let rig = Rig::synced(|_| {});
    rig.core
        .submit_all(vec![
            Action::Flag { message_id: E1.into() },
            Action::Archive { message_id: E2.into() },
            Action::Flag { message_id: E3.into() },
        ])
        .unwrap();
    let flagged = rig.core.threads(INBOX_ID.into(), ListFilter::Flagged, 10, 0).unwrap();
    assert_eq!(flagged.iter().map(|r| r.newest.id.as_str()).collect::<Vec<_>>(), [E1, E3]);
    assert!(!rig.inbox_ids().contains(&E2.to_string()));
    // Where it stops at an error is covered with the actions.
    let signed_out = Core::new(rig.path("other.db"), Arc::new(MemorySecrets::default())).unwrap();
    assert!(matches!(signed_out.submit_all(vec![]), Err(Error::SignInNeeded { .. })));
}

#[test]
fn threads_older_mail_and_thread_refresh_through_the_core() {
    let rig = Rig::synced(|s| {
        fill_junk(s, 160, false);
        s.set_thread(E1, "conversation");
    });
    assert!(rig.core.has_older(JUNK_ID.into()).unwrap());
    assert!(!rig.core.has_older(INBOX_ID.into()).unwrap());
    assert_eq!(rig.core.threads(JUNK_ID.into(), ListFilter::All, 1000, 0).unwrap().len(), 100);

    assert!(rig.core.load_older(JUNK_ID.into()).unwrap());
    assert_eq!(rig.core.threads(JUNK_ID.into(), ListFilter::All, 1000, 0).unwrap().len(), 149);
    assert!(!rig.core.load_older(JUNK_ID.into()).unwrap());
    assert_eq!(rig.core.threads(JUNK_ID.into(), ListFilter::All, 1000, 0).unwrap().len(), 160);
    assert!(!rig.core.has_older(JUNK_ID.into()).unwrap());
    // With nothing older, the call needs no server.
    rig.server.set_down(true);
    assert!(!rig.core.load_older(JUNK_ID.into()).unwrap());
    rig.server.set_down(false);

    let reply = rig.server.deliver("Alice Test", USERNAME, "Re: Lunch", "ok");
    rig.server.move_to(&reply, "sent");
    rig.server.set_thread(&reply, "conversation");
    assert_eq!(rig.core.thread_messages("conversation".into()).unwrap().len(), 1);
    assert!(rig.core.refresh_thread("conversation".into()).unwrap());
    assert_eq!(rig.core.thread_messages("conversation".into()).unwrap().len(), 2);
    assert_eq!(rig.core.message(reply.clone()).unwrap().unwrap().id, reply);
}

#[test]
fn reading_calls_need_a_signed_in_account() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cache.db").to_string_lossy().into_owned();
    let core = Core::new(path, Arc::new(MemorySecrets::default())).unwrap();
    assert!(matches!(core.message_body("m".into()), Err(Error::SignInNeeded { .. })));
    assert!(matches!(core.search("x".into(), None, 5), Err(Error::SignInNeeded { .. })));
    assert!(matches!(core.load_older("mb".into()), Err(Error::SignInNeeded { .. })));
    assert!(matches!(core.refresh_thread("t".into()), Err(Error::SignInNeeded { .. })));
    assert!(matches!(core.download_blob("b".into(), "x".into()), Err(Error::SignInNeeded { .. })));
    assert!(matches!(core.threads("mb".into(), ListFilter::All, 1, 0), Err(Error::SignInNeeded { .. })));
    assert!(matches!(core.thread_messages("t".into()), Err(Error::SignInNeeded { .. })));
}
