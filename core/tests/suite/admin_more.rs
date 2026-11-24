//! The admin areas added after the first ones, against the mock server, through the `Core` object
//! the way the app uses it: domains (DNS records, DKIM keys, add, switch, delete with the guard),
//! the received reports, the background tasks and the cluster. Each test is about one sentence of
//! the doc comments in `core/src/ffi/admin_more.rs`. The mock behaves like Stalwart 1.1.0 where
//! it matters (see `support/admin_mock/more.rs` and `dev/notes/stalwart-admin-more.md`).

use std::sync::Arc;

use crate::support::{
    admin_mock::{BOB_ID, DOMAIN_ID, OTHER_DOMAIN_ID},
    MockServer, USERNAME,
};
use kweb_core::{
    ffi::{Core, ReportKind, TaskState},
    secrets::MemorySecrets,
    Error,
};
use serde_json::{json, Value};
use tokio::runtime::Runtime;

struct Rig {
    // Field order is drop order: the core goes first, the mock and its runtime last.
    core: Arc<Core>,
    server: MockServer,
    _dir: tempfile::TempDir,
    _rt: Runtime,
}

impl Rig {
    /// Signed in with an admin key that may do everything.
    fn admin() -> Rig {
        Rig::with(None)
    }

    /// Signed in with an admin key that has only these permissions (`None`: all).
    fn with(permissions: Option<&[&str]>) -> Rig {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        let server = rt.block_on(MockServer::start());
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("cache.db").to_string_lossy().into_owned();
        let core = Core::new(db_path, Arc::new(MemorySecrets::default())).unwrap();
        let url = core.begin_sign_in_with_redirect(server.url(), "ovh.kweb.mail:/oauth/callback".into()).unwrap();
        core.complete_sign_in(server.authorize(&url).unwrap()).unwrap();
        let key = server.admin_key(USERNAME, permissions);
        core.admin_connect(key, None).unwrap();
        Rig { core, server, _dir: dir, _rt: rt }
    }

    fn calls(&self, method: &str) -> usize {
        self.server.admin_calls().iter().filter(|m| m.as_str() == method).count()
    }
}

fn invalid<T: std::fmt::Debug>(r: Result<T, Error>) -> String {
    match r {
        Err(Error::Invalid { msg }) => msg,
        other => panic!("expected Error::Invalid, got {other:?}"),
    }
}

// ---- what the key opens ----

#[test]
fn the_new_areas_are_open_where_their_permissions_are() {
    let rig = Rig::admin();
    let a = rig.core.admin_access().unwrap();
    assert!(a.may_domains && a.may_dmarc_reports && a.may_tls_reports && a.may_arf_reports);
    assert!(a.may_tasks && a.may_cluster && a.may_queue_control);

    let rig =
        Rig::with(Some(&["sysDomainQuery", "sysDomainGet", "sysTlsExternalReportQuery", "sysTlsExternalReportGet"]));
    let a = rig.core.admin_access().unwrap();
    assert!(a.may_domains && a.may_tls_reports);
    assert!(!a.may_dmarc_reports && !a.may_arf_reports && !a.may_tasks && !a.may_cluster && !a.may_queue_control);
    // Reading alone is not enough for a list, and a half of the pair is not either.
    let rig = Rig::with(Some(&["sysTaskQuery", "sysClusterNodeGet", "sysActionCreate", "actionPauseMtaQueue"]));
    let a = rig.core.admin_access().unwrap();
    assert!(!a.may_tasks && !a.may_cluster && a.may_queue_control);
}

#[test]
fn a_key_that_opens_only_a_new_area_is_taken() {
    let rig = Rig::with(Some(&["sysClusterNodeQuery", "sysClusterNodeGet"]));
    assert!(rig.core.admin_access().unwrap().connected);
}

// ---- domains ----

#[test]
fn the_domains_come_by_name_with_whether_they_take_mail() {
    let rig = Rig::admin();
    let rows = rig.core.admin_domain_rows().unwrap();
    let names: Vec<&str> = rows.iter().map(|d| d.name.as_str()).collect();
    assert_eq!(names, ["example.test", "other.test"]);
    assert!(rows.iter().all(|d| d.enabled && d.aliases.is_empty()));
}

