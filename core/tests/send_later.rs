//! M10 sending against the mock server: messages the server holds back (a send with a real undo,
//! a send for a later time), taking them back, and the sender identities with their signatures.
//!
//! The mock is async (axum), `Core` blocks and owns its own runtime, so every test is a plain
//! `#[test]` that keeps a separate runtime alive just for the mock. The rule under test is the
//! one of the project: a message is never sent twice and never lost, whatever fails between the
//! steps.

mod support;

use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use kweb_core::{
    compose::rules::iso_utc,
    ffi::{Core, Shade},
    model::{Address, Draft, DraftKind},
    secrets::MemorySecrets,
    store::Store,
    Error,
};
use support::{MockServer, Refusal, USERNAME};
use tokio::runtime::Runtime;

const E1: &str = "e1";

struct Rig {
    // Field order is drop order: the core goes first, the mock and its runtime last.
    core: Arc<Core>,
    server: MockServer,
    rt: Runtime,
    db_path: String,
    secrets: Arc<MemorySecrets>,
    _dir: tempfile::TempDir,
}

impl Rig {
    fn new() -> Rig {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        let server = rt.block_on(MockServer::start());
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("cache.db").to_string_lossy().into_owned();
        let secrets = Arc::new(MemorySecrets::default());
        let core = Core::new(db_path.clone(), secrets.clone()).unwrap();
        let url = core.begin_sign_in_with_redirect(server.url(), "ovh.kweb.mail:/oauth/callback".into()).unwrap();
        core.complete_sign_in(server.authorize(&url).unwrap()).unwrap();
        Rig { core, server, rt, db_path, secrets, _dir: dir }
    }

    /// A rig that has done its first pass.
    fn synced() -> Rig {
        let rig = Rig::new();
        rig.sync();
        rig
    }

    fn sync(&self) {
        self.core.check_once(Shade::Unknown).expect("a pass");
    }

    /// A message to Bo with a body, ready to send.
    fn note(&self, subject: &str) -> Draft {
        let mut draft = self.core.new_draft(DraftKind::New).unwrap();
        draft.to = vec![addr("Bo", "bo@example.test")];
        draft.subject = subject.into();
        draft.body = "Hello Bo".into();
        draft
    }

    fn store(&self) -> Store {
        Store::open(std::path::Path::new(&self.db_path)).unwrap()
    }

    /// A raw JMAP request, for what `Core` never asks.
    fn post(&self, token: &str, body: &serde_json::Value) -> String {
        let url = format!("{}/jmap", self.server.url());
        self.rt.block_on(async {
            let request = reqwest::Client::new()
                .post(&url)
                .bearer_auth(token)
                .header("content-type", "application/json")
                .body(body.to_string());
            request.send().await.unwrap().text().await.unwrap()
        })
    }

    /// One method call as the server answers it.
    fn call(&self, method: &str, args: serde_json::Value) -> serde_json::Value {
        let token = self.server.issue_access_token();
        let body = serde_json::json!({
            "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:mail", "urn:ietf:params:jmap:submission"],
            "methodCalls": [[method, args, "0"]],
        });
        let answer: serde_json::Value = serde_json::from_str(&self.post(&token, &body)).unwrap();
        answer["methodResponses"][0][1].clone()
    }

    fn account_id(&self) -> String {
        self.core.account().unwrap().id
    }
}

fn addr(name: &str, email: &str) -> Address {
    Address { name: name.into(), email: email.into() }
}

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64
}

fn is_invalid<T>(r: &Result<T, Error>) -> bool {
    matches!(r, Err(Error::Invalid { .. }))
}

fn message(e: &Error) -> String {
    e.to_string()
}

// ---- schedule_send ----------------------------------------------------------------------------

#[test]
fn a_scheduled_message_is_held_by_the_server_and_the_local_row_is_gone() {
    let rig = Rig::synced();
    let at = now() + 3600;
    let held = rig.core.schedule_send(rig.note("Next hour"), at).unwrap();
    assert_eq!(
        (held.subject.as_str(), held.to.clone(), held.send_at),
        ("Next hour", vec!["bo@example.test".to_string()], at)
    );

    // The server holds it with an absolute time, as the exact copy that was saved.
    let on_server = rig.server.held();
    assert_eq!(on_server.len(), 1);
    assert_eq!(on_server[0].id, held.id);
    assert_eq!(on_server[0].hold, Some(("HOLDUNTIL".to_string(), iso_utc(at))));
    assert_eq!(on_server[0].recipients, ["bo@example.test"]);
    assert_eq!(on_server[0].identity_id, "i1");
    let view = rig.server.view(&on_server[0].email_id);
    assert_eq!(
        (view.subject.as_str(), view.roles.clone(), view.keywords.clone()),
        ("Next hour", vec!["sent".to_string()], vec!["$seen".to_string()])
    );
    assert!(view.text.as_deref().unwrap().starts_with("Hello Bo"));
    assert!(rig.server.submissions().is_empty(), "nothing has left");

    // Here nothing is left of it: not a draft, not waiting.
    assert!(rig.core.drafts().unwrap().is_empty() && rig.core.pending_sends().unwrap().is_empty());
    rig.sync();
    assert_eq!(rig.server.held().len(), 1, "a pass neither sends it nor sends it again");
    assert!(rig.server.submissions().is_empty());

    // The server's list shows it, and when its time comes it leaves once.
    assert_eq!(rig.core.scheduled_sends().unwrap(), vec![held]);
    rig.server.release_held();
    rig.sync();
    assert_eq!(rig.server.submissions().len(), 1);
    assert!(rig.core.scheduled_sends().unwrap().is_empty());
}

#[test]
fn a_time_less_than_a_minute_or_more_than_seven_days_away_is_refused_before_anything_is_saved() {
    let rig = Rig::synced();
    let draft = rig.note("Out of range");
    let sent_before = rig.server.ids_in("sent");
    for bad in [now() - 10, now() + 30, now() + 7 * 86_400 + 120] {
        assert!(is_invalid(&rig.core.schedule_send(draft.clone(), bad)), "{bad}");
    }
    // Still an ordinary draft, and the server has seen nothing.
    assert_eq!(rig.core.drafts().unwrap().len(), 1);
    assert!(rig.server.held().is_empty() && rig.server.ids_in("sent") == sent_before);
    // The edges are fine.
    rig.core.schedule_send(draft, now() + 7 * 86_400 - 60).unwrap();
}

#[test]
fn the_same_checks_as_a_send_apply_to_a_scheduled_message() {
    let rig = Rig::synced();
    let mut draft = rig.note("Checks");
    draft.to.clear();
    assert!(is_invalid(&rig.core.schedule_send(draft.clone(), now() + 3600)));
    draft.to = vec![addr("", "no-at-sign")];
    assert!(is_invalid(&rig.core.schedule_send(draft.clone(), now() + 3600)));
    draft.to = vec![addr("", "fine@example.test")];
    draft.identity_id = "nobody".into();
    assert!(is_invalid(&rig.core.schedule_send(draft, now() + 3600)));
    assert!(rig.server.held().is_empty());
}

