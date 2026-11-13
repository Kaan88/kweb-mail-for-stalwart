//! The push loop against the mock JMAP server: what the phone would be told, and when.
//! The scenarios mirror the M2 table in PLAN.md. Timings are shortened so each runs in a
//! second or two; waiting is always polling with a deadline, never a blind sleep.

mod support;

use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use kweb_core::{
    actions, auth,
    model::{Account, Action, ConnectionStatus},
    notify::{self, OnScreen, Plan},
    push::{self, Deps, Handle, Listener, Timing},
    secrets::MemorySecrets,
    store::Store,
};
use support::MockServer;

const DEADLINE: Duration = Duration::from_secs(5);

// ---- a listener that remembers everything ---------------------------------------------

#[derive(Debug, Clone)]
enum Ev {
    Status(ConnectionStatus),
    Plan(Plan),
    CacheChanged,
    Alert,
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
    fn calendar_alert(&self, _event_id: String, _alert_id: String, _recurrence_id: Option<String>) {
        self.events.lock().unwrap().push(Ev::Alert);
    }
}

impl Recorder {
    fn all(&self) -> Vec<Ev> {
        self.events.lock().unwrap().clone()
    }

    fn statuses(&self) -> Vec<ConnectionStatus> {
        self.all()
            .into_iter()
            .filter_map(|e| match e {
                Ev::Status(s) => Some(s),
                _ => None,
            })
            .collect()
    }

    fn plans(&self) -> Vec<Plan> {
        self.all()
            .into_iter()
            .filter_map(|e| match e {
                Ev::Plan(p) => Some(p),
                _ => None,
            })
            .collect()
    }

    /// Message ids posted so far, in order, with whether the post was silent.
    fn posted(&self) -> Vec<(String, bool)> {
        self.plans().into_iter().flat_map(|p| p.post).map(|n| (n.message_id, n.silent)).collect()
    }

    fn posted_ids(&self) -> Vec<String> {
        self.posted().into_iter().map(|(id, _)| id).collect()
    }

    fn cancelled_ids(&self) -> Vec<String> {
        self.plans().into_iter().flat_map(|p| p.cancel).map(|c| c.message_id).collect()
    }

    fn is_live(&self) -> bool {
        self.statuses().iter().any(|s| matches!(s, ConnectionStatus::Live { .. }))
    }
}

/// Polls until `cond` holds; fails with `what` after the deadline.
async fn eventually(what: &str, mut cond: impl FnMut() -> bool) {
    eventually_within(DEADLINE, what, &mut cond).await
}

