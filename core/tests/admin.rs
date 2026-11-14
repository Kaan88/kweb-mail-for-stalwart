//! Admin step 2 against the mock server, through the `Core` object the way the app uses it: the
//! admin API key (connect, forget, refused), and the features every server has: the mail queue,
//! accounts, blocked addresses, logs and maintenance. Each test is about one sentence of the
//! doc comments in `core/src/ffi/admin.rs`. The mock behaves like Stalwart 1.1.0 where it
//! matters (see `support/admin_mock.rs`).

mod support;

use std::sync::Arc;

use kweb_core::{
    ffi::{Core, Maintenance, ServerAccountKind},
    secrets::{MemorySecrets, SecretStore},
    Error,
};
use serde_json::json;
use support::{
    admin_mock::{BOB_ID, DOMAIN_ID, GROUP_ID, OTHER_DOMAIN_ID},
    MockServer, ACCOUNT_ID, USERNAME,
};
use tokio::runtime::Runtime;

/// The name the core keeps the admin key under in the secret store.
const KEY_NAME: &str = "admin_key";
const NOW: i64 = 1_800_000_000;

struct Rig {
    // Field order is drop order: the core goes first, the mock and its runtime last.
    core: Arc<Core>,
    server: MockServer,
    secrets: Arc<MemorySecrets>,
    _dir: tempfile::TempDir,
    _rt: Runtime,
}

impl Rig {
    /// Signed in, with no admin key yet.
    fn new() -> Rig {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        let server = rt.block_on(MockServer::start());
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("cache.db").to_string_lossy().into_owned();
        let secrets = Arc::new(MemorySecrets::default());
        let core = Core::new(db_path, secrets.clone()).unwrap();
        let url = core.begin_sign_in_with_redirect(server.url(), "ovh.kweb.mail:/oauth/callback".into()).unwrap();
        core.complete_sign_in(server.authorize(&url).unwrap()).unwrap();
        Rig { core, server, secrets, _dir: dir, _rt: rt }
    }

    /// Signed in with an admin key that may do everything.
    fn admin() -> Rig {
        let rig = Rig::new();
        rig.connect_with(None);
        rig
    }

    /// Connects a key of the signed-in account with these permissions (`None`: all).
    fn connect_with(&self, permissions: Option<&[&str]>) -> String {
        let key = self.server.admin_key(USERNAME, permissions);
        self.core.admin_connect(key.clone()).unwrap();
        key
    }

    fn stored_key(&self) -> Option<String> {
        self.secrets.get(KEY_NAME.into()).unwrap()
    }

    /// How many calls of this management method the server got.
    fn calls(&self, method: &str) -> usize {
        self.server.admin_calls().iter().filter(|m| m.as_str() == method).count()
    }

    fn account_calls(&self) -> usize {
        self.server.admin_calls().iter().filter(|m| m.starts_with("x:Account/")).count()
    }
}

fn invalid<T: std::fmt::Debug>(r: Result<T, Error>) -> String {
    match r {
        Err(Error::Invalid { msg }) => msg,
        other => panic!("expected Error::Invalid, got {other:?}"),
    }
}

fn recipient(status: &str, extra: serde_json::Value) -> serde_json::Value {
    let mut r = json!({ "queueName": "remote", "retryCount": 0, "retryDue": "2027-01-15T08:00:00Z", "status": { "@type": status } });
    for (k, v) in extra.as_object().into_iter().flatten() {
        r["status"][k] = v.clone();
    }
    r
}

fn queued(from: &str, next: &str, recipients: serde_json::Value) -> serde_json::Value {
    json!({
        "returnPath": from, "size": 1234, "createdAt": "2027-01-15T07:00:00Z", "nextRetry": next,
        "recipients": recipients,
    })
}

// ---- the key ----

#[test]
fn without_a_key_nothing_is_open_and_it_is_not_an_error() {
    let rig = Rig::new();
    let access = rig.core.admin_access().unwrap();
    assert!(!access.connected && !access.key_refused);
    assert!(!access.may_queue && !access.may_accounts && !access.may_blocked && !access.may_logs);
    assert!(!access.may_maintenance && !access.may_dashboard && !access.may_tenants);
    assert!(invalid(rig.core.admin_summary()).contains("no admin key"));
}

#[test]
fn a_key_that_is_taken_is_stored_and_opens_the_areas_its_permissions_name() {
    let rig = Rig::new();
    let key = rig.server.admin_key(USERNAME, None);
    let access = rig.core.admin_connect(format!("  {key}\n")).unwrap();
    assert!(access.connected && !access.key_refused);
    assert_eq!(access.edition, "community");
    assert!(access.may_queue && access.may_accounts && access.may_blocked && access.may_logs && access.may_maintenance);
    assert_eq!(rig.stored_key(), Some(key));
    assert_eq!(rig.core.admin_access().unwrap(), access);
}

#[test]
fn a_key_with_fewer_permissions_opens_fewer_areas() {
    let rig = Rig::new();
    rig.connect_with(Some(&["sysQueuedMessageQuery", "sysQueuedMessageGet", "sysLogQuery", "sysLogGet"]));
    let access = rig.core.admin_access().unwrap();
    assert!(access.may_queue && access.may_logs);
    assert!(!access.may_accounts && !access.may_blocked && !access.may_maintenance);
    // A read alone is not enough for an area: the list needs the query and the get.
    let rig = Rig::new();
    rig.connect_with(Some(&["sysBlockedIpQuery", "sysQueuedMessageGet", "sysQueuedMessageQuery"]));
    assert!(!rig.core.admin_access().unwrap().may_blocked);
    // Maintenance needs the right to start actions and one action.
    let rig = Rig::new();
    rig.connect_with(Some(&["sysActionCreate", "sysLogQuery", "sysLogGet"]));
    assert!(!rig.core.admin_access().unwrap().may_maintenance);
    let rig = Rig::new();
    rig.connect_with(Some(&["sysActionCreate", "actionPauseMtaQueue"]));
    assert!(rig.core.admin_access().unwrap().may_maintenance);
}

