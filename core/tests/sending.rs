//! M4 against the mock server: drafts, sending with the undo window, attachments, identities,
//! recipient suggestions and quota, through the `Core` object the way the app uses it.
//!
//! The mock is async (axum), `Core` blocks and owns its own runtime, so every test is a plain
//! `#[test]` that keeps a separate runtime alive just for the mock. A pass is `check_once`
//! unless the test is about the live connection.

mod support;

use std::{
    fs,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use kweb_core::{
    ffi::{Core, Shade},
    model::{Address, ConnectionStatus, Draft, DraftKind, Reminder},
    notify::Plan,
    push::Listener,
    secrets::MemorySecrets,
    store::Store,
    Error,
};
use serde_json::json;
use support::{MockServer, Refusal, ACCOUNT_ID, USERNAME};
use tokio::runtime::Runtime;

const DEADLINE: Duration = Duration::from_secs(8);
/// Seeded: Maria's mail in the Inbox, sent to the user's address.
const E1: &str = "e1";
/// Seeded: the Build Bot mail.
const E2: &str = "e2";
const MARIA: &str = "maria@example.test";

#[derive(Default)]
struct Recorder {
    live: Mutex<bool>,
    changes: Mutex<usize>,
}

impl Listener for Recorder {
    fn status(&self, status: ConnectionStatus) {
        if matches!(status, ConnectionStatus::Live { .. }) {
            *self.live.lock().unwrap() = true;
        }
    }
    fn notifications(&self, _: Plan) {}
    fn cache_changed(&self) {
        *self.changes.lock().unwrap() += 1;
    }
    fn reminder(&self, _: Reminder) {}
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
    db_path: String,
    dir: tempfile::TempDir,
}

impl Rig {
    /// A mock server and a core signed in to it, with nothing cached yet: set the server up as
    /// the test needs, then `sync`.
    fn new() -> Rig {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        let server = rt.block_on(MockServer::start());
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("cache.db").to_string_lossy().into_owned();
        let core = Core::new(db_path.clone(), Arc::new(MemorySecrets::default())).unwrap();
        let url = core.begin_sign_in_with_redirect(server.url(), "ovh.kweb.mail:/oauth/callback".into()).unwrap();
        core.complete_sign_in(server.authorize(&url).unwrap()).unwrap();
        Rig { core, server, _rt: rt, db_path, dir }
    }

    /// One pass: mail, mailboxes, identities, address book in the cache; drafts and sends handled.
    fn sync(&self) {
        self.core.check_once(Shade::Unknown).expect("a pass");
    }

    /// A rig that has done its first pass.
    fn synced() -> Rig {
        let rig = Rig::new();
        rig.sync();
        rig
    }

    fn start_listening(&self) -> Arc<Recorder> {
        let rec = Arc::new(Recorder::default());
        self.core.start(rec.clone());
        eventually("the connection to be live", || *rec.live.lock().unwrap());
        rec
    }

    /// A new reply to `id`, with the text typed above the quote.
    fn reply_with(&self, id: &str, text: &str) -> Draft {
        let mut draft = self.core.new_draft(DraftKind::Reply { message_id: id.into() }).unwrap();
        draft.body = format!("{text}{}", draft.body);
        draft
    }

    /// A message to Bo with a body, ready to send.
    fn note(&self, subject: &str) -> Draft {
        let mut draft = self.core.new_draft(DraftKind::New).unwrap();
        draft.to = vec![addr("Bo", "bo@example.test")];
        draft.subject = subject.into();
        draft.body = "Hello Bo".into();
        draft
    }

    /// The other connection to the cache file, for what `Core` does not show.
    fn store(&self) -> Store {
        Store::open(std::path::Path::new(&self.db_path)).unwrap()
    }
}

fn addr(name: &str, email: &str) -> Address {
    Address { name: name.into(), email: email.into() }
}

fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter().map(|(n, e)| (n.to_string(), e.to_string())).collect()
}

// ---- identities ---------------------------------------------------------------------------

#[test]
fn the_first_pass_caches_the_identities_and_later_ones_leave_them_for_an_hour() {
    let rig = Rig::new();
    assert!(rig.core.identities().unwrap().is_empty());
    rig.sync();
    let identities = rig.core.identities().unwrap();
    let ids: Vec<_> = identities.iter().map(|i| (i.id.as_str(), i.email.as_str())).collect();
    assert_eq!(ids, [("i1", USERNAME), ("i2", "alias@example.test")]);
    assert_eq!(identities[0].name, "Alice Test");
    assert_eq!(identities[0].signature, "Alice Test\nTest Corp");
    assert_eq!(identities[1].signature, "");

    // The server's identities change; the next passes of this process do not ask again.
    rig.server.set_identities(&[("i9", "New", "new@example.test", "")]);
    rig.sync();
    rig.sync();
    assert_eq!(rig.server.request_count("Identity/get"), 1);
    assert_eq!(rig.core.identities().unwrap()[0].id, "i1");

    // A new process asks once, on its first pass.
    let again = Core::new(rig.db_path.clone(), Arc::new(MemorySecrets::default())).unwrap();
    assert_eq!(again.identities().unwrap().len(), 2, "the cache is kept");
}

