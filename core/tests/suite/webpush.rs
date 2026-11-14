//! Push through Google against the mock server: the subscription's life (prepare, subscribe,
//! verification, renew, drop, forget), what a received message does, and what is kept where.
//!
//! The mock "sends" the server's messages into a list, encrypted the way Stalwart does it; a
//! test hands them to `web_push_receive` the way Play services would hand them to the app.

use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use crate::support::{Delivery, MockServer, ACCOUNT_ID, INBOX_ID};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use kweb_core::{
    ffi::{
        Core, Shade, WebPushMessage, WebPushRegistration, WebPushState, WorkWaiting, EMAIL_PUSH_PROPERTIES, STATE_KEY,
        TYPES,
    },
    model::{Address, ConnectionStatus, Reminder},
    notify::Plan,
    push::Listener,
    secrets::{MemorySecrets, SecretStore},
    store::Store,
    Error,
};
use serde_json::{json, Value};
use tokio::runtime::Runtime;

const TOKEN: &str = "c8Fx-token_123:APA91bHsecrettoken.value~";
const SECOND_TOKEN: &str = "second-token-456:APA91bOther";

struct Rig {
    // Field order is drop order: the core goes first, the mock and its runtime last.
    core: Arc<Core>,
    server: MockServer,
    secrets: Arc<MemorySecrets>,
    _rt: Runtime,
    db_path: String,
    _dir: tempfile::TempDir,
}

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64
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
        Rig { core, server, secrets, _rt: rt, db_path, _dir: dir }
    }

    /// Signed in, and the first pass done (the seeded mail is cached and counts as old).
    fn synced() -> Rig {
        let rig = Rig::new();
        rig.core.check_once(Shade::Unknown).unwrap();
        rig
    }

    /// Prepared, subscribed and verified: the state is `Registered`.
    fn registered() -> Rig {
        let rig = Rig::synced();
        rig.register(TOKEN);
        let got = rig.feed(rig.server.webpush_take_deliveries());
        assert_eq!(got, [WebPushMessage::Verified]);
        rig
    }

    fn store(&self) -> Store {
        Store::open(std::path::Path::new(&self.db_path)).unwrap()
    }

    fn record(&self) -> Value {
        let text = self.store().kv_get(ACCOUNT_ID, STATE_KEY).unwrap();
        text.map_or(Value::Null, |t| serde_json::from_str(&t).unwrap())
    }

    fn edit_record(&self, f: impl FnOnce(&mut Value)) {
        let mut record = self.record();
        f(&mut record);
        self.store().kv_set(ACCOUNT_ID, STATE_KEY, &record.to_string()).unwrap();
    }

    /// The time of the last create is a minute and more ago.
    fn age_the_create(&self) {
        self.edit_record(|r| r["last_create_at"] = json!(now() - 120));
    }

    fn register(&self, token: &str) -> WebPushRegistration {
        let registration = self.core.web_push_prepare().unwrap().expect("the server has a key");
        let state = self.core.web_push_subscribe(token.into()).unwrap();
        assert!(matches!(state, WebPushState::Verifying { .. }), "{state:?}");
        registration
    }

    fn feed(&self, deliveries: Vec<Delivery>) -> Vec<WebPushMessage> {
        deliveries.into_iter().map(|d| self.core.web_push_receive(d.body, Shade::Unknown).unwrap()).collect()
    }

    fn calls(&self, method: &str) -> usize {
        self.server.request_count(method)
    }

    fn secret_texts(&self) -> Vec<String> {
        ["web_push_private_key", "web_push_auth"]
            .iter()
            .filter_map(|k| self.secrets.get(k.to_string()).unwrap())
            .collect()
    }
}

// ---- support for what the pass reports --------------------------------------------------

#[derive(Default)]
struct Recorder {
    plans: Mutex<Vec<Plan>>,
}

impl Listener for Recorder {
    fn status(&self, _: ConnectionStatus) {}
    fn notifications(&self, plan: Plan) {
        self.plans.lock().unwrap().push(plan);
    }
    fn cache_changed(&self) {}
    fn reminder(&self, _: Reminder) {}
}

fn posted(plan: &Plan) -> Vec<String> {
    plan.post.iter().map(|n| n.message_id.clone()).collect()
}

fn text_of(e: &Error) -> String {
    format!("{e} {e:?}")
}

fn assert_clean(text: &str, secrets: &[&str]) {
    for secret in secrets {
        assert!(!text.contains(secret), "'{secret}' leaked into: {text}");
    }
}

// ---- support --------------------------------------------------------------------------------

#[test]
fn a_server_with_a_key_supports_it_and_one_without_does_not() {
    let rig = Rig::new();
    assert!(rig.core.web_push_supported().unwrap());
    rig.server.webpush(|p| p.key = None);
    assert!(!rig.core.web_push_supported().unwrap());
    rig.server.webpush(|p| p.key = Some("not a key".into()));
    assert!(!rig.core.web_push_supported().unwrap());
    // Padding is accepted and stripped.
    let key = rig.server.webpush(|p| p.server_key());
    rig.server.webpush(|p| p.key = Some(format!("{key}=")));
    assert!(rig.core.web_push_supported().unwrap());
    let sender = rig.core.web_push_prepare().unwrap().unwrap().sender;
    assert_eq!(sender, key, "the sender has no padding");
}

#[test]
fn a_server_that_is_down_is_temporary_and_a_signed_out_core_needs_a_sign_in() {
    let rig = Rig::new();
    rig.server.set_down(true);
    assert!(matches!(rig.core.web_push_supported(), Err(Error::Temporary { .. })));
    assert!(matches!(rig.core.web_push_prepare(), Err(Error::Temporary { .. })));
    let out =
        Core::new(rig._dir.path().join("other.db").to_string_lossy().into_owned(), Arc::new(MemorySecrets::default()))
            .unwrap();
    assert!(matches!(out.web_push_supported(), Err(Error::SignInNeeded { .. })));
    assert!(matches!(out.web_push_prepare(), Err(Error::SignInNeeded { .. })));
    assert!(matches!(out.web_push_subscribe(TOKEN.into()), Err(Error::SignInNeeded { .. })));
    assert!(matches!(out.web_push_renew(), Err(Error::SignInNeeded { .. })));
    assert!(matches!(out.web_push_drop(), Err(Error::SignInNeeded { .. })));
    assert_eq!(out.web_push_state(), WebPushState::Off);
    assert_eq!(out.work_waiting().unwrap(), WorkWaiting::Nothing);
    out.web_push_forget().unwrap();
}

// ---- prepare --------------------------------------------------------------------------------