#[test]
fn the_enterprise_areas_are_open_only_on_a_licensed_server_where_the_key_may_read_them() {
    let rig = Rig::admin();
    let community = rig.core.admin_access().unwrap();
    assert!(!community.may_dashboard && !community.may_history && !community.may_recovery);
    assert!(!community.may_alerts && !community.may_tenants);

    rig.server.set_edition("enterprise");
    let licensed = rig.core.admin_access().unwrap();
    assert_eq!(licensed.edition, "enterprise");
    assert!(licensed.may_dashboard && licensed.may_history && licensed.may_recovery);
    assert!(licensed.may_alerts && licensed.may_tenants);

    // Licensed, but a key that lacks the read permission has no such area.
    let rig = Rig::new();
    rig.server.set_edition("enterprise");
    rig.connect_with(Some(&[
        "sysQueuedMessageQuery",
        "sysQueuedMessageGet",
        "sysMetricQuery",
        "sysMetricGet",
        "sysTenantGet",
    ]));
    let access = rig.core.admin_access().unwrap();
    assert!(access.may_dashboard && access.may_tenants);
    assert!(!access.may_history && !access.may_recovery && !access.may_alerts);
    // An area is open exactly when its first screen can work: a query without the get is not enough.
    let rig = Rig::new();
    rig.server.set_edition("enterprise");
    rig.connect_with(Some(&[
        "sysQueuedMessageQuery",
        "sysQueuedMessageGet",
        "sysMetricQuery",
        "sysTenantQuery",
        "sysTraceQuery",
        "sysAlertQuery",
    ]));
    let access = rig.core.admin_access().unwrap();
    assert!(!access.may_dashboard && !access.may_tenants && !access.may_history && !access.may_alerts);
}

#[test]
fn a_key_the_server_refuses_is_not_stored() {
    let rig = Rig::new();
    let key = rig.server.admin_key(USERNAME, None);
    rig.server.refuse_admin_key(&key);
    let msg = invalid(rig.core.admin_connect(key.clone()));
    assert!(msg.contains("did not accept"), "{msg}");
    assert!(!msg.contains(&key));
    assert_eq!(rig.stored_key(), None);
    let msg = invalid(rig.core.admin_connect("API_not_a_key".into()));
    assert!(msg.contains("did not accept"), "{msg}");
    assert!(invalid(rig.core.admin_connect("   ".into())).contains("paste"));
    assert_eq!(rig.stored_key(), None);
}

#[test]
fn a_key_of_another_account_is_not_stored() {
    let rig = Rig::new();
    let key = rig.server.admin_key("carol@example.test", None);
    let msg = invalid(rig.core.admin_connect(key.clone()));
    assert!(msg.contains("another account"), "{msg}");
    assert!(!msg.contains(&key));
    assert_eq!(rig.stored_key(), None);
}

#[test]
fn a_key_that_opens_nothing_administrative_is_not_stored() {
    let rig = Rig::new();
    let key = rig.server.admin_key(USERNAME, Some(&["emailReceive"]));
    let msg = invalid(rig.core.admin_connect(key.clone()));
    assert!(msg.contains("opens no admin area"), "{msg}");
    assert!(!msg.contains(&key));
    assert_eq!(rig.stored_key(), None);
}

#[test]
fn a_stored_key_the_server_later_refuses_is_connected_false_and_key_refused() {
    let rig = Rig::new();
    let key = rig.connect_with(None);
    rig.server.refuse_admin_key(&key);
    let access = rig.core.admin_access().unwrap();
    assert!(!access.connected && access.key_refused && !access.may_queue);
    // The calls say so in words, and the key is not in them.
    let msg = invalid(rig.core.admin_queue(0, 10));
    assert!(msg.contains("did not accept"), "{msg}");
    assert!(!msg.contains(&key));
}

#[test]
fn forgetting_the_key_and_signing_out_both_remove_it() {
    let rig = Rig::admin();
    assert!(rig.stored_key().is_some());
    rig.core.admin_disconnect().unwrap();
    assert_eq!(rig.stored_key(), None);
    assert!(!rig.core.admin_access().unwrap().connected);

    rig.connect_with(None);
    assert!(rig.stored_key().is_some());
    rig.core.sign_out().unwrap();
    assert_eq!(rig.stored_key(), None);
}

/// Signs the core of the rig in again at another mock server (the same account id and name, as
/// ids repeat between servers) without signing out first.
fn sign_in_again_elsewhere(rig: &Rig) -> MockServer {
    let other = rig._rt.block_on(MockServer::start());
    let url = rig.core.begin_sign_in_with_redirect(other.url(), "ovh.kweb.mail:/oauth/callback".into()).unwrap();
    rig.core.complete_sign_in(other.authorize(&url).unwrap()).unwrap();
    other.reset_request_counts();
    other
}

#[test]
fn a_second_sign_in_at_another_server_does_not_take_the_key_along() {
    let rig = Rig::admin();
    assert!(rig.stored_key().is_some());
    let other = sign_in_again_elsewhere(&rig);
    // Nothing is sent to the new server, and the key is gone with the old account's setting.
    let access = rig.core.admin_access().unwrap();
    assert!(!access.connected && !access.key_refused);
    assert_eq!(rig.stored_key(), None);
    assert_eq!((other.request_count("/api/account"), other.request_count("/jmap/session")), (0, 0));
    assert!(invalid(rig.core.admin_queue(0, 10)).contains("no admin key"));
    assert_eq!(other.admin_calls().len(), 0);
}

