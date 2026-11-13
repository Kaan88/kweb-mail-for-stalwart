//! The `Core` object end to end against the mock server, the way the cli and the Android app
//! use it: blocking calls from plain threads, a real loopback socket for the browser step.
//!
//! The mock is async (axum), `Core` blocks and owns its own runtime, so every test is a plain
//! `#[test]` that keeps a separate multi-thread runtime alive just for the mock.

mod support;

use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use kweb_core::{
    ffi::{Core, Shade},
    model::{Account, Action, ConnectionStatus, Reminder},
    notify::Plan,
    push::Listener,
    secrets::{MemorySecrets, SecretStore},
    Error,
};
use support::{MockServer, ACCOUNT_ID, INBOX_ID, USERNAME};
use tokio::runtime::Runtime;

const DEADLINE: Duration = Duration::from_secs(8);

#[derive(Debug, Clone)]
enum Ev {
    Status(ConnectionStatus),
    Plan(Plan),
    CacheChanged,
}

#[derive(Default)]
struct Recorder {
    events: Mutex<Vec<Ev>>,
}

impl Listener for Recorder {
    fn status(&self, status: ConnectionStatus) {
        self.events.lock().unwrap().push(Ev::Status(status));
    }
    fn notifications(&self, plan: Plan) {
        self.events.lock().unwrap().push(Ev::Plan(plan));
    }
    fn cache_changed(&self) {
        self.events.lock().unwrap().push(Ev::CacheChanged);
    }
    fn reminder(&self, _: Reminder) {}
}

impl Recorder {
    fn plans(&self) -> Vec<Plan> {
        let events = self.events.lock().unwrap();
        events.iter().filter_map(|e| if let Ev::Plan(p) = e { Some(p.clone()) } else { None }).collect()
    }

    fn is_live(&self) -> bool {
        let events = self.events.lock().unwrap();
        events.iter().any(|e| matches!(e, Ev::Status(ConnectionStatus::Live { .. })))
    }

    /// The post for this message, if one has been seen: `Some(silent)`.
    fn posted(&self, id: &str) -> Option<bool> {
        self.plans().iter().flat_map(|p| p.post.iter()).find(|n| n.message_id == id).map(|n| n.silent)
    }

    fn cancelled(&self, id: &str) -> bool {
        self.plans().iter().flat_map(|p| p.cancel.iter()).any(|c| c.message_id == id)
    }

    fn cache_changes(&self) -> usize {
        self.events.lock().unwrap().iter().filter(|e| matches!(e, Ev::CacheChanged)).count()
    }
}