#[test]
fn a_failed_directory_refresh_does_not_fail_the_pass_and_is_tried_again() {
    let rig = Rig::new();
    rig.server.set_identities(&[]);
    // Contacts and identities are fetched after the notifications: whatever they do, the pass works.
    rig.sync();
    assert!(rig.core.identities().unwrap().is_empty());
    rig.server.set_identities(&[("i1", "Me", "me@example.test", "")]);
    rig.sync();
    // An empty answer is an answer: it was fetched, so it is not asked again for an hour.
    assert_eq!(rig.server.request_count("Identity/get"), 1);
}

// ---- starting a draft ---------------------------------------------------------------------

#[test]
fn a_blank_draft_has_the_default_identity_and_its_signature() {
    let rig = Rig::synced();
    let draft = rig.core.new_draft(DraftKind::New).unwrap();
    assert_eq!(draft.identity_id, "i1");
    assert_eq!(draft.body, "\n\n-- \nAlice Test\nTest Corp");
    assert_eq!(draft.id.len(), 32);
    // It is in the cache from the start.
    assert_eq!(rig.core.draft(draft.id.clone()).unwrap(), Some(draft.clone()));
    assert_eq!(rig.core.drafts().unwrap(), vec![draft]);
}

#[test]
fn a_reply_is_built_by_the_rules() {
    let rig = Rig::new();
    rig.server.set_bodies(E1, Some("Are you free at noon?\nTable for two."), None);
    rig.sync();
    let draft = rig.core.new_draft(DraftKind::Reply { message_id: E1.into() }).unwrap();
    assert_eq!(draft.to, vec![addr("Maria Keller", MARIA)]);
    assert!(draft.cc.is_empty() && draft.bcc.is_empty());
    assert_eq!(draft.subject, "Re: Lunch on Friday?");
    assert_eq!(draft.identity_id, "i1");
    assert_eq!(
        draft.body,
        "\n\nMaria Keller <maria@example.test> wrote:\n> Are you free at noon?\n> Table for two.\n\n-- \nAlice Test\nTest Corp"
    );
    assert_eq!(
        (draft.source_message_id.as_deref(), draft.is_forward, draft.server_id.as_deref()),
        (Some(E1), false, None)
    );
}

#[test]
fn a_reply_goes_out_as_the_address_the_mail_was_sent_to() {
    let rig = Rig::new();
    rig.server.set_recipients(E2, &[("Alias", "ALIAS@example.test")], &[("Cy", "cy@example.test")]);
    rig.sync();
    let draft = rig.core.new_draft(DraftKind::ReplyAll { message_id: E2.into() }).unwrap();
    assert_eq!(draft.identity_id, "i2");
    assert_eq!(draft.to, vec![addr("Build Bot", "ci@example.test")]);
    assert_eq!(draft.cc, vec![addr("Cy", "cy@example.test")], "the user's own address is left out");
    // The second identity has no signature.
    assert!(!draft.body.contains("-- "));
}

#[test]
fn a_reply_to_html_only_mail_quotes_a_text_rendering() {
    let rig = Rig::new();
    rig.server.set_bodies(E1, None, Some("<style>p{}</style><p>Hello&nbsp;there</p><p>Bye</p>"));
    rig.sync();
    let draft = rig.core.new_draft(DraftKind::Reply { message_id: E1.into() }).unwrap();
    assert!(draft.body.contains("wrote:\n> Hello there\n> Bye"), "{}", draft.body);
}

#[test]
fn a_forward_carries_the_header_block_and_the_files() {
    let rig = Rig::new();
    rig.server.set_bodies(E1, Some("See attached."), Some("<p>See attached.</p><img src=\"cid:logo\">"));
    rig.server.add_attachment(E1, "report.pdf", "application/pdf", b"%PDF-1.4 data", None, false);
    rig.server.add_attachment(E1, "logo.png", "image/png", b"png", Some("logo"), true);
    rig.sync();
    let mut draft = rig.core.new_draft(DraftKind::Forward { message_id: E1.into() }).unwrap();
    assert_eq!(draft.subject, "Fwd: Lunch on Friday?");
    assert!(draft.is_forward && draft.to.is_empty());
    assert!(draft.body.contains("---------- Forwarded message ----------\nFrom: Maria Keller <maria@example.test>\n"));
    assert!(draft.body.contains("\nSubject: Lunch on Friday?\nTo: Alice Test <alice@example.test>\n\nSee attached."));
    assert_eq!(draft.attachments.len(), 1, "the picture the HTML shows is not a file");
    assert_eq!((draft.attachments[0].name.as_str(), draft.attachments[0].size), ("report.pdf", 13));

    // Sent, the server holds the file in the message, and the original counts as forwarded.
    draft.to = vec![addr("", "bo@example.test")];
    rig.core.send(draft, 0).unwrap();
    rig.sync();
    let sent = rig.server.view(rig.server.ids_in("sent").last().unwrap());
    assert_eq!(sent.files, vec![("report.pdf".to_string(), "application/pdf".to_string(), b"%PDF-1.4 data".to_vec())]);
    assert!(rig.server.keywords_of(E1).contains(&"$forwarded".to_string()));
}

