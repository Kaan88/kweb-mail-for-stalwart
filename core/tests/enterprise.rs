//! Admin design, step 3: what a licensed server adds, through the `Core` object the way the app
//! uses it, against the mock server (see `support/enterprise.rs` for what it does and where its
//! shapes come from). The admin key is put straight into the secret store: `admin_connect`
//! belongs to another part of the admin work.

mod support;

use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use kweb_core::{
    ffi::{Core, DeletedKind, DeliveryStatus, LiveFigures, LiveListener},
    secrets::{MemorySecrets, SecretStore},
    Error,
};
use serde_json::json;
use support::{iso, LiveEnd, MockServer, ADMIN_KEY};
use tokio::runtime::Runtime;

/// The id of the account the mock signs in as.
const ME: &str = support::ACCOUNT_ID;
const LICENCE: &str = "this needs an Enterprise licence on the server";

struct Rig {
    // Field order is drop order: the core goes first, the mock and its runtime last.
    core: Arc<Core>,
    server: MockServer,
    secrets: Arc<MemorySecrets>,
    _dir: tempfile::TempDir,
    _rt: Runtime,
}

impl Rig {
    /// A signed-in core, a licensed server that takes the admin key, and the key in the store.
    fn new() -> Rig {
        let rig = Rig::unlicensed();
        rig.server.enterprise().licensed(true);
        rig
    }

    /// The same, but the server has no licence.
    fn unlicensed() -> Rig {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        let server = rt.block_on(MockServer::start());
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("cache.db").to_string_lossy().into_owned();
        let secrets = Arc::new(MemorySecrets::default());
        let core = Core::new(db_path, secrets.clone()).unwrap();
        let url = core.begin_sign_in_with_redirect(server.url(), "ovh.kweb.mail:/oauth/callback".into()).unwrap();
        core.complete_sign_in(server.authorize(&url).unwrap()).unwrap();
        server.enterprise().accept_key(ADMIN_KEY);
        secrets.set("admin_key".into(), ADMIN_KEY.into()).unwrap();
        // What the core keeps beside a key: the server and the account it was accepted for.
        let binding = format!("{}\n{}\n{}", server.url().trim_end_matches('/').to_lowercase(), ME, support::USERNAME);
        secrets.set("admin_key_for".into(), binding).unwrap();
        Rig { core, server, secrets, _dir: dir, _rt: rt }
    }

    fn ent(&self) -> support::Enterprise {
        self.server.enterprise()
    }
}

fn invalid(e: Error) -> String {
    match e {
        Error::Invalid { msg } => msg,
        other => panic!("expected Error::Invalid, got {other:?}"),
    }
}

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64
}

/// The start of the hour `hours_ago` hours before the current one.
fn hour(hours_ago: i64) -> i64 {
    now().div_euclid(3600) * 3600 - hours_ago * 3600
}

// ---- the licence and the key ----

#[test]
fn every_function_is_refused_on_an_unlicensed_server_without_asking_further() {
    let rig = Rig::unlicensed();
    struct Quiet;
    impl LiveListener for Quiet {
        fn figures(&self, _: LiveFigures) {}
    }
    let all = [
        rig.core.admin_live("l".into(), Arc::new(Quiet)).unwrap_err(),
        rig.core.admin_history(24).unwrap_err(),
        rig.core.admin_deliveries(String::new(), None, 0, 10).unwrap_err(),
        rig.core.admin_delivery("t".into()).unwrap_err(),
        rig.core.admin_recovery_state().unwrap_err(),
        rig.core.admin_deleted_items(None, 0, 10).unwrap_err(),
        rig.core.admin_restore(None, vec!["a".into()]).unwrap_err(),
        rig.core.admin_alerts().unwrap_err(),
        rig.core.admin_set_alert_enabled("a".into(), true).unwrap_err(),
        rig.core.admin_tenants().unwrap_err(),
    ];
    for e in all {
        assert_eq!(invalid(e), LICENCE);
    }
    // Only the edition was asked: no management call and no stream.
    for method in [
        "x:Metric/query",
        "x:Trace/query",
        "x:DataRetention/get",
        "x:ArchivedItem/query",
        "x:Alert/get",
        "x:Tenant/get",
    ] {
        assert_eq!(rig.server.request_count(method), 0, "{method}");
    }
    assert_eq!(rig.server.request_count("/api/live/metrics"), 0);
    assert!(rig.server.request_count("/api/account") >= 10);
}

#[test]
fn the_edition_is_read_each_time_so_a_licence_that_arrives_is_used() {
    let rig = Rig::unlicensed();
    assert_eq!(invalid(rig.core.admin_tenants().unwrap_err()), LICENCE);
    rig.ent().licensed(true);
    assert!(rig.core.admin_tenants().unwrap().is_empty());
}

