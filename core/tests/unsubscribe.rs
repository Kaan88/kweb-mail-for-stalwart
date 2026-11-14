//! M10 against the mock server: leaving a mailing list, through the `Core` object the way the
//! app uses it. The mock gives the List-Unsubscribe headers as Stalwart 1.1.0 does: as a list of
//! addresses for `:asURLs`, and it fails the whole request for `:asText`
//! (`dev/notes/stalwart-mail-features.md`, section 5).
//!
//! The one-click request itself is an https POST to a public host, which cannot be answered by
//! a test double on this machine: its rules (the request, the address checks) are unit tests in
//! `unsubscribe.rs`. Here the choice between the ways, the refusals and the mail are tested.

mod support;

use std::sync::Arc;

use kweb_core::{
    ffi::{Core, Shade},
    model::Unsubscribe,
    secrets::MemorySecrets,
    Error,
};
use support::{MockServer, USERNAME};
use tokio::runtime::Runtime;

const E1: &str = "e1";
const E2: &str = "e2";
const ONE_CLICK: &str = "List-Unsubscribe=One-Click";

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

    fn refusal<T: std::fmt::Debug>(&self, result: Result<T, Error>) -> String {
        match result {
            Err(Error::Invalid { msg }) => msg,
            other => panic!("expected a refusal, got {other:?}"),
        }
    }
}

fn info(one_click: bool, by_mail: bool, web: Option<&str>) -> Option<Unsubscribe> {
    Some(Unsubscribe { one_click, by_mail, web_url: web.map(str::to_string) })
}

// ---- what a list offers --------------------------------------------------------------------

#[test]
fn both_ways_and_the_post_header_give_one_click_by_mail_and_a_web_page() {
    let rig = Rig::new();
    rig.server.set_unsubscribe_headers(
        E1,
        Some("<https://lists.example.org/u/abc?x=1&y=2>, <mailto:unsub@lists.example.org?subject=unsubscribe>"),
        Some(ONE_CLICK),
    );
    assert_eq!(
        rig.core.unsubscribe_info(E1.into()).unwrap(),
        info(true, true, Some("https://lists.example.org/u/abc?x=1&y=2"))
    );
    assert!(rig.server.request_count("Email/get") > 0);
}

#[test]
fn one_click_needs_the_exact_post_header() {
    let rig = Rig::new();
    let list = "<https://lists.example.org/u>";
    for post in [None, Some("List-Unsubscribe=one-click"), Some("Something=Else"), Some("")] {
        rig.server.set_unsubscribe_headers(E1, Some(list), post);
        assert_eq!(
            rig.core.unsubscribe_info(E1.into()).unwrap(),
            info(false, false, Some("https://lists.example.org/u"))
        );
    }
    // The header with the space a raw header has in front of it is the same header.
    rig.server.set_unsubscribe_headers(E1, Some(list), Some(" List-Unsubscribe=One-Click"));
    assert_eq!(rig.core.unsubscribe_info(E1.into()).unwrap(), info(true, false, Some("https://lists.example.org/u")));
    // The post header alone, with no https address: nothing to post to.
    rig.server.set_unsubscribe_headers(E1, Some("<mailto:u@lists.example.org>"), Some(ONE_CLICK));
    assert_eq!(rig.core.unsubscribe_info(E1.into()).unwrap(), info(false, true, None));
}

#[test]
fn http_addresses_are_ignored_and_the_first_https_address_is_the_web_page() {
    let rig = Rig::new();
    rig.server.set_unsubscribe_headers(
        E1,
        Some("(Use this) <http://lists.example.org/plain>,\r\n <https://lists.example.org/one>,\r\n\t<https://lists.example.org/two>"),
        None,
    );
    assert_eq!(
        rig.core.unsubscribe_info(E1.into()).unwrap(),
        info(false, false, Some("https://lists.example.org/one"))
    );
    rig.server.set_unsubscribe_headers(E1, Some("<http://lists.example.org/plain>"), Some(ONE_CLICK));
    assert_eq!(rig.core.unsubscribe_info(E1.into()).unwrap(), None, "nothing usable");
    rig.server.set_unsubscribe_headers(E1, Some("<ftp://x.org/u>, <javascript:alert(1)>"), None);
    assert_eq!(rig.core.unsubscribe_info(E1.into()).unwrap(), None);
}