#[test]
fn a_key_that_was_stored_for_another_server_is_dropped_and_never_sent_even_when_nothing_cleared_it() {
    let rig = Rig::admin();
    // A key whose record names another server (as after an update that kept an old store).
    rig.secrets.set("admin_key_for".into(), "https://elsewhere.test\nx\nalice".into()).unwrap();
    rig.server.reset_request_counts();
    assert!(!rig.core.admin_access().unwrap().connected);
    assert_eq!(rig.stored_key(), None);
    assert_eq!(rig.server.request_count("/api/account"), 0);
    // A key with no record at all is not trusted either.
    let rig = Rig::admin();
    rig.secrets.delete("admin_key_for".into()).unwrap();
    assert!(!rig.core.admin_access().unwrap().connected);
    assert_eq!(rig.stored_key(), None);
}

#[test]
fn a_key_with_a_character_a_header_cannot_carry_is_refused_not_a_panic() {
    let rig = Rig::new();
    rig.server.reset_request_counts();
    for bad in ["API_12\u{1}34", "API_12\n34", "API_12\u{7f}34", "API_ 1234", "API_ü1234"] {
        assert_eq!(invalid(rig.core.admin_connect(bad.into())), "this is not an API key", "{bad:?}");
    }
    assert_eq!(rig.stored_key(), None);
    assert_eq!(rig.server.request_count("/jmap/session"), 0, "refused before any connection");
}

#[test]
fn a_key_the_server_turned_down_is_not_sent_again() {
    let rig = Rig::new();
    let key = rig.connect_with(None);
    rig.server.refuse_admin_key(&key);
    assert!(rig.core.admin_access().unwrap().key_refused);
    rig.server.reset_request_counts();
    // Settings and the admin home ask again and again: nothing goes to the server any more.
    for _ in 0..3 {
        assert!(rig.core.admin_access().unwrap().key_refused);
        assert!(invalid(rig.core.admin_summary()).contains("did not accept"));
    }
    assert_eq!((rig.server.request_count("/api/account"), rig.server.request_count("/jmap/session")), (0, 0));
    // A new key ends it.
    rig.connect_with(None);
    assert!(rig.core.admin_access().unwrap().connected);
    // Forgetting the refused key clears the mark too.
    let key = rig.connect_with(None);
    rig.server.refuse_admin_key(&key);
    assert!(rig.core.admin_access().unwrap().key_refused);
    rig.core.admin_disconnect().unwrap();
    assert_eq!(rig.secrets.get("admin_key_refused".into()).unwrap(), None);
}

#[test]
fn a_key_the_server_answers_with_403_is_refused_too_and_kept() {
    let rig = Rig::new();
    let key = rig.connect_with(None);
    rig.server.forbid_admin_key(&key);
    let access = rig.core.admin_access().unwrap();
    assert!(!access.connected && access.key_refused);
    // It can pass (another address, an account switched on again): the key is kept, and tried again.
    assert_eq!(rig.stored_key(), Some(key.clone()));
    let msg = invalid(rig.core.admin_queue(0, 10));
    assert!(msg.contains("does not let this key in"), "{msg}");
    assert!(!msg.contains(&key));
    // A key that is pasted against a 403 is refused in the same words.
    let other = rig.server.admin_key(USERNAME, None);
    rig.server.forbid_admin_key(&other);
    assert!(invalid(rig.core.admin_connect(other)).contains("does not let this key in"));
}

#[test]
fn connecting_needs_a_signed_in_account() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let server = rt.block_on(MockServer::start());
    let dir = tempfile::tempdir().unwrap();
    let secrets = Arc::new(MemorySecrets::default());
    let core = Core::new(dir.path().join("c.db").to_string_lossy().into_owned(), secrets.clone()).unwrap();
    let key = server.admin_key(USERNAME, None);
    assert!(matches!(core.admin_connect(key), Err(Error::SignInNeeded { .. })));
    assert_eq!(secrets.get(KEY_NAME.into()).unwrap(), None);
}

#[test]
fn a_function_the_key_may_not_use_says_so_in_plain_words() {
    let rig = Rig::new();
    rig.connect_with(Some(&["sysLogQuery", "sysLogGet"]));
    assert_eq!(invalid(rig.core.admin_queue(0, 10)), "this key may not see the mail queue");
    assert_eq!(invalid(rig.core.admin_accounts(String::new(), 0, 10)), "this key may not see the accounts");
    assert_eq!(invalid(rig.core.admin_blocked(0, 10)), "this key may not see the blocked addresses");
    assert_eq!(invalid(rig.core.admin_maintenance(Maintenance::ReloadSettings)), "this key may not run that action");
    assert!(rig.core.admin_logs(String::new(), 10).is_ok());
}

// ---- the summary ----

#[test]
fn the_summary_has_the_figures_the_key_may_read() {
    let rig = Rig::admin();
    rig.server.queue_add(queued(
        "a@example.test",
        "2027-01-15T08:00:00Z",
        json!({ "x@y.test": recipient("Scheduled", json!({})) }),
    ));
    rig.server.block_add("10.0.0.1", "authFailure", NOW, None);
    rig.server.block_add("10.0.0.2", "manual", NOW, None);
    let summary = rig.core.admin_summary().unwrap();
    assert_eq!((summary.queue_waiting, summary.accounts, summary.blocked), (Some(1), Some(3), Some(2)));

    let rig = Rig::new();
    rig.connect_with(Some(&["sysQueuedMessageQuery", "sysQueuedMessageGet"]));
    let summary = rig.core.admin_summary().unwrap();
    assert_eq!((summary.queue_waiting, summary.accounts, summary.blocked), (Some(0), None, None));
}