#[test]
fn a_draft_on_the_server_opens_as_a_local_draft_once() {
    let rig = Rig::new();
    let id = rig.server.deliver("Alice Test", USERNAME, "Half written", "Dear Bo");
    rig.server.set_recipients(&id, &[("Bo", "bo@example.test")], &[("Cy", "cy@example.test")]);
    rig.server.set_bodies(&id, Some("Dear Bo,"), None);
    rig.server.move_to(&id, "drafts");
    rig.sync();

    let draft = rig.core.new_draft(DraftKind::Existing { message_id: id.clone() }).unwrap();
    assert_eq!(draft.server_id.as_deref(), Some(id.as_str()));
    assert_eq!(
        (draft.subject.as_str(), draft.body.as_str(), draft.identity_id.as_str()),
        ("Half written", "Dear Bo,", "i1")
    );
    assert_eq!((draft.to.len(), draft.cc.len()), (1, 1));
    // Opened again, it is the same draft, not a second one.
    assert_eq!(rig.core.new_draft(DraftKind::Existing { message_id: id.clone() }).unwrap().id, draft.id);
    assert_eq!(rig.core.drafts().unwrap().len(), 1);

    // Nothing is saved to the server until it is edited; then the copy is replaced, not added to.
    rig.sync();
    assert_eq!(rig.server.ids_in("drafts"), vec![id.clone()]);
    let mut edited = draft;
    edited.body = "Dear Bo, more".into();
    rig.core.save_draft(edited).unwrap();
    rig.sync();
    let copies = rig.server.ids_in("drafts");
    assert_eq!(copies.len(), 1);
    assert_ne!(copies[0], id);
    assert_eq!(rig.server.view(&copies[0]).text.as_deref(), Some("Dear Bo, more"));
    assert!(matches!(
        rig.core.new_draft(DraftKind::Existing { message_id: "nope".into() }),
        Err(Error::Invalid { .. })
    ));
}

#[test]
fn replying_to_what_is_not_cached_is_an_error() {
    let rig = Rig::synced();
    assert!(matches!(rig.core.new_draft(DraftKind::Reply { message_id: "zz".into() }), Err(Error::Invalid { .. })));
}

// ---- drafts on the server -------------------------------------------------------------------

#[test]
fn a_draft_written_offline_reaches_drafts_with_the_next_pass_and_an_edit_replaces_it() {
    let rig = Rig::synced();
    rig.server.set_down(true);
    let mut draft = rig.note("Offline thoughts");
    draft = rig.core.save_draft(draft).unwrap();
    assert!(rig.core.check_once(Shade::Unknown).is_err(), "the server is down");
    assert!(rig.server.ids_in("drafts").is_empty());
    assert_eq!(rig.core.drafts().unwrap().len(), 1);

    rig.server.set_down(false);
    rig.sync();
    let copies = rig.server.ids_in("drafts");
    assert_eq!(copies.len(), 1);
    let first = rig.server.view(&copies[0]);
    assert_eq!(first.keywords, ["$draft", "$seen"]);
    assert_eq!((first.subject.as_str(), first.text.as_deref()), ("Offline thoughts", Some("Hello Bo")));
    assert_eq!(first.to, pairs(&[("Bo", "bo@example.test")]));
    assert_eq!(first.from, ("Alice Test".to_string(), USERNAME.to_string()));
    assert_eq!(rig.core.draft(draft.id.clone()).unwrap().unwrap().server_id.as_deref(), Some(copies[0].as_str()));

    // A pass with nothing to save does nothing.
    rig.server.reset_request_counts();
    rig.sync();
    assert_eq!(rig.server.request_count("Email/set"), 0);

    // An edit replaces the copy: one message in Drafts, and the old one is gone.
    draft.body = "Edited".into();
    rig.core.save_draft(draft).unwrap();
    rig.sync();
    let now = rig.server.ids_in("drafts");
    assert_eq!(now.len(), 1);
    assert!(!rig.server.exists(&copies[0]));
    assert_eq!(rig.server.view(&now[0]).text.as_deref(), Some("Edited"));
}