#[test]
fn mail_without_the_headers_says_nothing_and_the_text_form_is_never_asked() {
    let rig = Rig::new();
    // The mock fails the whole `Email/get` for `header:List-Unsubscribe:asText`, as the server does.
    assert_eq!(rig.core.unsubscribe_info(E2.into()).unwrap(), None);
    rig.server.set_unsubscribe_headers(E2, Some("<mailto:u@lists.example.org>"), None);
    assert_eq!(rig.core.unsubscribe_info(E2.into()).unwrap(), info(false, true, None));
}

#[test]
fn a_message_the_server_does_not_have_is_refused() {
    let rig = Rig::new();
    rig.server.destroy(E1);
    rig.refusal(rig.core.unsubscribe_info(E1.into()));
    rig.refusal(rig.core.unsubscribe(E1.into()));
    rig.refusal(rig.core.unsubscribe_info("never".into()));
}

#[test]
fn an_unreachable_server_is_a_passing_trouble() {
    let rig = Rig::new();
    rig.server.set_down(true);
    assert!(matches!(rig.core.unsubscribe_info(E1.into()), Err(Error::Temporary { .. })));
    assert!(matches!(rig.core.unsubscribe(E1.into()), Err(Error::Temporary { .. })));
}

// ---- leaving by mail -----------------------------------------------------------------------

#[test]
fn leaving_by_mail_sends_a_plain_message_from_the_default_address_at_once() {
    let rig = Rig::new();
    rig.server.set_unsubscribe_headers(
        E1,
        Some("<mailto:leave@lists.example.org?subject=Unsub%20me&body=Please%0D%0Aremove%20me>"),
        None,
    );
    rig.core.unsubscribe(E1.into()).unwrap();
    // With no delay it goes with the next pass.
    rig.sync();
    let sent = rig.server.submissions();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].recipients, ["leave@lists.example.org"]);
    assert_eq!(sent[0].identity_id, "i1", "the default address");
    let mail = rig.server.view(&sent[0].email_id);
    assert_eq!(mail.from.1, USERNAME);
    assert_eq!(mail.subject, "Unsub me");
    assert_eq!(mail.text.as_deref(), Some("Please\nremove me"), "no signature, the text the address gives");
    assert!(mail.cc.is_empty() && mail.bcc.is_empty());
    // Nothing is left in the drafts, and no second message goes out.
    assert!(rig.core.drafts().unwrap().is_empty());
    assert!(rig.core.pending_sends().unwrap().is_empty());
    rig.sync();
    assert_eq!(rig.server.submissions().len(), 1);
}

#[test]
fn a_mailto_without_a_subject_or_text_gets_a_subject_and_no_text() {
    let rig = Rig::new();
    rig.server.set_unsubscribe_headers(E1, Some("<mailto:u@lists.example.org?cc=x@y.org&bcc=z@y.org>"), None);
    rig.core.unsubscribe(E1.into()).unwrap();
    rig.sync();
    let sent = rig.server.submissions();
    assert_eq!(sent[0].recipients, ["u@lists.example.org"], "no Cc or Bcc from the address");
    let mail = rig.server.view(&sent[0].email_id);
    assert_eq!(mail.subject, "unsubscribe");
    assert_eq!(mail.text.as_deref().unwrap_or("").trim(), "");
}

#[test]
fn a_long_subject_and_text_are_cut() {
    let rig = Rig::new();
    let long = "x".repeat(5000);
    rig.server.set_unsubscribe_headers(
        E1,
        Some(&format!("<mailto:u@lists.example.org?subject={long}&body={long}>")),
        None,
    );
    rig.core.unsubscribe(E1.into()).unwrap();
    rig.sync();
    let mail = rig.server.view(&rig.server.submissions()[0].email_id);
    assert_eq!(mail.subject.chars().count(), 200);
    assert_eq!(mail.text.unwrap().chars().count(), 1000);
}