#[test]
fn prepare_without_a_server_key_stores_nothing() {
    let rig = Rig::new();
    rig.server.webpush(|p| p.key = None);
    assert_eq!(rig.core.web_push_prepare().unwrap(), None);
    assert!(rig.secret_texts().is_empty());
    assert_eq!(rig.record(), Value::Null);
    assert_eq!(rig.core.web_push_state(), WebPushState::Off);
}

#[test]
fn prepare_makes_keys_once_and_the_subtype_stays() {
    let rig = Rig::new();
    let first = rig.core.web_push_prepare().unwrap().unwrap();
    let key = rig.server.webpush(|p| p.server_key());
    assert_eq!(first.sender, key);
    assert!(!first.sender.contains('='));
    let prefix = "wp:https://127.0.0.1/#";
    assert!(first.subtype.starts_with(prefix), "{}", first.subtype);
    let id = &first.subtype[prefix.len()..];
    assert_eq!(id.len(), 36);
    let secrets = rig.secret_texts();
    assert_eq!(secrets.len(), 2);

    let again = rig.core.web_push_prepare().unwrap().unwrap();
    assert_eq!(again, first);
    assert_eq!(rig.secret_texts(), secrets, "the keys are made once");
    assert_eq!(rig.core.web_push_state(), WebPushState::Off, "nothing is subscribed yet");
}

#[test]
fn a_secret_store_that_cannot_keep_the_keys_leaves_nothing_half_made() {
    struct Failing {
        inner: MemorySecrets,
        fail_on: &'static str,
    }
    impl SecretStore for Failing {
        fn get(&self, key: String) -> Result<Option<String>, kweb_core::secrets::SecretError> {
            self.inner.get(key)
        }
        fn set(&self, key: String, value: String) -> Result<(), kweb_core::secrets::SecretError> {
            if key == self.fail_on {
                return Err(kweb_core::secrets::SecretError::Failed { msg: "no space".into() });
            }
            self.inner.set(key, value)
        }
        fn delete(&self, key: String) -> Result<(), kweb_core::secrets::SecretError> {
            self.inner.delete(key)
        }
    }
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().unwrap();
    let server = rt.block_on(MockServer::start());
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(Failing { inner: MemorySecrets::default(), fail_on: "web_push_auth" });
    let core = Core::new(dir.path().join("c.db").to_string_lossy().into_owned(), store.clone()).unwrap();
    let url = core.begin_sign_in_with_redirect(server.url(), "ovh.kweb.mail:/oauth/callback".into()).unwrap();
    core.complete_sign_in(server.authorize(&url).unwrap()).unwrap();
    match core.web_push_prepare() {
        Err(Error::Store { msg }) => assert!(msg.contains("could not be kept"), "{msg}"),
        other => panic!("{other:?}"),
    }
    assert_eq!(store.inner.get("web_push_private_key".into()).unwrap(), None, "no half a pair is left");
    assert_eq!(store.inner.get("web_push_auth".into()).unwrap(), None);
}

// ---- subscribe ------------------------------------------------------------------------------

#[test]
fn subscribe_makes_the_subscription_the_experiment_showed_works() {
    let rig = Rig::synced();
    let registration = rig.register(TOKEN);
    let subs = rig.server.webpush_subscriptions();
    assert_eq!(subs.len(), 1);
    let sub = &subs[0];
    assert_eq!(sub.url, format!("https://fcm.googleapis.com/fcm/send/{TOKEN}"));
    let id = registration.subtype.rsplit('#').next().unwrap();
    assert_eq!(sub.device_client_id.as_deref(), Some(format!("kweb-{id}").as_str()));
    assert_eq!(sub.types, TYPES.map(String::from).to_vec());
    assert_eq!(sub.email_push, json!({ ACCOUNT_ID: { "properties": EMAIL_PUSH_PROPERTIES, "urgency": "high" } }));
    assert_eq!(sub.p256dh.len(), 65);
    assert_eq!(sub.auth.len(), 16);
    assert!(!sub.verified);
    assert!(sub.expires > now() + 6 * 24 * 3600 && sub.expires <= now() + 7 * 24 * 3600 + 5);
    assert!(matches!(rig.core.web_push_state(), WebPushState::Verifying { .. }));
    // The key the server was given belongs to the stored private key: a message encrypted for
    // it decrypts (see the verification below).
}

#[test]
fn a_token_that_cannot_be_right_is_refused_without_being_repeated() {
    let rig = Rig::synced();
    rig.core.web_push_prepare().unwrap();
    for bad in ["", "has space", "slash/slash", "new\nline", &"a".repeat(4097)] {
        match rig.core.web_push_subscribe(bad.into()) {
            Err(e @ Error::Invalid { .. }) => assert!(!text_of(&e).contains("slash/slash"), "{e}"),
            other => panic!("{bad:?}: {other:?}"),
        }
    }
    assert_eq!(rig.calls("PushSubscription/set"), 0);
    assert!(rig.server.webpush_subscriptions().is_empty());
}

#[test]
fn subscribe_without_prepare_is_invalid_and_after_forget_too() {
    let rig = Rig::synced();
    assert!(matches!(rig.core.web_push_subscribe(TOKEN.into()), Err(Error::Invalid { .. })));
    rig.core.web_push_prepare().unwrap();
    rig.core.web_push_forget().unwrap();
    assert!(matches!(rig.core.web_push_subscribe(TOKEN.into()), Err(Error::Invalid { .. })));
    assert_eq!(rig.calls("PushSubscription/set"), 0);
}

#[test]
fn the_verification_is_decrypted_written_back_and_accepted() {
    let rig = Rig::synced();
    rig.register(TOKEN);
    let deliveries = rig.server.webpush_take_deliveries();
    assert_eq!(deliveries.len(), 1);
    assert_eq!(deliveries[0].url, format!("https://fcm.googleapis.com/fcm/send/{TOKEN}"));
    // 86 bytes of header, the data in whole 128-byte steps, the tag.
    assert_eq!((deliveries[0].body.len() - 86 - 16) % 128, 0);
    assert_eq!(rig.feed(deliveries.clone()), [WebPushMessage::Verified]);
    assert!(rig.server.webpush_subscriptions()[0].verified);
    match rig.core.web_push_state() {
        WebPushState::Registered { expires, last_message_at } => {
            assert!(expires > now() + 6 * 24 * 3600);
            assert!(last_message_at.is_some_and(|t| (now() - t).abs() < 5));
        }
        other => panic!("{other:?}"),
    }
    // The same message again (a replay) is understood and does nothing.
    let sets = rig.calls("PushSubscription/set");
    assert_eq!(rig.feed(deliveries), [WebPushMessage::Ignored]);
    assert_eq!(rig.calls("PushSubscription/set"), sets, "nothing is written for a replay");
}