#[test]
fn without_a_connection_scheduling_is_a_temporary_error_and_the_draft_stays_a_draft() {
    let rig = Rig::synced();
    let draft = rig.note("Offline");
    rig.server.set_down(true);
    let result = rig.core.schedule_send(draft.clone(), now() + 3600);
    assert!(matches!(result, Err(Error::Temporary { .. })), "{result:?}");
    rig.server.set_down(false);
    assert_eq!(rig.core.drafts().unwrap().len(), 1, "still a draft");
    assert!(rig.core.pending_sends().unwrap().is_empty());
    assert!(rig.server.held().is_empty());
    // Back online it works, once.
    rig.core.schedule_send(rig.core.drafts().unwrap().remove(0), now() + 3600).unwrap();
    assert_eq!(rig.server.held().len(), 1);
}

#[test]
fn a_refusal_of_the_server_gives_the_draft_back_with_the_reason() {
    let rig = Rig::synced();
    rig.server.set_submission_refusal(Some(Refusal::Permanent("invalidEmail")));
    let result = rig.core.schedule_send(rig.note("Refused"), now() + 3600);
    assert!(is_invalid(&result), "{result:?}");
    let back = rig.core.drafts().unwrap();
    assert_eq!(back.len(), 1);
    assert!(back[0].send_error.as_deref().is_some_and(|e| e.contains("invalidEmail")));
    assert!(rig.server.held().is_empty() && rig.server.submissions().is_empty());
    // Once the cause is gone it can be scheduled, and goes once.
    rig.server.set_submission_refusal(None);
    rig.core.schedule_send(back[0].clone(), now() + 3600).unwrap();
    assert_eq!(rig.server.held().len(), 1);
}

#[test]
fn a_failed_submission_leaves_a_draft_and_nothing_held() {
    let rig = Rig::synced();
    rig.server.set_submission_refusal(Some(Refusal::Temporary));
    let result = rig.core.schedule_send(rig.note("Fails"), now() + 3600);
    assert!(matches!(result, Err(Error::Temporary { .. })), "{result:?}");
    assert_eq!(rig.core.drafts().unwrap().len(), 1, "a draft again, not lost");
    assert!(rig.server.held().is_empty());
    rig.server.set_submission_refusal(None);
    rig.core.schedule_send(rig.core.drafts().unwrap().remove(0), now() + 3600).unwrap();
    assert_eq!(rig.server.held().len(), 1, "exactly one");
    rig.sync();
    assert_eq!(rig.server.held().len(), 1);
    assert!(rig.core.drafts().unwrap().is_empty());
}

#[test]
fn a_lost_answer_is_settled_at_once_by_the_pending_submission_the_server_holds() {
    let rig = Rig::synced();
    rig.server.lose_next_submission_answer();
    let at = now() + 3600;
    let held = rig.core.schedule_send(rig.note("Lost answer"), at).unwrap();
    assert_eq!(rig.server.held().len(), 1, "the server holds it, once");
    assert_eq!(held.id, rig.server.held()[0].id);
    assert!(rig.core.drafts().unwrap().is_empty() && rig.core.pending_sends().unwrap().is_empty());
    rig.sync();
    assert_eq!(rig.server.held().len(), 1);
}

#[test]
fn a_lost_answer_that_cannot_be_checked_is_settled_by_the_next_pass_as_held() {
    let rig = Rig::synced();
    rig.server.lose_next_submission_answer();
    rig.server.fail_next_submission_queries(1);
    let result = rig.core.schedule_send(rig.note("Lost twice"), now() + 3600);
    assert!(matches!(result, Err(Error::Temporary { .. })), "{result:?}");
    assert_eq!(rig.server.held().len(), 1);
    // Not shown as a draft (it may be held), not offered for sending again.
    assert!(rig.core.drafts().unwrap().is_empty());
    rig.sync();
    assert_eq!(rig.server.held().len(), 1, "never twice");
    assert!(rig.core.drafts().unwrap().is_empty(), "the server holds it: the row is gone");
    assert_eq!(rig.core.scheduled_sends().unwrap().len(), 1);
}

#[test]
fn a_scheduling_the_server_never_got_is_a_draft_again_and_is_never_sent() {
    let rig = Rig::synced();
    // The request fails and so does the question afterwards.
    rig.server.set_submission_refusal(Some(Refusal::Temporary));
    rig.server.fail_next_submission_queries(1);
    let result = rig.core.schedule_send(rig.note("Never got there"), now() + 3600);
    assert!(matches!(result, Err(Error::Temporary { .. })), "{result:?}");
    assert!(rig.core.drafts().unwrap().is_empty(), "undecided until the server is asked");
    rig.server.set_submission_refusal(None);
    // The pass asks: nothing is held and the copy is a draft, so it is a draft again, and it is
    // not sent now (the user chose a later time, or none).
    rig.sync();
    assert!(rig.server.held().is_empty() && rig.server.submissions().is_empty());
    assert_eq!(rig.core.drafts().unwrap().len(), 1, "never none");
    rig.sync();
    assert!(rig.server.held().is_empty() && rig.server.submissions().is_empty());
}

#[test]
fn a_call_that_died_between_the_steps_leaves_a_draft_or_a_held_message_never_both_or_neither() {
    let rig = Rig::synced();
    let account = rig.account_id();
    let store = rig.store();
    // Died before the copy: the row says Scheduling.
    let first = rig.core.save_draft(rig.note("Died early")).unwrap();
    store.draft_schedule(&account, &first, now() + 3600, now()).unwrap();
    assert!(rig.core.drafts().unwrap().is_empty());
    rig.sync();
    assert_eq!(rig.core.drafts().unwrap().len(), 1, "a draft again");
    assert!(rig.server.held().is_empty() && rig.server.submissions().is_empty());

    // Died after the copy was saved and the submission started, before anything was asked.
    let second = rig.core.drafts().unwrap().remove(0);
    rig.sync();
    let row = store.draft_get(&account, &second.id).unwrap().unwrap();
    assert!(!row.dirty && row.draft.server_id.is_some(), "the copy is on the server");
    store.draft_schedule(&account, &second, now() + 3600, now()).unwrap();
    let row = store.draft_get(&account, &second.id).unwrap().unwrap();
    let copy = row.draft.server_id.clone().unwrap();
    store.draft_saved(&account, &second.id, &copy, row.rev).unwrap();
    assert!(store.draft_begin_schedule(&account, &second.id, row.rev, &copy).unwrap());
    rig.sync();
    assert_eq!(rig.core.drafts().unwrap().len(), 1, "the server holds nothing: a draft again");
    assert!(rig.server.held().is_empty() && rig.server.submissions().is_empty());
}

// ---- cancel_scheduled -------------------------------------------------------------------------