#[test]
fn a_domain_shows_the_records_to_publish_and_its_keys() {
    let rig = Rig::admin();
    let added = rig.core.admin_domain_add("Mail.Example.org".into()).unwrap();
    let d = rig.core.admin_domain(added.id.clone()).unwrap();
    assert_eq!(d.name, "mail.example.org");
    assert!(d.dns_zone.contains("mail.example.org. IN MX 10"), "{}", d.dns_zone);
    assert!(d.dkim_allowed && d.dkim_automatic);
    assert_eq!(d.dkim.len(), 2);
    let algorithms: Vec<&str> = d.dkim.iter().map(|k| k.algorithm.as_str()).collect();
    assert!(algorithms.contains(&"Ed25519 (SHA-256)") && algorithms.contains(&"RSA (SHA-256)"), "{algorithms:?}");
    assert!(d.dkim.iter().all(|k| k.stage == "active" && k.created_at.is_some()));
    // A domain whose keys are kept by hand says so.
    let hand = rig.core.admin_domain(DOMAIN_ID.into()).unwrap();
    assert!(!hand.dkim_automatic && hand.dkim.is_empty(), "{:?} {}", hand.dkim, hand.dkim_automatic);
}

#[test]
fn a_key_that_may_not_read_dkim_still_sees_the_domain() {
    let rig = Rig::with(Some(&["sysDomainQuery", "sysDomainGet"]));
    let d = rig.core.admin_domain(DOMAIN_ID.into()).unwrap();
    assert!(!d.dkim_allowed && d.dkim.is_empty());
    assert!(!d.dns_zone.is_empty());
}

#[test]
fn a_domain_that_is_gone_is_invalid() {
    let rig = Rig::admin();
    assert!(invalid(rig.core.admin_domain("nope".into())).contains("no longer"));
}

#[test]
fn a_domain_is_added_in_lower_case_and_a_bad_name_is_not_even_sent() {
    let rig = Rig::admin();
    let d = rig.core.admin_domain_add("  New.Example.ORG ".into()).unwrap();
    assert_eq!((d.name.as_str(), d.enabled), ("new.example.org", true));
    assert_eq!(rig.calls("x:Domain/set"), 1);
    for bad in ["", "a b.test", "-x.test", "a..test", "ü.test", "x@y.test"] {
        assert!(invalid(rig.core.admin_domain_add(bad.into())).contains("letters, digits"), "{bad}");
    }
    assert_eq!(rig.calls("x:Domain/set"), 1);
}

#[test]
fn a_domain_that_exists_is_invalid_saying_so() {
    let rig = Rig::admin();
    assert!(invalid(rig.core.admin_domain_add("EXAMPLE.test".into())).contains("already"));
}

#[test]
fn mail_for_a_domain_is_switched_off_and_on() {
    let rig = Rig::admin();
    rig.core.admin_domain_set_enabled(OTHER_DOMAIN_ID.into(), false).unwrap();
    assert_eq!(rig.server.server_domain(OTHER_DOMAIN_ID).unwrap()["enable"], false);
    let rows = rig.core.admin_domain_rows().unwrap();
    assert!(!rows.iter().find(|d| d.id == OTHER_DOMAIN_ID).unwrap().enabled);
    rig.core.admin_domain_set_enabled(OTHER_DOMAIN_ID.into(), true).unwrap();
    assert_eq!(rig.server.server_domain(OTHER_DOMAIN_ID).unwrap()["enable"], true);
    assert!(invalid(rig.core.admin_domain_set_enabled("nope".into(), false)).contains("no longer"));
}

#[test]
fn an_unused_domain_is_deleted_with_its_keys() {
    let rig = Rig::admin();
    let d = rig.core.admin_domain_add("gone.example.org".into()).unwrap();
    assert_eq!(rig.server.dkim_count(&d.id), 2);
    rig.core.admin_domain_delete(d.id.clone()).unwrap();
    assert!(rig.server.server_domain(&d.id).is_none());
    assert_eq!(rig.server.dkim_count(&d.id), 0);
    // One that is gone already is not an error.
    rig.core.admin_domain_delete(d.id).unwrap();
}