#[test]
fn a_verification_for_a_subscription_that_is_not_waited_for_is_ignored() {
    let rig = Rig::registered();
    // The server's verification for some other subscription id, encrypted for our keys.
    let id = rig.server.webpush_subscriptions()[0].id.clone();
    let other = json!({ "@type": "PushVerification", "pushSubscriptionId": "zz", "verificationCode": "x" });
    let body = rig.server.webpush(|p| p.encrypt_for(&id, other.to_string().as_bytes())).unwrap();
    assert_eq!(rig.core.web_push_receive(body, Shade::Unknown).unwrap(), WebPushMessage::Ignored);
}

#[test]
fn a_verification_that_could_not_be_written_is_kept_and_written_by_the_next_renew() {
    let rig = Rig::synced();
    rig.register(TOKEN);
    let deliveries = rig.server.webpush_take_deliveries();
    rig.server.webpush(|p| p.fail_next = 1);
    let e = rig.core.web_push_receive(deliveries[0].body.clone(), Shade::Unknown).unwrap_err();
    assert!(matches!(e, Error::Temporary { .. }), "{e}");
    assert!(matches!(rig.core.web_push_state(), WebPushState::Verifying { .. }));
    assert!(!rig.server.webpush_subscriptions()[0].verified);
    assert!(rig.record()["pending_code"].is_object());

    // Housekeeping goes to the server for it and the subscription is verified.
    let state = rig.core.web_push_renew().unwrap();
    assert!(matches!(state, WebPushState::Registered { .. }), "{state:?}");
    assert!(rig.server.webpush_subscriptions()[0].verified);
    assert!(rig.record()["pending_code"].is_null());
}

#[test]
fn a_code_the_server_does_not_accept_is_dropped_and_invalid() {
    let rig = Rig::synced();
    rig.register(TOKEN);
    let id = rig.server.webpush_subscriptions()[0].id.clone();
    let wrong = json!({ "@type": "PushVerification", "pushSubscriptionId": id, "verificationCode": "wrong-code" });
    let body = rig.server.webpush(|p| p.encrypt_for(&id, wrong.to_string().as_bytes())).unwrap();
    let e = rig.core.web_push_receive(body, Shade::Unknown).unwrap_err();
    assert!(matches!(e, Error::Invalid { .. }), "{e}");
    assert_clean(&text_of(&e), &["wrong-code"]);
    assert!(rig.record()["pending_code"].is_null());
    assert!(matches!(rig.core.web_push_state(), WebPushState::Verifying { .. }));
}

#[test]
fn a_verification_for_a_reused_id_does_not_verify_the_new_subscription() {
    // The server reuses ids. The old subscription's code, replayed after the id was given to a
    // new subscription, is the wrong code for it.
    let rig = Rig::synced();
    rig.register(TOKEN);
    let old = rig.server.webpush_take_deliveries();
    rig.age_the_create();
    rig.server.webpush(|p| p.set_verify_gap(Duration::ZERO));
    assert!(matches!(rig.core.web_push_subscribe(SECOND_TOKEN.into()).unwrap(), WebPushState::Verifying { .. }));
    let subs = rig.server.webpush_subscriptions();
    assert_eq!(subs.len(), 1);
    assert_eq!(subs[0].id, rig.server.webpush(|_| "s0".to_string()), "the id was reused");
    let e = rig.core.web_push_receive(old[0].body.clone(), Shade::Unknown);
    assert!(matches!(e, Err(Error::Invalid { .. })), "{e:?}");
    assert!(!rig.server.webpush_subscriptions()[0].verified);
    // The new one's own verification works.
    assert_eq!(rig.feed(rig.server.webpush_take_deliveries()), [WebPushMessage::Verified]);
}

#[test]
fn a_second_subscribe_inside_a_minute_is_refused_and_changes_nothing() {
    let rig = Rig::synced();
    rig.register(TOKEN);
    let sets = rig.calls("PushSubscription/set");
    match rig.core.web_push_subscribe(SECOND_TOKEN.into()) {
        Err(Error::Temporary { msg }) => assert!(msg.contains("seconds"), "{msg}"),
        other => panic!("{other:?}"),
    }
    assert_eq!(rig.calls("PushSubscription/set"), sets);
    assert_eq!(rig.server.webpush_subscriptions().len(), 1);
    assert!(rig.server.webpush_subscriptions()[0].url.ends_with(TOKEN));
    // A minute on it goes through, and replaces the first.
    rig.age_the_create();
    assert!(matches!(rig.core.web_push_subscribe(SECOND_TOKEN.into()).unwrap(), WebPushState::Verifying { .. }));
    let subs = rig.server.webpush_subscriptions();
    assert_eq!(subs.len(), 1);
    assert!(subs[0].url.ends_with(SECOND_TOKEN));
}

#[test]
fn the_same_verified_token_is_only_extended() {
    let rig = Rig::registered();
    let before = rig.server.webpush_subscriptions();
    // Only a few days are left on it.
    rig.server.webpush(|p| p.set_expires("s0", now() + 2 * 24 * 3600));
    rig.edit_record(|r| r["expires"] = json!(now() + 2 * 24 * 3600));
    let sets_before = rig.server.webpush(|p| p.sets.len());
    let state = rig.core.web_push_subscribe(TOKEN.into()).unwrap();
    match state {
        WebPushState::Registered { expires, .. } => assert!(expires > now() + 6 * 24 * 3600),
        other => panic!("{other:?}"),
    }
    let sets = rig.server.webpush(|p| p.sets.clone());
    assert_eq!(sets.len(), sets_before + 1);
    assert!(sets.last().unwrap().get("create").is_none() && sets.last().unwrap().get("destroy").is_none());
    let after = rig.server.webpush_subscriptions();
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].id, before[0].id);
    assert!(after[0].verified, "no new verification is needed");
    assert!(after[0].expires > now() + 6 * 24 * 3600);
}