#[test]
fn without_an_address_to_send_from_nothing_is_sent_and_no_draft_is_left() {
    let rig = Rig::new();
    rig.server.set_identities(&[]);
    // The identities are cached for an hour: a new core fetches none.
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("cache.db").to_string_lossy().into_owned();
    let other = Core::new(db, Arc::new(MemorySecrets::default())).unwrap();
    let url = other.begin_sign_in_with_redirect(rig.server.url(), "ovh.kweb.mail:/oauth/callback".into()).unwrap();
    other.complete_sign_in(rig.server.authorize(&url).unwrap()).unwrap();
    other.check_once(Shade::Unknown).unwrap();
    rig.server.set_unsubscribe_headers(E1, Some("<mailto:u@lists.example.org>"), None);
    rig.refusal(other.unsubscribe(E1.into()));
    assert!(other.drafts().unwrap().is_empty());
    other.check_once(Shade::Unknown).unwrap();
    assert!(rig.server.submissions().is_empty());
}

// ---- the choice and the refusals -----------------------------------------------------------

#[test]
fn a_list_with_no_usable_way_is_refused() {
    let rig = Rig::new();
    rig.refusal(rig.core.unsubscribe(E2.into()));
    // Only a web page: the host opens it, the core does not.
    rig.server.set_unsubscribe_headers(E1, Some("<https://lists.example.org/u>"), None);
    let why = rig.refusal(rig.core.unsubscribe(E1.into()));
    assert!(why.contains("web page"), "{why}");
    // http is ignored altogether.
    rig.server.set_unsubscribe_headers(E1, Some("<http://lists.example.org/u>"), Some(ONE_CLICK));
    rig.refusal(rig.core.unsubscribe(E1.into()));
    rig.sync();
    assert!(rig.server.submissions().is_empty());
}

#[test]
fn a_one_click_address_on_this_device_or_a_private_network_is_never_asked() {
    let rig = Rig::new();
    // The mock's own address is on this device: a request would show in its counts.
    for address in [
        rig.server.url().replace("http://", "https://"),
        "https://127.0.0.1:9/u".to_string(),
        "https://localhost/u".to_string(),
        "https://[::1]/u".to_string(),
        "https://192.168.1.1/u".to_string(),
        "https://10.1.2.3/u".to_string(),
        "https://169.254.169.254/latest".to_string(),
        "https://printer.local/u".to_string(),
        "https://intranet/u".to_string(),
    ] {
        rig.server.set_unsubscribe_headers(E1, Some(&format!("<{address}>")), Some(ONE_CLICK));
        assert!(rig.core.unsubscribe_info(E1.into()).unwrap().unwrap().one_click, "{address}: the list says so");
        let why = rig.refusal(rig.core.unsubscribe(E1.into()));
        assert!(why.contains("private network"), "{address}: {why}");
    }
}

#[test]
fn a_one_click_address_that_is_not_allowed_leaves_by_mail_when_the_list_has_that() {
    let rig = Rig::new();
    rig.server.set_unsubscribe_headers(
        E1,
        Some("<https://127.0.0.1:9/u>, <mailto:u@lists.example.org?subject=bye>"),
        Some(ONE_CLICK),
    );
    rig.core.unsubscribe(E1.into()).unwrap();
    rig.sync();
    let sent = rig.server.submissions();
    assert_eq!(sent.len(), 1);
    assert_eq!(rig.server.view(&sent[0].email_id).subject, "bye");
}

#[test]
fn a_list_that_cannot_be_reached_is_a_passing_trouble_and_sends_no_mail() {
    let rig = Rig::new();
    // A name that cannot exist (RFC 6761), so nothing is asked of anyone.
    rig.server.set_unsubscribe_headers(
        E1,
        Some("<https://lists.example.invalid/u/abc>, <mailto:u@lists.example.org>"),
        Some(ONE_CLICK),
    );
    match rig.core.unsubscribe(E1.into()) {
        Err(Error::Temporary { msg }) => {
            assert!(msg.contains("lists.example.invalid"), "{msg}");
            assert!(!msg.contains("/u/abc"), "the address of the request is not repeated: {msg}");
        }
        other => panic!("expected a passing trouble, got {other:?}"),
    }
    rig.sync();
    assert!(rig.server.submissions().is_empty(), "the mail is not a second try");
}