// ---- the queue ----

#[test]
fn the_queue_has_the_next_message_to_be_tried_first() {
    let rig = Rig::admin();
    let late = rig.server.queue_add(queued(
        "late@example.test",
        "2027-01-16T00:00:00Z",
        json!({ "a@b.test": recipient("Scheduled", json!({})) }),
    ));
    let soon = rig.server.queue_add(queued(
        "soon@example.test",
        "2027-01-15T08:00:00Z",
        json!({ "a@b.test": recipient("Scheduled", json!({})) }),
    ));
    let mid = rig.server.queue_add(queued(
        "mid@example.test",
        "2027-01-15T20:00:00Z",
        json!({ "a@b.test": recipient("Scheduled", json!({})) }),
    ));
    let page = rig.core.admin_queue(0, 10).unwrap();
    assert_eq!(page.total, 3);
    let ids: Vec<&str> = page.messages.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, [soon.as_str(), mid.as_str(), late.as_str()]);
    // A page from an offset, and the total of all.
    let page = rig.core.admin_queue(1, 1).unwrap();
    assert_eq!((page.messages.len(), page.total, page.messages[0].id.as_str()), (1, 3, mid.as_str()));
}

#[test]
fn a_page_of_the_queue_holds_at_most_a_hundred() {
    let rig = Rig::admin();
    for n in 0..105 {
        rig.server.queue_add(queued("a@example.test", &format!("2027-02-01T00:{:02}:00Z", n % 60), json!({})));
    }
    let page = rig.core.admin_queue(0, 500).unwrap();
    assert_eq!((page.messages.len(), page.total), (100, 105));
}

#[test]
fn a_queued_message_says_who_it_is_for_and_what_went_wrong() {
    let rig = Rig::admin();
    let id = rig.server.queue_add(queued(
        "alice@example.test",
        "2027-01-15T08:00:00Z",
        json!({
            "done@far.test": recipient("Completed", json!({ "responseMessage": "2.0.0 Ok: queued", "responseCode": 250 })),
            "later@far.test": {
                "queueName": "remote", "retryCount": 3, "retryDue": "2027-01-15T09:00:00Z",
                "status": { "@type": "TemporaryFailure", "errorType": "connectionError", "errorMessage": "connection refused" },
            },
            "wait@far.test": recipient("Scheduled", json!({})),
        }),
    ));
    let m = rig.core.admin_queued_message(id.clone()).unwrap();
    assert_eq!((m.id.as_str(), m.from.as_str(), m.size, m.tries), (id.as_str(), "alice@example.test", 1234, 3));
    let by: Vec<(&str, &str)> = m.recipients.iter().map(|r| (r.address.as_str(), r.status.as_str())).collect();
    assert_eq!(
        by,
        [("done@far.test", "delivered"), ("later@far.test", "failed for now"), ("wait@far.test", "waiting")]
    );
    assert_eq!(m.recipients[0].detail, "2.0.0 Ok: queued");
    assert!(
        m.recipients[1].detail.contains("could not be reached")
            && m.recipients[1].detail.contains("connection refused")
    );
    assert_eq!(m.error, m.recipients[1].detail);
    assert!(m.next_try_at.is_some() && m.queued_at.is_some());
    assert!(m.next_try_at > m.queued_at);
    // A permanent failure is the error before a temporary one.
    let id = rig.server.queue_add(queued(
        "",
        "2027-01-15T08:00:00Z",
        json!({
            "a@far.test": recipient("TemporaryFailure", json!({ "errorType": "dnsError" })),
            "b@far.test": recipient("PermanentFailure", json!({ "errorType": "unexpectedResponse", "responseCode": 550, "responseMessage": "no such user" })),
        }),
    ));
    let m = rig.core.admin_queued_message(id).unwrap();
    assert_eq!(m.error, "the other server refused it: 550 no such user");
    assert_eq!(m.from, "");
}

#[test]
fn a_message_that_is_not_in_the_queue_is_invalid() {
    let rig = Rig::admin();
    assert!(invalid(rig.core.admin_queued_message("nope".into())).contains("no longer in the queue"));
    assert!(invalid(rig.core.admin_queue_retry("nope".into())).contains("no longer in the queue"));
}

#[test]
fn a_retry_makes_the_message_due_now() {
    let rig = Rig::admin();
    let id = rig.server.queue_add(queued(
        "a@example.test",
        "2999-01-01T00:00:00Z",
        json!({ "a@b.test": recipient("Scheduled", json!({})) }),
    ));
    rig.core.admin_queue_retry(id.clone()).unwrap();
    let held = rig.server.queue_message(&id).unwrap();
    assert!(held["nextRetry"].as_str().unwrap() < "2020", "{held}");
    // It stays in the queue: the server delivers it.
    assert_eq!(rig.server.queue_ids(), [id]);
}

#[test]
fn a_cancel_takes_the_message_out_and_one_that_is_gone_is_not_an_error() {
    let rig = Rig::admin();
    let id = rig.server.queue_add(queued("a@example.test", "2027-01-15T08:00:00Z", json!({})));
    let keep = rig.server.queue_add(queued("b@example.test", "2027-01-15T09:00:00Z", json!({})));
    rig.core.admin_queue_cancel(id.clone()).unwrap();
    assert_eq!(rig.server.queue_ids(), [keep]);
    rig.core.admin_queue_cancel(id).unwrap();
}