#[test]
fn stale_subscriptions_of_this_device_are_destroyed_and_others_are_left() {
    let rig = Rig::synced();
    let registration = rig.register(TOKEN);
    let device = format!("kweb-{}", registration.subtype.rsplit('#').next().unwrap());
    // A subscription of another device, and the record loses its own (as if the answer of the
    // create had been lost).
    let foreign = rig.server.webpush(|p| p.add_foreign("another-phone"));
    rig.edit_record(|r| {
        r["subscription_id"] = Value::Null;
        r["last_create_at"] = Value::Null;
        r["created_at"] = Value::Null;
    });
    rig.server.webpush(|p| p.set_verify_gap(Duration::ZERO));
    rig.core.web_push_subscribe(SECOND_TOKEN.into()).unwrap();
    let subs = rig.server.webpush_subscriptions();
    let ours: Vec<_> = subs.iter().filter(|s| s.device_client_id.as_deref() == Some(device.as_str())).collect();
    assert_eq!(ours.len(), 1, "{subs:?}");
    assert!(ours[0].url.ends_with(SECOND_TOKEN));
    assert!(subs.iter().any(|s| s.id == foreign), "another device's subscription stays");
}

#[test]
fn fifteen_subscriptions_of_other_devices_are_refused_in_plain_words() {
    let rig = Rig::synced();
    rig.core.web_push_prepare().unwrap();
    for n in 0..15 {
        rig.server.webpush(|p| p.add_foreign(&format!("phone-{n}")));
    }
    match rig.core.web_push_subscribe(TOKEN.into()) {
        Err(Error::Invalid { msg }) => assert!(msg.contains("too many"), "{msg}"),
        other => panic!("{other:?}"),
    }
    assert_eq!(rig.core.web_push_state(), WebPushState::Off);
}