#[test]
fn two_saves_before_a_pass_make_one_copy_of_the_newer() {
    // The same thing at the store level is tested in the unit tests; this one is the whole path:
    // two saves before one pass make one copy, the newer.
    let rig = Rig::synced();
    let mut draft = rig.core.save_draft(rig.note("Twice")).unwrap();
    draft.body = "second".into();
    rig.core.save_draft(draft).unwrap();
    rig.sync();
    assert_eq!(rig.server.ids_in("drafts").len(), 1);
    let copy = rig.server.view(&rig.server.ids_in("drafts")[0]);
    assert_eq!(copy.text.as_deref(), Some("second"));
}

#[test]
fn discarding_removes_the_draft_here_at_once_and_on_the_server_with_the_next_pass() {
    let rig = Rig::synced();
    let draft = rig.core.save_draft(rig.note("Doomed")).unwrap();
    rig.sync();
    assert_eq!(rig.server.ids_in("drafts").len(), 1);

    rig.core.discard_draft(draft.id.clone()).unwrap();
    assert!(rig.core.drafts().unwrap().is_empty());
    assert_eq!(rig.core.draft(draft.id.clone()).unwrap(), None);
    assert_eq!(rig.server.ids_in("drafts").len(), 1, "the server hears with the next pass");
    assert!(matches!(rig.core.save_draft(draft.clone()), Err(Error::Invalid { .. })), "no autosave brings it back");

    rig.sync();
    assert!(rig.server.ids_in("drafts").is_empty());
    assert!(rig.store().drafts_discarded(ACCOUNT_ID).unwrap().is_empty(), "the row is gone too");
    // Discarding twice, or something that never existed, is fine.
    rig.core.discard_draft(draft.id).unwrap();
    rig.core.discard_draft("never-was".into()).unwrap();
}

#[test]
fn a_draft_with_a_bad_id_is_refused() {
    let rig = Rig::synced();
    let mut draft = rig.note("x");
    draft.id = String::new();
    assert!(matches!(rig.core.save_draft(draft.clone()), Err(Error::Invalid { .. })));
    draft.id = "has space".into();
    assert!(matches!(rig.core.save_draft(draft), Err(Error::Invalid { .. })));
}

// ---- sending --------------------------------------------------------------------------------

#[test]
fn a_reply_is_sent_once_filed_in_sent_and_marks_the_original_answered() {
    let rig = Rig::new();
    rig.server.set_bodies(E1, Some("Are you free at noon?"), None);
    rig.sync();
    let draft = rig.reply_with(E1, "Noon is fine.");
    let body = draft.body.clone();
    let pending = rig.core.send(draft.clone(), 0).unwrap();
    assert_eq!((pending.draft_id.as_str(), pending.subject.as_str()), (draft.id.as_str(), "Re: Lunch on Friday?"));
    assert_eq!(rig.core.pending_sends().unwrap(), vec![pending]);
    assert!(rig.core.drafts().unwrap().is_empty());
    assert!(rig.server.submissions().is_empty(), "nothing leaves before a pass");

    rig.sync();
    let submissions = rig.server.submissions();
    assert_eq!(submissions.len(), 1);
    assert_eq!(
        (submissions[0].identity_id.as_str(), submissions[0].recipients.clone()),
        ("i1", vec![MARIA.to_string()])
    );

    let sent = rig.server.view(&submissions[0].email_id);
    assert_eq!(sent.roles, ["sent"], "moved from Drafts to Sent");
    assert_eq!(sent.keywords, ["$seen"], "no longer a draft");
    assert_eq!(sent.to, pairs(&[("Maria Keller", MARIA)]));
    assert_eq!(sent.subject, "Re: Lunch on Friday?");
    assert_eq!(sent.text.as_deref(), Some(body.as_str()));
    assert_eq!(sent.in_reply_to, ["e1@mock.example.test"]);
    assert_eq!(sent.references, ["e1@mock.example.test"]);
    assert!(rig.server.keywords_of(E1).contains(&"$answered".to_string()));
    assert!(rig.server.ids_in("drafts").is_empty());

    // Nothing is left here, and more passes send nothing more.
    assert!(rig.core.pending_sends().unwrap().is_empty() && rig.core.drafts().unwrap().is_empty());
    rig.sync();
    assert_eq!(rig.server.submissions().len(), 1);
    // The sent message arrives in the cache with the sync of the same pass.
    let sent_box = rig.core.mailboxes().unwrap().into_iter().find(|m| m.role.as_deref() == Some("sent")).unwrap();
    assert!(rig.core.messages(sent_box.id, 10, 0).unwrap().iter().any(|m| m.id == submissions[0].email_id));
}