#[test]
fn a_held_message_can_be_taken_back_and_is_a_draft_again_in_drafts() {
    let rig = Rig::synced();
    let held = rig.core.schedule_send(rig.note("Changed my mind"), now() + 3600).unwrap();
    let copy = rig.server.held()[0].email_id.clone();

    let draft = rig.core.cancel_scheduled(held.id.clone()).unwrap();
    assert_eq!((draft.subject.as_str(), draft.server_id.as_deref()), ("Changed my mind", Some(copy.as_str())));
    assert_eq!(draft.to, vec![addr("Bo", "bo@example.test")]);
    // On the server: cancelled, never leaves, and the message is a draft in Drafts, not in Sent.
    assert!(rig.server.held().is_empty());
    assert_eq!(rig.server.cancelled().len(), 1);
    let view = rig.server.view(&copy);
    assert_eq!(view.roles, ["drafts"]);
    assert!(view.keywords.contains(&"$draft".to_string()));
    assert!(!rig.server.ids_in("sent").contains(&copy));
    rig.server.release_held();
    rig.sync();
    assert!(rig.server.submissions().is_empty(), "nothing arrives");
    // Here it is one local draft that knows its copy, and a pass does not make a second copy.
    assert_eq!(rig.core.drafts().unwrap(), vec![draft.clone()]);
    assert_eq!(rig.server.ids_in("drafts"), vec![copy]);

    // Scheduled again, it goes once.
    rig.core.schedule_send(draft, now() + 3600).unwrap();
    assert_eq!(rig.server.held().len(), 1);
    rig.server.release_held();
    rig.sync();
    assert_eq!(rig.server.submissions().len(), 1);
    assert_eq!(rig.server.cancelled().len(), 1);
}

#[test]
fn it_is_too_late_to_take_back_what_has_left_and_an_unknown_one_is_invalid() {
    let rig = Rig::synced();
    let held = rig.core.schedule_send(rig.note("Gone"), now() + 3600).unwrap();
    rig.server.release_held();
    let result = rig.core.cancel_scheduled(held.id.clone());
    assert!(is_invalid(&result), "{result:?}");
    assert!(rig.core.drafts().unwrap().is_empty(), "no draft was made");
    assert!(rig.server.cancelled().is_empty());
    assert_eq!(rig.server.submissions().len(), 1);
    assert!(is_invalid(&rig.core.cancel_scheduled("nope".into())));
}

#[test]
fn a_cancel_that_worked_but_could_not_put_the_message_back_says_where_it_is_and_can_be_finished() {
    let rig = Rig::synced();
    let held = rig.core.schedule_send(rig.note("Half done"), now() + 3600).unwrap();
    let copy = rig.server.held()[0].email_id.clone();
    // The cancel is one request and the move another: the second fails.
    rig.server.set_email_set_budget(Some(0));
    let failure = rig.core.cancel_scheduled(held.id.clone()).unwrap_err();
    let text = message(&failure);
    assert!(matches!(failure, Error::Temporary { .. }), "{failure:?}");
    assert!(text.contains("cancelled") && text.contains("Sent Items") && text.contains("Drafts"), "{text}");
    assert_eq!(rig.server.cancelled().len(), 1, "it will not be sent");
    assert_eq!(rig.server.view(&copy).roles, ["sent"], "and it is not lost: it is in Sent");
    assert!(rig.server.held().is_empty());

    // Trying again finishes the move.
    rig.server.set_email_set_budget(None);
    let draft = rig.core.cancel_scheduled(held.id).unwrap();
    assert_eq!(draft.server_id.as_deref(), Some(copy.as_str()));
    assert_eq!(rig.server.view(&copy).roles, ["drafts"]);
    assert_eq!(rig.server.cancelled().len(), 1);
}

#[test]
fn a_cancelled_message_that_was_not_put_back_is_put_back_by_the_next_pass_without_the_caller() {
    let rig = Rig::synced();
    let held = rig.core.schedule_send(rig.note("Left in Sent"), now() + 3600).unwrap();
    let copy = rig.server.held()[0].email_id.clone();
    rig.server.set_email_set_budget(Some(0));
    assert!(rig.core.cancel_scheduled(held.id).is_err());
    assert_eq!(rig.server.view(&copy).roles, ["sent"]);
    // The caller does not come back; the list has no id left either.
    rig.server.set_email_set_budget(None);
    assert!(rig.core.scheduled_sends().unwrap().is_empty());
    rig.sync();
    assert_eq!(rig.server.view(&copy).roles, ["drafts"]);
    assert!(rig.server.view(&copy).keywords.contains(&"$draft".to_string()));
    assert!(rig.server.submissions().is_empty());
}

#[test]
fn a_message_scheduled_again_is_not_moved_by_the_cancel_of_an_older_try() {
    let rig = Rig::synced();
    let first = rig.core.schedule_send(rig.note("Twice"), now() + 3600).unwrap();
    let draft = rig.core.cancel_scheduled(first.id.clone()).unwrap();
    rig.core.schedule_send(draft, now() + 7200).unwrap();
    // The old, cancelled submission is no way to take the new one back.
    let result = rig.core.cancel_scheduled(first.id);
    assert!(is_invalid(&result), "{result:?}");
    assert_eq!(rig.server.held().len(), 1);
}

#[test]
fn the_list_is_sorted_by_time_here_and_leaves_out_the_undo_window_of_a_send() {
    let rig = Rig::synced();
    let t = now();
    let later = rig.core.schedule_send(rig.note("Later"), t + 7200).unwrap();
    let sooner = rig.core.schedule_send(rig.note("Sooner"), t + 600).unwrap();
    let middle = rig.core.schedule_send(rig.note("Middle"), t + 3600).unwrap();
    // A send in its undo window is held by the server too, but it is not a scheduled message.
    rig.core.send(rig.note("In the window"), 3600).unwrap();
    assert_eq!(rig.server.held().len(), 4);
    assert_eq!(rig.core.pending_sends().unwrap().len(), 1);
    assert_eq!(rig.core.scheduled_sends().unwrap(), vec![sooner, middle, later]);
}

#[test]
fn destroying_a_submission_does_not_stop_a_held_message_and_the_core_never_does_it() {
    // The mock keeps to what the server does: a destroyed submission is gone from the lists and
    // the message leaves all the same. This is why the core cancels instead.
    let rig = Rig::synced();
    let held = rig.core.schedule_send(rig.note("Destroyed"), now() + 3600).unwrap();
    let token = rig.server.issue_access_token();
    let body = serde_json::json!({
        "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:mail", "urn:ietf:params:jmap:submission"],
        "methodCalls": [["EmailSubmission/set", { "accountId": rig.server.account_id(), "destroy": [held.id] }, "0"]],
    });
    let answer = rig.post(&token, &body);
    assert!(answer.contains("destroyed"), "{answer}");
    assert!(rig.server.held().len() == 1, "still going to be sent");
    assert!(rig.core.scheduled_sends().unwrap().is_empty(), "though no list shows it");
    rig.server.release_held();
    assert_eq!(rig.server.submissions().len(), 1);
}