#[test]
fn a_refused_address_or_keys_are_named_in_fixed_words() {
    let rig = Rig::synced();
    rig.core.web_push_prepare().unwrap();
    rig.server.webpush(|p| p.refuse_create = Some(("invalidProperties", "url")));
    match rig.core.web_push_subscribe(TOKEN.into()) {
        Err(e @ Error::Invalid { .. }) => {
            assert!(text_of(&e).contains("push address"));
            assert_clean(&text_of(&e), &[TOKEN]);
        }
        other => panic!("{other:?}"),
    }
    rig.server.webpush(|p| p.refuse_create = Some(("invalidProperties", "keys")));
    match rig.core.web_push_subscribe(TOKEN.into()) {
        Err(e @ Error::Invalid { .. }) => assert!(text_of(&e).contains("push keys")),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_changed_server_key_is_found_by_subscribe_and_renew_and_fixed_by_prepare() {
    let rig = Rig::registered();
    let old = rig.core.web_push_prepare().unwrap().unwrap();
    let new_key = rig.server.webpush(|p| p.rotate_key());

    // Subscribe refuses: the token was made for the other key.
    match rig.core.web_push_subscribe(TOKEN.into()) {
        Err(Error::Invalid { msg }) => assert!(msg.contains("key changed"), "{msg}"),
        other => panic!("{other:?}"),
    }
    // Housekeeping finds it, and the state remembers.
    rig.edit_record(|r| r["last_look"] = json!(now() - 3 * 24 * 3600));
    assert_eq!(rig.core.web_push_renew().unwrap(), WebPushState::KeyChanged);
    assert_eq!(rig.core.web_push_state(), WebPushState::KeyChanged);
    assert_eq!(rig.core.web_push_renew().unwrap(), WebPushState::KeyChanged);

    // Registering again: the new key, the same registration id, the old token forgotten.
    let new = rig.core.web_push_prepare().unwrap().unwrap();
    assert_eq!(new.sender, new_key);
    assert_ne!(new.sender, old.sender);
    assert_eq!(new.subtype, old.subtype);
    assert!(rig.record()["token"].is_null());
    rig.age_the_create();
    rig.server.webpush(|p| p.set_verify_gap(Duration::ZERO));
    assert!(matches!(rig.core.web_push_subscribe(SECOND_TOKEN.into()).unwrap(), WebPushState::Verifying { .. }));
    assert_eq!(rig.server.webpush_subscriptions().len(), 1);
    assert_eq!(rig.feed(rig.server.webpush_take_deliveries()), [WebPushMessage::Verified]);
    assert!(matches!(rig.core.web_push_state(), WebPushState::Registered { .. }));
}

// ---- renew ----------------------------------------------------------------------------------

#[test]
fn renew_goes_to_the_server_only_when_there_is_a_reason() {
    let rig = Rig::registered();
    let (gets, sets) = (rig.calls("PushSubscription/get"), rig.calls("PushSubscription/set"));
    let state = rig.core.web_push_renew().unwrap();
    assert!(matches!(state, WebPushState::Registered { .. }));
    assert_eq!((rig.calls("PushSubscription/get"), rig.calls("PushSubscription/set")), (gets, sets));

    // A day since the last look: it looks, and extends nothing.
    rig.edit_record(|r| r["last_look"] = json!(now() - 25 * 3600));
    rig.core.web_push_renew().unwrap();
    assert_eq!(rig.calls("PushSubscription/get"), gets + 1);
    assert_eq!(rig.calls("PushSubscription/set"), sets);

    // Less than four days left: it extends to a week from now.
    rig.server.webpush(|p| p.set_expires("s0", now() + 3 * 24 * 3600));
    rig.edit_record(|r| r["expires"] = json!(now() + 3 * 24 * 3600));
    match rig.core.web_push_renew().unwrap() {
        WebPushState::Registered { expires, .. } => assert!(expires > now() + 6 * 24 * 3600),
        other => panic!("{other:?}"),
    }
    assert!(rig.server.webpush_subscriptions()[0].expires > now() + 6 * 24 * 3600);
}

#[test]
fn a_subscription_the_server_dropped_is_gone() {
    let rig = Rig::registered();
    rig.server.webpush(|p| p.remove("s0"));
    // The next 15-minute check finds it, not one a day later.
    rig.edit_record(|r| r["last_look"] = json!(now() - 15 * 60));
    assert_eq!(rig.core.web_push_renew().unwrap(), WebPushState::Gone);
    assert_eq!(rig.core.web_push_state(), WebPushState::Gone);
}

#[test]
fn a_subscription_whose_time_ran_out_is_gone_without_asking() {
    let rig = Rig::registered();
    rig.server.webpush(|p| p.set_expires("s0", now() - 10));
    rig.edit_record(|r| {
        r["expires"] = json!(now() - 10);
        r["last_look"] = json!(now());
    });
    assert_eq!(rig.core.web_push_state(), WebPushState::Gone);
    let gets = rig.calls("PushSubscription/get");
    // Verified, no time left: housekeeping asks (less than four days is left); the server has
    // let go of it (an expired subscription is not listed) and it stays gone.
    assert_eq!(rig.core.web_push_renew().unwrap(), WebPushState::Gone);
    assert!(rig.calls("PushSubscription/get") > gets);
}

#[test]
fn a_verifying_subscription_stays_verifying_and_off_stays_off() {
    let rig = Rig::synced();
    assert_eq!(rig.core.web_push_renew().unwrap(), WebPushState::Off);
    rig.register(TOKEN);
    let gets = rig.calls("PushSubscription/get");
    assert!(matches!(rig.core.web_push_renew().unwrap(), WebPushState::Verifying { .. }));
    assert_eq!(rig.calls("PushSubscription/get"), gets, "the host's clock decides when to try again");
}

#[test]
fn renew_with_the_server_down_is_temporary_and_changes_nothing() {
    let rig = Rig::registered();
    rig.edit_record(|r| r["last_look"] = json!(now() - 25 * 3600));
    let before = rig.record();
    rig.server.set_down(true);
    assert!(matches!(rig.core.web_push_renew(), Err(Error::Temporary { .. })));
    assert_eq!(rig.record(), before);
}

// ---- receive --------------------------------------------------------------------------------

#[test]
fn an_email_push_runs_the_pass_and_returns_its_plan() {
    let rig = Rig::registered();
    let id = rig.server.deliver("Maria", "maria@example.test", "Lunch plans", "Shall we?");
    // The message carries a preview; nothing of it is used.
    let deliveries = rig.server.webpush_email_push("A subject the pass never reads");
    assert_eq!(deliveries.len(), 1);
    let got = rig.core.web_push_receive(deliveries[0].body.clone(), Shade::Unknown).unwrap();
    let WebPushMessage::Checked { plan } = got else { panic!("{got:?}") };
    assert_eq!(posted(&plan), std::slice::from_ref(&id));
    assert_eq!(plan.post[0].subject, "Lunch plans", "{plan:?}");
    assert!(!format!("{plan:?}").contains("A subject the pass never reads"));
    assert!(rig.core.messages(INBOX_ID.into(), 10, 0).unwrap().iter().any(|m| m.id == id), "the cache has it");
    // The same message again: the rule shows nothing twice.
    let again = rig.core.web_push_receive(deliveries[0].body.clone(), Shade::Unknown).unwrap();
    let WebPushMessage::Checked { plan } = again else { panic!("{again:?}") };
    assert!(plan.post.is_empty(), "{plan:?}");
}

#[test]
fn the_listener_is_told_as_with_check_once() {
    let rig = Rig::registered();
    let listener = Arc::new(Recorder::default());
    rig.core.start(listener.clone());
    std::thread::sleep(Duration::from_millis(300));
    rig.core.stop();
    let id = rig.server.deliver("Maria", "maria@example.test", "Hello", "x");
    let got = rig.feed(rig.server.webpush_email_push("s"));
    assert!(matches!(got[0], WebPushMessage::Checked { .. }));
    let plans = listener.plans.lock().unwrap().clone();
    assert!(plans.iter().any(|p| posted(p) == [id.clone()]), "{plans:?}");
}

#[test]
fn a_state_change_for_this_account_runs_the_pass_and_removes_what_was_read_elsewhere() {
    let rig = Rig::registered();
    let id = rig.server.deliver("Maria", "maria@example.test", "Hello", "x");
    let first = rig.feed(rig.server.webpush_email_push("s"));
    let WebPushMessage::Checked { plan } = &first[0] else { panic!("{first:?}") };
    assert_eq!(posted(plan), std::slice::from_ref(&id));

    rig.server.mark_read(&id);
    let got = rig.feed(rig.server.webpush_state_change());
    let WebPushMessage::Checked { plan } = &got[0] else { panic!("{got:?}") };
    assert!(plan.cancel.iter().any(|c| c.message_id == id), "{plan:?}");
}

#[test]
fn a_state_change_or_email_push_for_another_account_is_ignored() {
    let rig = Rig::registered();
    let id = rig.server.webpush_subscriptions()[0].id.clone();
    let encrypt = |plain: Value| rig.server.webpush(|p| p.encrypt_for(&id, plain.to_string().as_bytes())).unwrap();
    for plain in [
        json!({ "@type": "StateChange", "changed": { "somebody-else": { "Email": "1" } } }),
        json!({ "@type": "EmailPush", "accountId": "somebody-else", "emails": [] }),
        json!({ "@type": "SomethingNew", "accountId": ACCOUNT_ID }),
        json!({ "no": "type" }),
    ] {
        let got = rig.core.web_push_receive(encrypt(plain.clone()), Shade::Unknown).unwrap();
        assert_eq!(got, WebPushMessage::Ignored, "{plain}");
    }
    // Not JSON at all, but a valid record.
    let junk = rig.server.webpush(|p| p.encrypt_for(&id, b"just text")).unwrap();
    assert_eq!(rig.core.web_push_receive(junk, Shade::Unknown).unwrap(), WebPushMessage::Ignored);
}

#[test]
fn a_pass_that_cannot_reach_the_server_is_an_error_for_the_host_to_hand_over() {
    let rig = Rig::registered();
    let deliveries = rig.server.webpush_email_push("s");
    rig.server.set_down(true);
    let e = rig.core.web_push_receive(deliveries[0].body.clone(), Shade::Unknown).unwrap_err();
    assert!(matches!(e, Error::Temporary { .. }), "{e}");
}

fn alert_event() -> Value {
    json!({ "@type": "Event", "title": "Dentist", "start": "2026-11-12T10:00:00", "timeZone": "Europe/Istanbul",
        "locations": { "l1": { "name": "Clinic" } } })
}

#[test]
fn a_calendar_alert_is_fetched_and_returned_as_a_reminder() {
    let rig = Rig::registered();
    let event = rig.server.add_event(alert_event());
    let got = rig.feed(rig.server.webpush_calendar_alert(&event, "al1"));
    let WebPushMessage::Reminder { reminder } = &got[0] else { panic!("{got:?}") };
    assert_eq!((reminder.event_id.as_str(), reminder.alert_id.as_str()), (event.as_str(), "al1"));
    assert_eq!((reminder.title.as_str(), reminder.location.as_str()), ("Dentist", "Clinic"));
    assert_eq!(reminder.local_start.as_deref(), Some("2026-11-12T10:00:00"));
}

#[test]
fn a_calendar_alert_whose_fetch_fails_still_comes_with_its_ids() {
    let rig = Rig::registered();
    rig.server.set_calendar_refused(true);
    let event = rig.server.add_event(alert_event());
    let got = rig.feed(rig.server.webpush_calendar_alert(&event, "al2"));
    let WebPushMessage::Reminder { reminder } = &got[0] else { panic!("{got:?}") };
    assert_eq!((reminder.event_id.as_str(), reminder.alert_id.as_str()), (event.as_str(), "al2"));
    assert!(reminder.title.is_empty());
    // And a deleted event.
    rig.server.set_calendar_refused(false);
    let got = rig.feed(rig.server.webpush_calendar_alert("deleted-meanwhile", "al3"));
    assert!(matches!(&got[0], WebPushMessage::Reminder { reminder } if reminder.title.is_empty()));
}

#[test]
fn what_does_not_decrypt_is_undecryptable_and_never_an_error() {
    let rig = Rig::registered();
    let id = rig.server.webpush_subscriptions()[0].id.clone();
    let good = rig.server.webpush(|p| p.encrypt_for(&id, br#"{"@type":"StateChange","changed":{}}"#)).unwrap();
    // Cut short at every length, and bent at every byte.
    for cut in (0..good.len()).step_by(7) {
        assert_eq!(
            rig.core.web_push_receive(good[..cut].to_vec(), Shade::Unknown).unwrap(),
            WebPushMessage::Undecryptable
        );
    }
    let mut bent = good.clone();
    let last = bent.len() - 1;
    bent[last] ^= 1;
    assert_eq!(rig.core.web_push_receive(bent, Shade::Unknown).unwrap(), WebPushMessage::Undecryptable);
    // More than one record (a second one appended).
    let mut two = good.clone();
    two.extend_from_slice(&good[86..]);
    assert_eq!(rig.core.web_push_receive(two, Shade::Unknown).unwrap(), WebPushMessage::Undecryptable);
    // Empty, junk, and the same size of random bytes.
    assert_eq!(rig.core.web_push_receive(Vec::new(), Shade::Unknown).unwrap(), WebPushMessage::Undecryptable);
    assert_eq!(rig.core.web_push_receive(vec![7; 300], Shade::Unknown).unwrap(), WebPushMessage::Undecryptable);

    // A body encrypted for other keys (another account's, say).
    let other = Rig::registered();
    let foreign = other.server.webpush(|p| p.encrypt_for("s0", b"{}")).unwrap();
    assert_eq!(rig.core.web_push_receive(foreign, Shade::Unknown).unwrap(), WebPushMessage::Undecryptable);
    // And it still decrypts what is for it.
    assert!(matches!(
        rig.core.web_push_receive(good, Shade::Unknown).unwrap(),
        WebPushMessage::Checked { .. } | WebPushMessage::Ignored
    ));
}

#[test]
fn an_account_without_keys_finds_everything_undecryptable() {
    let rig = Rig::synced();
    assert_eq!(rig.core.web_push_receive(vec![1; 200], Shade::Unknown).unwrap(), WebPushMessage::Undecryptable);
}

#[test]
fn two_messages_at_once_post_the_mail_once() {
    let rig = Rig::registered();
    let id = rig.server.deliver("Maria", "maria@example.test", "Both at once", "x");
    let one = rig.server.webpush_email_push("a")[0].body.clone();
    let two = rig.server.webpush_state_change()[0].body.clone();
    let results: Vec<_> = std::thread::scope(|scope| {
        let a = scope.spawn(|| rig.core.web_push_receive(one.clone(), Shade::Unknown));
        let b = scope.spawn(|| rig.core.web_push_receive(two.clone(), Shade::Unknown));
        vec![a.join().unwrap(), b.join().unwrap()]
    });
    let mut posts = 0;
    for result in results {
        let WebPushMessage::Checked { plan } = result.unwrap() else { panic!("not checked") };
        posts += plan.post.iter().filter(|n| n.message_id == id).count();
    }
    assert_eq!(posts, 1, "the passes take turns, so the mail is posted once");
}

// ---- drop and forget --------------------------------------------------------------------------

#[test]
fn drop_destroys_on_the_server_and_keeps_the_keys_until_forget() {
    let rig = Rig::registered();
    let foreign = rig.server.webpush(|p| p.add_foreign("another-phone"));
    let secrets = rig.secret_texts();
    rig.core.web_push_drop().unwrap();
    let left: Vec<_> = rig.server.webpush_subscriptions().into_iter().map(|s| s.id).collect();
    assert_eq!(left, [foreign], "only the other device's subscription is left");
    assert_eq!(rig.core.web_push_state(), WebPushState::Off);
    assert_eq!(rig.secret_texts(), secrets, "the keys stay");
    // A message already on its way still decrypts; a dropped subscription's verification is ignored.

    rig.core.web_push_forget().unwrap();
    assert!(rig.secret_texts().is_empty());
    assert_eq!(rig.record(), Value::Null, "the record is gone");
    assert_eq!(rig.core.web_push_state(), WebPushState::Off);
    // Twice is fine.
    rig.core.web_push_forget().unwrap();
    rig.core.web_push_drop().unwrap();
}

#[test]
fn drop_with_nothing_to_drop_does_nothing() {
    let rig = Rig::synced();
    let (gets, sets) = (rig.calls("PushSubscription/get"), rig.calls("PushSubscription/set"));
    rig.core.web_push_drop().unwrap();
    assert_eq!((rig.calls("PushSubscription/get"), rig.calls("PushSubscription/set")), (gets, sets));
}

#[test]
fn a_message_for_a_dropped_subscription_still_decrypts_until_forget_and_not_after() {
    let rig = Rig::registered();
    let id = rig.server.webpush_subscriptions()[0].id.clone();
    let in_flight =
        rig.server.webpush(|p| p.encrypt_for(&id, br#"{"@type":"StateChange","changed":{"x":{}}}"#)).unwrap();
    let verification = rig.server.webpush(|p| {
        p.encrypt_for(
            &id,
            json!({ "@type": "PushVerification", "pushSubscriptionId": id, "verificationCode": "c" })
                .to_string()
                .as_bytes(),
        )
    });
    rig.core.web_push_drop().unwrap();
    assert_eq!(rig.core.web_push_receive(in_flight.clone(), Shade::Unknown).unwrap(), WebPushMessage::Ignored);
    assert_eq!(rig.core.web_push_receive(verification.unwrap(), Shade::Unknown).unwrap(), WebPushMessage::Ignored);
    rig.core.web_push_forget().unwrap();
    assert_eq!(rig.core.web_push_receive(in_flight, Shade::Unknown).unwrap(), WebPushMessage::Undecryptable);
}

#[test]
fn a_drop_that_cannot_reach_the_server_is_finished_by_the_next_renew() {
    let rig = Rig::registered();
    rig.server.set_down(true);
    let e = rig.core.web_push_drop().unwrap_err();
    assert!(matches!(e, Error::Temporary { .. }), "{e}");
    assert_eq!(rig.core.web_push_state(), WebPushState::Off, "nothing is expected to arrive any more");
    assert_eq!(rig.record()["leftovers"].as_array().unwrap().len(), 1);
    assert_eq!(rig.server.webpush_subscriptions().len(), 1);

    rig.server.set_down(false);
    rig.core.web_push_forget().unwrap();
    assert_eq!(rig.record()["leftovers"].as_array().unwrap().len(), 1, "the list survives forget");
    rig.core.web_push_renew().unwrap();
    assert!(rig.server.webpush_subscriptions().is_empty());
    assert_eq!(rig.record(), Value::Null, "nothing is left");
}

#[test]
fn sign_out_destroys_the_subscription_and_removes_the_keys() {
    let rig = Rig::registered();
    assert_eq!(rig.secret_texts().len(), 2);
    rig.core.sign_out().unwrap();
    assert!(rig.server.webpush_subscriptions().is_empty());
    assert!(rig.secret_texts().is_empty());
}

#[test]
fn sign_out_with_the_server_down_still_removes_the_keys() {
    let rig = Rig::registered();
    rig.server.set_down(true);
    let started = Instant::now();
    rig.core.sign_out().unwrap();
    assert!(started.elapsed() < Duration::from_secs(10));
    assert!(rig.secret_texts().is_empty());
}

// ---- what is kept where ---------------------------------------------------------------------

#[test]
fn no_key_token_code_or_body_is_in_an_error_or_the_cache_file() {
    let rig = Rig::registered();
    let secrets = rig.secret_texts();
    let id = rig.server.webpush_subscriptions()[0].id.clone();
    let plain = json!({ "@type": "EmailPush", "accountId": ACCOUNT_ID, "emails": [{ "subject": "Top secret subject", "preview": "Top secret preview" }] });
    let body = rig.server.webpush(|p| p.encrypt_for(&id, plain.to_string().as_bytes())).unwrap();
    let code = rig.server.webpush(|p| p.accepted_codes.clone())[0].1.clone();

    // Errors from every call that can fail.
    let mut texts = Vec::new();
    rig.server.set_down(true);
    texts.push(rig.core.web_push_receive(body.clone(), Shade::Unknown).unwrap_err());
    texts.push(rig.core.web_push_subscribe(TOKEN.into()).unwrap_err());
    texts.push(rig.core.web_push_prepare().unwrap_err());
    rig.edit_record(|r| r["last_look"] = json!(now() - 25 * 3600));
    texts.push(rig.core.web_push_renew().unwrap_err());
    rig.server.set_down(false);
    texts.push(rig.core.web_push_subscribe("bad token".into()).unwrap_err());
    let mut forbidden: Vec<&str> = secrets.iter().map(String::as_str).collect();
    forbidden.extend([TOKEN, code.as_str(), "Top secret"]);
    for e in &texts {
        assert_clean(&text_of(e), &forbidden);
    }
    // The Debug of the types that hold them.
    assert_clean(&format!("{:?}", rig.core.web_push_state()), &forbidden);
    let registration = rig.core.web_push_prepare().unwrap().unwrap();
    assert_clean(&format!("{registration:?}"), &secrets.iter().map(String::as_str).collect::<Vec<_>>());

    // The cache file: the keys and the body of a message are not in it, whether a pass ran or not.
    rig.core.web_push_receive(body, Shade::Unknown).unwrap();
    drop(rig.store());
    let mut bytes = std::fs::read(&rig.db_path).unwrap();
    if let Ok(wal) = std::fs::read(format!("{}-wal", rig.db_path)) {
        bytes.extend(wal);
    }
    let haystack = String::from_utf8_lossy(&bytes).into_owned();
    for needle in secrets.iter().map(String::as_str).chain(["Top secret"]) {
        assert!(!haystack.contains(needle), "'{needle}' is in the cache file");
    }
    // They were base64url texts of the right length.
    assert_eq!(URL_SAFE_NO_PAD.decode(&secrets[0]).unwrap().len(), 32);
}

// ---- work that is waiting -----------------------------------------------------------------------

fn addr(name: &str, email: &str) -> Address {
    Address { name: name.into(), email: email.into() }
}

#[test]
fn work_waiting_is_nothing_with_nothing_to_do() {
    let rig = Rig::synced();
    assert_eq!(rig.core.work_waiting().unwrap(), WorkWaiting::Nothing);
}

#[test]
fn a_queued_action_is_due_now_until_a_pass_sends_it() {
    let rig = Rig::synced();
    rig.server.set_down(true);
    rig.core.submit(kweb_core::model::Action::MarkRead { message_id: "e1".into() }).unwrap();
    assert_eq!(rig.core.work_waiting().unwrap(), WorkWaiting::Due { in_ms: 0 });
    rig.server.set_down(false);
    rig.core.check_once(Shade::Unknown).unwrap();
    assert_eq!(rig.core.work_waiting().unwrap(), WorkWaiting::Nothing);
}

fn note(rig: &Rig, subject: &str) -> kweb_core::model::Draft {
    let mut draft = rig.core.new_draft(kweb_core::model::DraftKind::New).unwrap();
    draft.to = vec![addr("Bo", "bo@example.test")];
    draft.subject = subject.into();
    draft.body = "Hello Bo".into();
    draft
}

#[test]
fn a_draft_that_is_not_on_the_server_is_due_and_one_that_is_is_not() {
    let rig = Rig::synced();
    let draft = rig.core.save_draft(note(&rig, "Unsaved")).unwrap();
    assert_eq!(rig.core.work_waiting().unwrap(), WorkWaiting::Due { in_ms: 0 });
    rig.core.check_once(Shade::Unknown).unwrap();
    assert_eq!(rig.core.work_waiting().unwrap(), WorkWaiting::Nothing, "the pass saved {:?}", draft.id);
}

#[test]
fn a_message_in_its_undo_window_is_due_when_it_leaves() {
    let rig = Rig::synced();
    rig.server.set_down(true);
    let pending = rig.core.send(note(&rig, "Held here"), 30).unwrap();
    let WorkWaiting::Due { in_ms } = rig.core.work_waiting().unwrap() else { panic!("nothing is waiting?") };
    assert!((25_000..=30_000).contains(&in_ms), "{in_ms}");
    assert!(pending.send_at >= now());

    let draft = rig.core.undo_send(pending.draft_id.clone()).unwrap();
    assert_eq!(draft.id, pending.draft_id);
    assert_eq!(rig.core.work_waiting().unwrap(), WorkWaiting::Due { in_ms: 0 }, "the draft is not on the server");
}

#[test]
fn a_held_message_whose_time_has_come_is_due_now() {
    let rig = Rig::synced();
    let sent_before = rig.server.ids_in("sent").len();
    rig.server.set_down(true);
    rig.core.send(note(&rig, "Leaves now"), 1).unwrap();
    std::thread::sleep(Duration::from_millis(1100));
    assert_eq!(rig.core.work_waiting().unwrap(), WorkWaiting::Due { in_ms: 0 });
    rig.server.set_down(false);
    rig.core.check_once(Shade::Unknown).unwrap();
    assert_eq!(rig.server.ids_in("sent").len(), sent_before + 1, "the pass sent it");
    assert_eq!(rig.core.work_waiting().unwrap(), WorkWaiting::Nothing);
}

#[test]
fn a_message_the_server_holds_is_due_a_few_seconds_after_its_time_and_a_scheduled_one_is_not() {
    let rig = Rig::synced();
    rig.core.check_once(Shade::Unknown).unwrap();
    let pending = rig.core.send(note(&rig, "Held there"), 20).unwrap();
    // The server holds it now (the hand-over happened): due 3 s after the send time.
    let WorkWaiting::Due { in_ms } = rig.core.work_waiting().unwrap() else { panic!("nothing is waiting?") };
    assert!((20_000..=23_000).contains(&in_ms), "{in_ms}");
    let _ = pending;

    // A send-later is the server's alone: nothing here waits for it.
    let rig = Rig::synced();
    rig.core.schedule_send(note(&rig, "Later"), now() + 3600).unwrap();
    assert_eq!(rig.core.work_waiting().unwrap(), WorkWaiting::Nothing);
}

// ---- the server reuses ids ------------------------------------------------------------------

/// Our subscription ran out on the server, and another device of the account got its id.
fn reused_by_another_device(rig: &Rig) -> (String, i64) {
    rig.server.webpush(|p| p.set_expires("s0", now() - 10));
    let foreign = rig.server.webpush(|p| p.add_foreign("another-phone"));
    assert_eq!(foreign, "s0", "the id was reused");
    let expires = rig.server.webpush_subscriptions()[0].expires;
    (foreign, expires)
}

#[test]
fn subscribe_does_not_extend_a_subscription_of_another_device_that_took_the_id() {
    let rig = Rig::registered();
    let (foreign, expires) = reused_by_another_device(&rig);
    rig.edit_record(|r| r["expires"] = json!(now() + 2 * 24 * 3600));
    rig.age_the_create();
    let state = rig.core.web_push_subscribe(TOKEN.into()).unwrap();
    assert!(matches!(state, WebPushState::Verifying { .. }), "a new one is made: {state:?}");
    let subs = rig.server.webpush_subscriptions();
    let other = subs.iter().find(|s| s.id == foreign).unwrap();
    assert_eq!(other.expires, expires, "the other device's subscription is not touched");
    assert_eq!(other.device_client_id.as_deref(), Some("another-phone"));
    assert_eq!(subs.len(), 2);
}

#[test]
fn renew_finds_ours_gone_when_another_device_took_the_id() {
    let rig = Rig::registered();
    let (foreign, expires) = reused_by_another_device(&rig);
    rig.edit_record(|r| {
        r["expires"] = json!(now() + 2 * 24 * 3600);
        r["last_look"] = json!(now() - 25 * 3600);
    });
    assert_eq!(rig.core.web_push_renew().unwrap(), WebPushState::Gone);
    let subs = rig.server.webpush_subscriptions();
    assert_eq!(subs.iter().find(|s| s.id == foreign).unwrap().expires, expires, "not extended");
}

#[test]
fn a_leftover_whose_id_went_to_another_device_is_not_destroyed() {
    let rig = Rig::registered();
    rig.server.set_down(true);
    assert!(rig.core.web_push_drop().is_err());
    rig.server.set_down(false);
    // It ended on its own, and another device got the id.
    rig.server.webpush(|p| p.remove("s0"));
    let foreign = rig.server.webpush(|p| p.add_foreign("another-phone"));
    assert_eq!(foreign, "s0");
    rig.core.web_push_forget().unwrap();
    rig.core.web_push_renew().unwrap();
    let subs = rig.server.webpush_subscriptions();
    assert_eq!(subs.len(), 1, "{subs:?}");
    assert_eq!(subs[0].device_client_id.as_deref(), Some("another-phone"));
    assert_eq!(rig.record(), Value::Null, "the left-over is dropped as gone");
}

#[test]
fn a_drop_after_a_failed_one_leaves_a_foreign_subscription_with_the_reused_id() {
    let rig = Rig::registered();
    rig.server.set_down(true);
    assert!(rig.core.web_push_drop().is_err());
    rig.server.set_down(false);
    rig.server.webpush(|p| p.remove("s0"));
    rig.server.webpush(|p| p.add_foreign("another-phone"));
    rig.core.web_push_drop().unwrap();
    assert_eq!(rig.server.webpush_subscriptions().len(), 1);
}

// ---- the receiver's seconds ------------------------------------------------------------------

#[test]
fn an_email_push_returns_before_the_bodies_are_fetched() {
    let rig = Rig::registered();
    rig.server.deliver("Maria", "maria@example.test", "Lunch plans", "Shall we?");
    rig.server.set_bodies_delay(Duration::from_secs(4));
    let deliveries = rig.server.webpush_email_push("x");
    let started = std::time::Instant::now();
    let got = rig.core.web_push_receive(deliveries[0].body.clone(), Shade::Unknown).unwrap();
    assert!(started.elapsed() < Duration::from_secs(3), "waited for the prefetch: {:?}", started.elapsed());
    let WebPushMessage::Checked { plan } = got else { panic!("{got:?}") };
    assert_eq!(plan.post.len(), 1);
    // The 15-minute check still fetches them.
    let calls = rig.calls("Email/get");
    rig.server.set_bodies_delay(Duration::ZERO);
    rig.core.check_once(Shade::Unknown).unwrap();
    assert!(rig.calls("Email/get") > calls);
}

#[test]
fn work_waiting_counts_a_discarded_draft_that_the_server_still_holds() {
    let rig = Rig::synced();
    let draft = rig.core.save_draft(note(&rig, "To throw away")).unwrap();
    rig.core.check_once(Shade::Unknown).unwrap();
    assert_eq!(rig.core.work_waiting().unwrap(), WorkWaiting::Nothing);
    rig.server.set_down(true);
    rig.core.discard_draft(draft.id).unwrap();
    assert_eq!(rig.core.work_waiting().unwrap(), WorkWaiting::Due { in_ms: 0 });
    rig.server.set_down(false);
    rig.core.check_once(Shade::Unknown).unwrap();
    assert_eq!(rig.core.work_waiting().unwrap(), WorkWaiting::Nothing);
}