#[test]
fn a_key_that_may_only_read_the_queue_cannot_cancel() {
    let rig = Rig::new();
    rig.connect_with(Some(&["sysQueuedMessageQuery", "sysQueuedMessageGet"]));
    let id = rig.server.queue_add(queued("a@example.test", "2027-01-15T08:00:00Z", json!({})));
    assert_eq!(invalid(rig.core.admin_queue_cancel(id.clone())), "this key may not change the mail queue");
    assert_eq!(invalid(rig.core.admin_queue_retry(id.clone())), "this key may not change the mail queue");
    assert_eq!(rig.server.queue_ids(), [id]);
}

// ---- accounts ----

#[test]
fn accounts_and_groups_come_by_name_with_what_the_server_knows() {
    let rig = Rig::admin();
    let page = rig.core.admin_accounts(String::new(), 0, 50).unwrap();
    assert_eq!(page.total, 3);
    let names: Vec<&str> = page.accounts.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(names, ["alice", "bob", "staff"]);
    let (alice, bob, staff) = (&page.accounts[0], &page.accounts[1], &page.accounts[2]);
    assert_eq!(
        (alice.id.as_str(), alice.email.as_str(), alice.kind, alice.is_admin),
        (ACCOUNT_ID, "alice@example.test", ServerAccountKind::User, true)
    );
    assert_eq!(alice.used, Some(2048));
    assert_eq!(alice.quota, None);
    assert!(!alice.is_suspended);
    assert_eq!(
        (bob.id.as_str(), bob.kind, bob.is_admin, bob.description.as_str()),
        (BOB_ID, ServerAccountKind::User, false, "Bob Builder")
    );
    assert_eq!(
        (bob.quota, bob.aliases.clone(), bob.groups.clone()),
        (Some(1_000_000), vec!["bobby@other.test".to_string()], vec!["staff".to_string()])
    );
    assert_eq!(
        (staff.id.as_str(), staff.kind, staff.is_admin, staff.used),
        (GROUP_ID, ServerAccountKind::Group, false, None)
    );
    // One account, by id.
    let one = rig.core.admin_account(BOB_ID.into()).unwrap();
    assert_eq!(&one, bob);
    assert!(invalid(rig.core.admin_account("nope".into())).contains("no longer on the server"));
}

#[test]
fn a_text_finds_accounts_by_part_of_a_name_address_or_description_without_regard_to_case() {
    let rig = Rig::admin();
    let find = |text: &str| -> Vec<String> {
        rig.core.admin_accounts(text.into(), 0, 50).unwrap().accounts.into_iter().map(|a| a.name).collect()
    };
    assert_eq!(find("BOB"), ["bob"]);
    // The server alone would not find a part of a word or of an address.
    assert_eq!(find("bui"), ["bob"]);
    assert_eq!(find("bob@exam"), ["bob"]);
    assert_eq!(find("example.test"), ["alice", "bob", "staff"]);
    assert_eq!(find("all staff"), ["staff"]);
    assert_eq!(find("builder staff"), Vec::<String>::new());
    assert_eq!(find("   "), ["alice", "bob", "staff"]);
    let page = rig.core.admin_accounts("example".into(), 1, 1).unwrap();
    assert_eq!((page.accounts.len(), page.total, page.accounts[0].name.as_str()), (1, 3, "bob"));
}

#[test]
fn a_page_of_accounts_holds_at_most_a_hundred() {
    let rig = Rig::admin();
    rig.server.add_users(130);
    let page = rig.core.admin_accounts(String::new(), 0, 500).unwrap();
    assert_eq!((page.accounts.len(), page.total), (100, 133));
    let rest = rig.core.admin_accounts(String::new(), 100, 100).unwrap();
    assert_eq!(rest.accounts.len(), 33);
    let found = rig.core.admin_accounts("Bulk".into(), 0, 500).unwrap();
    assert_eq!((found.accounts.len(), found.total), (100, 130));
    assert_eq!(found.accounts[0].name, "user000");
}

#[test]
fn the_domains_are_by_name() {
    let rig = Rig::admin();
    let domains = rig.core.admin_domains().unwrap();
    let list: Vec<(&str, &str)> = domains.iter().map(|d| (d.id.as_str(), d.name.as_str())).collect();
    assert_eq!(list, [(DOMAIN_ID, "example.test"), (OTHER_DOMAIN_ID, "other.test")]);
}

#[test]
fn a_user_is_made_with_a_name_a_domain_a_description_and_a_password() {
    let rig = Rig::admin();
    let made = rig
        .core
        .admin_create_account("ann.lee-2_x".into(), OTHER_DOMAIN_ID.into(), " Ann Lee ".into(), "longenough".into())
        .unwrap();
    assert_eq!(
        (made.name.as_str(), made.email.as_str(), made.description.as_str()),
        ("ann.lee-2_x", "ann.lee-2_x@other.test", "Ann Lee")
    );
    assert_eq!((made.kind, made.is_admin, made.is_suspended), (ServerAccountKind::User, false, false));
    let held = rig.server.server_account(&made.id).unwrap();
    assert_eq!(held["credentials"]["0"]["secret"], "longenough");
    assert_eq!(held["roles"]["@type"], "User");
    assert_eq!(rig.core.admin_accounts(String::new(), 0, 50).unwrap().total, 4);
}

#[test]
fn a_name_the_app_does_not_allow_is_refused_before_the_server_is_asked() {
    let rig = Rig::admin();
    for bad in ["", "a b", "a@b", "Bad Name", "ünï", &"x".repeat(65)] {
        let msg =
            invalid(rig.core.admin_create_account(bad.into(), DOMAIN_ID.into(), String::new(), "longenough".into()));
        assert!(msg.contains("letters, digits"), "{bad:?}: {msg}");
    }
    assert_eq!(rig.calls("x:Account/set"), 0);
    assert_eq!(rig.server.server_account_count(), 3);
}