// ---- Core::send with a real undo -----------------------------------------------------------------

#[test]
fn a_send_with_a_delay_is_handed_to_the_server_at_once_and_the_undo_cancels_it_there() {
    let rig = Rig::synced();
    let draft = rig.note("Think twice");
    let pending = rig.core.send(draft.clone(), 3600).unwrap();
    // The server holds it for the delay, as a string, and the row waits here for the undo.
    let held = rig.server.held();
    assert_eq!(held.len(), 1);
    assert!(
        matches!(held[0].hold.as_ref(), Some((k, v)) if k == "HOLDFOR" && v.parse::<i64>().is_ok_and(|s| (3590..=3600).contains(&s)))
    );
    assert!(rig.server.submissions().is_empty());
    assert_eq!(rig.core.pending_sends().unwrap(), vec![pending.clone()]);
    assert!(rig.core.drafts().unwrap().is_empty());
    rig.sync();
    assert_eq!(rig.server.held().len(), 1, "a pass does not send it again");
    assert_eq!(rig.core.pending_sends().unwrap().len(), 1);

    let back = rig.core.undo_send(draft.id.clone()).unwrap();
    assert_eq!((back.id.as_str(), back.subject.as_str()), (draft.id.as_str(), "Think twice"));
    assert!(rig.server.held().is_empty());
    assert_eq!(rig.server.cancelled().len(), 1);
    let copy = held[0].email_id.clone();
    assert_eq!(rig.server.view(&copy).roles, ["drafts"]);
    assert!(rig.server.view(&copy).keywords.contains(&"$draft".to_string()));
    assert!(rig.core.pending_sends().unwrap().is_empty());
    assert_eq!(rig.core.drafts().unwrap().len(), 1);
    assert!(is_invalid(&rig.core.undo_send(draft.id.clone())), "already back");
    rig.sync();
    assert_eq!(rig.server.ids_in("drafts"), vec![copy], "no second copy");
    assert!(rig.server.submissions().is_empty());
}

#[test]
fn a_held_send_leaves_even_if_the_device_never_runs_another_pass() {
    let rig = Rig::synced();
    rig.core.send(rig.note("Phone died"), 2).unwrap();
    assert_eq!(rig.server.held().len(), 1);
    // No pass, no live connection: the time comes and the server sends it by itself.
    std::thread::sleep(Duration::from_millis(2300));
    assert_eq!(rig.server.submissions().len(), 1, "it left without the client");

    // A new process finds the row, sees from the server that it has left and lets go of it.
    let again = Core::new(rig.db_path.clone(), rig.secrets.clone()).unwrap();
    assert_eq!(again.pending_sends().unwrap().len(), 0, "past its time: not waiting any more");
    again.check_once(Shade::Unknown).unwrap();
    assert!(again.pending_sends().unwrap().is_empty() && again.drafts().unwrap().is_empty());
    assert_eq!(rig.server.submissions().len(), 1, "not sent a second time");
}

#[test]
fn without_a_connection_the_send_is_held_here_exactly_as_before() {
    let rig = Rig::synced();
    let draft = rig.note("Offline send");
    rig.server.set_down(true);
    let pending = rig.core.send(draft.clone(), 3600).unwrap();
    rig.server.set_down(false);
    assert!(rig.server.held().is_empty() && rig.server.submissions().is_empty());
    assert_eq!(rig.core.pending_sends().unwrap(), vec![pending]);
    rig.sync();
    assert!(rig.server.held().is_empty(), "not due: still only here");
    // The local undo works as it always did, without the network.
    rig.server.set_down(true);
    let back = rig.core.undo_send(draft.id).unwrap();
    rig.server.set_down(false);
    assert_eq!(back.subject, "Offline send");
    assert!(rig.core.pending_sends().unwrap().is_empty());
}

#[test]
fn a_hand_over_whose_answer_is_lost_is_one_held_message_and_can_still_be_undone() {
    let rig = Rig::synced();
    let draft = rig.note("Lost on the way");
    rig.server.lose_next_submission_answer();
    // The send is accepted here; the server holds it and the client never heard.
    rig.core.send(draft.clone(), 3600).unwrap();
    assert_eq!(rig.server.held().len(), 1);
    assert_eq!(rig.core.pending_sends().unwrap().len(), 1);
    // The next pass asks the server and finds the pending submission: one held message.
    rig.sync();
    assert_eq!(rig.server.held().len(), 1, "never twice");
    assert_eq!(rig.core.pending_sends().unwrap().len(), 1);
    assert!(rig.core.drafts().unwrap().is_empty());
    // And the undo still takes it back.
    let back = rig.core.undo_send(draft.id).unwrap();
    assert_eq!(back.subject, "Lost on the way");
    assert!(rig.server.held().is_empty() && rig.server.submissions().is_empty());
}

#[test]
fn an_undo_after_a_lost_answer_does_not_wait_for_a_pass() {
    let rig = Rig::synced();
    let draft = rig.note("Undo at once");
    rig.server.lose_next_submission_answer();
    rig.core.send(draft.clone(), 3600).unwrap();
    // The row is `submitting` here; the undo asks the server and finds the held submission.
    let back = rig.core.undo_send(draft.id).unwrap();
    assert_eq!(back.subject, "Undo at once");
    assert!(rig.server.held().is_empty() && rig.server.submissions().is_empty());
    assert_eq!(rig.server.cancelled().len(), 1);
}

#[test]
fn a_hand_over_that_failed_is_held_here_and_goes_once_held_by_the_server_with_the_next_pass() {
    let rig = Rig::synced();
    rig.server.set_submission_refusal(Some(Refusal::Temporary));
    rig.core.send(rig.note("Retry"), 3600).unwrap();
    assert!(rig.server.held().is_empty() && rig.server.submissions().is_empty());
    assert_eq!(rig.core.pending_sends().unwrap().len(), 1, "it still waits");
    rig.sync();
    assert!(rig.server.held().is_empty(), "still failing");
    rig.server.set_submission_refusal(None);
    rig.sync();
    // Held for what is left of the time, once.
    let held = rig.server.held();
    assert_eq!(held.len(), 1);
    assert!(matches!(held[0].hold.as_ref(), Some((k, _)) if k == "HOLDFOR"));
    rig.sync();
    assert_eq!(rig.server.held().len(), 1);
    rig.server.release_held();
    rig.sync();
    assert_eq!(rig.server.submissions().len(), 1);
}

