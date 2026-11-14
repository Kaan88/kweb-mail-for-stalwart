//! M10 against the mock server: folders, their notifications, spam and blocking, through the
//! `Core` object the way the app uses them. The mock answers `Mailbox/set` like Stalwart 1.1.0
//! (`dev/notes/stalwart-mail-features.md`): it lets role folders be renamed and Drafts be
//! destroyed, so what the app refuses it refuses itself.

mod support;

use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use kweb_core::{
    ffi::{Core, Shade},
    model::{Action, ConnectionStatus, Mailbox, Reminder},
    notify::Plan,
    push::Listener,
    secrets::MemorySecrets,
    Error,
};
use serde_json::json;
use support::{MockServer, ARCHIVE_ID, DRAFTS_ID, INBOX_ID, JUNK_ID, SENT_ID, TRASH_ID, USERNAME};
use tokio::runtime::Runtime;

const DEADLINE: Duration = Duration::from_secs(8);

#[derive(Default)]
struct Recorder {
    plans: Mutex<Vec<Plan>>,
    live: Mutex<bool>,
}

impl Listener for Recorder {
    fn status(&self, status: ConnectionStatus) {
        if matches!(status, ConnectionStatus::Live { .. }) {
            *self.live.lock().unwrap() = true;
        }
    }
    fn notifications(&self, plan: Plan) {
        self.plans.lock().unwrap().push(plan);
    }
    fn cache_changed(&self) {}
    fn reminder(&self, _: Reminder) {}
}

impl Recorder {
    fn posted(&self, id: &str) -> bool {
        self.plans.lock().unwrap().iter().flat_map(|p| p.post.iter()).any(|n| n.message_id == id)
    }
    fn cancelled(&self, id: &str) -> bool {
        self.plans.lock().unwrap().iter().flat_map(|p| p.cancel.iter()).any(|c| c.message_id == id)
    }
}

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
    _rt: Runtime,
    _dir: tempfile::TempDir,
}

impl Rig {
    /// Signed in and synced once, so the seeded mail is old news.
    fn new() -> Rig {
        Rig::build(|_| {})
    }

    /// Like `new`, with `prepare` run on the server before the core signs in.
    fn build(prepare: impl FnOnce(&MockServer)) -> Rig {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        let server = rt.block_on(MockServer::start());
        prepare(&server);
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("cache.db").to_string_lossy().into_owned();
        let core = Core::new(db_path, Arc::new(MemorySecrets::default())).unwrap();
        let url = core.begin_sign_in_with_redirect(server.url(), "ovh.kweb.mail:/oauth/callback".into()).unwrap();
        core.complete_sign_in(server.authorize(&url).unwrap()).unwrap();
        let rig = Rig { core, server, _rt: rt, _dir: dir };
        rig.sync();
        rig
    }

    fn sync(&self) -> Plan {
        self.core.check_once(Shade::Unknown).unwrap()
    }

    /// A folder made through the core, and switched on for notifications when `notify`.
    fn folder(&self, name: &str, notify: bool) -> Mailbox {
        let made = self.core.create_mailbox(name.into(), None).unwrap();
        if notify {
            self.core.set_mailbox_notify(made.id.clone(), true).unwrap();
        }
        made
    }

    fn cached(&self, id: &str) -> Option<Mailbox> {
        self.core.mailboxes().unwrap().into_iter().find(|m| m.id == id)
    }

    fn asked(&self, method: &str) -> usize {
        self.server.request_count(method)
    }