#[test]
fn a_domain_an_account_uses_is_not_deleted_and_the_server_is_not_asked() {
    let rig = Rig::admin();
    let msg = invalid(rig.core.admin_domain_delete(DOMAIN_ID.into()));
    assert!(msg.contains("account") && msg.contains("still use"), "{msg}");
    assert_eq!(rig.calls("x:Domain/set"), 0);
    assert!(rig.server.server_domain(DOMAIN_ID).is_some());
}

#[test]
fn a_domain_with_an_alias_in_it_is_not_deleted() {
    let rig = Rig::admin();
    let d = rig.core.admin_domain_add("alias.example.org".into()).unwrap();
    rig.server.account_alias(BOB_ID, "bobby", &d.id);
    let msg = invalid(rig.core.admin_domain_delete(d.id.clone()));
    assert!(msg.contains("alias"), "{msg}");
    assert_eq!(rig.server.dkim_count(&d.id), 2);
    assert!(rig.server.server_domain(&d.id).is_some());
}

#[test]
fn a_domain_the_server_says_is_used_by_something_else_keeps_its_keys() {
    let rig = Rig::admin();
    let d = rig.core.admin_domain_add("list.example.org".into()).unwrap();
    rig.server.domain_link(&d.id, "MailingList");
    let msg = invalid(rig.core.admin_domain_delete(d.id.clone()));
    assert!(msg.contains("mailing lists"), "{msg}");
    assert_eq!(rig.server.dkim_count(&d.id), 2, "the keys must stay when the domain stays");
    assert!(rig.server.server_domain(&d.id).is_some());
}

#[test]
fn a_key_that_cannot_look_at_the_accounts_does_not_delete_a_domain() {
    let rig = Rig::with(Some(&["sysDomainQuery", "sysDomainGet", "sysDomainCreate", "sysDomainDestroy"]));
    let d = rig.core.admin_domain_add("careful.example.org".into()).unwrap();
    let msg = invalid(rig.core.admin_domain_delete(d.id.clone()));
    assert!(msg.contains("may not"), "{msg}");
    assert!(rig.server.server_domain(&d.id).is_some());
}

#[test]
fn a_key_without_the_right_cannot_change_domains() {
    let rig = Rig::with(Some(&["sysDomainQuery", "sysDomainGet"]));
    assert!(invalid(rig.core.admin_domain_add("x.example.org".into())).contains("may not"));
    assert!(invalid(rig.core.admin_domain_set_enabled(DOMAIN_ID.into(), false)).contains("may not"));
}

// ---- reports ----

fn dmarc(received: &str, org: &str, records: Value) -> Value {
    json!({
        "from": format!("noreply@{org}"), "to": ["postmaster@example.test"], "subject": format!("Report Domain: example.test Submitter: {org}"),
        "receivedAt": received, "expiresAt": "2027-02-15T08:00:00Z",
        "report": {
            "orgName": org, "email": format!("dmarc@{org}"), "reportId": "r-1", "policyDomain": "example.test",
            "policyDisposition": "reject", "policyAdkim": "relaxed", "policyAspf": "strict", "policyTestingMode": false,
            "dateRangeStart": "2027-01-14T00:00:00Z", "dateRangeEnd": "2027-01-15T00:00:00Z", "records": records,
        },
    })
}

fn record(ip: &str, count: u64, dkim: &str, spf: &str) -> Value {
    json!({
        "sourceIp": ip, "count": count, "evaluatedDisposition": "none", "evaluatedDkim": dkim, "evaluatedSpf": spf,
        "headerFrom": "example.test", "envelopeFrom": "example.test",
        "dkimResults": [{ "domain": "example.test", "selector": "s1", "result": dkim }],
        "spfResults": [{ "domain": "example.test", "scope": "mailFrom", "result": spf }],
        "policyOverrideReasons": [],
    })
}