#[test]
fn a_refusal_at_the_hand_over_is_told_at_once_and_the_draft_is_back_with_the_reason() {
    let rig = Rig::synced();
    rig.server.set_submission_refusal(Some(Refusal::Permanent("invalidEmail")));
    let result = rig.core.send(rig.note("Refused"), 3600);
    match &result {
        Err(Error::Invalid { msg }) => assert!(msg.contains("invalidEmail"), "{msg}"),
        other => panic!("{other:?}"),
    }
    let back = rig.core.drafts().unwrap();
    assert_eq!(back.len(), 1);
    assert!(back[0].send_error.is_some());
    assert!(rig.core.pending_sends().unwrap().is_empty());
    assert!(rig.server.held().is_empty() && rig.server.submissions().is_empty());
}

#[test]
fn it_is_too_late_to_undo_a_message_the_server_has_released_and_the_row_is_let_go() {
    let rig = Rig::synced();
    let draft = rig.note("Released");
    rig.core.send(draft.clone(), 3600).unwrap();
    rig.server.release_held();
    let result = rig.core.undo_send(draft.id);
    assert!(is_invalid(&result), "{result:?}");
    assert!(rig.core.pending_sends().unwrap().is_empty() && rig.core.drafts().unwrap().is_empty());
    assert_eq!(rig.server.submissions().len(), 1);
    rig.sync();
    assert_eq!(rig.server.submissions().len(), 1);
}

#[test]
fn the_undo_needs_the_server_for_a_message_the_server_holds_and_the_message_still_leaves_if_it_cannot_be_reached() {
    let rig = Rig::synced();
    let draft = rig.note("Cannot reach");
    rig.core.send(draft.clone(), 3600).unwrap();
    rig.server.set_down(true);
    let result = rig.core.undo_send(draft.id.clone());
    rig.server.set_down(false);
    assert!(matches!(result, Err(Error::Temporary { .. })), "{result:?}");
    assert_eq!(rig.core.pending_sends().unwrap().len(), 1, "still waiting");
    assert_eq!(rig.server.held().len(), 1);
    // Reachable again, it works.
    assert!(rig.core.undo_send(draft.id).is_ok());
    assert!(rig.server.held().is_empty());
}

#[test]
fn a_held_message_that_was_thrown_away_is_cancelled_on_the_server_and_its_copy_removed() {
    let rig = Rig::synced();
    let draft = rig.note("Never mind");
    rig.core.send(draft.clone(), 3600).unwrap();
    let copy = rig.server.held()[0].email_id.clone();
    rig.core.discard_draft(draft.id).unwrap();
    assert!(rig.core.pending_sends().unwrap().is_empty() && rig.core.drafts().unwrap().is_empty());
    rig.sync();
    assert!(rig.server.held().is_empty(), "cancelled");
    assert_eq!(rig.server.cancelled().len(), 1);
    assert!(!rig.server.exists(&copy));
    rig.server.release_held();
    assert!(rig.server.submissions().is_empty(), "nothing arrives");
}

#[test]
fn a_held_message_cannot_be_thrown_away_without_the_server_and_still_leaves() {
    let rig = Rig::synced();
    let draft = rig.note("Keep waiting");
    rig.core.send(draft.clone(), 3600).unwrap();
    rig.server.set_down(true);
    let result = rig.core.discard_draft(draft.id.clone());
    rig.server.set_down(false);
    assert!(matches!(result, Err(Error::Temporary { .. })), "{result:?}");
    assert_eq!(rig.core.pending_sends().unwrap().len(), 1, "nothing was thrown away");
    assert_eq!(rig.server.held().len(), 1);
    rig.core.discard_draft(draft.id).unwrap();
    assert!(rig.server.held().is_empty());
}

#[test]
fn a_message_that_was_undone_and_whose_row_was_not_updated_is_not_sent_by_the_next_pass() {
    let rig = Rig::synced();
    let account = rig.account_id();
    let store = rig.store();
    let draft = rig.note("Undone");
    rig.core.send(draft.clone(), 3600).unwrap();
    let back = rig.core.undo_send(draft.id.clone()).unwrap();
    // As if the process died after the copy was put back and before the row was updated.
    let row = store.draft_get(&account, &back.id).unwrap().unwrap();
    let copy = row.draft.server_id.clone().unwrap();
    store.draft_send(&account, &back, now() - 1, now()).unwrap();
    let row = store.draft_get(&account, &back.id).unwrap().unwrap();
    store.draft_saved(&account, &back.id, &copy, row.rev).unwrap();
    assert!(store.draft_begin_submit(&account, &back.id, row.rev, &copy).unwrap());
    rig.sync();
    assert!(rig.server.held().is_empty() && rig.server.submissions().is_empty(), "never sent");
    assert_eq!(rig.core.drafts().unwrap().len(), 1, "a draft again");
}

#[test]
fn a_message_thrown_away_after_it_left_stays_in_sent() {
    let rig = Rig::synced();
    let draft = rig.note("Too late to throw away");
    rig.core.send(draft.clone(), 3600).unwrap();
    let copy = rig.server.held()[0].email_id.clone();
    rig.server.release_held();
    // The server released it; the row has not heard: the discard says it is too late.
    let result = rig.core.discard_draft(draft.id);
    assert!(is_invalid(&result), "{result:?}");
    rig.sync();
    assert_eq!(rig.server.submissions().len(), 1);
    assert!(rig.server.exists(&copy), "a sent message is not removed");
    rig.sync();
    assert_eq!(rig.server.submissions().len(), 1);
}

#[test]
fn a_message_another_device_cancelled_comes_back_as_a_draft_when_its_time_has_come() {
    let rig = Rig::synced();
    let draft = rig.note("Cancelled elsewhere");
    rig.core.send(draft.clone(), 2).unwrap();
    let copy = rig.server.held()[0].email_id.clone();
    rig.server.cancel_held();
    std::thread::sleep(Duration::from_millis(2300));
    rig.sync();
    assert!(rig.server.submissions().is_empty());
    assert!(rig.core.pending_sends().unwrap().is_empty());
    let back = rig.core.drafts().unwrap();
    assert_eq!(back.len(), 1, "not lost");
    assert_eq!(back[0].server_id.as_deref(), Some(copy.as_str()));
    assert_eq!(rig.server.view(&copy).roles, ["drafts"]);
}

#[test]
fn a_send_without_a_delay_is_sent_at_once_by_the_pass_as_before() {
    let rig = Rig::synced();
    rig.core.send(rig.note("Now"), 0).unwrap();
    assert!(rig.server.held().is_empty() && rig.server.submissions().is_empty(), "only the pass sends");
    rig.sync();
    assert_eq!(rig.server.submissions().len(), 1);
    assert_eq!(rig.server.submissions()[0].hold, None);
    // A delay beyond what the server holds is kept here.
    rig.core.send(rig.note("Far"), 8 * 86_400).unwrap();
    assert!(rig.server.held().is_empty());
    assert_eq!(rig.core.pending_sends().unwrap().len(), 1);
}

// ---- identities -------------------------------------------------------------------------------------

fn details(rig: &Rig) -> Vec<(String, bool)> {
    rig.core.identity_details().unwrap().into_iter().map(|d| (d.id, d.is_default)).collect()
}