    fn refusal<T: std::fmt::Debug>(&self, result: Result<T, Error>) -> String {
        match result {
            Err(Error::Invalid { msg }) => msg,
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    /// The ids in a mailbox as the cache lists them.
    fn listed(&self, mailbox_id: &str) -> Vec<String> {
        self.core.messages(mailbox_id.into(), 500, 0).unwrap().into_iter().map(|m| m.id).collect()
    }
}

fn posted(plan: &Plan) -> Vec<&str> {
    plan.post.iter().map(|n| n.message_id.as_str()).collect()
}

fn cancelled(plan: &Plan) -> Vec<&str> {
    plan.cancel.iter().map(|c| c.message_id.as_str()).collect()
}

fn deliver(rig: &Rig, folder: &str, subject: &str) -> String {
    rig.server.deliver_in(folder, "Dora", "dora@example.test", subject, "preview")
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64
}

// ---- notifications per folder --------------------------------------------------------------

#[test]
fn mail_that_a_filter_or_the_server_delivers_into_a_folder_that_notifies_is_posted() {
    let rig = Rig::new();
    let work = rig.folder("Work", true);
    let other = rig.folder("Other", false);
    assert!(rig.cached(&work.id).unwrap().notify && !rig.cached(&other.id).unwrap().notify);
    let quiet = deliver(&rig, &other.id, "quiet");
    let loud = deliver(&rig, &work.id, "loud");
    let plan = rig.sync();
    assert_eq!(posted(&plan), [loud.as_str()]);
    assert!(rig.sync().is_empty(), "once");
    // The Inbox goes on notifying as before.
    let inbox_mail = rig.server.deliver("Eve", "eve@example.test", "inbox", "p");
    assert_eq!(posted(&rig.sync()), [inbox_mail.as_str()]);
    assert!(rig.listed(&other.id).contains(&quiet));
}

#[test]
fn mail_moved_into_a_folder_that_notifies_by_another_client_is_posted_if_it_is_new() {
    let rig = Rig::new();
    let work = rig.folder("Work", true);
    let kept = rig.folder("Kept", false);
    // New mail that another client files straight away into "Kept" and then into "Work".
    let new = deliver(&rig, &kept.id, "new");
    assert!(rig.sync().is_empty());
    rig.server.move_to_mailbox(&new, &work.id);
    assert_eq!(posted(&rig.sync()), [new.as_str()]);
    // Old unread mail that is moved in (it arrived before the folder was switched on) is not.
    let old = rig.server.deliver_in_at(&kept.id, "Old", "old@example.test", "old", "p", now() - 3600);
    rig.sync();
    rig.server.move_to_mailbox(&old, &work.id);
    assert!(rig.sync().is_empty());
}

#[test]
fn a_folder_switched_on_later_stays_quiet_about_the_mail_already_in_it() {
    let rig = Rig::new();
    let work = rig.folder("Work", false);
    let before = rig.server.deliver_in_at(&work.id, "Dora", "dora@example.test", "before", "p", now() - 60);
    rig.sync();
    rig.core.set_mailbox_notify(work.id.clone(), true).unwrap();
    assert!(rig.sync().is_empty(), "no notification for the old mail");
    let after = deliver(&rig, &work.id, "after");
    let plan = rig.sync();
    assert_eq!(posted(&plan), [after.as_str()]);
    assert!(!posted(&plan).contains(&before.as_str()));
}

#[test]
fn a_message_in_the_inbox_and_in_a_folder_that_notifies_is_one_notification() {
    let rig = Rig::new();
    let work = rig.folder("Work", true);
    let id = rig.server.deliver("Eve", "eve@example.test", "both", "p");
    rig.server.add_to_mailbox(&id, &work.id);
    let plan = rig.sync();
    assert_eq!(posted(&plan), [id.as_str()]);
    assert!(rig.sync().is_empty());
    // Read: gone once, not twice.
    rig.server.mark_read(&id);
    assert_eq!(cancelled(&rig.sync()), [id.as_str()]);
}

#[test]
fn a_notification_of_a_folder_is_handled_like_one_of_the_inbox() {
    let rig = Rig::new();
    let work = rig.folder("Work", true);
    let ids: Vec<String> = (0..5).map(|i| deliver(&rig, &work.id, &format!("m{i}"))).collect();
    assert_eq!(rig.sync().post.len(), 5);

    // Mark read from the notification.
    rig.core.submit(Action::MarkRead { message_id: ids[0].clone() }).unwrap();
    assert_eq!(cancelled(&rig.sync()), [ids[0].as_str()]);
    assert!(rig.server.is_read(&ids[0]));
    // Archive.
    rig.core.submit(Action::Archive { message_id: ids[1].clone() }).unwrap();
    assert_eq!(cancelled(&rig.sync()), [ids[1].as_str()]);
    assert_eq!(rig.server.mailboxes_of(&ids[1]), [ARCHIVE_ID]);
    // Delete.
    rig.core.submit(Action::Delete { message_id: ids[2].clone() }).unwrap();
    assert_eq!(cancelled(&rig.sync()), [ids[2].as_str()]);
    assert_eq!(rig.server.mailboxes_of(&ids[2]), [TRASH_ID]);
    // Read on another device.
    rig.server.mark_read(&ids[3]);
    assert_eq!(cancelled(&rig.sync()), [ids[3].as_str()]);
    // Swiped: it does not come back, also when the shade was emptied.
    rig.core.notification_swiped(ids[4].clone()).unwrap();
    let plan = rig.core.check_once(Shade::Cleared).unwrap();
    assert!(plan.post.is_empty(), "{plan:?}");
}

#[test]
fn switching_a_folder_off_withdraws_its_notifications_in_the_next_plan() {
    let rig = Rig::new();
    let work = rig.folder("Work", true);
    let a = deliver(&rig, &work.id, "a");
    let inbox_mail = rig.server.deliver("Eve", "eve@example.test", "i", "p");
    assert_eq!(rig.sync().post.len(), 2);
    rig.core.set_mailbox_notify(work.id.clone(), false).unwrap();
    assert!(!rig.cached(&work.id).unwrap().notify);
    assert_eq!(cancelled(&rig.sync()), [a.as_str()], "the Inbox's stays: {inbox_mail}");
    // On again later: the mail still unread in it is old now (the clock counts whole seconds).
    std::thread::sleep(Duration::from_millis(1100));
    rig.core.set_mailbox_notify(work.id.clone(), true).unwrap();
    assert!(rig.sync().is_empty());
}

#[test]
fn the_choice_survives_a_refresh_of_the_folder_list_and_the_rules_about_it_hold() {
    let rig = Rig::new();
    let work = rig.folder("Work", true);
    // The server's list says nothing about the choice, and a full refresh replaces the rows.
    rig.server.forget_changes_before_now();
    rig.sync();
    assert!(rig.cached(&work.id).unwrap().notify);
    // The Inbox cannot be switched off; Drafts, Sent, Trash and Junk cannot be switched on.
    assert!(rig.cached(INBOX_ID).unwrap().notify);
    assert!(rig.refusal(rig.core.set_mailbox_notify(INBOX_ID.into(), false)).contains("Inbox"));
    assert!(rig.core.set_mailbox_notify(INBOX_ID.into(), true).is_ok());
    for id in [DRAFTS_ID, SENT_ID, TRASH_ID, JUNK_ID] {
        rig.refusal(rig.core.set_mailbox_notify(id.into(), true));
        assert!(!rig.cached(id).unwrap().notify);
    }
    assert!(rig.core.set_mailbox_notify(JUNK_ID.into(), false).is_ok());
    rig.refusal(rig.core.set_mailbox_notify("nope".into(), true));
    // Another client removes the folder: the choice goes with it.
    rig.server.remove_mailbox(&work.id);
    rig.sync();
    assert!(rig.cached(&work.id).is_none());
}

#[test]
fn live_the_push_pass_learns_of_mail_in_a_folder_that_notifies() {
    let rig = Rig::new();
    let work = rig.folder("Work", true);
    let kept = rig.folder("Kept", false);
    let rec = Arc::new(Recorder::default());
    rig.core.start(rec.clone());
    eventually("the connection to be live", || *rec.live.lock().unwrap());

    // A filter delivers into the folder.
    let id = deliver(&rig, &work.id, "live");
    eventually("the post", || rec.posted(&id));
    // Another client moves new mail into it.
    let moved = deliver(&rig, &kept.id, "moved");
    rig.server.move_to_mailbox(&moved, &work.id);
    eventually("the post of the moved mail", || rec.posted(&moved));
    // Read elsewhere: withdrawn.
    rig.server.mark_read(&id);
    eventually("the cancel", || rec.cancelled(&id));
    // Switched off: withdrawn in the next plan without any mail arriving.
    rig.core.set_mailbox_notify(work.id.clone(), false).unwrap();
    eventually("the cancel after switching off", || rec.cancelled(&moved));
    rig.core.stop();
}

#[test]
fn switching_off_withdraws_at_once_while_the_server_cannot_be_reached() {
    let rig = Rig::new();
    let work = rig.folder("Work", true);
    let rec = Arc::new(Recorder::default());
    rig.core.start(rec.clone());
    eventually("the connection to be live", || *rec.live.lock().unwrap());
    let id = deliver(&rig, &work.id, "on screen");
    eventually("the post", || rec.posted(&id));
    rig.server.set_down(true);
    rig.core.set_mailbox_notify(work.id.clone(), false).unwrap();
    assert!(rec.cancelled(&id), "no pass is needed for it");
    // And with the live loop stopped altogether.
    rig.server.set_down(false);
    rig.core.set_mailbox_notify(work.id.clone(), true).unwrap();
    rig.core.stop();
    let second = rig.server.deliver_in_at(&work.id, "Dora", "dora@example.test", "second", "p", now() + 5);
    assert_eq!(posted(&rig.sync()), [second.as_str()]);
    rig.server.set_down(true);
    rig.core.set_mailbox_notify(work.id.clone(), false).unwrap();
    assert!(rec.cancelled(&second));
}

#[test]
fn a_folder_switched_on_counts_from_the_newest_mail_it_holds_not_from_the_phones_clock() {
    let rig = Rig::new();
    let work = rig.folder("Work", false);
    // The server's clock is ahead of the phone's: its mail looks as if it came from the future.
    let ahead = rig.server.deliver_in_at(&work.id, "Dora", "dora@example.test", "ahead", "p", now() + 90);
    rig.sync();
    rig.core.set_mailbox_notify(work.id.clone(), true).unwrap();
    assert!(rig.sync().is_empty(), "{ahead} is old news");
    let newer = rig.server.deliver_in_at(&work.id, "Dora", "dora@example.test", "newer", "p", now() + 95);
    assert_eq!(posted(&rig.sync()), [newer.as_str()]);
}

// ---- making, renaming, moving and deleting folders ----------------------------------------------

#[test]
fn a_folder_is_made_on_the_server_and_is_in_the_cache_when_the_call_returns() {
    let rig = Rig::new();
    let made = rig.core.create_mailbox("  Receipts ".into(), None).unwrap();
    assert_eq!((made.name.as_str(), made.parent_id.clone(), made.role.clone()), ("Receipts", None, None));
    assert!(!made.notify);
    assert_eq!(rig.cached(&made.id), Some(made.clone()));
    assert!(rig
        .server
        .mailbox_list()
        .iter()
        .any(|(id, name, parent, _)| *id == made.id && name == "Receipts" && parent.is_none()));
    // Inside another folder, including the Inbox and Archive.
    let child = rig.core.create_mailbox("2026".into(), Some(made.id.clone())).unwrap();
    assert_eq!(child.parent_id.as_deref(), Some(made.id.as_str()));
    assert!(rig.core.create_mailbox("Old".into(), Some(ARCHIVE_ID.into())).is_ok());
    assert!(rig.core.create_mailbox("Lists".into(), Some(INBOX_ID.into())).is_ok());
    // The same name under another parent is fine.
    assert!(rig.core.create_mailbox("2026".into(), None).is_ok());
}

#[test]
fn a_folder_that_cannot_be_made_is_refused_before_the_server_is_asked() {
    let rig = Rig::new();
    let work = rig.folder("Work", false);
    let before = rig.asked("Mailbox/set");
    for bad in ["", "   ", "a/b", "line\nbreak", "tab\there", "nul\u{0}"] {
        rig.refusal(rig.core.create_mailbox(bad.into(), None));
    }
    assert!(rig.refusal(rig.core.create_mailbox("x".repeat(256), None)).contains("too long"));
    // The name is taken, whatever the case.
    assert!(rig.refusal(rig.core.create_mailbox("work".into(), None)).contains("already"));
    assert!(rig.refusal(rig.core.create_mailbox("INBOX".into(), None)).contains("already"));
    // No folders inside Drafts, Trash or Junk.
    for parent in [DRAFTS_ID, TRASH_ID, JUNK_ID] {
        assert!(rig
            .refusal(rig.core.create_mailbox("x".into(), Some(parent.into())))
            .contains("Drafts, Trash or Junk"));
    }
    rig.refusal(rig.core.create_mailbox("x".into(), Some("zz".into())));
    // Too deep: ten levels are fine, the eleventh is not.
    let mut parent = work.id.clone();
    for level in 2..=10 {
        parent = rig.core.create_mailbox(format!("level {level}"), Some(parent)).unwrap().id;
    }
    let asked = rig.asked("Mailbox/set");
    assert!(rig.refusal(rig.core.create_mailbox("level 11".into(), Some(parent))).contains("10 levels"));
    assert_eq!(rig.asked("Mailbox/set"), asked, "refused in the core");
    assert_eq!(before + 9, asked);
}

#[test]
fn what_the_server_refuses_after_a_change_elsewhere_comes_back_in_plain_words() {
    let rig = Rig::new();
    // Another client made "Taken" and removed the parent after this device last looked.
    let parent = rig.folder("Parent", false);
    rig.server.add_mailbox("other-1", "Taken", None);
    let msg = rig.refusal(rig.core.create_mailbox("Taken".into(), None));
    assert_eq!(msg, "A folder with that name is already there.");
    rig.server.remove_mailbox(&parent.id);
    let msg = rig.refusal(rig.core.create_mailbox("Child".into(), Some(parent.id.clone())));
    assert!(msg.contains("is gone"), "{msg}");
    assert!(!msg.contains("invalidProperties") && !msg.contains("Parent ID"));
    // The folder was renamed away and deleted by another client.
    let w = rig.folder("Work", false);
    rig.server.remove_mailbox(&w.id);
    let msg = rig.refusal(rig.core.rename_mailbox(w.id.clone(), "Jobs".into()));
    assert!(msg.contains("no longer on the server"), "{msg}");
}

#[test]
fn a_folder_the_user_made_is_renamed_on_the_server_and_in_the_cache() {
    let rig = Rig::new();
    let work = rig.folder("Work", true);
    rig.core.rename_mailbox(work.id.clone(), " Jobs ".into()).unwrap();
    let cached = rig.cached(&work.id).unwrap();
    assert_eq!(cached.name, "Jobs");
    assert!(cached.notify, "renaming does not touch the choice");
    assert!(rig.server.mailbox_list().iter().any(|(id, name, ..)| *id == work.id && name == "Jobs"));
    // The same name, or only its case, is fine; it does not ask the server for the same name.
    let asked = rig.asked("Mailbox/set");
    rig.core.rename_mailbox(work.id.clone(), "Jobs".into()).unwrap();
    assert_eq!(rig.asked("Mailbox/set"), asked);
    rig.core.rename_mailbox(work.id.clone(), "JOBS".into()).unwrap();
    assert_eq!(rig.cached(&work.id).unwrap().name, "JOBS");
}

#[test]
fn folders_with_a_role_and_taken_names_cannot_be_renamed() {
    let rig = Rig::new();
    let work = rig.folder("Work", false);
    rig.folder("Bills", false);
    let asked = rig.asked("Mailbox/set");
    for id in [INBOX_ID, ARCHIVE_ID, TRASH_ID, DRAFTS_ID, SENT_ID, JUNK_ID] {
        assert!(rig.refusal(rig.core.rename_mailbox(id.into(), "X".into())).contains("own folders"), "{id}");
    }
    assert!(rig.refusal(rig.core.rename_mailbox(work.id.clone(), "bills".into())).contains("already"));
    rig.refusal(rig.core.rename_mailbox(work.id.clone(), "a/b".into()));
    rig.refusal(rig.core.rename_mailbox(work.id.clone(), "  ".into()));
    rig.refusal(rig.core.rename_mailbox("zz".into(), "x".into()));
    assert_eq!(rig.asked("Mailbox/set"), asked);
    assert_eq!(rig.cached(INBOX_ID).unwrap().name, "Inbox");
}

#[test]
fn a_folder_is_moved_under_another_or_to_the_top() {
    let rig = Rig::new();
    let a = rig.folder("A", false);
    let b = rig.folder("B", false);
    rig.core.move_mailbox(b.id.clone(), Some(a.id.clone())).unwrap();
    assert_eq!(rig.cached(&b.id).unwrap().parent_id.as_deref(), Some(a.id.as_str()));
    assert!(rig
        .server
        .mailbox_list()
        .iter()
        .any(|(id, _, parent, _)| *id == b.id && parent.as_deref() == Some(a.id.as_str())));
    rig.core.move_mailbox(b.id.clone(), None).unwrap();
    assert_eq!(rig.cached(&b.id).unwrap().parent_id, None);
    // Where it is already: nothing asked.
    let asked = rig.asked("Mailbox/set");
    rig.core.move_mailbox(b.id.clone(), None).unwrap();
    assert_eq!(rig.asked("Mailbox/set"), asked);
}

#[test]
fn a_folder_does_not_go_into_itself_or_into_a_role_folder_that_takes_none() {
    let rig = Rig::new();
    let a = rig.folder("A", false);
    let b = rig.core.create_mailbox("B".into(), Some(a.id.clone())).unwrap();
    let c = rig.core.create_mailbox("C".into(), Some(b.id.clone())).unwrap();
    let asked = rig.asked("Mailbox/set");
    assert!(rig.refusal(rig.core.move_mailbox(a.id.clone(), Some(a.id.clone()))).contains("inside itself"));
    assert!(rig.refusal(rig.core.move_mailbox(a.id.clone(), Some(c.id.clone()))).contains("inside itself"));
    assert!(rig.refusal(rig.core.move_mailbox(b.id.clone(), Some(JUNK_ID.into()))).contains("Drafts, Trash or Junk"));
    for id in [INBOX_ID, TRASH_ID, DRAFTS_ID, SENT_ID, ARCHIVE_ID] {
        assert!(rig.refusal(rig.core.move_mailbox(id.into(), Some(a.id.clone()))).contains("own folders"), "{id}");
    }
    assert_eq!(rig.asked("Mailbox/set"), asked);
    // A name that is taken at the target.
    rig.folder("B", false);
    let asked = rig.asked("Mailbox/set");
    assert!(rig.refusal(rig.core.move_mailbox(b.id.clone(), None)).contains("already"));
    assert_eq!(rig.asked("Mailbox/set"), asked);
    // Nesting counts the folders below: A with B and C inside cannot go to level nine.
    let mut deep = rig.folder("Deep", false).id;
    for level in 2..=8 {
        deep = rig.core.create_mailbox(format!("d{level}"), Some(deep)).unwrap().id;
    }
    assert!(rig.refusal(rig.core.move_mailbox(a.id.clone(), Some(deep))).contains("10 levels"));
}

#[test]
fn an_empty_folder_is_deleted_from_the_server_and_the_cache() {
    let rig = Rig::new();
    let work = rig.folder("Work", true);
    rig.core.delete_mailbox(work.id.clone(), false).unwrap();
    assert!(rig.cached(&work.id).is_none());
    assert!(rig.server.mailbox_list().iter().all(|(id, ..)| *id != work.id));
    // Same for the same call with mail allowed.
    let other = rig.folder("Other", false);
    rig.core.delete_mailbox(other.id.clone(), true).unwrap();
    assert!(rig.cached(&other.id).is_none());
}

#[test]
fn a_folder_with_folders_inside_or_a_role_is_not_deleted() {
    let rig = Rig::new();
    let a = rig.folder("A", false);
    rig.core.create_mailbox("B".into(), Some(a.id.clone())).unwrap();
    let asked = rig.asked("Mailbox/set");
    for with_mail in [false, true] {
        assert!(rig.refusal(rig.core.delete_mailbox(a.id.clone(), with_mail)).contains("folders inside"));
        for id in [INBOX_ID, ARCHIVE_ID, TRASH_ID, DRAFTS_ID, SENT_ID, JUNK_ID] {
            assert!(rig.refusal(rig.core.delete_mailbox(id.into(), with_mail)).contains("own folders"), "{id}");
        }
    }
    rig.refusal(rig.core.delete_mailbox("zz".into(), false));
    assert_eq!(rig.asked("Mailbox/set"), asked);
    assert!(rig.cached(&a.id).is_some() && rig.cached(DRAFTS_ID).is_some());
}

#[test]
fn a_folder_with_mail_is_refused_unless_the_mail_may_go_to_the_trash() {
    let rig = Rig::new();
    let work = rig.folder("Work", false);
    deliver(&rig, &work.id, "one");
    rig.sync();
    let asked = rig.asked("Mailbox/set");
    assert!(rig.refusal(rig.core.delete_mailbox(work.id.clone(), false)).contains("still holds mail"));
    assert_eq!(rig.asked("Mailbox/set"), asked);
    // The cache says empty, the server does not (mail arrived since): the server's refusal is
    // put in words.
    let empty = rig.folder("Empty", false);
    deliver(&rig, &empty.id, "late");
    let msg = rig.refusal(rig.core.delete_mailbox(empty.id.clone(), false));
    assert!(msg.contains("still holds mail"), "{msg}");
    assert!(rig.cached(&empty.id).is_some());
}

#[test]
fn deleting_with_mail_moves_it_to_the_trash_and_leaves_mail_that_is_elsewhere_too() {
    let rig = Rig::new();
    let work = rig.folder("Work", false);
    let only = deliver(&rig, &work.id, "only here");
    let also = deliver(&rig, &work.id, "also in the archive");
    rig.server.add_to_mailbox(&also, ARCHIVE_ID);
    let unread_only = only.clone();
    rig.sync();
    rig.core.delete_mailbox(work.id.clone(), true).unwrap();
    assert!(rig.cached(&work.id).is_none());
    assert!(rig.server.mailbox_list().iter().all(|(id, ..)| *id != work.id));
    // Moved, never destroyed.
    assert!(rig.server.exists(&only) && rig.server.exists(&also));
    assert_eq!(rig.server.mailboxes_of(&only), [TRASH_ID]);
    assert_eq!(rig.server.mailboxes_of(&also), [ARCHIVE_ID], "it just left the folder");
    // The cache agrees at once, before any sync.
    assert!(rig.listed(TRASH_ID).contains(&unread_only));
    assert!(rig.listed(ARCHIVE_ID).contains(&also) && !rig.listed(TRASH_ID).contains(&also));
    // And after a sync nothing changes.
    rig.sync();
    assert!(rig.listed(TRASH_ID).contains(&only));
    assert!(rig.cached(&work.id).is_none());
}

#[test]
fn a_big_folder_is_emptied_in_batches_and_the_mail_that_was_in_it_is_not_notified() {
    let rig = Rig::new();
    let work = rig.folder("Work", true);
    let ids: Vec<String> = (0..120).map(|i| deliver(&rig, &work.id, &format!("m{i}"))).collect();
    let plan = rig.sync();
    assert_eq!(plan.post.len(), 120);
    let sets = rig.asked("Email/set");
    rig.core.delete_mailbox(work.id.clone(), true).unwrap();
    assert!(rig.asked("Email/set") - sets >= 3, "batches of at most 50");
    assert_eq!(rig.server.ids_in("trash").len(), 120);
    assert!(ids.iter().all(|id| rig.server.exists(id)));
    // The notifications of mail that left the folder are withdrawn.
    assert_eq!(rig.sync().cancel.len(), 120);
}

#[test]
fn a_folder_that_has_a_folder_inside_elsewhere_is_refused_before_its_mail_is_moved() {
    let rig = Rig::new();
    let work = rig.folder("Work", false);
    let mail = deliver(&rig, &work.id, "stays");
    rig.sync();
    // Another client made a folder inside it; this device has not synced yet.
    rig.server.add_mailbox("sub", "Sub", Some(&work.id));
    assert!(rig.refusal(rig.core.delete_mailbox(work.id.clone(), true)).contains("folders inside"));
    assert_eq!(rig.server.mailboxes_of(&mail), [work.id.as_str()], "the mail was not moved");
    assert!(rig.cached(&work.id).is_some());
}

#[test]
fn a_failure_part_way_leaves_the_folder_and_says_how_far_it_got() {
    let rig = Rig::new();
    let work = rig.folder("Work", false);
    for i in 0..120 {
        deliver(&rig, &work.id, &format!("m{i}"));
    }
    rig.sync();
    rig.server.set_email_set_budget(Some(60));
    let msg = match rig.core.delete_mailbox(work.id.clone(), true) {
        Err(Error::Temporary { msg }) => msg,
        other => panic!("expected a passing failure, got {other:?}"),
    };
    assert!(msg.contains("60 messages were moved to the Trash"), "{msg}");
    assert!(msg.contains("folder was not deleted"), "{msg}");
    assert!(rig.cached(&work.id).is_some());
    assert!(rig.server.mailbox_list().iter().any(|(id, ..)| *id == work.id));
    assert_eq!(rig.server.ids_in("trash").len(), 60);
    // Run it again when the server is well: the rest moves and the folder goes.
    rig.server.set_email_set_budget(None);
    rig.core.delete_mailbox(work.id.clone(), true).unwrap();
    assert_eq!(rig.server.ids_in("trash").len(), 120);
    assert!(rig.cached(&work.id).is_none());
}

#[test]
fn a_folder_the_server_cannot_be_asked_about_stays() {
    let rig = Rig::new();
    let work = rig.folder("Work", false);
    rig.server.set_down(true);
    assert!(matches!(rig.core.delete_mailbox(work.id.clone(), false), Err(Error::Temporary { .. })));
    assert!(matches!(rig.core.create_mailbox("X".into(), None), Err(Error::Temporary { .. })));
    assert!(matches!(rig.core.rename_mailbox(work.id.clone(), "Y".into()), Err(Error::Temporary { .. })));
    rig.server.set_down(false);
    assert_eq!(rig.cached(&work.id).unwrap().name, "Work");
    assert!(rig.cached(&work.id).is_some());
}

// ---- spam ----------------------------------------------------------------------------------

#[test]
fn spam_goes_to_junk_with_the_keywords_and_the_cache_shows_it_at_once() {
    let rig = Rig::new();
    let id = rig.server.deliver("Spammer", "spam@example.test", "win", "p");
    rig.sync();
    rig.core.report_spam(vec![id.clone()], true).unwrap();
    // Before anything reached the server.
    assert_eq!(rig.listed(JUNK_ID), std::slice::from_ref(&id));
    assert!(!rig.listed(INBOX_ID).contains(&id));
    rig.sync();
    assert_eq!(rig.server.mailboxes_of(&id), [JUNK_ID]);
    let keywords = rig.server.keywords_of(&id);
    assert!(keywords.contains(&"$junk".to_string()) && !keywords.contains(&"$notjunk".to_string()), "{keywords:?}");
}

#[test]
fn not_spam_goes_back_to_the_inbox_with_the_keywords_the_other_way_round() {
    let rig = Rig::new();
    let id = rig.server.deliver("Friend", "friend@example.test", "hi", "p");
    rig.server.move_to(&id, "junk");
    rig.server.add_keyword(&id, "$junk");
    rig.sync();
    rig.core.report_spam(vec![id.clone()], false).unwrap();
    assert_eq!(rig.listed(INBOX_ID)[0], id);
    assert!(!rig.listed(JUNK_ID).contains(&id));
    rig.sync();
    assert_eq!(rig.server.mailboxes_of(&id), [INBOX_ID]);
    let keywords = rig.server.keywords_of(&id);
    assert!(keywords.contains(&"$notjunk".to_string()) && !keywords.contains(&"$junk".to_string()), "{keywords:?}");
    // A message that has both from before ends with the right one.
    rig.server.add_keyword(&id, "$junk");
    rig.sync();
    rig.core.report_spam(vec![id.clone()], false).unwrap();
    rig.sync();
    assert!(!rig.server.keywords_of(&id).contains(&"$junk".to_string()));
}

#[test]
fn several_messages_are_reported_in_one_call_and_it_works_offline() {
    let rig = Rig::new();
    let a = rig.server.deliver("S", "s@example.test", "a", "p");
    let b = rig.server.deliver("S", "s@example.test", "b", "p");
    rig.sync();
    rig.server.set_down(true);
    rig.core.report_spam(vec![a.clone(), b.clone()], true).unwrap();
    assert_eq!(rig.listed(JUNK_ID).len(), 2);
    assert!(rig.core.check_once(Shade::Unknown).is_err());
    assert_eq!(rig.listed(JUNK_ID).len(), 2, "still there offline");
    rig.server.set_down(false);
    rig.sync();
    for id in [&a, &b] {
        assert_eq!(rig.server.mailboxes_of(id), [JUNK_ID]);
        assert!(rig.server.keywords_of(id).contains(&"$junk".to_string()));
    }
}

#[test]
fn spam_reported_in_order_with_other_actions_keeps_the_order() {
    let rig = Rig::new();
    let id = rig.server.deliver("S", "s@example.test", "a", "p");
    rig.sync();
    rig.core.submit(Action::MarkRead { message_id: id.clone() }).unwrap();
    rig.core.report_spam(vec![id.clone()], true).unwrap();
    rig.core.report_spam(vec![id.clone()], false).unwrap();
    rig.sync();
    assert!(rig.server.is_read(&id));
    assert_eq!(rig.server.mailboxes_of(&id), [INBOX_ID], "the last report wins");
    let keywords = rig.server.keywords_of(&id);
    assert!(keywords.contains(&"$notjunk".to_string()) && !keywords.contains(&"$junk".to_string()));
}

#[test]
fn a_message_reported_as_spam_leaves_the_notifications() {
    let rig = Rig::new();
    let work = rig.folder("Work", true);
    let in_inbox = rig.server.deliver("S", "s@example.test", "i", "p");
    let in_work = deliver(&rig, &work.id, "w");
    let both = rig.server.deliver("S", "s@example.test", "b", "p");
    rig.server.add_to_mailbox(&both, &work.id);
    assert_eq!(rig.sync().post.len(), 3);
    rig.core.report_spam(vec![in_inbox.clone(), in_work.clone(), both.clone()], true).unwrap();
    let plan = rig.sync();
    let mut gone = cancelled(&plan);
    gone.sort();
    let mut all = vec![in_inbox.as_str(), in_work.as_str(), both.as_str()];
    all.sort();
    assert_eq!(gone, all);
    assert!(plan.post.is_empty());
}

#[test]
fn spam_needs_a_junk_folder_and_a_message_the_server_lost_is_dropped() {
    let rig = Rig::new();
    let id = rig.server.deliver("S", "s@example.test", "a", "p");
    rig.sync();
    // Gone on the server before the report is sent: the queue does not get stuck.
    rig.server.destroy(&id);
    rig.core.report_spam(vec![id.clone()], true).unwrap();
    rig.sync();
    rig.sync();
    assert!(rig.core.message(id).unwrap().is_none());
    // No Junk folder.
    rig.server.remove_mailbox(JUNK_ID);
    rig.sync();
    let other = rig.server.deliver("S", "s@example.test", "b", "p");
    rig.sync();
    let msg = rig.refusal(rig.core.report_spam(vec![other.clone()], true));
    assert!(msg.contains("no Junk"), "{msg}");
    assert!(rig.listed(INBOX_ID).contains(&other), "nothing was changed");
    // Not spam still works without Junk.
    assert!(rig.core.report_spam(vec![other], false).is_ok());
}

// ---- blocking a sender -----------------------------------------------------------------------

const SCRIPT: &str = "kweb-filters";

#[test]
fn blocking_a_sender_adds_the_first_rule_switches_the_filters_on_and_trashes_the_message() {
    let rig = Rig::new();
    let id = rig.server.deliver("Pest", "pest@example.test", "buy now", "p");
    rig.sync();
    let f = rig.core.block_sender(id.clone()).unwrap();
    assert!(f.is_active && !f.is_edited);
    assert_eq!(f.rules.len(), 1);
    let rule = &f.rules[0];
    assert_eq!(rule.name, "Blocked: pest@example.test");
    assert!(rule.enabled && rule.delete && rule.stop && rule.move_to.is_none());
    assert_eq!(rule.conditions[0].value, "pest@example.test");
    let text = rig.server.sieve_text(SCRIPT).unwrap();
    assert!(text.contains("pest@example.test"), "{text}");
    assert_eq!(rig.server.sieve_active().as_deref(), Some(SCRIPT));
    // The message is in the Trash in the cache at once and on the server after a pass.
    assert_eq!(rig.listed(TRASH_ID), std::slice::from_ref(&id));
    rig.sync();
    assert_eq!(rig.server.mailboxes_of(&id), [TRASH_ID]);
    assert!(rig.server.exists(&id), "moved, not destroyed");
}

#[test]
fn the_rule_goes_first_and_the_users_own_rules_stay() {
    let rig = Rig::new();
    let id = rig.server.deliver("Pest", "pest@example.test", "x", "p");
    rig.sync();
    let mut mine = rig.core.filter_from_message("e1".into()).unwrap();
    mine.move_to = Some(ARCHIVE_ID.into());
    rig.core.filters().unwrap();
    rig.core.save_filters(vec![mine], false, false).unwrap();
    let f = rig.core.block_sender(id).unwrap();
    assert_eq!(f.rules.len(), 2);
    assert!(f.rules[0].name.starts_with("Blocked: ") && f.rules[1].move_to.as_deref() == Some(ARCHIVE_ID));
    assert!(f.is_active);
}

#[test]
fn a_sender_that_is_blocked_already_changes_no_filter() {
    let rig = Rig::new();
    let first = rig.server.deliver("Pest", "Pest@Example.test", "one", "p");
    let second = rig.server.deliver("Pest", "pest@example.test", "two", "p");
    rig.sync();
    rig.core.block_sender(first).unwrap();
    let writes = rig.asked("SieveScript/set");
    let f = rig.core.block_sender(second.clone()).unwrap();
    assert_eq!(f.rules.len(), 1);
    assert_eq!(rig.asked("SieveScript/set"), writes, "nothing was written");
    assert_eq!(rig.listed(TRASH_ID).len(), 2, "the message still goes to the Trash");
    // A script that was switched off since is switched on again, and no rule is added.
    rig.core.set_filters_active(false).unwrap();
    let third = rig.server.deliver("Pest", "pest@example.test", "three", "p");
    rig.sync();
    let f = rig.core.block_sender(third).unwrap();
    assert!(f.is_active && f.rules.len() == 1);
}

#[test]
fn blocking_is_refused_in_words_when_it_would_change_what_the_user_decides() {
    let rig = Rig::new();
    let id = rig.server.deliver("Pest", "pest@example.test", "x", "p");
    rig.sync();
    let untouched = |rig: &Rig| {
        assert_eq!(rig.asked("SieveScript/set"), 0, "nothing was written");
        assert!(rig.listed(INBOX_ID).contains(&id), "the message did not move");
    };

    // The away message is on.
    rig.server.set_vacation(json!({ "isEnabled": true, "textBody": "I am away." }));
    assert!(rig.refusal(rig.core.block_sender(id.clone())).contains("away message"));
    untouched(&rig);
    rig.server.set_vacation(json!({ "isEnabled": false }));
    rig.server.set_sieve_active("vacation", false);

    // Another script is the active one.
    rig.server.put_sieve_script("theirs", "keep;\n", true);
    let msg = rig.refusal(rig.core.block_sender(id.clone()));
    assert!(msg.contains("theirs"), "{msg}");
    untouched(&rig);
    assert_eq!(rig.server.sieve_active().as_deref(), Some("theirs"));
    rig.server.set_sieve_active("theirs", false);

    // The app's script was edited by hand.
    rig.server.put_sieve_script(SCRIPT, "keep;\n", false);
    assert!(rig.refusal(rig.core.block_sender(id.clone())).contains("by hand"));
    untouched(&rig);
    assert_eq!(rig.server.sieve_text(SCRIPT).unwrap(), "keep;\n");
}

#[test]
fn blocking_says_so_when_another_rule_points_at_a_folder_that_is_gone() {
    let rig = Rig::new();
    let news = rig.folder("News", false);
    let id = rig.server.deliver("Pest", "pest@example.test", "x", "p");
    rig.sync();
    let mut mine = rig.core.filter_from_message("e1".into()).unwrap();
    mine.move_to = Some(news.id.clone());
    rig.core.filters().unwrap();
    rig.core.save_filters(vec![mine], false, false).unwrap();
    rig.core.delete_mailbox(news.id.clone(), false).unwrap();
    let writes = rig.asked("SieveScript/set");
    let msg = rig.refusal(rig.core.block_sender(id.clone()));
    assert!(msg.contains("Blocking") && msg.contains("no longer exists"), "{msg}");
    assert_eq!(rig.asked("SieveScript/set"), writes);
    assert!(rig.listed(INBOX_ID).contains(&id), "the message did not move");
}

#[test]
fn blocking_is_refused_without_filters_for_ones_own_address_and_for_an_unknown_message() {
    let rig = Rig::new();
    // One of the user's own addresses: the seeded mail in Sent is from them.
    let own = rig.listed(SENT_ID)[0].clone();
    assert!(rig.refusal(rig.core.block_sender(own)).contains("your own addresses"));
    let me = rig.server.deliver("Me", &USERNAME.to_uppercase(), "from myself", "p");
    rig.sync();
    assert!(rig.refusal(rig.core.block_sender(me)).contains("your own addresses"));
    // An own address that merely ends in the blocked one would be caught by "From contains".
    let suffix = rig.server.deliver("Lice", &USERNAME[1..], "ends the same", "p");
    rig.sync();
    assert!(rig.refusal(rig.core.block_sender(suffix)).contains("your own addresses"));
    rig.refusal(rig.core.block_sender("nope".into()));
    // A message with no sender address.
    let nobody = rig.server.deliver("Nobody", " ", "none", "p");
    rig.sync();
    rig.refusal(rig.core.block_sender(nobody));
}

#[test]
fn blocking_is_refused_in_words_when_the_server_has_no_filters() {
    let rig = Rig::build(|server| server.set_sieve_enabled(false));
    let id = rig.server.deliver("Pest", "pest@example.test", "x", "p");
    rig.sync();
    assert!(rig.refusal(rig.core.block_sender(id.clone())).contains("no mail filters"));
    assert!(rig.listed(INBOX_ID).contains(&id));
    assert_eq!(rig.asked("SieveScript/set"), 0);
}

#[test]
fn a_message_that_is_in_the_trash_already_is_not_destroyed_by_blocking_its_sender() {
    let rig = Rig::new();
    let id = rig.server.deliver("Pest", "pest@example.test", "x", "p");
    rig.server.move_to(&id, "trash");
    rig.sync();
    rig.core.block_sender(id.clone()).unwrap();
    rig.sync();
    assert!(rig.server.exists(&id));
    assert_eq!(rig.server.mailboxes_of(&id), [TRASH_ID]);
}

#[test]
fn blocking_leaves_the_notification_like_any_message_that_goes_to_the_trash() {
    let rig = Rig::new();
    let id = rig.server.deliver("Pest", "pest@example.test", "x", "p");
    assert_eq!(posted(&rig.sync()), [id.as_str()]);
    rig.core.block_sender(id.clone()).unwrap();
    assert_eq!(cancelled(&rig.sync()), [id.as_str()]);
}