#[test]
fn a_key_the_server_refuses_is_said_so() {
    let rig = Rig::new();
    rig.secrets.set("admin_key".into(), "API_not_the_key".into()).unwrap();
    let message = invalid(rig.core.admin_tenants().unwrap_err());
    assert!(message.contains("did not accept the admin key"), "{message}");
    assert!(!message.contains("API_not_the_key"));
}

#[test]
fn no_key_is_invalid() {
    let rig = Rig::new();
    rig.secrets.delete("admin_key".into()).unwrap();
    assert!(invalid(rig.core.admin_alerts().unwrap_err()).contains("no admin key"));
}

#[test]
fn a_key_without_the_permission_may_not() {
    let rig = Rig::new();
    rig.ent().permissions(&["authenticate"]);
    assert_eq!(invalid(rig.core.admin_history(24).unwrap_err()), "this key may not read the server's history");
    assert_eq!(
        invalid(rig.core.admin_deliveries(String::new(), None, 0, 10).unwrap_err()),
        "this key may not read the delivery history"
    );
    // The setting is not readable: the state is "unknown", not a refusal, so the list can still be tried.
    let unknown = rig.core.admin_recovery_state().unwrap();
    assert_eq!((unknown.is_on, unknown.keeps_days), (true, None));
    assert_eq!(
        invalid(rig.core.admin_deleted_items(None, 0, 10).unwrap_err()),
        "this key may not look at deleted items"
    );
    assert_eq!(
        invalid(rig.core.admin_restore(None, vec!["a".into()]).unwrap_err()),
        "this key may not look at deleted items"
    );
    assert_eq!(invalid(rig.core.admin_alerts().unwrap_err()), "this key may not read the alerts");
    assert_eq!(
        invalid(rig.core.admin_set_alert_enabled("a".into(), true).unwrap_err()),
        "this key may not change alerts"
    );
    assert_eq!(invalid(rig.core.admin_tenants().unwrap_err()), "this key may not read the tenants");
    // Refused before any management call was made.
    assert_eq!(rig.server.request_count("x:Metric/query"), 0);
}

/// The permissions an area needs are the ones its flag in `admin_access` asks for: the first screen of
/// each works with exactly these.
#[test]
fn the_first_screen_of_each_area_works_with_exactly_the_permissions_its_flag_asks_for() {
    let rig = Rig::new();
    rig.ent().permissions(&["authenticate", "sysMetricQuery", "sysMetricGet"]);
    assert!(rig.core.admin_history(24).is_ok());
    rig.ent().permissions(&["authenticate", "sysTraceQuery", "sysTraceGet"]);
    assert!(rig.core.admin_deliveries(String::new(), None, 0, 10).is_ok());
    rig.ent().permissions(&["authenticate", "sysAlertGet"]);
    assert!(rig.core.admin_alerts().is_ok());
    rig.ent().permissions(&["authenticate", "sysTenantGet"]);
    assert!(rig.core.admin_tenants().is_ok());
    // Recovery: the list and the state work without the right to read the retention setting.
    rig.ent().permissions(&["authenticate", "sysArchivedItemQuery", "sysArchivedItemGet"]);
    assert!(rig.core.admin_recovery_state().is_ok());
    assert!(rig.core.admin_deleted_items(None, 0, 10).is_ok());
}

#[test]
fn a_method_the_server_forbids_after_the_account_said_yes_is_also_may_not() {
    let rig = Rig::new();
    // The account lists the permission, the method then answers `forbidden`: the key's
    // permission list changed in between, or the server judges more than it lists.
    let ent = rig.ent();
    ent.permissions(&["sysAlertGet", "sysAlertUpdate"]);
    assert!(rig.core.admin_alerts().unwrap().is_empty());
}

// ---- live ----

#[derive(Default)]
struct Collect(Mutex<Vec<LiveFigures>>);

impl LiveListener for Collect {
    fn figures(&self, figures: LiveFigures) {
        self.0.lock().unwrap().push(figures);
    }
}

fn frame(queue: u64, users: u64) -> serde_json::Value {
    json!([
        {"metric": "queue.count", "@type": "Gauge", "count": queue},
        {"metric": "user.count", "@type": "Gauge", "count": users},
        {"metric": "server.memory", "@type": "Gauge", "count": 123_456},
        {"metric": "delivery.active-connections", "@type": "Gauge", "count": 2},
        {"metric": "something.else", "@type": "Counter", "count": 9},
    ])
}