#[test]
fn the_identities_come_from_the_server_with_what_can_be_changed() {
    let rig = Rig::synced();
    rig.server.set_identity_extras("i1", &[("Desk", "desk@example.test"), ("", "b@example.test")], "<b>html</b>");
    rig.server.set_identity_may_delete("i2", false);
    let list = rig.core.identity_details().unwrap();
    assert_eq!(list.len(), 2);
    let one = &list[0];
    assert_eq!((one.id.as_str(), one.name.as_str(), one.email.as_str()), ("i1", "Alice Test", USERNAME));
    assert_eq!(
        (one.reply_to.as_str(), one.signature.as_str()),
        ("desk@example.test, b@example.test", "Alice Test\nTest Corp")
    );
    assert!(one.is_default && one.may_delete);
    assert!(!list[1].is_default && !list[1].may_delete);
}

#[test]
fn the_default_sender_comes_first_everywhere_and_new_drafts_start_with_it() {
    let rig = Rig::new();
    rig.server
        .set_identities(&[("i1", "One", "one@example.test", "One sig"), ("i2", "Two", "two@example.test", "Two\nsig")]);
    rig.sync();
    assert_eq!(rig.core.identities().unwrap()[0].id, "i1");
    assert!(rig.core.set_default_identity("nope".into()).is_err());
    rig.core.set_default_identity("i2".into()).unwrap();
    assert_eq!(rig.core.identities().unwrap().iter().map(|i| i.id.as_str()).collect::<Vec<_>>(), ["i2", "i1"]);
    assert_eq!(details(&rig), [("i2".to_string(), true), ("i1".to_string(), false)]);

    // A new message starts with it and its signature under the text.
    let draft = rig.core.new_draft(DraftKind::New).unwrap();
    assert_eq!(draft.identity_id, "i2");
    assert_eq!(draft.body, "\n\n-- \nTwo\nsig");

    // It is kept on this device: a new process knows it.
    let again = Core::new(rig.db_path.clone(), Arc::new(MemorySecrets::default())).unwrap();
    assert_eq!(again.identities().unwrap()[0].id, "i2");
    // A server that no longer has it falls back to the server's first.
    rig.server.set_identities(&[("i1", "One", "one@example.test", "One sig")]);
    assert_eq!(details(&rig), [("i1".to_string(), true)]);
    assert_eq!(rig.core.identities().unwrap()[0].id, "i1");
}

#[test]
fn a_reply_and_a_forward_get_the_signature_of_the_sender_the_reply_goes_out_as() {
    let rig = Rig::new();
    rig.server.set_identities(&[("i1", "One", USERNAME, "One sig"), ("i2", "Two", "two@example.test", "Two sig")]);
    rig.server.set_bodies(E1, Some("Are you free?"), None);
    rig.server.set_recipients(E1, &[("Two", "two@example.test")], &[]);
    rig.sync();
    // The reply goes out as the address the mail was sent to, and its signature is above the
    // quoted original; what the user types goes above the signature.
    let reply = rig.core.new_draft(DraftKind::Reply { message_id: E1.into() }).unwrap();
    assert_eq!(reply.identity_id, "i2");
    assert!(
        reply.body.starts_with("\n\n-- \nTwo sig\n\nMaria Keller <maria@example.test> wrote:\n> Are you free?"),
        "{}",
        reply.body
    );
    // The forward has it under the text, after the forwarded message.
    let forward = rig.core.new_draft(DraftKind::Forward { message_id: E1.into() }).unwrap();
    assert!(forward.body.ends_with("\n\n-- \nTwo sig"), "{}", forward.body);
    // A default the user chose is what a message to an unknown address gets.
    rig.core.set_default_identity("i2".into()).unwrap();
    let blank = rig.core.new_draft(DraftKind::New).unwrap();
    assert_eq!((blank.identity_id.as_str(), blank.body.as_str()), ("i2", "\n\n-- \nTwo sig"));
}

#[test]
fn changing_the_sender_of_an_open_draft_swaps_a_signature_nobody_touched() {
    let rig = Rig::new();
    rig.server.set_identities(&[("i1", "One", USERNAME, "One sig"), ("i2", "Two", "two@example.test", "Two sig")]);
    rig.sync();
    let mut draft = rig.core.new_draft(DraftKind::New).unwrap();
    draft.body = format!("Text{}", draft.body);
    let draft = rig.core.save_draft(draft).unwrap();
    assert_eq!(draft.body, "Text\n\n-- \nOne sig");

    let mut changed = draft.clone();
    changed.identity_id = "i2".into();
    let saved = rig.core.save_draft(changed).unwrap();
    assert_eq!((saved.identity_id.as_str(), saved.body.as_str()), ("i2", "Text\n\n-- \nTwo sig"));

    // A signature the user wrote in is left alone.
    let mut edited = saved.clone();
    edited.body = "Text\n\n-- \nTwo sig, and my own words".into();
    let edited = rig.core.save_draft(edited).unwrap();
    let mut back = edited.clone();
    back.identity_id = "i1".into();
    let kept = rig.core.save_draft(back).unwrap();
    assert_eq!((kept.identity_id.as_str(), kept.body.as_str()), ("i1", "Text\n\n-- \nTwo sig, and my own words"));

    // The same when the sender is changed in the call that sends.
    let mut last = rig.core.save_draft(rig.note("Sender changed at send")).unwrap();
    last.body = "Hi\n\n-- \nOne sig".into();
    let last = rig.core.save_draft(last).unwrap();
    let mut sending = last.clone();
    sending.identity_id = "i2".into();
    rig.core.send(sending, 0).unwrap();
    rig.sync();
    let sent = rig.server.view(&rig.server.submissions().last().unwrap().email_id);
    assert_eq!(sent.text.as_deref(), Some("Hi\n\n-- \nTwo sig"));
}

#[test]
fn a_new_sender_is_made_from_one_of_the_accounts_addresses() {
    let rig = Rig::synced();
    rig.server.set_identity_extras("i1", &[], "<b>keep</b>");
    let made = rig
        .core
        .save_identity(
            None,
            "Work".into(),
            "alias@example.test".into(),
            "r1@example.test; r2@example.test".into(),
            "Work sig".into(),
        )
        .unwrap();
    assert_eq!((made.name.as_str(), made.email.as_str()), ("Work", "alias@example.test"));
    assert_eq!((made.reply_to.as_str(), made.signature.as_str()), ("r1@example.test, r2@example.test", "Work sig"));
    assert!(!made.is_default && made.may_delete);
    // Only the fields we own went to the server.
    let sent = &rig.server.identity_requests()[0]["create"]["new"];
    let mut keys: Vec<_> = sent.as_object().unwrap().keys().cloned().collect();
    keys.sort();
    assert_eq!(keys, ["email", "name", "replyTo", "textSignature"]);
    // The cache was refreshed: the new sender can be chosen at once.
    assert!(rig.core.identities().unwrap().iter().any(|i| i.id == made.id && i.signature == "Work sig"));

    // An address the account does not have is refused by the server, in its words.
    let foreign =
        rig.core.save_identity(None, "X".into(), "nobody@elsewhere.example".into(), String::new(), String::new());
    assert!(matches!(&foreign, Err(Error::Invalid { msg }) if msg.contains("not configured")), "{foreign:?}");
    let empty = rig.core.save_identity(None, "X".into(), "not an address".into(), String::new(), String::new());
    assert!(is_invalid(&empty));
    assert_eq!(rig.server.identity_requests().len(), 2, "the bad shape never left");
}