#[test]
fn reports_come_newest_first_with_what_passed_and_failed() {
    let rig = Rig::admin();
    rig.server.report_add(
        "dmarc",
        dmarc("2027-01-15T08:00:00Z", "old.example", json!({ "0": record("192.0.2.1", 4, "pass", "pass") })),
    );
    rig.server.report_add(
        "dmarc",
        dmarc(
            "2027-01-16T08:00:00Z",
            "new.example",
            json!({ "0": record("192.0.2.9", 10, "pass", "fail"), "1": record("198.51.100.7", 3, "fail", "fail") }),
        ),
    );
    let page = rig.core.admin_reports(ReportKind::Dmarc, 0, 10).unwrap();
    assert_eq!(page.total, 2);
    let first = &page.reports[0];
    assert_eq!((first.organization.as_str(), first.domain.as_str()), ("new.example", "example.test"));
    assert_eq!((first.passed, first.failed), (Some(10), Some(3)));
    assert_eq!(page.reports[1].organization, "old.example");
    assert_eq!(page.reports[1].passed, Some(4));
    // The server cannot sort, so the order is made here and a second page continues it.
    let rest = rig.core.admin_reports(ReportKind::Dmarc, 1, 10).unwrap();
    assert_eq!(rest.reports.len(), 1);
    assert_eq!(rest.reports[0].organization, "old.example");
    assert_eq!(rest.total, 2);
    // The other kinds are separate lists.
    assert_eq!(rig.core.admin_reports(ReportKind::Tls, 0, 10).unwrap().total, 0);
}

#[test]
fn a_page_of_reports_holds_at_most_a_hundred() {
    let rig = Rig::admin();
    for n in 0..105 {
        rig.server.report_add("arf", json!({ "from": "x@y.test", "subject": "s", "receivedAt": format!("2027-01-15T08:{:02}:00Z", n % 60), "report": {} }));
    }
    let page = rig.core.admin_reports(ReportKind::Arf, 0, 500).unwrap();
    assert_eq!((page.reports.len(), page.total), (100, 105));
}

#[test]
fn a_dmarc_report_shows_each_source_with_its_figures_in_words() {
    let rig = Rig::admin();
    let id = rig.server.report_add(
        "dmarc",
        dmarc(
            "2027-01-16T08:00:00Z",
            "mail.example",
            json!({ "0": record("198.51.100.7", 3, "fail", "fail"), "1": record("192.0.2.9", 10, "pass", "softFail") }),
        ),
    );
    let d = rig.core.admin_report(ReportKind::Dmarc, id).unwrap();
    assert_eq!(d.subject, "Report Domain: example.test Submitter: mail.example");
    assert_eq!(d.to, ["postmaster@example.test"]);
    assert!(d.period_start.is_some() && d.period_end > d.period_start && d.expires_at.is_some());
    let fact = |label: &str| d.summary.iter().find(|f| f.label == label).map(|f| f.value.as_str());
    assert_eq!(fact("Sent by"), Some("mail.example"));
    assert_eq!(fact("Policy"), Some("p=reject"));
    assert_eq!(fact("Messages that passed"), Some("10"));
    assert_eq!(fact("Messages that failed"), Some("3"));
    // The busiest source first.
    assert_eq!(d.groups[0].title, "192.0.2.9 - 10 messages");
    assert_eq!(d.groups[1].title, "198.51.100.7 - 3 messages");
    let first = |label: &str| d.groups[0].facts.iter().find(|f| f.label == label).map(|f| f.value.as_str());
    assert_eq!(first("Result"), Some("passed"));
    assert_eq!(first("SPF"), Some("soft fail"));
    assert_eq!(first("SPF checks"), Some("example.test: soft fail"));
    assert_eq!(first("DKIM signatures"), Some("example.test (s1): pass"));
}