#[test]
fn a_short_password_is_refused_without_being_shown() {
    let rig = Rig::admin();
    let msg = invalid(rig.core.admin_create_account("ann".into(), DOMAIN_ID.into(), String::new(), "short12".into()));
    assert!(msg.contains("at least 8 characters") && !msg.contains("short12"), "{msg}");
    let msg = invalid(rig.core.admin_reset_password(BOB_ID.into(), "short12".into()));
    assert!(msg.contains("at least 8 characters") && !msg.contains("short12"), "{msg}");
    assert_eq!(rig.calls("x:Account/set"), 0);
}

#[test]
fn a_name_that_is_taken_or_a_domain_that_is_unknown_is_invalid() {
    let rig = Rig::admin();
    let msg =
        invalid(rig.core.admin_create_account("bob".into(), DOMAIN_ID.into(), String::new(), "longenough".into()));
    assert!(msg.contains("already taken"), "{msg}");
    let msg =
        invalid(rig.core.admin_create_account("zed".into(), "nodomain".into(), String::new(), "longenough".into()));
    assert!(msg.contains("no such domain"), "{msg}");
    assert_eq!(rig.server.server_account_count(), 3);
    // The same name in another domain is another address.
    rig.core.admin_create_account("bob".into(), OTHER_DOMAIN_ID.into(), String::new(), "longenough".into()).unwrap();
}

#[test]
fn the_description_and_the_storage_limit_are_changed_and_the_limit_can_be_taken_away() {
    let rig = Rig::admin();
    let changed = rig.core.admin_update_account(BOB_ID.into(), "  Robert  ".into(), Some(5_000_000)).unwrap();
    assert_eq!((changed.description.as_str(), changed.quota), ("Robert", Some(5_000_000)));
    let held = rig.server.server_account(BOB_ID).unwrap();
    assert_eq!(held["quotas"]["maxDiskQuota"], 5_000_000);
    // What the app does not own stays.
    assert_eq!(held["memberGroupIds"][GROUP_ID], true);
    assert_eq!(held["aliases"]["0"]["name"], "bobby");
    let cleared = rig.core.admin_update_account(BOB_ID.into(), String::new(), None).unwrap();
    assert_eq!((cleared.description.as_str(), cleared.quota), ("", None));
    assert!(rig.server.server_account(BOB_ID).unwrap()["description"].is_null());
    assert!(rig.server.server_account(BOB_ID).unwrap()["quotas"].get("maxDiskQuota").is_none());
    assert!(invalid(rig.core.admin_update_account("nope".into(), "x".into(), None)).contains("no longer on the server"));
}

#[test]
fn a_new_password_is_set_and_is_never_in_an_error() {
    let rig = Rig::admin();
    rig.core.admin_reset_password(BOB_ID.into(), "brand-new-pass".into()).unwrap();
    assert_eq!(rig.server.server_account(BOB_ID).unwrap()["credentials"]["0"]["secret"], "brand-new-pass");
    let msg = invalid(rig.core.admin_reset_password(GROUP_ID.into(), "brand-new-pass".into()));
    assert!(msg.contains("no password") && !msg.contains("brand-new-pass"), "{msg}");
    let msg = invalid(rig.core.admin_reset_password("nope".into(), "brand-new-pass".into()));
    assert!(msg.contains("no longer on the server") && !msg.contains("brand-new-pass"), "{msg}");
}

#[test]
fn signing_in_is_switched_off_and_on_by_the_authenticate_permission() {
    let rig = Rig::admin();
    rig.core.admin_suspend_account(BOB_ID.into(), true).unwrap();
    assert!(rig.core.admin_account(BOB_ID.into()).unwrap().is_suspended);
    assert_eq!(rig.server.server_account(BOB_ID).unwrap()["permissions"]["disabledPermissions"]["authenticate"], true);
    // Suspending twice changes nothing more.
    rig.core.admin_suspend_account(BOB_ID.into(), true).unwrap();
    rig.core.admin_suspend_account(BOB_ID.into(), false).unwrap();
    assert!(!rig.core.admin_account(BOB_ID.into()).unwrap().is_suspended);
    assert_eq!(rig.server.server_account(BOB_ID).unwrap()["permissions"]["@type"], "Inherit");
    // Switching on one that never was off is no change.
    rig.core.admin_suspend_account(BOB_ID.into(), false).unwrap();
    assert_eq!(rig.server.server_account(BOB_ID).unwrap()["permissions"]["@type"], "Inherit");
}

#[test]
fn other_permissions_of_an_account_stay_when_it_is_suspended_and_switched_on_again() {
    let rig = Rig::admin();
    // Bob has a list of his own: one thing is disabled besides signing in.
    let own = json!({ "@type": "Merge", "enabledPermissions": {}, "disabledPermissions": { "emailSend": true } });
    rig.server.set_server_account_field(BOB_ID, "permissions", own);
    rig.core.admin_suspend_account(BOB_ID.into(), true).unwrap();
    let held = rig.server.server_account(BOB_ID).unwrap();
    assert_eq!(held["permissions"]["disabledPermissions"], json!({ "emailSend": true, "authenticate": true }));
    assert!(rig.core.admin_account(BOB_ID.into()).unwrap().is_suspended);
    rig.core.admin_suspend_account(BOB_ID.into(), false).unwrap();
    let held = rig.server.server_account(BOB_ID).unwrap();
    assert_eq!(held["permissions"]["@type"], "Merge");
    assert_eq!(held["permissions"]["disabledPermissions"], json!({ "emailSend": true }));
    assert!(!rig.core.admin_account(BOB_ID.into()).unwrap().is_suspended);

    // A list that replaces the inherited ones has signing in among what is enabled.
    let own = json!({ "@type": "Replace", "enabledPermissions": { "authenticate": true, "emailSend": true }, "disabledPermissions": {} });
    rig.server.set_server_account_field(BOB_ID, "permissions", own);
    rig.core.admin_suspend_account(BOB_ID.into(), true).unwrap();
    assert!(rig.core.admin_account(BOB_ID.into()).unwrap().is_suspended);
    assert_eq!(
        rig.server.server_account(BOB_ID).unwrap()["permissions"]["enabledPermissions"],
        json!({ "emailSend": true })
    );
    rig.core.admin_suspend_account(BOB_ID.into(), false).unwrap();
    assert!(!rig.core.admin_account(BOB_ID.into()).unwrap().is_suspended);
    assert_eq!(rig.server.server_account(BOB_ID).unwrap()["permissions"]["enabledPermissions"]["emailSend"], true);
}