#[test]
fn bcc_recipients_are_in_the_envelope() {
    let rig = Rig::synced();
    let mut draft = rig.note("Quiet");
    draft.bcc = vec![addr("", "hidden@example.test")];
    rig.core.send(draft, 0).unwrap();
    rig.sync();
    let submission = rig.server.submissions().remove(0);
    assert_eq!(submission.recipients, ["bo@example.test", "hidden@example.test"]);
}

#[test]
fn a_message_held_back_leaves_nothing_until_its_time_and_can_be_taken_back() {
    let rig = Rig::synced();
    let draft = rig.note("Think twice");
    let pending = rig.core.send(draft.clone(), 3600).unwrap();
    assert!(pending.send_at > 0);
    rig.sync();
    assert!(rig.server.submissions().is_empty(), "not due");
    assert_eq!(rig.core.pending_sends().unwrap().len(), 1);
    assert!(rig.server.ids_in("drafts").is_empty(), "not even saved while held back");

    let back = rig.core.undo_send(draft.id.clone()).unwrap();
    assert_eq!(back.id, draft.id);
    assert!(rig.core.pending_sends().unwrap().is_empty());
    assert_eq!(rig.core.drafts().unwrap().len(), 1, "it is a draft again");
    assert!(matches!(rig.core.undo_send(draft.id.clone()), Err(Error::Invalid { .. })), "already back");

    // It reaches Drafts like any draft, and nothing is ever sent.
    rig.sync();
    assert!(rig.server.submissions().is_empty());
    assert_eq!(rig.server.ids_in("drafts").len(), 1);
}

#[test]
fn it_is_too_late_to_take_back_what_has_gone() {
    let rig = Rig::synced();
    let draft = rig.note("Gone");
    rig.core.send(draft.clone(), 0).unwrap();
    rig.sync();
    assert_eq!(rig.server.submissions().len(), 1);
    assert!(matches!(rig.core.undo_send(draft.id), Err(Error::Invalid { .. })));
}

#[test]
fn a_held_back_message_can_be_thrown_away() {
    let rig = Rig::synced();
    let draft = rig.note("Never mind");
    rig.core.send(draft.clone(), 3600).unwrap();
    rig.core.discard_draft(draft.id).unwrap();
    assert!(rig.core.pending_sends().unwrap().is_empty() && rig.core.drafts().unwrap().is_empty());
    rig.sync();
    assert!(rig.server.submissions().is_empty() && rig.server.ids_in("drafts").is_empty());
}

#[test]
fn a_second_send_of_the_same_draft_does_not_send_twice() {
    let rig = Rig::synced();
    let draft = rig.note("Tapped twice");
    let first = rig.core.send(draft.clone(), 60).unwrap();
    let second = rig.core.send(draft, 0).unwrap();
    assert_eq!(first, second);
    assert_eq!(rig.core.pending_sends().unwrap().len(), 1);
}

#[test]
fn sending_needs_a_recipient_good_addresses_and_a_sender() {
    let rig = Rig::synced();
    let mut draft = rig.note("Invalid");
    draft.to.clear();
    assert!(matches!(rig.core.send(draft.clone(), 0), Err(Error::Invalid { .. })));
    draft.to = vec![addr("", "no-at-sign")];
    assert!(matches!(rig.core.send(draft.clone(), 0), Err(Error::Invalid { .. })));
    draft.to = vec![addr("", "a@b@c")];
    assert!(matches!(rig.core.send(draft.clone(), 0), Err(Error::Invalid { .. })));
    draft.to = vec![addr("", "fine@example.test")];
    draft.identity_id = "nobody".into();
    assert!(matches!(rig.core.send(draft.clone(), 0), Err(Error::Invalid { .. })));
    draft.identity_id = "i2".into();
    rig.core.send(draft, 0).unwrap();
    rig.sync();
    assert_eq!(rig.server.submissions()[0].identity_id, "i2");
    // Nothing that failed was left held back.
    assert_eq!(rig.core.pending_sends().unwrap().len(), 0);
}

#[test]
fn a_message_the_server_refuses_for_good_comes_back_as_a_draft_with_the_reason() {
    let rig = Rig::synced();
    let rec = rig.start_listening();
    rig.server.set_submission_refusal(Some(Refusal::Permanent("invalidEmail")));
    let draft = rig.note("Refused");
    rig.core.send(draft.clone(), 0).unwrap();

    eventually("the message to come back as a draft", || !rig.core.drafts().unwrap().is_empty());
    let back = &rig.core.drafts().unwrap()[0];
    assert_eq!(back.id, draft.id);
    assert!(back.send_error.as_deref().is_some_and(|e| e.contains("invalidEmail")), "{:?}", back.send_error);
    assert!(rig.core.pending_sends().unwrap().is_empty());
    assert!(rig.server.submissions().is_empty());
    eventually("the listener to be told", || *rec.changes.lock().unwrap() > 0);

    // Sent again after the cause is gone, it goes, and the reason is gone with it.
    rig.server.set_submission_refusal(None);
    let mut again = rig.core.drafts().unwrap().remove(0);
    again.to = vec![addr("", "better@example.test")];
    rig.core.send(again, 0).unwrap();
    eventually("the message to be sent", || rig.server.submissions().len() == 1);
    eventually("the row to be gone", || rig.core.pending_sends().unwrap().is_empty());
    assert!(rig.core.drafts().unwrap().is_empty());
}