#[test]
fn a_tls_report_shows_each_policy_and_what_went_wrong() {
    let rig = Rig::admin();
    let id = rig.server.report_add(
        "tls",
        json!({
            "from": "tls-rpt@example.net", "to": ["postmaster@example.test"], "subject": "TLS report", "receivedAt": "2027-01-16T08:00:00Z",
            "report": {
                "organizationName": "Example Net", "contactInfo": "tls@example.net", "reportId": "t-9",
                "dateRangeStart": "2027-01-15T00:00:00Z", "dateRangeEnd": "2027-01-16T00:00:00Z",
                "policies": [{
                    "policyDomain": "example.test", "policyType": "sts", "mxHosts": ["mail.example.test"],
                    "totalSuccessfulSessions": 40, "totalFailedSessions": 2,
                    "failureDetails": [{ "resultType": "certificateExpired", "failedSessionCount": 2, "receivingMxHostname": "mail.example.test" }],
                }],
            },
        }),
    );
    let row = &rig.core.admin_reports(ReportKind::Tls, 0, 10).unwrap().reports[0];
    assert_eq!((row.passed, row.failed, row.domain.as_str()), (Some(40), Some(2), "example.test"));
    let d = rig.core.admin_report(ReportKind::Tls, id).unwrap();
    assert_eq!(d.groups[0].title, "example.test (MTA-STS)");
    let facts: Vec<(&str, &str)> = d.groups[0].facts.iter().map(|f| (f.label.as_str(), f.value.as_str())).collect();
    assert!(facts.contains(&("What went wrong", "certificate expired: 2 sessions (mail.example.test)")), "{facts:?}");
    assert!(facts.contains(&("Sessions that failed", "2")));
}

#[test]
fn an_abuse_report_shows_its_figures_and_none_of_the_message() {
    let rig = Rig::admin();
    let id = rig.server.report_add(
        "arf",
        json!({
            "from": "abuse@example.net", "to": ["postmaster@example.test"], "subject": "abuse report", "receivedAt": "2027-01-16T08:00:00Z",
            "report": {
                "feedbackType": "abuse", "reportingMta": "mta.example.net", "incidents": 3, "sourceIp": "203.0.113.5",
                "reportedDomains": ["example.test"], "deliveryResult": "delivered",
                "message": "Subject: SECRET CONTENT", "headers": "From: someone@example.test\nSubject: SECRET CONTENT",
            },
        }),
    );
    let row = &rig.core.admin_reports(ReportKind::Arf, 0, 10).unwrap().reports[0];
    assert_eq!(
        (row.note.as_str(), row.failed, row.passed, row.domain.as_str()),
        ("abuse", Some(3), None, "example.test")
    );
    let d = rig.core.admin_report(ReportKind::Arf, id).unwrap();
    assert!(d.groups.is_empty());
    let all: String = d.summary.iter().map(|f| format!("{}={}\n", f.label, f.value)).collect();
    assert!(
        all.contains("Source address=203.0.113.5") && all.contains("What happened to the message=delivered"),
        "{all}"
    );
    assert!(!all.contains("SECRET"), "{all}");
}

#[test]
fn a_report_that_is_gone_is_invalid_and_deleting_one_that_is_gone_is_not() {
    let rig = Rig::admin();
    assert!(invalid(rig.core.admin_report(ReportKind::Tls, "nope".into())).contains("no longer"));
    rig.core.admin_report_delete(ReportKind::Tls, "nope".into()).unwrap();
}

#[test]
fn a_report_is_deleted_and_only_the_one() {
    let rig = Rig::admin();
    let a = rig.server.report_add("dmarc", dmarc("2027-01-15T08:00:00Z", "a.example", json!([])));
    let b = rig.server.report_add("dmarc", dmarc("2027-01-16T08:00:00Z", "b.example", json!([])));
    rig.core.admin_report_delete(ReportKind::Dmarc, a.clone()).unwrap();
    assert_eq!(rig.server.report_ids("dmarc"), [b]);
}