async fn eventually_within(limit: Duration, what: &str, cond: &mut impl FnMut() -> bool) {
    let start = Instant::now();
    while !cond() {
        assert!(start.elapsed() < limit, "gave up waiting for: {what}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

// ---- the rig --------------------------------------------------------------------------

fn fast() -> Timing {
    Timing {
        first_backoff: Duration::from_millis(50),
        max_backoff: Duration::from_millis(400),
        // Long: only the silent-stream test wants a re-arm.
        rearm_after: Duration::from_secs(60),
        settle: Duration::from_millis(30),
        first_auth_retry: Duration::from_millis(50),
        max_auth_failures: 4,
    }
}

struct Rig {
    server: MockServer,
    rec: Arc<Recorder>,
    deps: Deps,
}

/// Signs in to a fresh mock server through the real OAuth flow. Mail delivered from now on is
/// "after sign-in"; the rule counts a mail as new when it arrived after `signed_in_at`.
async fn rig(timing: Timing) -> Rig {
    rig_with(timing, 3600).await
}

/// Like `rig`, with access tokens that live `lifetime_secs` seconds from the very first one.
async fn rig_with(timing: Timing, lifetime_secs: u64) -> Rig {
    let server = MockServer::start().await;
    server.set_access_token_lifetime(lifetime_secs);
    let redirect = "http://127.0.0.1:5555/callback";
    let pending = auth::begin_sign_in(&server.url(), redirect, None).await.expect("begin sign-in");
    let callback = server.authorize(&pending.authorize_url).expect("authorize");
    let signed_in =
        auth::complete_sign_in(pending, &callback, Arc::new(MemorySecrets::default())).await.expect("complete sign-in");
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
    let account = Account {
        id: server.account_id().to_string(),
        server_url: server.url(),
        username: server.username().to_string(),
        signed_in_at: now - 5,
    };
    let store = Arc::new(Store::open_in_memory().unwrap());
    store.save_account(&account).unwrap();
    let rec = Arc::new(Recorder::default());
    let deps = Deps { store, tokens: signed_in.tokens, account, listener: rec.clone(), timing };
    server.reset_request_counts();
    Rig { server, rec, deps }
}

impl Rig {
    /// Starts the loop and waits until the stream is armed.
    async fn start(&self) -> Handle {
        let handle = push::spawn(self.deps.clone());
        let rec = self.rec.clone();
        eventually("the loop to go live", move || rec.is_live()).await;
        self.server.wait_for_event_streams(1).await;
        handle
    }

    fn deliver(&self, subject: &str) -> String {
        self.server.deliver("Ada", "ada@example.org", subject, "preview text")
    }

    async fn wait_posted(&self, id: &str) {
        let rec = self.rec.clone();
        let id = id.to_string();
        eventually(&format!("a post for {id}"), move || rec.posted_ids().contains(&id)).await;
    }

    async fn wait_cancelled(&self, id: &str) {
        let rec = self.rec.clone();
        let id = id.to_string();
        eventually(&format!("a cancel for {id}"), move || rec.cancelled_ids().contains(&id)).await;
    }

    fn never_needed_sign_in(&self) {
        assert!(!self.rec.statuses().contains(&ConnectionStatus::SignInNeeded), "{:?}", self.rec.statuses());
    }
}

// ---- 1. mail arrives ------------------------------------------------------------------

#[tokio::test]
async fn new_mail_is_posted_and_the_status_goes_connecting_then_live() {
    let r = rig(fast()).await;
    let handle = r.start().await;
    let id = r.deliver("Lunch?");
    r.wait_posted(&id).await;

    let post = r.rec.plans().into_iter().flat_map(|p| p.post).find(|n| n.message_id == id).unwrap();
    assert_eq!(post.from, "Ada");
    assert_eq!(post.subject, "Lunch?");
    assert!(!post.silent, "genuinely new mail makes a sound");

    let statuses = r.rec.statuses();
    assert_eq!(statuses[0], ConnectionStatus::Connecting);
    assert!(matches!(statuses[1], ConnectionStatus::Live { .. }), "{statuses:?}");
    assert!(matches!(handle.status(), ConnectionStatus::Live { .. }));
    assert!(r.rec.all().iter().any(|e| matches!(e, Ev::CacheChanged)));
    handle.stop().await;
}

// ---- 2. read, archived or deleted elsewhere -------------------------------------------

#[tokio::test]
async fn mail_read_archived_or_deleted_elsewhere_is_cancelled() {
    let r = rig(fast()).await;
    let handle = r.start().await;
    let read = r.deliver("read elsewhere");
    let archived = r.deliver("archived elsewhere");
    let deleted = r.deliver("deleted elsewhere");
    let kept = r.deliver("stays unread");
    for id in [&read, &archived, &deleted, &kept] {
        r.wait_posted(id).await;
    }

    r.server.mark_read(&read);
    r.wait_cancelled(&read).await;
    r.server.move_to(&archived, "archive");
    r.wait_cancelled(&archived).await;
    r.server.destroy(&deleted);
    r.wait_cancelled(&deleted).await;

    assert!(!r.rec.cancelled_ids().contains(&kept));
    // A cancel is silent by nature; and nothing is posted twice.
    let mut posted = r.rec.posted_ids();
    posted.sort();
    posted.dedup();
    assert_eq!(posted.len(), 4);
    handle.stop().await;
}

// ---- 3. outage: three mails, one read elsewhere ---------------------------------------

#[tokio::test]
async fn after_an_outage_exactly_the_unread_mail_is_posted() {
    let r = rig(fast()).await;
    let handle = r.start().await;

    r.server.set_down(true);
    let first = r.deliver("first");
    let second = r.deliver("second, read on the desktop");
    let third = r.deliver("third");
    r.server.mark_read(&second);
    let rec = r.rec.clone();
    eventually("the loop to notice the outage", move || {
        rec.statuses().iter().any(|s| matches!(s, ConnectionStatus::Offline { .. }))
    })
    .await;
    assert!(r.rec.posted_ids().is_empty(), "nothing can have been posted while the server was down");

    r.server.set_down(false);
    r.wait_posted(&first).await;
    r.wait_posted(&third).await;
    // Let the catch-up settle, then check nothing else came: no post for the read mail and no
    // cancel for it (it was never shown).
    tokio::time::sleep(Duration::from_millis(300)).await;
    let mut posted = r.rec.posted_ids();
    posted.sort();
    let mut expected = vec![first, third];
    expected.sort();
    assert_eq!(posted, expected);
    assert!(r.rec.cancelled_ids().is_empty(), "{:?}", r.rec.cancelled_ids());
    assert!(matches!(handle.status(), ConnectionStatus::Live { .. }));
    handle.stop().await;
}

// ---- 4. the server drops the stream ---------------------------------------------------

#[tokio::test]
async fn a_dropped_stream_is_reopened_and_mail_from_the_gap_is_posted() {
    let r = rig(fast()).await;
    let handle = r.start().await;

    r.server.drop_event_streams();
    let id = r.deliver("sent while the stream was closed");
    r.wait_posted(&id).await;
    r.server.wait_for_event_streams(1).await;
    assert!(r.rec.statuses().iter().any(|s| matches!(s, ConnectionStatus::Offline { .. })));

    // And the new stream really works.
    let later = r.deliver("after the reconnect");
    r.wait_posted(&later).await;
    handle.stop().await;
}

// ---- 5. the stream dies silently ------------------------------------------------------

#[tokio::test]
async fn a_silent_stream_is_replaced_after_the_rearm_interval() {
    let r = rig(Timing { rearm_after: Duration::from_millis(700), ..fast() }).await;
    let handle = r.start().await;

    r.server.set_push_enabled(false);
    let id = r.deliver("no event for this one");
    r.wait_posted(&id).await;
    // The old stream was dropped when the new one took over.
    r.server.wait_for_event_streams(1).await;
    handle.stop().await;
}

// ---- 6. access tokens expire ----------------------------------------------------------

#[tokio::test]
async fn expired_access_tokens_are_renewed_without_missing_mail() {
    let r = rig(fast()).await;
    let handle = r.start().await;

    r.server.expire_access_tokens();
    let first = r.deliver("arrives with a dead token");
    r.wait_posted(&first).await;
    assert!(r.server.token_requests() >= 1, "the token must have been renewed");

    let rec = r.rec.clone();
    eventually("the loop to be live again", move || {
        matches!(rec.statuses().last(), Some(ConnectionStatus::Live { .. }))
    })
    .await;
    let second = r.deliver("arrives after the renewal");
    r.wait_posted(&second).await;
    r.never_needed_sign_in();
    handle.stop().await;
}

#[tokio::test]
async fn a_token_past_its_renew_time_is_renewed_and_the_stream_swapped() {
    // The shortest lifetime the core accepts is 30 s, renewed after 15 s: the one slow test.
    let r = rig_with(fast(), 30).await;
    let handle = r.start().await;

    let server = &r.server;
    eventually_within(Duration::from_secs(25), "the token to be renewed", &mut || server.token_requests() >= 1).await;
    server.wait_for_event_streams(1).await;
    let id = r.deliver("after the proactive renewal");
    r.wait_posted(&id).await;
    r.never_needed_sign_in();
    assert!(
        !r.rec.statuses().iter().any(|s| matches!(s, ConnectionStatus::Offline { .. })),
        "a planned renewal is not an outage: {:?}",
        r.rec.statuses()
    );
    handle.stop().await;
}

// ---- 7. the grant is revoked ----------------------------------------------------------

#[tokio::test]
async fn a_revoked_grant_ends_in_sign_in_needed_without_spinning() {
    let r = rig(fast()).await;
    let handle = r.start().await;

    r.server.revoke_grant();
    r.server.expire_access_tokens();
    r.deliver("never seen");
    let rec = r.rec.clone();
    eventually("SignInNeeded", move || rec.statuses().last() == Some(&ConnectionStatus::SignInNeeded)).await;
    assert_eq!(handle.status(), ConnectionStatus::SignInNeeded);

    // Several back-off periods later it is still waiting, not asking again and again.
    let before = r.server.token_requests();
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert_eq!(r.server.token_requests(), before);
    assert!(before <= 3, "{before} token requests");
    assert!(r.rec.posted_ids().is_empty());

    // A poke asks once more (the user may have signed in again) and gives up again.
    handle.poke();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(handle.status(), ConnectionStatus::SignInNeeded);
    assert!(r.server.token_requests() <= before + 1);
    handle.stop().await;
}

// ---- 8. check_once --------------------------------------------------------------------

#[tokio::test]
async fn check_once_after_a_cleared_shade_posts_silently() {
    let r = rig(fast()).await;
    let a = r.deliver("one");
    let b = r.deliver("two");

    let first = push::check_once(&r.deps, OnScreen::Unknown).await.unwrap();
    let mut ids: Vec<_> = first.post.iter().map(|n| n.message_id.clone()).collect();
    ids.sort();
    let mut expected = vec![a.clone(), b.clone()];
    expected.sort();
    assert_eq!(ids, expected);
    assert!(first.post.iter().all(|n| !n.silent));

    // Nothing changed: an empty plan, and the listener is not bothered with it.
    let again = push::check_once(&r.deps, OnScreen::Unknown).await.unwrap();
    assert!(again.is_empty());
    assert_eq!(r.rec.plans().len(), 1);

    // The phone restarted and lost its notifications: they come back, quietly.
    let cleared = push::check_once(&r.deps, OnScreen::Cleared).await.unwrap();
    assert_eq!(cleared.post.len(), 2);
    assert!(cleared.post.iter().all(|n| n.silent));
    assert!(cleared.cancel.is_empty());
    assert_eq!(r.rec.plans().len(), 2);

    // Only one of them is really there: only the other is put back.
    let one_there = [notify::android_id(&a)];
    let partial = push::check_once(&r.deps, OnScreen::Ids(&one_there)).await.unwrap();
    assert_eq!(partial.post.len(), 1);
    assert_eq!(partial.post[0].message_id, b);
}

#[tokio::test]
async fn check_once_renews_an_expired_token_once() {
    let r = rig(fast()).await;
    let id = r.deliver("hello");
    r.server.expire_access_tokens();
    let plan = push::check_once(&r.deps, OnScreen::Unknown).await.unwrap();
    assert_eq!(plan.post.len(), 1);
    assert_eq!(plan.post[0].message_id, id);
}

// ---- 9. poke after an action ----------------------------------------------------------

#[tokio::test]
async fn poke_sends_a_queued_mark_read_and_cancels_the_notification() {
    let r = rig(fast()).await;
    let handle = r.start().await;
    let id = r.deliver("read me on the phone");
    r.wait_posted(&id).await;

    actions::submit(&r.deps.store, &r.deps.account.id, &Action::MarkRead { message_id: id.clone() }).unwrap();
    assert!(!r.server.is_read(&id), "queued only: no pass has run yet");

    handle.poke();
    r.wait_cancelled(&id).await;
    assert!(r.server.is_read(&id));
    handle.stop().await;
}

// ---- 10. stop, and a server that forgets its changes ----------------------------------

#[tokio::test]
async fn stop_ends_the_loop_and_dropping_the_handle_does_too() {
    let r = rig(fast()).await;
    let handle = r.start().await;
    handle.stop().await;
    r.server.wait_for_event_streams(0).await;
    let id = r.deliver("nobody is listening");
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!r.rec.posted_ids().contains(&id));

    let r = rig(fast()).await;
    let handle = r.start().await;
    drop(handle);
    r.server.wait_for_event_streams(0).await;
}

#[tokio::test]
async fn cannot_calculate_changes_mid_session_still_ends_in_the_right_notifications() {
    let r = rig(fast()).await;
    let handle = r.start().await;
    let old = r.deliver("shown before");
    r.wait_posted(&old).await;

    r.server.forget_changes_before_now();
    r.server.mark_read(&old);
    let new = r.deliver("arrives after the server forgot");
    r.wait_posted(&new).await;
    r.wait_cancelled(&old).await;
    assert!(matches!(handle.status(), ConnectionStatus::Live { .. }));
    handle.stop().await;
}