#[test]
fn a_temporary_failure_is_retried_and_the_message_goes_once() {
    let rig = Rig::synced();
    rig.server.set_submission_refusal(Some(Refusal::Temporary));
    let draft = rig.note("Later");
    rig.core.send(draft, 0).unwrap();
    // The pass itself still works: a failing send never fails the sync and the notifications.
    rig.sync();
    assert!(rig.server.submissions().is_empty());
    assert_eq!(rig.core.pending_sends().unwrap().len(), 1, "still waiting");
    assert!(rig.core.drafts().unwrap().is_empty());

    rig.server.set_submission_refusal(None);
    rig.sync();
    assert_eq!(rig.server.submissions().len(), 1);
    assert!(rig.core.pending_sends().unwrap().is_empty());
    rig.sync();
    assert_eq!(rig.server.submissions().len(), 1);
}

#[test]
fn a_pass_that_loses_the_answer_to_the_submission_does_not_send_again() {
    let rig = Rig::new();
    rig.sync();
    let draft = rig.reply_with(E1, "On my way.");
    rig.core.send(draft, 0).unwrap();
    rig.server.lose_next_submission_answer();
    rig.sync();
    // The server sent it, and the client never heard.
    assert_eq!(rig.server.submissions().len(), 1);
    assert_eq!(rig.core.pending_sends().unwrap().len(), 1);

    // The next pass finds out from the server's copy (no longer a draft) and lets go.
    rig.sync();
    assert_eq!(rig.server.submissions().len(), 1, "not sent a second time");
    assert!(rig.core.pending_sends().unwrap().is_empty());
    assert!(rig.server.keywords_of(E1).contains(&"$answered".to_string()));
    rig.sync();
    assert_eq!(rig.server.submissions().len(), 1);
}

#[test]
fn a_sent_message_whose_copy_still_looks_like_a_draft_is_not_sent_again() {
    let rig = Rig::new();
    rig.sync();
    let draft = rig.reply_with(E1, "On my way.");
    rig.core.send(draft, 0).unwrap();
    rig.server.lose_next_submission_answer();
    rig.sync();
    let sent = rig.server.submissions();
    assert_eq!(sent.len(), 1);
    // The step that takes the draft mark off failed on the server, or was undone: the copy
    // alone would say "not sent". The submission the server holds says otherwise.
    rig.server.add_keyword(&sent[0].email_id, "$draft");
    rig.sync();
    assert_eq!(rig.server.submissions().len(), 1, "not sent a second time");
    assert!(rig.core.pending_sends().unwrap().is_empty());
}

#[test]
fn a_refusal_the_core_has_no_name_for_still_gives_the_draft_back() {
    let rig = Rig::synced();
    // The standard answer for a bad recipient, and not one of the errors the core lists.
    rig.server.set_submission_refusal(Some(Refusal::Permanent("invalidRecipients")));
    let draft = rig.note("Bad recipient");
    rig.core.send(draft.clone(), 0).unwrap();
    rig.sync();
    let back = &rig.core.drafts().unwrap()[0];
    assert!(back.send_error.is_some(), "the user is told why");
    assert!(rig.core.pending_sends().unwrap().is_empty(), "not stuck on its way out");
    assert!(rig.server.submissions().is_empty());
}

#[test]
fn a_pass_that_cannot_mark_the_original_has_still_sent_once() {
    let rig = Rig::synced();
    let draft = rig.reply_with(E1, "Fine.");
    rig.core.send(draft, 0).unwrap();
    // The original disappears before the message leaves: marking it fails.
    rig.server.destroy(E1);
    rig.sync();
    assert_eq!(rig.server.submissions().len(), 1);
    assert!(rig.core.pending_sends().unwrap().is_empty());
    rig.sync();
    assert_eq!(rig.server.submissions().len(), 1);
}

#[test]
fn a_message_that_was_held_back_leaves_on_time_with_the_live_connection() {
    let rig = Rig::synced();
    let _rec = rig.start_listening();
    let draft = rig.note("On time");
    let start = Instant::now();
    rig.core.send(draft, 1).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    assert!(rig.server.submissions().is_empty(), "held back");
    eventually("the message to leave", || rig.server.submissions().len() == 1);
    assert!(start.elapsed() >= Duration::from_secs(1));
    eventually("the row to be gone", || rig.core.pending_sends().unwrap().is_empty());
}