fn wait_for(what: &str, mut done: impl FnMut() -> bool) {
    let start = Instant::now();
    while !done() {
        assert!(start.elapsed().as_secs() < 10, "timed out waiting for {what}");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

#[test]
fn live_figures_arrive_until_the_run_is_stopped() {
    let rig = Rig::new();
    rig.ent().live_frames(
        vec![frame(3, 7), frame(4, 7), json!({"interval": 5000}), json!([{"metric": "queue.count", "count": 5}])],
        30,
        LiveEnd::Hold,
    );
    let heard = Arc::new(Collect::default());
    let core = rig.core.clone();
    let listener = heard.clone();
    let run = std::thread::spawn(move || core.admin_live("run-1".into(), listener));
    wait_for("four frames", || heard.0.lock().unwrap().len() >= 3);
    assert_eq!(rig.server.enterprise().live_open(), 1);
    rig.core.admin_live_stop("run-1".into());
    assert!(run.join().unwrap().is_ok());
    let got = heard.0.lock().unwrap().clone();
    assert_eq!(
        got[0],
        LiveFigures { queue: Some(3), connections: Some(2), users: Some(7), memory_bytes: Some(123_456) }
    );
    assert_eq!(got[1].queue, Some(4));
    // A figure the server does not send is None; a frame that is not a list is skipped.
    assert_eq!(got[2], LiveFigures { queue: Some(5), connections: None, users: None, memory_bytes: None });
    assert_eq!(got.len(), 3);
    // The request: the metrics asked for, every five seconds, with the key.
    let (query, key_ok) = rig.ent().live_requests().remove(0);
    assert!(key_ok);
    assert!(query.contains("interval=5"), "{query}");
    for name in ["queue.count", "user.count", "server.memory", "delivery.active-connections"] {
        assert!(query.contains(name), "{query}");
    }
    // The connection was let go.
    wait_for("the stream to close", || rig.ent().live_open() == 0);
}

#[test]
fn a_stop_ends_the_call_also_while_it_is_still_checking_the_licence() {
    let rig = Rig::new();
    rig.ent().account_delay_ms(8000);
    let heard = Arc::new(Collect::default());
    let core = rig.core.clone();
    let listener = heard.clone();
    let started = Instant::now();
    let run = std::thread::spawn(move || core.admin_live("slow".into(), listener));
    std::thread::sleep(std::time::Duration::from_millis(300));
    rig.core.admin_live_stop("slow".into());
    assert!(run.join().unwrap().is_ok());
    assert!(started.elapsed().as_secs() < 4, "the stop was not kept waiting for the network: {:?}", started.elapsed());
    assert_eq!(rig.server.request_count("/api/live/metrics"), 0);
    assert!(heard.0.lock().unwrap().is_empty());
}

#[test]
fn a_stop_before_the_start_is_remembered() {
    let rig = Rig::new();
    rig.ent().live_frames(vec![frame(1, 1)], 0, LiveEnd::Hold);
    rig.core.admin_live_stop("early".into());
    let heard = Arc::new(Collect::default());
    assert!(rig.core.admin_live("early".into(), heard.clone()).is_ok());
    assert!(heard.0.lock().unwrap().is_empty());
    assert_eq!(rig.server.request_count("/api/live/metrics"), 0);
    // The stop was used up: the same name runs again.
    let core = rig.core.clone();
    let run = std::thread::spawn({
        let heard = heard.clone();
        move || core.admin_live("early".into(), heard)
    });
    wait_for("a frame", || !heard.0.lock().unwrap().is_empty());
    rig.core.admin_live_stop("early".into());
    assert!(run.join().unwrap().is_ok());
}

#[test]
fn stopping_another_run_does_not_touch_this_one() {
    let rig = Rig::new();
    rig.ent().live_frames(vec![frame(1, 1)], 0, LiveEnd::Hold);
    let heard = Arc::new(Collect::default());
    let core = rig.core.clone();
    let run = std::thread::spawn({
        let heard = heard.clone();
        move || core.admin_live("mine".into(), heard)
    });
    wait_for("a frame", || !heard.0.lock().unwrap().is_empty());
    rig.core.admin_live_stop("someone-else".into());
    std::thread::sleep(std::time::Duration::from_millis(200));
    assert!(!run.is_finished());
    rig.core.admin_live_stop("mine".into());
    assert!(run.join().unwrap().is_ok());
}

#[test]
fn a_stream_that_ends_by_itself_is_temporary() {
    let rig = Rig::new();
    rig.ent().live_frames(vec![frame(1, 1)], 0, LiveEnd::Close);
    let heard = Arc::new(Collect::default());
    match rig.core.admin_live("ends".into(), heard.clone()) {
        Err(Error::Temporary { msg }) => assert!(msg.contains("ended"), "{msg}"),
        other => panic!("{other:?}"),
    }
    assert_eq!(heard.0.lock().unwrap().len(), 1);
}

#[test]
fn a_server_that_goes_quiet_is_temporary() {
    let rig = Rig::new();
    rig.ent().live_frames(vec![frame(1, 1)], 0, LiveEnd::Hold);
    let heard = Arc::new(Collect::default());
    let start = Instant::now();
    match rig.core.admin_live("quiet".into(), heard.clone()) {
        Err(Error::Temporary { msg }) => assert!(msg.contains("stopped sending"), "{msg}"),
        other => panic!("{other:?}"),
    }
    assert_eq!(heard.0.lock().unwrap().len(), 1);
    assert!(start.elapsed().as_secs() >= 14);
}

#[test]
fn a_server_that_does_not_answer_in_time_is_temporary() {
    let rig = Rig::new();
    rig.ent().live_stall_ms(13_000);
    let start = Instant::now();
    match rig.core.admin_live("stalled".into(), Arc::new(Collect::default())) {
        Err(Error::Temporary { msg }) => assert!(msg.contains("did not answer in time"), "{msg}"),
        other => panic!("{other:?}"),
    }
    assert!(start.elapsed().as_secs() < 13);
}

#[test]
fn a_run_name_in_use_is_refused() {
    let rig = Rig::new();
    rig.ent().live_frames(vec![frame(1, 1)], 0, LiveEnd::Hold);
    let heard = Arc::new(Collect::default());
    let core = rig.core.clone();
    let run = std::thread::spawn({
        let heard = heard.clone();
        move || core.admin_live("twice".into(), heard)
    });
    wait_for("a frame", || !heard.0.lock().unwrap().is_empty());
    assert!(invalid(rig.core.admin_live("twice".into(), heard.clone()).unwrap_err()).contains("already running"));
    rig.core.admin_live_stop("twice".into());
    run.join().unwrap().unwrap();
}

#[test]
fn live_with_a_key_that_may_not_is_refused_before_the_stream() {
    let rig = Rig::new();
    rig.ent().permissions(&["authenticate"]);
    let e = rig.core.admin_live("nope".into(), Arc::new(Collect::default())).unwrap_err();
    assert_eq!(invalid(e), "this key may not follow the live figures");
    assert_eq!(rig.server.request_count("/api/live/metrics"), 0);
}

// ---- history ----

#[test]
fn history_has_one_point_per_hour_with_zero_for_the_empty_ones() {
    let rig = Rig::new();
    let ent = rig.ent();
    ent.counter("queue.message-queued", hour(0), 5);
    ent.counter("queue.message-queued", hour(2), 4);
    ent.counter("queue.message-queued", hour(2) + 1800, 6);
    ent.counter("queue.authenticated-message-queued", hour(1), 2);
    ent.counter("queue.dsn-queued", hour(1), 1);
    ent.histogram("delivery.total-time", hour(3), 4, 6000);
    ent.counter("message-ingest.ham", hour(1), 99); // another metric: ignored
    ent.counter("queue.message-queued", hour(30), 50); // before the span: ignored
    let h = rig.core.admin_history(24).unwrap();
    for chart in [&h.received, &h.delivered, &h.delivery_seconds] {
        assert_eq!(chart.len(), 24);
        assert!(chart.windows(2).all(|w| w[1].at - w[0].at == 3600), "oldest first, an hour apart");
    }
    let at = |chart: &Vec<kweb_core::ffi::ChartPoint>, t: i64| chart.iter().find(|p| p.at == t).map(|p| p.value);
    assert_eq!(at(&h.received, hour(2)), Some(10.0), "counter rows add up");
    assert_eq!(at(&h.received, hour(0)), Some(5.0));
    assert_eq!(at(&h.received, hour(1)), Some(0.0), "an empty hour is a zero point");
    assert_eq!(at(&h.delivered, hour(1)), Some(3.0));
    assert_eq!(at(&h.delivery_seconds, hour(3)), Some(1.5), "sum over count, in seconds");
    assert_eq!(at(&h.delivery_seconds, hour(2)), Some(0.0));
    assert_eq!(h.received.iter().map(|p| p.value).sum::<f64>(), 15.0);
}

#[test]
fn history_beyond_48_hours_is_one_point_per_day_in_utc() {
    let rig = Rig::new();
    let ent = rig.ent();
    let today = now().div_euclid(86_400) * 86_400;
    ent.counter("queue.message-queued", today - 86_400 + 3 * 3600, 2);
    ent.counter("queue.message-queued", today - 86_400 + 20 * 3600, 3);
    ent.histogram("delivery.total-time", today - 2 * 86_400 + 3600, 1, 2000);
    ent.histogram("delivery.total-time", today - 2 * 86_400 + 7200, 3, 6000);
    let week = rig.core.admin_history(168).unwrap();
    assert_eq!(week.received.len(), 7);
    assert!(week.received.windows(2).all(|w| w[1].at - w[0].at == 86_400));
    assert!(week.received.iter().all(|p| p.at % 86_400 == 0));
    let yesterday = week.received.iter().find(|p| p.at == today - 86_400).map(|p| p.value);
    assert_eq!(yesterday, Some(5.0));
    let slow = week.delivery_seconds.iter().find(|p| p.at == today - 2 * 86_400).map(|p| p.value);
    assert_eq!(slow, Some(2.0), "two rows in a day are averaged by their counts: 8000 over 4");
    assert_eq!(rig.core.admin_history(720).unwrap().received.len(), 30);
}

#[test]
fn history_pages_through_all_rows() {
    let rig = Rig::new();
    let ent = rig.ent();
    ent.query_cap(3);
    for i in 0..10 {
        ent.counter("queue.message-queued", hour(0) + i, 1);
    }
    let h = rig.core.admin_history(24).unwrap();
    assert_eq!(h.received.last().unwrap().value, 10.0);
    assert!(rig.server.request_count("x:Metric/query") >= 4);
}

#[test]
fn history_takes_only_24_168_or_720_hours() {
    let rig = Rig::new();
    for hours in [0, 1, 48, 100, 721] {
        assert!(invalid(rig.core.admin_history(hours).unwrap_err()).contains("24 hours"));
    }
    assert_eq!(rig.server.request_count("x:Metric/query"), 0);
}

// ---- deliveries ----

const MX: &[(&str, &str)] = &[("hostname", "mx.example.org"), ("code", "250"), ("details", "2.0.0 Ok queued")];

fn seed_traces(rig: &Rig) {
    let ent = rig.ent();
    let t = now() - 3600;
    ent.trace(
        "tdone",
        t,
        "a@kweb.test",
        "bob@example.org",
        1200,
        &[
            ("delivery.attempt-start", &[("queueId", "1001"), ("to", "bob@example.org")]),
            ("delivery.domain-delivery-start", &[("domain", "example.org")]),
            ("delivery.dsn-success", MX),
            ("delivery.completed", &[]),
        ],
    );
    ent.trace(
        "tretry",
        t + 10,
        "a@kweb.test",
        "carol@example.net",
        800,
        &[
            ("delivery.attempt-start", &[("queueId", "1002")]),
            ("delivery.connect-error", &[("reason", "timed out")]),
            // As the server logs an attempt that will be retried: no delay report yet.
            ("queue.rescheduled", &[]),
            ("delivery.attempt-end", &[]),
        ],
    );
    ent.trace(
        "tfail",
        t + 20,
        "a@kweb.test",
        "nobody@example.com",
        500,
        &[
            ("delivery.attempt-start", &[("queueId", "1003")]),
            ("delivery.rcpt-to-rejected", &[("code", "550")]),
            ("delivery.dsn-perm-fail", &[]),
        ],
    );
    ent.trace(
        "tunknown",
        t + 30,
        "b@kweb.test",
        "dave@example.org",
        300,
        &[("delivery.attempt-start", &[("queueId", "1004")])],
    );
    // An inbound session: not a delivery attempt of the server.
    ent.trace("tinbound", t + 40, "x@elsewhere.test", "a@kweb.test", 100, &[("smtp.connection-start", &[])]);
}

#[test]
fn deliveries_come_newest_first_with_their_status() {
    let rig = Rig::new();
    seed_traces(&rig);
    let page = rig.core.admin_deliveries(String::new(), None, 0, 50).unwrap();
    assert_eq!(page.total, 4);
    let ids: Vec<&str> = page.deliveries.iter().map(|d| d.id.as_str()).collect();
    assert_eq!(ids, ["tunknown", "tfail", "tretry", "tdone"]);
    let by = |id: &str| page.deliveries.iter().find(|d| d.id == id).unwrap().clone();
    assert_eq!(by("tdone").status, DeliveryStatus::Delivered);
    assert_eq!(by("tretry").status, DeliveryStatus::Retrying);
    assert_eq!(by("tfail").status, DeliveryStatus::Failed);
    assert_eq!(by("tunknown").status, DeliveryStatus::Unknown);
    let done = by("tdone");
    assert_eq!(
        (done.from.as_str(), done.to.clone(), done.size),
        ("a@kweb.test", vec!["bob@example.org".to_string()], 1200)
    );
    assert!(done.at > now() - 4000 && done.at < now());
}

#[test]
fn deliveries_can_be_searched_and_limited_in_time() {
    let rig = Rig::new();
    seed_traces(&rig);
    // The server indexes whole values: an address, a domain, a host name.
    let found = rig.core.admin_deliveries("carol@example.net".into(), None, 0, 50).unwrap();
    assert_eq!(found.deliveries.iter().map(|d| d.id.as_str()).collect::<Vec<_>>(), ["tretry"]);
    assert_eq!(found.total, 1);
    assert_eq!(rig.core.admin_deliveries("example.org".into(), None, 0, 50).unwrap().deliveries[0].id, "tdone");
    assert_eq!(rig.core.admin_deliveries("carol".into(), None, 0, 50).unwrap().total, 0, "part of an address");
    // A queue id is searched too.
    assert_eq!(rig.core.admin_deliveries("1003".into(), None, 0, 50).unwrap().deliveries[0].id, "tfail");
    // Blank is all.
    assert_eq!(rig.core.admin_deliveries("   ".into(), None, 0, 50).unwrap().total, 4);
    // Since: only what came at or after.
    let since = now() - 3600 + 25;
    let recent = rig.core.admin_deliveries(String::new(), Some(since), 0, 50).unwrap();
    assert_eq!(recent.deliveries.iter().map(|d| d.id.as_str()).collect::<Vec<_>>(), ["tunknown"]);
    assert_eq!(rig.core.admin_deliveries("nothing-like-this".into(), None, 0, 50).unwrap().total, 0);
}

#[test]
fn a_list_without_a_start_reaches_back_further_than_a_day() {
    let rig = Rig::new();
    let t = now() - 5 * 86_400;
    rig.ent().trace(
        "told",
        t,
        "a@kweb.test",
        "old@example.org",
        10,
        &[("delivery.attempt-start", &[("queueId", "777")]), ("delivery.completed", &[])],
    );
    seed_traces(&rig);
    let all = rig.core.admin_deliveries(String::new(), None, 0, 50).unwrap();
    assert_eq!(all.total, 5, "the server's own default is one day; the core asks for more");
    assert_eq!(all.deliveries.last().unwrap().id, "told");
    // A start still limits it.
    let recent = rig.core.admin_deliveries(String::new(), Some(now() - 86_400), 0, 50).unwrap();
    assert_eq!(recent.total, 4);
    // A queue id is asked for as one and finds the old message too.
    assert_eq!(rig.core.admin_deliveries("777".into(), None, 0, 50).unwrap().deliveries[0].id, "told");
}

#[test]
fn a_message_to_several_recipients_has_each_of_them() {
    let rig = Rig::new();
    let mut events = serde_json::Map::new();
    events.insert(
        "0".into(),
        json!({"event": "delivery.attempt-start", "timestamp": iso(now() - 600), "keyValues": {
            "0": {"key": "to", "value": {"@type": "List", "value": [
                {"@type": "String", "value": "a@x.test"}, {"@type": "String", "value": "b@y.test"}]}}}}),
    );
    // The trace's own `to` is the list joined with "; ", as the server writes it.
    rig.ent().trace_json(json!({
        "id": "tmany", "timestamp": iso(now() - 600), "from": "a@kweb.test", "to": "a@x.test; b@y.test",
        "size": 5, "events": events,
    }));
    let page = rig.core.admin_deliveries(String::new(), None, 0, 10).unwrap();
    assert_eq!(page.deliveries[0].to, ["a@x.test", "b@y.test"]);
}

#[test]
fn deliveries_page_with_a_total() {
    let rig = Rig::new();
    seed_traces(&rig);
    let first = rig.core.admin_deliveries(String::new(), None, 0, 3).unwrap();
    assert_eq!((first.deliveries.len(), first.total), (3, 4));
    let second = rig.core.admin_deliveries(String::new(), None, 3, 3).unwrap();
    assert_eq!(second.deliveries.iter().map(|d| d.id.as_str()).collect::<Vec<_>>(), ["tdone"]);
    assert_eq!(second.total, 4);
}

#[test]
fn a_delivery_shows_its_steps_in_plain_words() {
    let rig = Rig::new();
    seed_traces(&rig);
    let detail = rig.core.admin_delivery("tdone".into()).unwrap();
    assert_eq!(detail.delivery.id, "tdone");
    assert_eq!(detail.delivery.status, DeliveryStatus::Delivered);
    let titles: Vec<&str> = detail.steps.iter().map(|s| s.title.as_str()).collect();
    assert_eq!(
        titles,
        ["Delivery attempt started", "Delivery to the domain started", "Report sent: delivered", "Delivery finished"]
    );
    let ok = &detail.steps[2];
    assert_eq!(ok.detail, "mx.example.org; they said 250 2.0.0 Ok queued");
    assert_eq!(ok.raw, "delivery.dsn-success hostname=mx.example.org code=250 details=\"2.0.0 Ok queued\"");
    assert_eq!(detail.steps[1].detail, "example.org");
    assert!(detail.steps.windows(2).all(|w| w[0].at <= w[1].at));
}

#[test]
fn an_event_not_in_the_table_shows_its_name_made_readable() {
    let rig = Rig::new();
    rig.ent().trace(
        "tx",
        now() - 100,
        "a@kweb.test",
        "b@example.org",
        1,
        &[("delivery.attempt-start", &[]), ("delivery.some-new-event", &[("zone", "a b")])],
    );
    let detail = rig.core.admin_delivery("tx".into()).unwrap();
    assert_eq!(detail.steps[1].title, "Delivery some new event");
    assert_eq!(detail.steps[1].raw, "delivery.some-new-event zone=\"a b\"");
    assert_eq!(detail.steps[1].detail, "");
}

#[test]
fn elapsed_time_and_typed_values_are_put_in_words() {
    let rig = Rig::new();
    rig.ent().trace_json(json!({
        "id": "tt", "timestamp": iso(now() - 50), "from": "a@kweb.test", "to": ["b@example.org", "c@example.org"], "size": 7,
        "events": {
            "0": {"event": "delivery.attempt-start", "timestamp": iso(now() - 50), "keyValues": {}},
            "1": {"event": "delivery.attempt-end", "timestamp": iso(now() - 49), "keyValues": {
                "0": {"key": "elapsed", "value": {"@type": "Duration", "value": 1234}},
                "1": {"key": "to", "value": {"@type": "List", "value": [
                    {"@type": "String", "value": "b@example.org"}, {"@type": "String", "value": "c@example.org"}]}},
                "2": {"key": "total", "value": {"@type": "UnsignedInt", "value": 2}},
            }},
        }
    }));
    let detail = rig.core.admin_delivery("tt".into()).unwrap();
    assert_eq!(detail.delivery.to, ["b@example.org", "c@example.org"]);
    assert_eq!(detail.steps[1].detail, "for b@example.org,c@example.org; took 1.2 seconds");
    assert_eq!(detail.steps[1].raw, "delivery.attempt-end elapsed=1234 to=b@example.org,c@example.org total=2");
}

#[test]
fn a_delivery_the_server_no_longer_has_is_invalid() {
    let rig = Rig::new();
    assert!(invalid(rig.core.admin_delivery("gone".into()).unwrap_err()).contains("no longer keeps"));
}

// ---- recovery ----

#[test]
fn recovery_state_is_off_or_days() {
    let rig = Rig::new();
    let off = rig.core.admin_recovery_state().unwrap();
    assert_eq!((off.is_on, off.keeps_days), (false, None));
    rig.ent().retention(Some(30 * 86_400_000));
    let on = rig.core.admin_recovery_state().unwrap();
    assert_eq!((on.is_on, on.keeps_days), (true, Some(30)));
}

fn seed_archive(rig: &Rig) {
    let ent = rig.ent();
    let t = now() - 1000;
    ent.archived("m1", ME, "Email", "Plans for Friday", t);
    ent.archived("e1", ME, "CalendarEvent", "Dentist", t + 10);
    ent.archived("c1", ME, "ContactCard", "Ann Lee", t + 20);
    ent.archived("f1", ME, "FileNode", "notes.txt", t + 30);
    ent.archived("s1", ME, "SieveScript", "vacation", t + 40);
    ent.archived("o1", "other", "Email", "Not yours", t + 50);
}

#[test]
fn deleted_items_come_newest_first_for_the_keys_own_account() {
    let rig = Rig::new();
    seed_archive(&rig);
    let page = rig.core.admin_deleted_items(None, 0, 50).unwrap();
    assert_eq!(page.total, 5);
    let ids: Vec<&str> = page.items.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(ids, ["s1", "f1", "c1", "e1", "m1"]);
    let kinds: Vec<DeletedKind> = page.items.iter().map(|i| i.kind).collect();
    assert_eq!(
        kinds,
        [DeletedKind::Other, DeletedKind::File, DeletedKind::Contact, DeletedKind::Event, DeletedKind::Mail]
    );
    assert_eq!(page.items[4].title, "Plans for Friday");
    assert!(page.items[4].deleted_at.unwrap() < now());
    // The own account named by its id is the same.
    assert_eq!(rig.core.admin_deleted_items(Some(ME.into()), 0, 50).unwrap().total, 5);
}

#[test]
fn deleted_items_page_and_are_asked_again_when_the_server_cannot_sort() {
    let rig = Rig::new();
    seed_archive(&rig);
    let page = rig.core.admin_deleted_items(None, 1, 2).unwrap();
    assert_eq!(page.items.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(), ["f1", "c1"]);
    rig.ent().archived_sort(false);
    let page = rig.core.admin_deleted_items(None, 0, 50).unwrap();
    assert_eq!(page.total, 5, "asked again without the sort");
}

#[test]
fn another_accounts_deleted_items_need_impersonation() {
    let rig = Rig::new();
    seed_archive(&rig);
    let theirs = rig.core.admin_deleted_items(Some("other".into()), 0, 50).unwrap();
    assert_eq!(theirs.items.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(), ["o1"]);
    rig.ent().permissions(&["sysArchivedItemQuery", "sysArchivedItemGet", "sysArchivedItemUpdate"]);
    assert!(invalid(rig.core.admin_deleted_items(Some("other".into()), 0, 50).unwrap_err()).contains("another account"));
    assert_eq!(rig.core.admin_deleted_items(None, 0, 50).unwrap().total, 5);
}

#[test]
fn restoring_asks_the_server_to_put_items_back() {
    let rig = Rig::new();
    seed_archive(&rig);
    rig.core.admin_restore(None, vec!["m1".into(), "f1".into()]).unwrap();
    let mut asked = rig.ent().restore_requests();
    asked.sort();
    assert_eq!(asked, ["f1", "m1"]);
    // Nothing to restore is nothing asked.
    rig.core.admin_restore(None, Vec::new()).unwrap();
    assert_eq!(rig.server.request_count("x:ArchivedItem/set"), 1);
}

#[test]
fn restoring_an_item_the_server_no_longer_keeps_says_so() {
    let rig = Rig::new();
    seed_archive(&rig);
    assert!(invalid(rig.core.admin_restore(None, vec!["zzz".into()]).unwrap_err()).contains("no longer keeps"));
}

#[test]
fn restoring_needs_the_right_to_update() {
    let rig = Rig::new();
    seed_archive(&rig);
    rig.ent().permissions(&["sysArchivedItemQuery", "sysArchivedItemGet"]);
    assert_eq!(
        invalid(rig.core.admin_restore(None, vec!["m1".into()]).unwrap_err()),
        "this key may not restore deleted items"
    );
    assert!(rig.ent().restore_requests().is_empty());
}

// ---- alerts and tenants ----

#[test]
fn alerts_are_listed_with_a_summary_and_can_be_switched() {
    let rig = Rig::new();
    let ent = rig.ent();
    ent.alert(json!({
        "id": "a1", "enable": true,
        "condition": {"match": {"0": {"if": "metric('queue.count') > 5000", "then": "true"}}, "else": "false"},
        "emailAlert": {"@type": "Enabled", "fromAddress": "alerts@kweb.test", "to": {"ops@kweb.test": true, "me@kweb.test": true}, "subject": "Queue is long", "body": "x"},
        "eventAlert": {"@type": "Enabled"},
    }));
    ent.alert(json!({
        "id": "a2", "enable": false,
        "condition": {"match": {"0": {"if": "metric('server.memory') >= 2000000000", "then": "true"}}, "else": "false"},
        "emailAlert": {"@type": "Disabled"}, "eventAlert": {"@type": "Disabled"},
    }));
    let alerts = rig.core.admin_alerts().unwrap();
    assert_eq!(alerts.len(), 2);
    let a1 = &alerts[0];
    assert_eq!((a1.id.as_str(), a1.name.as_str(), a1.is_enabled), ("a1", "Queue is long", true));
    assert!(a1.summary.starts_with("When the number of queued messages is above 5000, it emails "), "{}", a1.summary);
    assert!(a1.summary.contains("ops@kweb.test") && a1.summary.contains("me@kweb.test"));
    assert!(a1.summary.ends_with("and records an event on the server."), "{}", a1.summary);
    let a2 = &alerts[1];
    assert!(!a2.is_enabled);
    assert_eq!(a2.summary, "When the memory in use is at least 2000000000, it tells nobody.");
    rig.core.admin_set_alert_enabled("a2".into(), true).unwrap();
    assert_eq!(rig.ent().alert_enabled("a2"), Some(true));
    rig.core.admin_set_alert_enabled("a1".into(), false).unwrap();
    assert_eq!(rig.ent().alert_enabled("a1"), Some(false));
    assert!(rig.core.admin_alerts().unwrap().iter().find(|a| a.id == "a1").is_some_and(|a| !a.is_enabled));
}

#[test]
fn an_alert_that_is_gone_is_invalid() {
    let rig = Rig::new();
    assert!(invalid(rig.core.admin_set_alert_enabled("nope".into(), true).unwrap_err()).contains("no longer there"));
}

#[test]
fn tenants_come_with_quota_and_use() {
    let rig = Rig::new();
    let ent = rig.ent();
    ent.tenant(json!({"id": "t1", "name": "Acme", "quotas": {"maxDiskQuota": 5_000_000}, "usedDiskQuota": 1_234_567}));
    ent.tenant(json!({"id": "t2", "name": "Open", "quotas": {}, "usedDiskQuota": 0}));
    let tenants = rig.core.admin_tenants().unwrap();
    assert_eq!(tenants.len(), 2);
    assert_eq!(
        (tenants[0].name.as_str(), tenants[0].quota, tenants[0].used),
        ("Acme", Some(5_000_000), Some(1_234_567))
    );
    assert_eq!((tenants[1].quota, tenants[1].used), (None, Some(0)));
}

#[test]
fn nothing_secret_reaches_an_error() {
    let rig = Rig::new();
    rig.ent().permissions(&["authenticate"]);
    let errors = [
        format!("{:?}", rig.core.admin_tenants().unwrap_err()),
        format!("{:?}", rig.core.admin_history(5).unwrap_err()),
    ];
    rig.secrets.set("admin_key".into(), "API_wrong_key_value".into()).unwrap();
    let refused = format!("{:?}", rig.core.admin_alerts().unwrap_err());
    for text in errors.iter().chain([&refused]) {
        assert!(!text.contains("API_"), "{text}");
    }
}