/// Polls from the test thread; a failed wait names what was missing.
fn eventually(what: &str, mut cond: impl FnMut() -> bool) {
    let start = Instant::now();
    while !cond() {
        assert!(start.elapsed() < DEADLINE, "timed out waiting for: {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

struct Rig {
    // Field order is drop order: the core goes first, the mock and its runtime last.
    core: Arc<Core>,
    server: MockServer,
    rt: Runtime,
    secrets: Arc<MemorySecrets>,
    db_path: String,
    _dir: tempfile::TempDir,
}

impl Rig {
    /// A mock server and a signed-out core.
    fn new() -> Rig {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        let server = rt.block_on(MockServer::start());
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("cache.db").to_string_lossy().into_owned();
        let secrets = Arc::new(MemorySecrets::default());
        let core = Core::new(db_path.clone(), secrets.clone()).unwrap();
        Rig { core, server, rt, secrets, db_path, _dir: dir }
    }

    /// A rig whose core is signed in through the whole browser round trip.
    fn signed_in() -> Rig {
        let rig = Rig::new();
        rig.sign_in(&rig.core);
        rig
    }

    /// Plays the browser: opens the authorize URL, follows the 302 to the loopback socket.
    fn sign_in(&self, core: &Core) -> Account {
        let url = core.begin_sign_in(self.server.url()).unwrap();
        let browser = self.rt.spawn(async move { reqwest::get(url).await.map(|r| r.status()) });
        let account = core.finish_sign_in().unwrap();
        let status = self.rt.block_on(browser).unwrap().unwrap();
        assert!(status.is_success(), "the callback page answered {status}");
        account
    }

    fn reopen(&self) -> Arc<Core> {
        Core::new(self.db_path.clone(), self.secrets.clone() as Arc<dyn SecretStore>).unwrap()
    }

    fn start_listening(&self) -> Arc<Recorder> {
        let rec = Arc::new(Recorder::default());
        self.core.start(rec.clone());
        eventually("the connection to be live", || rec.is_live());
        rec
    }
}

#[test]
fn sign_in_yields_the_mock_account() {
    let rig = Rig::new();
    assert_eq!(rig.core.account(), None);
    let account = rig.sign_in(&rig.core);
    assert_eq!(account.id, ACCOUNT_ID);
    assert_eq!(account.username, USERNAME);
    assert_eq!(account.server_url, rig.server.url());
    assert_eq!(rig.core.account(), Some(account));
}

#[test]
fn a_second_core_on_the_same_files_is_signed_in_without_a_browser() {
    let rig = Rig::signed_in();
    let first = rig.core.account().unwrap();
    let second = rig.reopen();
    assert_eq!(second.account(), Some(first));
    // The refresh token from the shared secrets is enough to sync.
    second.check_once(Shade::Unknown).unwrap();
    assert!(!second.mailboxes().unwrap().is_empty());
}

#[test]
fn check_once_fills_the_cache_with_the_seeded_mail() {
    let rig = Rig::signed_in();
    rig.core.check_once(Shade::Unknown).unwrap();

    let boxes = rig.core.mailboxes().unwrap();
    let inbox = boxes.iter().find(|m| m.role.as_deref() == Some("inbox")).expect("an inbox");
    assert_eq!(inbox.id, INBOX_ID);
    assert_eq!(inbox.total_emails, 3);
    assert_eq!(inbox.unread_emails, 2);
    assert!(boxes.iter().any(|m| m.role.as_deref() == Some("trash")));

    let messages = rig.core.messages(INBOX_ID.into(), 10, 0).unwrap();
    let subjects: Vec<_> = messages.iter().map(|m| m.subject.as_str()).collect();
    assert_eq!(subjects, ["Lunch on Friday?", "Nightly build passed", "Invoice 2026-114"], "newest first");
    assert!(messages[0].is_unread && !messages[2].is_unread);

    let page = rig.core.messages(INBOX_ID.into(), 1, 1).unwrap();
    assert_eq!(page.len(), 1);
    assert_eq!(page[0].subject, "Nightly build passed");
}

#[test]
fn live_mail_is_posted_and_read_elsewhere_is_cancelled() {
    let rig = Rig::signed_in();
    rig.core.check_once(Shade::Unknown).unwrap();
    let rec = rig.start_listening();

    let id = rig.server.deliver("Dora", "dora@example.test", "Live one", "just arrived");
    eventually("the post", || rec.posted(&id).is_some());
    assert_eq!(rec.posted(&id), Some(false), "a first announcement is not silent");
    eventually("the cache to show it", || {
        rig.core.messages(INBOX_ID.into(), 20, 0).unwrap().iter().any(|m| m.id == id)
    });
    assert!(rec.cache_changes() > 0);

    // Another device reads it.
    rig.server.mark_read(&id);
    eventually("the cancel", || rec.cancelled(&id));
    // Mail that was already there at sign-in was never announced.
    assert!(rec.plans().iter().flat_map(|p| p.post.iter()).all(|n| n.message_id == id));
}

#[test]
fn marking_read_in_the_core_reaches_the_server_and_cancels() {
    let rig = Rig::signed_in();
    rig.core.check_once(Shade::Unknown).unwrap();
    let rec = rig.start_listening();

    let id = rig.server.deliver("Eve", "eve@example.test", "Read me here", "on this device");
    eventually("the post", || rec.posted(&id).is_some());

    rig.core.submit(Action::MarkRead { message_id: id.clone() }).unwrap();
    eventually("the server to see it read", || rig.server.is_read(&id));
    eventually("the cancel", || rec.cancelled(&id));
    let cached = rig.core.messages(INBOX_ID.into(), 20, 0).unwrap();
    assert!(!cached.iter().find(|m| m.id == id).unwrap().is_unread);
}

#[test]
fn a_swiped_notification_stays_away_but_the_other_comes_back_silent() {
    let rig = Rig::signed_in();
    rig.core.check_once(Shade::Unknown).unwrap();
    let rec = rig.start_listening();

    let swiped = rig.server.deliver("Fay", "fay@example.test", "Swipe me", "away");
    let kept = rig.server.deliver("Gus", "gus@example.test", "Keep me", "here");
    eventually("both posts", || rec.posted(&swiped).is_some() && rec.posted(&kept).is_some());
    // The live connection would act on its own; this part is about one deterministic pass.
    rig.core.stop();

    rig.core.notification_swiped(swiped.clone()).unwrap();
    let plan = rig.core.check_once(Shade::Cleared).unwrap();
    assert!(plan.post.iter().all(|n| n.message_id != swiped), "the swiped mail came back");
    let again = plan.post.iter().find(|n| n.message_id == kept).expect("the other one is put back");
    assert!(again.silent);
}

#[test]
fn sign_out_wipes_the_account_and_the_next_core_starts_signed_out() {
    let rig = Rig::signed_in();
    rig.core.check_once(Shade::Unknown).unwrap();
    let _rec = rig.start_listening();

    rig.core.sign_out().unwrap();
    assert_eq!(rig.core.account(), None);
    assert_eq!(rig.core.status(), ConnectionStatus::SignedOut);
    assert!(matches!(rig.core.check_once(Shade::Unknown), Err(Error::SignInNeeded { .. })));

    let fresh = rig.reopen();
    assert_eq!(fresh.account(), None);
    assert_eq!(fresh.status(), ConnectionStatus::SignedOut);
    // Signing out twice is not an error.
    rig.core.sign_out().unwrap();
}

#[test]
fn finish_without_begin_is_an_error() {
    let rig = Rig::new();
    assert!(matches!(rig.core.finish_sign_in(), Err(Error::Invalid { .. })));
}