#[test]
fn a_key_that_may_only_read_reports_cannot_delete_them() {
    let rig = Rig::with(Some(&["sysDmarcExternalReportQuery", "sysDmarcExternalReportGet"]));
    let id = rig.server.report_add("dmarc", dmarc("2027-01-15T08:00:00Z", "a.example", json!([])));
    assert!(invalid(rig.core.admin_report_delete(ReportKind::Dmarc, id.clone())).contains("may not"));
    assert_eq!(rig.server.report_ids("dmarc"), [id]);
    // And a kind the key has no right to is refused in words, not as a failure.
    assert!(invalid(rig.core.admin_reports(ReportKind::Tls, 0, 10)).contains("may not"));
}

// ---- tasks ----

fn task(kind: &str, status: Value) -> Value {
    json!({ "@type": kind, "status": status })
}

fn pending(due: &str) -> Value {
    json!({ "@type": "Pending", "due": due, "createdAt": "2027-01-01T00:00:00Z" })
}

#[test]
fn tasks_come_due_first_with_their_state_and_why_one_failed() {
    let rig = Rig::admin();
    let mut later = task("TlsReport", pending("2027-03-01T00:00:00Z"));
    later["reportId"] = json!("x");
    rig.server.task_add(later);
    let mut dkim = task("DkimManagement", pending("2027-02-01T00:00:00Z"));
    dkim["domainId"] = json!(DOMAIN_ID);
    rig.server.task_add(dkim);
    let failed = task(
        "DmarcReport",
        json!({ "@type": "Failed", "failedAttemptNumber": 6, "failureReason": "the remote\nserver said no", "failedAt": "2027-02-02T00:00:00Z" }),
    );
    rig.server.task_add(failed);
    let page = rig.core.admin_tasks(None, 0, 10).unwrap();
    assert_eq!(page.total, 3);
    let kinds: Vec<&str> = page.tasks.iter().map(|t| t.kind.as_str()).collect();
    assert_eq!(kinds[..2], ["Send a DMARC report", "Rotate DKIM keys"]);
    let dkim = &page.tasks[1];
    assert_eq!((dkim.state, dkim.subject.as_str(), dkim.attempts), (TaskState::Waiting, "example.test", 0));
    let failed = page.tasks.iter().find(|t| t.state == TaskState::Failed).unwrap();
    assert_eq!((failed.attempts, failed.reason.as_str()), (6, "the remote server said no"));
    assert_eq!(failed.kind_code, "DmarcReport");
    // A state narrows the list.
    let only = rig.core.admin_tasks(Some(TaskState::Failed), 0, 10).unwrap();
    assert_eq!((only.tasks.len(), only.total), (1, 1));
    let retrying = rig.core.admin_tasks(Some(TaskState::Retrying), 0, 10).unwrap();
    assert!(retrying.tasks.is_empty());
}

#[test]
fn a_page_of_tasks_continues_where_the_last_ended() {
    let rig = Rig::admin();
    for n in 0..5 {
        rig.server.task_add(task("MergeThreads", pending(&format!("2027-02-0{}T00:00:00Z", n + 1))));
    }
    let first = rig.core.admin_tasks(None, 0, 2).unwrap();
    let second = rig.core.admin_tasks(None, 2, 2).unwrap();
    assert_eq!((first.tasks.len(), second.tasks.len(), first.total), (2, 2, 5));
    assert_ne!(first.tasks[0].id, second.tasks[0].id);
}

#[test]
fn a_task_is_run_again_by_making_it_due_now() {
    let rig = Rig::admin();
    let id = rig.server.task_add(task(
        "DmarcReport",
        json!({ "@type": "Failed", "failedAttemptNumber": 6, "failureReason": "no", "failedAt": "2027-02-02T00:00:00Z" }),
    ));
    rig.core.admin_task_retry(id.clone()).unwrap();
    let held = rig.server.server_task(&id).unwrap();
    assert_eq!(held["status"]["@type"], "Pending");
    assert!(held["status"]["due"].as_str().unwrap() < "2001");
    // Now it waits for its time, and a waiting task is not run early (a reminder sent early, keys rotated early).
    let msg = invalid(rig.core.admin_task_retry(id));
    assert!(msg.contains("has not failed"), "{msg}");
    // The task the server is running now cannot be touched: words, not a failure.
    let busy = rig
        .server
        .task_add(task("DmarcReport", json!({ "@type": "Retry", "attemptNumber": 2, "due": "2027-02-02T00:00:00Z" })));
    rig.server.task_set_busy(&busy);
    assert!(invalid(rig.core.admin_task_retry(busy)).contains("right now"));
    assert!(invalid(rig.core.admin_task_retry("nope".into())).contains("no longer"));
}