#[test]
fn the_signed_in_account_cannot_be_suspended_or_deleted_and_the_server_is_not_asked() {
    let rig = Rig::admin();
    let msg = invalid(rig.core.admin_suspend_account(ACCOUNT_ID.into(), true));
    assert!(msg.contains("signed in with cannot be suspended"), "{msg}");
    let msg = invalid(rig.core.admin_delete_account(ACCOUNT_ID.into()));
    assert!(msg.contains("signed in with cannot be deleted"), "{msg}");
    assert_eq!(rig.account_calls(), 0);
    assert!(rig.server.server_account(ACCOUNT_ID).is_some());
    // Switching it on is harmless.
    rig.core.admin_suspend_account(ACCOUNT_ID.into(), false).unwrap();
}

#[test]
fn an_account_is_deleted_and_a_group_with_members_is_refused_saying_so() {
    let rig = Rig::admin();
    let msg = invalid(rig.core.admin_delete_account(GROUP_ID.into()));
    assert!(msg.contains("still has members"), "{msg}");
    assert!(rig.server.server_account(GROUP_ID).is_some());
    rig.core.admin_delete_account(BOB_ID.into()).unwrap();
    assert!(rig.server.server_account(BOB_ID).is_none());
    // The group has no members now.
    assert_eq!(rig.server.server_account(GROUP_ID).unwrap()["@type"], "Group");
    rig.core.admin_delete_account(GROUP_ID.into()).unwrap();
    assert_eq!(rig.server.server_account_count(), 1);
    assert!(invalid(rig.core.admin_delete_account(BOB_ID.into())).contains("no longer on the server"));
}

#[test]
fn a_key_without_the_right_cannot_change_accounts() {
    let rig = Rig::new();
    rig.connect_with(Some(&["sysAccountQuery", "sysAccountGet", "sysDomainGet"]));
    assert_eq!(rig.core.admin_accounts(String::new(), 0, 10).unwrap().total, 3);
    assert_eq!(invalid(rig.core.admin_delete_account(BOB_ID.into())), "this key may not delete accounts");
    assert_eq!(
        invalid(rig.core.admin_update_account(BOB_ID.into(), "x".into(), None)),
        "this key may not change accounts"
    );
    let msg =
        invalid(rig.core.admin_create_account("zed".into(), DOMAIN_ID.into(), String::new(), "longenough".into()));
    assert_eq!(msg, "this key may not make accounts");
}

#[test]
fn accounts_can_be_read_by_a_key_that_may_not_read_domains() {
    let rig = Rig::new();
    rig.connect_with(Some(&["sysAccountQuery", "sysAccountGet", "sysAccountUpdate", "sysAccountCreate"]));
    assert!(rig.core.admin_access().unwrap().may_accounts);
    let page = rig.core.admin_accounts(String::new(), 0, 10).unwrap();
    assert_eq!(page.total, 3);
    let bob = page.accounts.iter().find(|a| a.id == BOB_ID).unwrap();
    assert!(bob.aliases.is_empty(), "an alias needs its domain's name: left out, not an error");
    assert_eq!(rig.core.admin_account(BOB_ID.into()).unwrap().name, "bob");
    // A change that succeeds is not reported as an error by its re-read.
    let changed = rig.core.admin_update_account(BOB_ID.into(), "New words".into(), None).unwrap();
    assert_eq!(changed.description, "New words");
    // The domains themselves still need their own right.
    assert_eq!(invalid(rig.core.admin_domains()), "this key may not see the domains");
}

#[test]
fn a_password_the_server_turns_down_is_told_in_the_servers_terms_not_as_eight_characters() {
    let rig = Rig::admin();
    rig.server.set_password_rules(Some(8), Some(20), &["password"]);
    let ask = |password: &str| invalid(rig.core.admin_reset_password(BOB_ID.into(), password.into()));
    let weak = ask("password1234");
    assert!(weak.contains("too weak") && !weak.contains("password1234"), "{weak}");
    let long = ask(&"x".repeat(25));
    assert!(long.contains("too long") && long.contains("20"), "{long}");
    assert!(!long.contains(&"x".repeat(25)));
    // A minimum that is not 8, on the way in through a create as well.
    rig.server.set_password_rules(Some(12), None, &[]);
    let msg = invalid(rig.core.admin_create_account(
        "ann".into(),
        DOMAIN_ID.into(),
        String::new(),
        "tenletters1".to_string(),
    ));
    assert!(msg.contains("at least 12"), "{msg}");
    assert!(rig.core.admin_reset_password(BOB_ID.into(), "twelve-chars!".into()).is_ok());
}