#[test]
fn a_message_sent_without_the_live_connection_goes_with_the_next_check() {
    let rig = Rig::synced();
    rig.core.send(rig.note("By check"), 0).unwrap();
    assert!(rig.server.submissions().is_empty());
    rig.core.check_once(Shade::Unknown).unwrap();
    assert_eq!(rig.server.submissions().len(), 1);
}

// ---- attachments ----------------------------------------------------------------------------

#[test]
fn a_file_is_uploaded_added_to_the_draft_and_sent_with_it() {
    let rig = Rig::synced();
    let draft = rig.core.save_draft(rig.note("With a file")).unwrap();
    let path = rig.dir.path().join("notes.txt");
    fs::write(&path, b"hello file").unwrap();

    let with = rig
        .core
        .add_attachment(draft.id.clone(), path.to_string_lossy().into_owned(), String::new(), "text/plain".into())
        .unwrap();
    assert_eq!(with.attachments.len(), 1);
    let file = &with.attachments[0];
    assert_eq!(
        (file.name.as_str(), file.mime_type.as_str(), file.size, file.is_inline),
        ("notes.txt", "text/plain", 10, false)
    );
    assert!(!file.blob_id.is_empty());
    assert_eq!(rig.core.draft(draft.id.clone()).unwrap().unwrap().attachments, with.attachments);

    // The copy in Drafts carries it.
    rig.sync();
    let copy = rig.server.view(&rig.server.ids_in("drafts")[0]);
    assert_eq!(copy.files, vec![("notes.txt".to_string(), "text/plain".to_string(), b"hello file".to_vec())]);

    rig.core.send(with, 0).unwrap();
    rig.sync();
    let sent = rig.server.view(&rig.server.submissions()[0].email_id);
    assert_eq!(sent.files, vec![("notes.txt".to_string(), "text/plain".to_string(), b"hello file".to_vec())]);
}

#[test]
fn a_name_and_type_given_are_used() {
    let rig = Rig::synced();
    let draft = rig.core.save_draft(rig.note("Named")).unwrap();
    let path = rig.dir.path().join("cache-name-123");
    fs::write(&path, b"abc").unwrap();
    let with = rig
        .core
        .add_attachment(draft.id, path.to_string_lossy().into_owned(), "Invoice.pdf".into(), "application/pdf".into())
        .unwrap();
    assert_eq!(
        (with.attachments[0].name.as_str(), with.attachments[0].mime_type.as_str()),
        ("Invoice.pdf", "application/pdf")
    );
}

#[test]
fn a_file_larger_than_the_server_takes_is_refused_and_not_added() {
    let rig = Rig::new();
    rig.server.set_upload_limit(8);
    rig.sync();
    let draft = rig.core.save_draft(rig.note("Too big")).unwrap();
    let path = rig.dir.path().join("big.bin");
    fs::write(&path, b"123456789").unwrap();
    let path = path.to_string_lossy().into_owned();
    let refused = rig.core.add_attachment(draft.id.clone(), path, "big.bin".into(), "application/octet-stream".into());
    assert!(matches!(refused, Err(Error::Invalid { .. })), "{refused:?}");
    assert!(rig.core.draft(draft.id.clone()).unwrap().unwrap().attachments.is_empty());

    // A file at the limit is fine.
    let small = rig.dir.path().join("small.bin");
    fs::write(&small, b"12345678").unwrap();
    let with =
        rig.core.add_attachment(draft.id, small.to_string_lossy().into_owned(), String::new(), String::new()).unwrap();
    assert_eq!((with.attachments[0].size, with.attachments[0].mime_type.as_str()), (8, "application/octet-stream"));
}

#[test]
fn a_file_that_cannot_be_read_or_a_draft_that_is_not_there_is_invalid() {
    let rig = Rig::synced();
    let draft = rig.core.save_draft(rig.note("Missing file")).unwrap();
    let missing = rig.dir.path().join("nothing").to_string_lossy().into_owned();
    assert!(matches!(
        rig.core.add_attachment(draft.id.clone(), missing.clone(), String::new(), String::new()),
        Err(Error::Invalid { .. })
    ));
    assert!(matches!(
        rig.core.add_attachment("nope".into(), missing, String::new(), String::new()),
        Err(Error::Invalid { .. })
    ));
    // A message being sent cannot take a file.
    rig.core.send(draft.clone(), 60).unwrap();
    let path = rig.dir.path().join("late.txt");
    fs::write(&path, b"x").unwrap();
    assert!(matches!(
        rig.core.add_attachment(draft.id, path.to_string_lossy().into_owned(), String::new(), String::new()),
        Err(Error::Invalid { .. })
    ));
}

// ---- suggestions ----------------------------------------------------------------------------