#[test]
fn a_changed_sender_sends_only_what_changed_and_never_the_html_signature_or_the_address() {
    let rig = Rig::synced();
    rig.server.set_identity_extras("i1", &[("Desk", "desk@example.test")], "<b>html</b>");
    let saved = rig
        .core
        .save_identity(
            Some("i1".into()),
            "Alice Renamed".into(),
            "ignored@example.test".into(),
            "desk@example.test".into(),
            "Alice Test\nTest Corp".into(),
        )
        .unwrap();
    assert_eq!((saved.name.as_str(), saved.email.as_str()), ("Alice Renamed", USERNAME), "the address stays");
    let request = &rig.server.identity_requests()[0];
    assert_eq!(request["update"]["i1"], serde_json::json!({ "name": "Alice Renamed" }));
    let record = &rig.server.identity_records()[0];
    assert_eq!(
        record.reply_to,
        vec![("Desk".to_string(), "desk@example.test".to_string())],
        "the Reply-To with its name is kept"
    );
    assert_eq!(record.html_signature, "<b>html</b>");

    // Reply-To and signature change, and can be emptied again.
    rig.core
        .save_identity(
            Some("i1".into()),
            "Alice Renamed".into(),
            String::new(),
            "new@example.test".into(),
            "New".into(),
        )
        .unwrap();
    assert_eq!(
        rig.server.identity_requests()[1]["update"]["i1"],
        serde_json::json!({
            "textSignature": "New", "replyTo": [{ "email": "new@example.test" }]
        })
    );
    let cleared = rig
        .core
        .save_identity(Some("i1".into()), "Alice Renamed".into(), String::new(), String::new(), String::new())
        .unwrap();
    assert_eq!((cleared.reply_to.as_str(), cleared.signature.as_str()), ("", ""));
    assert_eq!(
        rig.server.identity_requests()[2]["update"]["i1"],
        serde_json::json!({ "textSignature": "", "replyTo": null })
    );
    assert_eq!(rig.server.identity_records()[0].html_signature, "<b>html</b>", "never touched");
    // Nothing changed: nothing asked.
    rig.core
        .save_identity(Some("i1".into()), "Alice Renamed".into(), String::new(), String::new(), String::new())
        .unwrap();
    assert_eq!(rig.server.identity_requests().len(), 3);
    assert!(is_invalid(&rig.core.save_identity(
        Some("zz".into()),
        "X".into(),
        String::new(),
        String::new(),
        String::new()
    )));
}

#[test]
fn what_the_server_would_refuse_is_said_first_in_plain_words() {
    let rig = Rig::synced();
    let long_name = "n".repeat(255);
    let result = rig.core.save_identity(Some("i1".into()), long_name, String::new(), String::new(), String::new());
    let text = message(&result.unwrap_err());
    assert!(text.contains("name") && text.contains("254") && text.contains("255"), "{text}");
    let long_signature = "s".repeat(2048);
    let result = rig.core.save_identity(Some("i1".into()), "Fine".into(), String::new(), String::new(), long_signature);
    let text = message(&result.unwrap_err());
    assert!(text.contains("signature") && text.contains("2047") && text.contains("2048"), "{text}");
    let bad =
        rig.core.save_identity(Some("i1".into()), "Fine".into(), String::new(), "not an address".into(), String::new());
    assert!(message(&bad.unwrap_err()).contains("not an address"));
    assert!(rig.server.identity_requests().is_empty(), "none of it reached the server");
    // The limits themselves are fine.
    rig.core.save_identity(Some("i1".into()), "n".repeat(254), String::new(), String::new(), "s".repeat(2047)).unwrap();
    assert_eq!(rig.server.identity_requests().len(), 1);
}

#[test]
fn a_sender_can_be_deleted_but_not_the_last_one_nor_one_the_server_keeps_nor_one_a_message_waits_on() {
    let rig = Rig::synced();
    // One a message that waits here for its time uses.
    let mut waiting = rig.note("Waits for i2");
    waiting.identity_id = "i2".into();
    rig.core.send(waiting.clone(), 3600).unwrap();
    let busy = rig.core.delete_identity("i2".into()).unwrap_err();
    assert!(matches!(busy, Error::Invalid { .. }) && message(&busy).contains("waiting"), "{busy:?}");
    // Taken back, it is a draft: that does not hold the sender.
    rig.core.undo_send(waiting.id).unwrap();
    // One the server holds a scheduled message of.
    let mut scheduled = rig.note("Scheduled as i2");
    scheduled.identity_id = "i2".into();
    rig.core.schedule_send(scheduled, now() + 3600).unwrap();
    let busy = rig.core.delete_identity("i2".into()).unwrap_err();
    assert!(matches!(busy, Error::Invalid { .. }) && message(&busy).contains("holding"), "{busy:?}");
    rig.server.release_held();
    // One the server will not let go of.
    rig.server.set_identity_may_delete("i2", false);
    assert!(is_invalid(&rig.core.delete_identity("i2".into())));
    rig.server.set_identity_may_delete("i2", true);

    rig.core.set_default_identity("i2".into()).unwrap();
    rig.core.delete_identity("i2".into()).unwrap();
    assert_eq!(
        rig.core.identities().unwrap().iter().map(|i| i.id.as_str()).collect::<Vec<_>>(),
        ["i1"],
        "the cache follows"
    );
    assert_eq!(details(&rig), [("i1".to_string(), true)], "the default that went falls back");
    // The last one stays.
    let last = rig.core.delete_identity("i1".into()).unwrap_err();
    assert!(matches!(last, Error::Invalid { .. }) && message(&last).contains("only"), "{last:?}");
    assert_eq!(rig.server.identity_records().len(), 1);
    // One that is gone already is as good as deleted.
    rig.core.delete_identity("i2".into()).unwrap();
}

