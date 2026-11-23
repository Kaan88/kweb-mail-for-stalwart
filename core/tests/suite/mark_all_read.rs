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
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        let server = rt.block_on(MockServer::start());
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
fn unread_mail_the_cache_does_not_hold_is_marked_on_the_server() {
    let rig = Rig::new();
    // Arrived and not synced: only the server knows it.
    let outside = rig.server.deliver("Anna", "anna@example.org", "Not in the cache", "hello");
    assert!(rig.core.message(outside.clone()).unwrap().is_none());
    let changed = rig.core.mark_all_read(vec![INBOX_ID.to_string()]).unwrap();
    assert!(!changed.contains(&outside));
    assert!(rig.server.is_read(&outside));
}

#[test]
fn a_list_of_no_folders_is_refused() {
    let rig = Rig::new();
    assert!(rig.core.mark_all_read(Vec::new()).is_err());
}