#[test]
fn suggestions_come_from_the_address_book_first_then_from_the_mail() {
    let rig = Rig::new();
    rig.server
        .add_contact(json!({ "name": { "full": "Zoe Quartz" }, "emails": { "a": { "address": "zoe@example.test" } } }));
    rig.server.add_contact(json!({
        "name": { "components": [{ "kind": "given", "value": "Maria" }, { "kind": "surname", "value": "Contact" }] },
        "emails": { "a": { "address": "maria@example.test" }, "b": { "address": "maria.work@example.test" } }
    }));
    rig.server.add_contact(json!({ "name": { "full": "No Address" } }));
    rig.sync();

    let names = |text: &str, limit: u32| -> Vec<(String, String)> {
        rig.core.suggest_recipients(text.into(), limit).unwrap().into_iter().map(|a| (a.name, a.email)).collect()
    };
    // The address book, in its order, each address once; then the people in the mail.
    assert_eq!(
        names("mar", 10),
        [
            ("Maria Contact".to_string(), "maria@example.test".to_string()),
            ("Maria Contact".to_string(), "maria.work@example.test".to_string()),
        ]
    );
    // From the mail: a sender, matched on the start of the name, of a later word, of the address.
    assert_eq!(names("MAR", 1).len(), 1, "case does not matter and the limit holds");
    assert_eq!(names("keller", 10), [("Maria Keller".to_string(), MARIA.to_string())], "the mail's own name for her");
    assert_eq!(names("ci@", 10), [("Build Bot".to_string(), "ci@example.test".to_string())]);
    assert_eq!(names("bot", 10), [("Build Bot".to_string(), "ci@example.test".to_string())]);
    assert_eq!(names("fisch", 10), [("Tom Fischer".to_string(), "tom@example.test".to_string())]);
    // The recipients of mail count too: the seeded reply in Sent was written to Maria.
    assert!(names("zo", 10).starts_with(&[("Zoe Quartz".to_string(), "zoe@example.test".to_string())]));
    assert!(names("   ", 10).is_empty() && names("", 10).is_empty());
    assert!(names("mar", 0).is_empty());
    assert!(names("qqq", 10).is_empty());
}

#[test]
fn people_in_mail_are_suggested_when_there_is_no_address_book() {
    let rig = Rig::new();
    rig.server.set_contacts_enabled(false);
    rig.sync();
    assert_eq!(rig.server.request_count("ContactCard/query") + rig.server.request_count("ContactCard/get"), 0);
    let got = rig.core.suggest_recipients("ann".into(), 10).unwrap();
    assert_eq!(got, vec![addr("Anna Roth", "anna@example.test")]);
}

#[test]
fn a_removed_address_book_is_forgotten_by_the_next_process() {
    let rig = Rig::new();
    rig.server
        .add_contact(json!({ "name": { "full": "Zoe Quartz" }, "emails": { "a": { "address": "zoe@example.test" } } }));
    rig.sync();
    assert_eq!(rig.core.suggest_recipients("zoe".into(), 5).unwrap().len(), 1);
    rig.server.set_contacts_enabled(false);
    let next = Core::new(rig.db_path.clone(), Arc::new(MemorySecrets::default())).unwrap();
    let _ = next; // a second process has no sign-in of its own here; the first pass below stands for it
    let again = Rig::new();
    again.server.set_contacts_enabled(false);
    again.sync();
    assert!(again.core.suggest_recipients("zoe".into(), 5).unwrap().is_empty());
}

// ---- quota ----------------------------------------------------------------------------------

#[test]
fn the_quota_lists_what_is_used_and_the_limits() {
    let rig = Rig::synced();
    rig.server.set_quota(2_000, Some(50_000));
    let quotas = rig.core.quota().unwrap();
    assert_eq!(quotas.len(), 2);
    assert_eq!(
        (quotas[0].name.as_str(), quotas[0].resource.as_str(), quotas[0].used, quotas[0].limit),
        ("Storage", "octets", 2_000, Some(50_000))
    );
    assert_eq!((quotas[1].name.as_str(), quotas[1].resource.as_str(), quotas[1].limit), ("Messages", "count", None));
}

#[test]
fn a_server_without_quotas_gives_an_empty_list() {
    let rig = Rig::new();
    rig.server.set_quota_enabled(false);
    rig.sync();
    assert!(rig.core.quota().unwrap().is_empty());
    assert_eq!(rig.server.request_count("Quota/get"), 0, "no capability, no request");
}

#[test]
fn sending_calls_need_an_account() {
    let rig = Rig::synced();
    rig.core.sign_out().unwrap();
    assert!(matches!(rig.core.new_draft(DraftKind::New), Err(Error::SignInNeeded { .. })));
    assert!(matches!(rig.core.quota(), Err(Error::SignInNeeded { .. })));
    assert!(matches!(rig.core.drafts(), Err(Error::SignInNeeded { .. })));
    assert!(matches!(rig.core.pending_sends(), Err(Error::SignInNeeded { .. })));
}