#[test]
fn the_identity_calls_need_an_account() {
    let dir = tempfile::tempdir().unwrap();
    let core =
        Core::new(dir.path().join("c.db").to_string_lossy().into_owned(), Arc::new(MemorySecrets::default())).unwrap();
    assert!(core.identity_details().is_err());
    assert!(core.save_identity(None, String::new(), String::new(), String::new(), String::new()).is_err());
    assert!(core.delete_identity("i1".into()).is_err());
    assert!(core.set_default_identity("i1".into()).is_err());
    assert!(core.scheduled_sends().is_err());
    assert!(core.cancel_scheduled("s1".into()).is_err());
    assert!(core
        .schedule_send(
            Draft {
                id: "d".into(),
                identity_id: "i1".into(),
                to: vec![],
                cc: vec![],
                bcc: vec![],
                subject: String::new(),
                body: String::new(),
                attachments: vec![],
                source_message_id: None,
                is_forward: false,
                server_id: None,
                send_error: None,
                updated_at: 0,
            },
            0
        )
        .is_err());
}

// ---- the mock keeps to the notes -------------------------------------------------------------------

#[test]
fn the_mock_answers_a_hold_as_the_probed_server_does() {
    use serde_json::json;
    let rig = Rig::synced();
    let draft = rig.core.save_draft(rig.note("For the mock")).unwrap();
    rig.sync();
    let copy = rig.server.ids_in("drafts")[0].clone();
    let account = rig.server.account_id();
    let create = |parameters: serde_json::Value| {
        let envelope = json!({ "mailFrom": { "email": USERNAME, "parameters": parameters }, "rcptTo": [{ "email": "bo@example.test" }] });
        let args = json!({ "accountId": account, "create": { "s": { "identityId": "i1", "emailId": copy, "envelope": envelope } } });
        rig.call("EmailSubmission/set", args)
    };
    let refused = |answer: &serde_json::Value| answer["notCreated"]["s"].clone();

    // The value is a string; a number, zero and words are refused.
    assert_eq!(refused(&create(json!({ "HOLDFOR": 120 })))["type"], "invalidProperties");
    assert_eq!(refused(&create(json!({ "HOLDFOR": "0" })))["type"], "invalidProperties");
    assert_eq!(refused(&create(json!({ "HOLDFOR": "abc" })))["type"], "invalidProperties");
    assert_eq!(refused(&create(json!({ "FUTURERELEASE": "60" })))["type"], "invalidProperties");
    // Not both, nothing in the past, nothing beyond seven days.
    assert_eq!(
        refused(&create(json!({ "HOLDFOR": "60", "HOLDUNTIL": iso_utc(now() + 600) })))["type"],
        "forbiddenMailFrom"
    );
    assert_eq!(refused(&create(json!({ "HOLDUNTIL": iso_utc(now() - 60) })))["type"], "forbiddenMailFrom");
    assert_eq!(refused(&create(json!({ "HOLDFOR": "604801" })))["type"], "forbiddenMailFrom");
    assert_eq!(refused(&create(json!({ "HOLDUNTIL": iso_utc(now() + 8 * 86_400) })))["type"], "forbiddenMailFrom");
    assert!(rig.server.held().is_empty());

    // Seven days is the limit and it is accepted, as pending.
    let made = create(json!({ "HOLDFOR": "604800" }));
    assert_eq!(made["created"]["s"]["undoStatus"], "pending");
    let id = made["created"]["s"]["id"].as_str().unwrap().to_string();

    // The server cannot sort by the time, and cancels once.
    let sorted = rig.call("EmailSubmission/query", json!({ "accountId": account, "sort": [{ "property": "sendAt" }] }));
    assert_eq!(sorted["type"], "unsupportedSort");
    let cancel = |id: &str| {
        rig.call("EmailSubmission/set", json!({ "accountId": account, "update": { id: { "undoStatus": "canceled" } } }))
    };
    assert!(cancel(&id)["updated"].get(&id).is_some());
    assert_eq!(cancel(&id)["notUpdated"][&id]["type"], "cannotUnsend");
    let other = rig.call(
        "EmailSubmission/set",
        json!({ "accountId": account, "update": { id.clone(): { "undoStatus": "final" } } }),
    );
    assert_eq!(other["notUpdated"][&id]["type"], "invalidProperties");
    drop(draft);
}

#[test]
fn the_mock_answers_identity_changes_as_the_probed_server_does() {
    use serde_json::json;
    let rig = Rig::synced();
    let account = rig.server.account_id();
    let set = |args: serde_json::Value| {
        let mut args = args;
        args["accountId"] = json!(account);
        rig.call("Identity/set", args)
    };
    let kind = |answer: &serde_json::Value, group: &str, key: &str| {
        answer[group][key]["description"].as_str().unwrap_or_default().to_string()
    };
    // The address is fixed after creation, and the server says so for it and for mayDelete.
    let answer = set(json!({ "update": { "i1": { "email": "x@example.test" } } }));
    assert_eq!(kind(&answer, "notUpdated", "i1"), "Field could not be set.");
    let answer = set(json!({ "update": { "i1": { "mayDelete": false } } }));
    assert_eq!(kind(&answer, "notUpdated", "i1"), "Field could not be set.");
    let answer = set(json!({ "update": { "i1": { "id": "i9" } } }));
    assert_eq!(kind(&answer, "notUpdated", "i1"), "The id property is immutable.");
    let answer = set(json!({ "update": { "i1": { "colour": "red" } } }));
    assert_eq!(kind(&answer, "notUpdated", "i1"), "Invalid property.");
    // The limits: 254 for the name, 2047 for a signature.
    assert!(set(json!({ "update": { "i1": { "name": "n".repeat(254) } } }))["updated"].get("i1").is_some());
    assert_eq!(
        set(json!({ "update": { "i1": { "name": "n".repeat(255) } } }))["notUpdated"]["i1"]["properties"],
        json!(["name"])
    );
    assert_eq!(
        set(json!({ "update": { "i1": { "textSignature": "s".repeat(2048) } } }))["notUpdated"]["i1"]["properties"],
        json!(["textSignature"])
    );
    assert_eq!(
        set(json!({ "update": { "i1": { "htmlSignature": "s".repeat(2048) } } }))["notUpdated"]["i1"]["properties"],
        json!(["htmlSignature"])
    );
    // Creating: an address of the account, nothing else.
    assert_eq!(kind(&set(json!({ "create": { "n": { "name": "x" } } })), "notCreated", "n"), "Missing e-mail address.");
    assert_eq!(
        kind(&set(json!({ "create": { "n": { "email": "nope" } } })), "notCreated", "n"),
        "Invalid e-mail address."
    );
    assert_eq!(
        kind(&set(json!({ "create": { "n": { "email": "other@elsewhere.example" } } })), "notCreated", "n"),
        "E-mail address not configured for this account."
    );
    assert_eq!(
        kind(&set(json!({ "create": { "n": { "email": USERNAME, "mayDelete": true } } })), "notCreated", "n"),
        "Field could not be set."
    );
    // The same address twice is fine, and a destroy of an unknown one is notFound.
    assert!(set(json!({ "create": { "n": { "email": USERNAME, "name": "Twin" } } }))["created"].get("n").is_some());
    assert_eq!(set(json!({ "destroy": ["zz"] }))["notDestroyed"]["zz"]["type"], "notFound");
}