#[test]
fn a_task_the_server_keeps_for_itself_is_not_deleted() {
    let rig = Rig::admin();
    for kind in ["AcmeRenewal", "DkimManagement", "StoreMaintenance", "DmarcReportWindow", "SomethingNew"] {
        let id = rig.server.task_add(task(kind, pending("2027-02-01T00:00:00Z")));
        let msg = invalid(rig.core.admin_task_delete(id.clone()));
        assert!(msg.contains("its own work"), "{kind}: {msg}");
        assert!(rig.server.server_task(&id).is_some());
    }
    assert_eq!(rig.calls("x:Task/set"), 0);
}

#[test]
fn a_task_is_deleted_and_one_that_is_gone_is_not_an_error() {
    let rig = Rig::admin();
    let id = rig.server.task_add(task("MergeThreads", pending("2027-02-01T00:00:00Z")));
    let other = rig.server.task_add(task("MergeThreads", pending("2027-02-02T00:00:00Z")));
    rig.core.admin_task_delete(id.clone()).unwrap();
    assert_eq!(rig.server.task_ids(), [other]);
    rig.core.admin_task_delete(id).unwrap();
}

#[test]
fn the_task_that_finishes_an_accounts_deletion_is_not_deleted() {
    let rig = Rig::admin();
    let id = rig.server.task_add(task("DestroyAccount", pending("2027-02-01T00:00:00Z")));
    let msg = invalid(rig.core.admin_task_delete(id.clone()));
    assert!(msg.contains("deletion of an account"), "{msg}");
    assert!(rig.server.server_task(&id).is_some());
    assert_eq!(rig.calls("x:Task/set"), 0);
    // Running it again is allowed once it failed.
    let failed = rig.server.task_add(task(
        "DestroyAccount",
        json!({ "@type": "Failed", "failedAttemptNumber": 1, "failureReason": "no" }),
    ));
    rig.core.admin_task_retry(failed).unwrap();
}

#[test]
fn a_key_that_may_only_read_tasks_cannot_change_them() {
    let rig = Rig::with(Some(&["sysTaskQuery", "sysTaskGet"]));
    let id = rig
        .server
        .task_add(task("MergeThreads", json!({ "@type": "Failed", "failedAttemptNumber": 1, "failureReason": "no" })));
    assert!(invalid(rig.core.admin_task_retry(id.clone())).contains("may not"));
    assert!(invalid(rig.core.admin_task_delete(id.clone())).contains("may not"));
    assert!(rig.server.server_task(&id).is_some());
}

// ---- the cluster ----

#[test]
fn the_cluster_has_its_nodes_by_number() {
    let rig = Rig::admin();
    rig.server.cluster_set(vec![
        json!({ "id": "b", "nodeId": 2, "hostname": "two.example.test", "status": "stale", "lastRenewal": "2027-01-15T07:00:00Z" }),
        json!({ "id": "a", "nodeId": 1, "hostname": "one.example.test", "status": "active", "lastRenewal": "2027-01-15T08:00:00Z" }),
    ]);
    let nodes = rig.core.admin_cluster().unwrap();
    let got: Vec<(u32, &str, &str)> =
        nodes.iter().map(|n| (n.node_id, n.hostname.as_str(), n.status.as_str())).collect();
    assert_eq!(got, [(1, "one.example.test", "active"), (2, "two.example.test", "stale")]);
    assert!(nodes[0].last_renewal > nodes[1].last_renewal);
}

#[test]
fn nothing_of_the_new_areas_is_cached_between_calls() {
    let rig = Rig::admin();
    rig.core.admin_cluster().unwrap();
    rig.core.admin_cluster().unwrap();
    assert_eq!(rig.calls("x:ClusterNode/get"), 2);
}