#[test]
fn a_refusal_that_is_not_about_the_keys_rights_does_not_send_the_admin_to_make_a_new_key() {
    let rig = Rig::admin();
    rig.server.set_account_external(BOB_ID);
    let msg = invalid(rig.core.admin_reset_password(BOB_ID.into(), "longenough1".into()));
    assert!(msg.contains("outside directory") && !msg.contains("this key may not"), "{msg}");
    rig.server.set_account_limit(3);
    let msg =
        invalid(rig.core.admin_create_account("zed".into(), DOMAIN_ID.into(), String::new(), "longenough1".into()));
    assert!(msg.contains("limit of accounts") && !msg.contains("this key may not"), "{msg}");
}

// ---- blocked addresses ----

#[test]
fn blocked_addresses_come_newest_first_with_the_reason() {
    let rig = Rig::admin();
    let old = rig.server.block_add("10.0.0.1", "authFailure", NOW - 3600, None);
    let new = rig.server.block_add("10.0.0.2", "manual", NOW, Some(NOW + 600));
    let mid = rig.server.block_add("2001:db8::/32", "portScanning", NOW - 60, None);
    let list = rig.core.admin_blocked(0, 10).unwrap();
    let ids: Vec<&str> = list.iter().map(|b| b.id.as_str()).collect();
    assert_eq!(ids, [new.as_str(), mid.as_str(), old.as_str()]);
    assert_eq!(list[0].address, "10.0.0.2");
    assert_eq!((list[0].blocked_at, list[0].expires_at), (Some(NOW), Some(NOW + 600)));
    assert_eq!(list[1].address, "2001:db8::/32");
    assert_eq!(list[2].reason, "Excessive failed authentication attempts");
    assert_eq!(list[2].expires_at, None);
    let page = rig.core.admin_blocked(1, 1).unwrap();
    assert_eq!((page.len(), page[0].id.as_str()), (1, mid.as_str()));
}

#[test]
fn an_address_is_unblocked_and_one_that_is_gone_is_not_an_error() {
    let rig = Rig::admin();
    let a = rig.server.block_add("10.0.0.1", "authFailure", NOW, None);
    let b = rig.server.block_add("10.0.0.2", "manual", NOW, None);
    rig.core.admin_unblock(a.clone()).unwrap();
    assert_eq!(rig.server.blocked_ids(), [b]);
    rig.core.admin_unblock(a).unwrap();
}

// ---- logs ----

#[test]
fn the_log_has_the_newest_lines_first_even_when_the_server_gives_them_by_file() {
    let rig = Rig::admin();
    // The server's order is by file, not by time: an old file can come after a newer one.
    rig.server.log_add(NOW - 100, "info", "server.start", "version = 1");
    rig.server.log_add(NOW, "warn", "auth.failed", "remoteIp = 10.0.0.1");
    rig.server.log_add(NOW - 50, "error", "store.error", "disk full");
    rig.server.log_add(NOW - 200, "debug", "network.listen", "port = 25");
    let lines = rig.core.admin_logs(String::new(), 10).unwrap();
    let at: Vec<Option<i64>> = lines.iter().map(|l| l.at).collect();
    assert_eq!(at, [Some(NOW), Some(NOW - 50), Some(NOW - 100), Some(NOW - 200)]);
    assert_eq!(lines[0].level, "warning");
    assert_eq!(lines[0].text, "auth.failed remoteIp = 10.0.0.1");
    assert_eq!(lines[1].level, "error");
    assert_eq!(lines[3].level, "debug");
    assert_eq!(rig.core.admin_logs(String::new(), 2).unwrap().len(), 2);
}

#[test]
fn the_log_is_searched_by_a_text_and_holds_at_most_two_hundred_lines() {
    let rig = Rig::admin();
    for n in 0..250 {
        rig.server.log_add(NOW - n, "info", "smtp.connect", &format!("remoteIp = 10.0.{}.{}", n / 100, n % 100));
    }
    rig.server.log_add(NOW + 5, "warn", "auth.failed", "accountName = \"mallory\"");
    let found = rig.core.admin_logs("mallory".into(), 10).unwrap();
    assert_eq!(found.len(), 1);
    assert!(found[0].text.contains("mallory"));
    assert_eq!(rig.core.admin_logs(String::new(), 1000).unwrap().len(), 200);
    assert_eq!(rig.core.admin_logs("   ".into(), 3).unwrap()[0].at, Some(NOW + 5));
    assert!(rig.core.admin_logs("no such text".into(), 10).unwrap().is_empty());
}

// ---- maintenance ----

#[test]
fn a_maintenance_action_is_started_on_the_server() {
    let rig = Rig::admin();
    for what in [
        Maintenance::ReloadSettings,
        Maintenance::ReloadCertificates,
        Maintenance::PauseQueue,
        Maintenance::ResumeQueue,
    ] {
        rig.core.admin_maintenance(what).unwrap();
    }
    assert_eq!(
        rig.server.admin_actions(),
        ["ReloadSettings", "ReloadTlsCertificates", "PauseMtaQueue", "ResumeMtaQueue"]
    );
}

#[test]
fn a_key_without_the_action_permission_cannot_start_it() {
    let rig = Rig::new();
    rig.connect_with(Some(&["sysActionCreate", "actionPauseMtaQueue"]));
    rig.core.admin_maintenance(Maintenance::PauseQueue).unwrap();
    assert_eq!(invalid(rig.core.admin_maintenance(Maintenance::ReloadSettings)), "this key may not run that action");
    assert_eq!(rig.server.admin_actions(), ["PauseMtaQueue"]);
}

#[test]
fn nothing_is_cached_between_calls() {
    let rig = Rig::admin();
    assert_eq!(rig.core.admin_blocked(0, 10).unwrap().len(), 0);
    rig.server.block_add("10.0.0.1", "manual", NOW, None);
    assert_eq!(rig.core.admin_blocked(0, 10).unwrap().len(), 1);
    assert_eq!(rig.calls("x:BlockedIp/query"), 2);
}
