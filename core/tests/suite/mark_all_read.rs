//! "Mark all as read" against the mock server, through the `Core` object the way the app uses it.

use std::sync::Arc;

use crate::support::*;
use kweb_core::{ffi::Core, ffi::Shade, model::Action, secrets::MemorySecrets};
use tokio::runtime::Runtime;

struct Rig {
    // Field order is drop order: the core goes first, the mock and its runtime last.
    core: Arc<Core>,
    server: MockServer,
    _rt: Runtime,
    _dir: tempfile::TempDir,
}

impl Rig {
    /// Signed in and synced once.
    fn new() -> Rig {
        Rig::with(|_| ())
    }

    /// Like `new`, with `before` run on the server before the first sync.
    fn with(before: impl FnOnce(&MockServer)) -> Rig {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        let server = rt.block_on(MockServer::start());
        before(&server);
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("cache.db").to_string_lossy().into_owned();
        let core = Core::new(db_path, Arc::new(MemorySecrets::default())).unwrap();
        let url = core.begin_sign_in_with_redirect(server.url(), "ovh.kweb.mail:/oauth/callback".into()).unwrap();
        core.complete_sign_in(server.authorize(&url).unwrap()).unwrap();
        let rig = Rig { core, server, _rt: rt, _dir: dir };
        rig.sync();
        rig
    }

    fn sync(&self) {
        self.core.check_once(Shade::Unknown).expect("a pass");
    }
}

#[test]
fn the_cached_unread_mail_is_marked_and_its_ids_come_back() {
    let rig = Rig::new();
    let mut changed = rig.core.mark_all_read(vec![INBOX_ID.to_string()]).unwrap();
    changed.sort();
    assert_eq!(changed, vec!["e1".to_string(), "e2".to_string()]);
    // At once in the cache, before anything reached the server.
    assert!(!rig.core.message("e1".into()).unwrap().unwrap().is_unread);
    assert!(!rig.core.message("e2".into()).unwrap().unwrap().is_unread);
    rig.sync();
    assert!(rig.server.is_read("e1") && rig.server.is_read("e2"));
    // Nothing is left, so nothing comes back.
    assert!(rig.core.mark_all_read(vec![INBOX_ID.to_string()]).unwrap().is_empty());
}

#[test]
fn the_ids_given_back_undo_it_and_other_folders_are_left_alone() {
    let rig = Rig::new();
    rig.server.move_to("e2", "archive");
    rig.sync();
    let changed = rig.core.mark_all_read(vec![INBOX_ID.to_string()]).unwrap();
    assert_eq!(changed, vec!["e1".to_string()]);
    rig.sync();
    assert!(rig.server.is_read("e1"));
    assert!(!rig.server.is_read("e2"));
    let undo: Vec<Action> = changed.into_iter().map(|id| Action::MarkUnread { message_id: id }).collect();
    rig.core.submit_all(undo).unwrap();
    rig.sync();
    assert!(!rig.server.is_read("e1"));
}

#[test]
fn unread_mail_older_than_the_window_is_marked_on_the_server() {
    // 105 old unread mails in Junk: the first pass caches the newest 100 and sets a window.
    const LONG_AGO: i64 = 1_700_000_000;
    let mut old = Vec::new();
    let rig = Rig::with(|server| {
        for i in 0..105 {
            let id = server.deliver_at("Sam", "sam@example.test", &format!("junk {i}"), "spam", LONG_AGO + i);
            server.move_to(&id, "junk");
            old.push(id);
        }
    });
    let outside: Vec<&String> = old.iter().filter(|id| rig.core.message((*id).clone()).unwrap().is_none()).collect();
    assert_eq!(outside.len(), 5, "the oldest five are outside the window");
    let changed = rig.core.mark_all_read(vec![JUNK_ID.to_string()]).unwrap();
    assert_eq!(changed.len(), 100);
    for id in outside {
        assert!(!changed.contains(id));
        assert!(rig.server.is_read(id), "{id} is older than the window");
    }
}

#[test]
fn unread_mail_with_the_windows_oldest_time_is_marked_too() {
    // Six mails share the oldest time, so some of them fall outside the window of 100 at exactly its edge.
    const EDGE: i64 = 1_700_000_005;
    let mut old = Vec::new();
    let rig = Rig::with(|server| {
        for i in 0..105i64 {
            let id = server.deliver_at("Sam", "sam@example.test", &format!("junk {i}"), "spam", EDGE + (i - 5).max(0));
            server.move_to(&id, "junk");
            old.push(id);
        }
    });
    let outside: Vec<&String> = old.iter().filter(|id| rig.core.message((*id).clone()).unwrap().is_none()).collect();
    assert_eq!(outside.len(), 5);
    rig.core.mark_all_read(vec![JUNK_ID.to_string()]).unwrap();
    for id in outside {
        assert!(rig.server.is_read(id), "{id} has the window's oldest time");
    }
}

#[test]
fn mail_that_arrived_after_the_last_pass_stays_unread() {
    let rig = Rig::new();
    // Arrived and not synced: the user has not seen it, so it is not the cache's to mark.
    let new = rig.server.deliver("Anna", "anna@example.org", "Not in the cache", "hello");
    assert!(rig.core.message(new.clone()).unwrap().is_none());
    let changed = rig.core.mark_all_read(vec![INBOX_ID.to_string()]).unwrap();
    assert!(!changed.contains(&new));
    assert!(!rig.server.is_read(&new));
    rig.sync();
    assert!(rig.core.message(new).unwrap().unwrap().is_unread);
}

#[test]
fn a_list_of_no_folders_is_refused() {
    let rig = Rig::new();
    assert!(rig.core.mark_all_read(Vec::new()).is_err());
}
