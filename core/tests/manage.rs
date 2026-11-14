//! Admin design, step 1, against the mock server and through the `Core` object as the app uses
//! it: what the server offers, masked addresses, and the account's own security settings
//! (password, two-step codes, app passwords, API keys, passkeys, connected apps). The mock is
//! written from what Stalwart 1.1.0 answered (`support/manage.rs`).
//!
//! The last test is not run with the others: it is the real thing against a server that offers
//! masked addresses (none of the test servers does).

mod support;

use std::{path::PathBuf, sync::Arc};

use kweb_core::{
    ffi::Core,
    jmap::Connection,
    model::{ConnectedApp, MaskedAddress},
    secrets::MemorySecrets,
    Error,
};
use serde_json::json;
use support::{Edition, MockServer, ACCOUNT_PASSWORD, OTP_CODE};
use tokio::runtime::Runtime;

struct Rig {
    // Field order is drop order: the core goes first, the mock and its runtime last.
    core: Arc<Core>,
    server: MockServer,
    dir: tempfile::TempDir,
    _rt: Runtime,
}

impl Rig {
    /// A signed-in core against a licensed server that offers and grants the account scopes.
    fn new() -> Rig {
        Rig::with(Edition::Licensed, true, true)
    }

    fn with(edition: Edition, offered: bool, granted: bool) -> Rig {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        let server = rt.block_on(MockServer::start());
        server.manage(|m| m.edition = edition);
        server.set_account_scopes(offered, granted);
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("cache.db").to_string_lossy().into_owned();
        let core = Core::new(db_path, Arc::new(MemorySecrets::default())).unwrap();
        let url = core.begin_sign_in_with_redirect(server.url(), "ovh.kweb.mail:/oauth/callback".into()).unwrap();
        core.complete_sign_in(server.authorize(&url).unwrap()).unwrap();
        Rig { core, server, dir, _rt: rt }
    }

    fn requests(&self, method: &str) -> usize {
        self.server.request_count(method)
    }
}

fn invalid(e: Error) -> String {
    match e {
        Error::Invalid { msg } => msg,
        other => panic!("expected Error::Invalid, got {other:?}"),
    }
}

// ---- what the server offers ----

#[test]
fn a_licensed_server_offers_masked_addresses_and_the_security_settings() {
    let rig = Rig::new();
    let f = rig.core.account_features().unwrap();
    assert_eq!(f.edition, "enterprise");
    assert!(f.masked_email && f.security && !f.needs_sign_in);
}

#[test]
fn a_server_without_the_licence_refuses_masked_addresses_in_words() {
    let rig = Rig::with(Edition::Unlicensed, true, true);
    let f = rig.core.account_features().unwrap();
    assert_eq!(f.edition, "community");
    // The permission is there; the licence is what is missing, and only a real read finds that out.
    assert!(!f.masked_email && f.security && !f.needs_sign_in);
    assert!(rig.requests("x:MaskedEmail/query") >= 1);
    assert_eq!(invalid(rig.core.masked_addresses().unwrap_err()), "This server does not offer masked addresses.");
    assert_eq!(
        invalid(rig.core.create_masked_address("a".into(), String::new(), String::new()).unwrap_err()),
        "This server does not offer masked addresses."
    );
}

#[test]
fn a_server_that_knows_none_of_it_is_not_an_error() {
    let rig = Rig::with(Edition::Unaware, false, false);
    let f = rig.core.account_features().unwrap();
    assert_eq!((f.edition.as_str(), f.masked_email, f.security, f.needs_sign_in), ("", false, false, false));
    assert!(matches!(rig.core.masked_addresses(), Err(Error::Invalid { .. })));
    assert!(matches!(rig.core.account_security(), Err(Error::Invalid { .. })));
}

#[test]
fn a_failed_read_is_an_error_not_an_answer_that_nothing_is_offered() {
    // The metadata cannot be read just now.
    let rig = Rig::new();
    rig.server.manage(|m| m.metadata_down = true);
    assert!(matches!(rig.core.account_features(), Err(Error::Temporary { .. })));
    rig.server.manage(|m| m.metadata_down = false);
    assert!(rig.core.account_features().unwrap().security);

    // The server fails at the probes, or limits the requests.
    rig.server.manage(|m| m.method_failure = Some("serverFail"));
    assert!(matches!(rig.core.account_features(), Err(Error::Temporary { .. })));
    rig.server.manage(|m| m.method_failure = None);
    for status in [429, 503] {
        rig.server.manage(|m| m.account_status = Some(status));
        assert!(matches!(rig.core.account_features(), Err(Error::Temporary { .. })), "{status}");
    }
    rig.server.manage(|m| m.account_status = None);
    assert!(rig.core.account_features().unwrap().masked_email);
}

#[test]
fn masked_addresses_need_the_permission_as_well_as_the_answer() {
    let rig = Rig::new();
    rig.server.manage(|m| m.masked_permissions = false);
    let before = rig.requests("x:MaskedEmail/query");
    let f = rig.core.account_features().unwrap();
    assert!(!f.masked_email && f.security);
    // Not even asked: without the permission the read would only be refused.
    assert_eq!(rig.requests("x:MaskedEmail/query"), before);
}

#[test]
fn a_sign_in_without_the_scopes_the_server_offers_is_told_to_sign_in_again() {
    // Offered, but the token was granted neither.
    let rig = Rig::with(Edition::Licensed, true, false);
    let f = rig.core.account_features().unwrap();
    assert!(f.needs_sign_in && !f.masked_email && !f.security);
    assert_eq!(f.edition, "enterprise");
    // Nothing beyond the edition was asked.
    assert_eq!(rig.requests("x:AccountPassword/get") + rig.requests("x:MaskedEmail/query"), 0);
    // What is tried anyway is refused in words that say how to go on.
    assert!(invalid(rig.core.masked_addresses().unwrap_err()).contains("Sign in again"));
    assert!(invalid(rig.core.account_security().unwrap_err()).contains("Sign in again"));
}

#[test]
fn a_sign_in_made_before_the_server_offered_the_scopes_counts_as_lacking_them() {
    // Signed in while the server did not list them: the stored sign-in has no record of them.
    let rig = Rig::with(Edition::Licensed, false, true);
    let f = rig.core.account_features().unwrap();
    assert!(!f.needs_sign_in && f.security, "nothing to sign in for while they are not offered");
    // The server starts to offer them: the old sign-in has not been granted them.
    rig.server.set_account_scopes(true, true);
    let f = rig.core.account_features().unwrap();
    assert!(f.needs_sign_in && !f.masked_email && !f.security);
    // Signing in again is what it takes.
    let url = rig.core.begin_sign_in_with_redirect(rig.server.url(), "ovh.kweb.mail:/oauth/callback".into()).unwrap();
    rig.core.complete_sign_in(rig.server.authorize(&url).unwrap()).unwrap();
    let f = rig.core.account_features().unwrap();
    assert!(!f.needs_sign_in && f.masked_email && f.security);
}

// ---- masked addresses ----

#[test]
fn masked_addresses_are_listed_newest_first_with_the_limit() {
    let rig = Rig::new();
    rig.server.manage(|m| {
        m.max_masked = 25;
        m.add_masked("old@mask.test", "old one", "https://old.example", true, 1_700_000_000);
        m.add_masked("new@mask.test", "", "", false, 1_800_000_000);
        m.add_masked("mid@mask.test", "mid", "", true, 1_750_000_000);
    });
    let list = rig.core.masked_addresses().unwrap();
    assert_eq!(list.limit, Some(25));
    let emails: Vec<&str> = list.addresses.iter().map(|a| a.email.as_str()).collect();
    assert_eq!(emails, ["new@mask.test", "mid@mask.test", "old@mask.test"]);
    let old = &list.addresses[2];
    assert_eq!(
        (old.description.as_str(), old.for_domain.as_str(), old.enabled),
        ("old one", "https://old.example", true)
    );
    assert_eq!(old.created_at, Some(1_700_000_000));
    assert!(!list.addresses[0].enabled && list.addresses[0].description.is_empty());
}

#[test]
fn the_limit_is_none_where_the_account_may_not_read_it() {
    let rig = Rig::new();
    rig.server.manage(|m| m.limit_readable = false);
    let list = rig.core.masked_addresses().unwrap();
    assert!(list.addresses.is_empty());
    assert_eq!(list.limit, None);
}

#[test]
fn a_masked_address_is_made_switched_changed_and_deleted() {
    let rig = Rig::new();
    let made: MaskedAddress = rig
        .core
        .create_masked_address(" newsletter ".into(), "https://shop.example".into(), "kwebtest".into())
        .unwrap();
    assert!(made.email.starts_with("kwebtest"), "{}", made.email);
    assert_eq!(
        (made.description.as_str(), made.for_domain.as_str(), made.enabled),
        ("newsletter", "https://shop.example", true)
    );
    assert!(made.created_at.is_some());
    assert_eq!(rig.core.masked_addresses().unwrap().addresses, vec![made.clone()]);

    rig.core.update_masked_address(made.id.clone(), false, None).unwrap();
    let shown = &rig.core.masked_addresses().unwrap().addresses[0];
    assert!(!shown.enabled);
    // No note given: the note stays.
    assert_eq!(shown.description, "newsletter");
    rig.core.update_masked_address(made.id.clone(), true, Some("shop".into())).unwrap();
    let shown = &rig.core.masked_addresses().unwrap().addresses[0];
    assert!(shown.enabled && shown.description == "shop");

    rig.core.delete_masked_address(made.id.clone()).unwrap();
    assert!(rig.core.masked_addresses().unwrap().addresses.is_empty());
    // Gone already is not an error to delete; to change it is.
    rig.core.delete_masked_address(made.id.clone()).unwrap();
    assert!(invalid(rig.core.update_masked_address(made.id, true, None).unwrap_err()).contains("no longer exists"));
}

#[test]
fn blank_notes_sites_and_prefixes_are_not_sent() {
    let rig = Rig::new();
    let made = rig.core.create_masked_address("  ".into(), " ".into(), String::new()).unwrap();
    assert_eq!((made.description.as_str(), made.for_domain.as_str()), ("", ""));
    assert_eq!(rig.server.manage(|m| m.masked_domain(&made.id)), None);
    assert!(made.email.ends_with("@mask.example.test"));
}

#[test]
fn a_prefix_is_checked_before_the_server_is_asked() {
    let rig = Rig::new();
    for bad in ["has space", "at@sign", &"a".repeat(33), "ümlaut"] {
        let msg = invalid(rig.core.create_masked_address(String::new(), String::new(), bad.into()).unwrap_err());
        assert!(msg.contains("start of the address"), "{msg}");
    }
    assert_eq!(rig.requests("x:MaskedEmail/set"), 0);
    // What the rule allows goes through (this mock server takes letters, digits and underscores).
    rig.core.create_masked_address(String::new(), String::new(), "ab_c9".into()).unwrap();
    assert_eq!(rig.requests("x:MaskedEmail/set"), 1);
}

#[test]
fn a_prefix_the_server_refuses_is_put_in_plain_words() {
    let rig = Rig::new();
    // The rule of the app allows capitals, this server does not.
    let msg = invalid(rig.core.create_masked_address(String::new(), String::new(), "Shop".into()).unwrap_err());
    assert!(msg.contains("does not accept that start"), "{msg}");
    // A hyphen passes the app's check too; the mock server takes only letters, digits and _.
    let msg = invalid(rig.core.create_masked_address(String::new(), String::new(), "a-b".into()).unwrap_err());
    assert!(msg.contains("does not accept that start"), "{msg}");
}

#[test]
fn the_limit_reached_says_how_many_are_allowed() {
    let rig = Rig::new();
    rig.server.manage(|m| m.max_masked = 2);
    rig.core.create_masked_address("1".into(), String::new(), String::new()).unwrap();
    rig.core.create_masked_address("2".into(), String::new(), String::new()).unwrap();
    let msg = invalid(rig.core.create_masked_address("3".into(), String::new(), String::new()).unwrap_err());
    assert_eq!(msg, "You have reached the limit of 2 masked addresses.");
    // A switched-off address counts too, and the limit is what the list reports.
    assert_eq!(rig.core.masked_addresses().unwrap().limit, Some(2));
}

// ---- the account's security settings ----

#[test]
fn the_security_overview_has_everything_in_one_request() {
    let rig = Rig::new();
    rig.server.manage(|m| {
        m.add_credential(false, "phone mail", Some("2030-01-01T00:00:00Z"));
        m.add_credential(true, "backup script", None);
        m.add_passkey("Pixel", 1_700_000_000);
        m.add_grant("other-client", Some("Other app"), &["mail", "files"], false);
        m.add_grant("waiting", Some("Waiting"), &["mail"], true);
        m.add_grant("nameless", None, &["contacts"], false);
    });
    let before = rig.requests("/jmap");
    let s = rig.core.account_security().unwrap();
    assert_eq!(rig.requests("/jmap") - before, 1, "one request");
    assert!(!s.two_step);
    assert_eq!(s.app_passwords.len(), 1);
    assert_eq!(
        (s.app_passwords[0].description.as_str(), s.app_passwords[0].expires_at),
        ("phone mail", Some(1_893_456_000))
    );
    assert!(s.app_passwords[0].created_at.is_some());
    assert_eq!(
        (s.api_keys.len(), s.api_keys[0].description.as_str(), s.api_keys[0].expires_at),
        (1, "backup script", None)
    );
    assert_eq!((s.passkeys[0].name.as_str(), s.passkeys[0].created_at), ("Pixel", Some(1_700_000_000)));
    // The grant the app signed in with is told from the others by its client id. `pending` is
    // only the refresh flag of the server and makes no difference.
    let apps: Vec<&ConnectedApp> = s.connected_apps.iter().collect();
    assert_eq!(apps.len(), 4);
    let own: Vec<&&ConnectedApp> = apps.iter().filter(|a| a.is_this_app).collect();
    assert_eq!(own.len(), 1);
    assert_eq!(own[0].name, "kweb mail");
    assert!(own[0].scopes.contains(&"security".to_string()));
    let other = apps.iter().find(|a| a.name == "Other app").unwrap();
    assert!(!other.is_this_app && other.scopes == ["files", "mail"]);
    assert!(apps.iter().any(|a| a.name == "nameless"));
    assert!(apps.iter().any(|a| a.name == "Waiting"));
}

#[test]
fn a_secret_is_never_listed() {
    let rig = Rig::new();
    let made = rig.core.create_app_password("mail program".into(), None).unwrap();
    assert!(!made.secret.is_empty() && !made.id.is_empty());
    let dump = format!("{:?}", rig.core.account_security().unwrap());
    assert!(!dump.contains(&made.secret), "{dump}");
}

#[test]
fn the_password_is_changed_and_a_wrong_one_is_refused_in_words() {
    let rig = Rig::new();
    let msg = invalid(rig.core.change_password("not-the-password".into(), "brand-new-pass".into()).unwrap_err());
    assert_eq!(msg, "The current password is not right.");
    assert!(!msg.contains("not-the-password") && !msg.contains("brand-new-pass"));
    assert_eq!(rig.server.manage(|m| m.password()), ACCOUNT_PASSWORD);

    let weak = invalid(rig.core.change_password(ACCOUNT_PASSWORD.into(), "short".into()).unwrap_err());
    assert_eq!(weak, "Password must be at least 8 characters long.");
    assert_eq!(rig.server.manage(|m| m.password()), ACCOUNT_PASSWORD);

    rig.core.change_password(ACCOUNT_PASSWORD.into(), "brand-new-pass".into()).unwrap();
    assert_eq!(rig.server.manage(|m| m.password()), "brand-new-pass");
    // Typing nothing is told at once.
    assert!(matches!(rig.core.change_password(String::new(), "x".into()), Err(Error::Invalid { .. })));
    assert!(matches!(rig.core.change_password("x".into(), String::new()), Err(Error::Invalid { .. })));
}

#[test]
fn no_password_or_code_is_put_in_the_cache_or_an_error() {
    let rig = Rig::new();
    let mut errors = vec![
        rig.core.change_password("wrong-current-pw".into(), "another-new-pw".into()).unwrap_err().to_string(),
        rig.core.disable_two_step("wrong-current-pw".into()).unwrap_err().to_string(),
        rig.core
            .enable_two_step(
                "wrong-current-pw".into(),
                "otpauth://totp/x?secret=JBSWY3DPEHPK3PXP".into(),
                "654321".into(),
            )
            .unwrap_err()
            .to_string(),
        format!("{:?}", rig.core.change_password("wrong-current-pw".into(), "another-new-pw".into())),
    ];
    rig.core.change_password(ACCOUNT_PASSWORD.into(), "another-new-pw".into()).unwrap();
    errors.push(rig.core.change_password("wrong-current-pw".into(), "yet-another-pw".into()).unwrap_err().to_string());
    for e in &errors {
        for secret in ["wrong-current-pw", "another-new-pw", "yet-another-pw", "654321", "JBSWY3DPEHPK3PXP"] {
            assert!(!e.contains(secret), "{e}");
        }
    }
    // The cache holds none of it either.
    let mut bytes = Vec::new();
    for name in ["cache.db", "cache.db-wal"] {
        bytes.extend(std::fs::read(rig.dir.path().join(name)).unwrap_or_default());
    }
    let text = String::from_utf8_lossy(&bytes);
    for secret in [ACCOUNT_PASSWORD, "wrong-current-pw", "another-new-pw", "yet-another-pw"] {
        assert!(!text.contains(secret), "{secret} is in the cache");
    }
}

// An authenticator app's side of the code, written out in the test so that it does not depend on the core's own.
fn hotp(key: &[u8], counter: u64) -> String {
    use sha1::{Digest, Sha1};
    let mut block = [0u8; 64];
    block[..key.len()].copy_from_slice(key);
    let (mut inner, mut outer) = (Sha1::new(), Sha1::new());
    inner.update(block.map(|b| b ^ 0x36));
    inner.update(counter.to_be_bytes());
    outer.update(block.map(|b| b ^ 0x5c));
    outer.update(inner.finalize());
    let mac = outer.finalize();
    let o = (mac[19] & 15) as usize;
    let n = u32::from_be_bytes([mac[o] & 0x7f, mac[o + 1], mac[o + 2], mac[o + 3]]) % 1_000_000;
    format!("{n:06}")
}

fn base32(text: &str) -> Vec<u8> {
    let (mut out, mut buf, mut bits) = (Vec::new(), 0u32, 0u32);
    for c in text.chars().filter(|c| *c != ' ') {
        buf = (buf << 5)
            | u32::from(b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567".iter().position(|a| *a as char == c).unwrap() as u8);
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    out
}

fn code_at(setup_secret: &str, offset: i64) -> String {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
    hotp(&base32(setup_secret), ((now / 30) + offset) as u64)
}

#[test]
fn a_new_secret_is_made_on_the_device_and_nothing_is_sent() {
    let rig = Rig::new();
    let before = rig.requests("/jmap");
    let setup = rig.core.new_two_step().unwrap();
    assert_eq!(rig.requests("/jmap"), before);
    // Issuer: the server's host. Label: the account's address.
    assert!(
        setup.otpauth_url.starts_with(&format!("otpauth://totp/{}?secret=", rig.server.username())),
        "{}",
        setup.otpauth_url
    );
    assert!(setup.otpauth_url.ends_with("&issuer=127.0.0.1"), "{}", setup.otpauth_url);
    assert_eq!(setup.secret.split(' ').count(), 8);
    assert!(setup.secret.split(' ').all(|g| g.len() == 4));
    assert_eq!(base32(&setup.secret).len(), 20);
    assert!(setup.otpauth_url.contains(&setup.secret.replace(' ', "")));
    assert_ne!(rig.core.new_two_step().unwrap().secret, setup.secret);
}

#[test]
fn two_step_codes_are_switched_on_after_the_code_is_checked() {
    let rig = Rig::new();
    let setup = rig.core.new_two_step().unwrap();
    // A wrong code (and an empty one) is refused here: nothing is sent.
    for bad in ["000000", "", "12345", "abcdef"] {
        let code = if bad == "000000" && code_at(&setup.secret, 0) == "000000" { "000001" } else { bad };
        let msg = invalid(
            rig.core.enable_two_step(ACCOUNT_PASSWORD.into(), setup.otpauth_url.clone(), code.into()).unwrap_err(),
        );
        assert!(msg.contains("not right"), "{msg}");
    }
    assert_eq!(rig.requests("x:AccountPassword/set"), 0);
    // An address that is not one of ours is refused without a request too.
    assert!(matches!(
        rig.core.enable_two_step(ACCOUNT_PASSWORD.into(), "https://example.org".into(), "123456".into()),
        Err(Error::Invalid { .. })
    ));
    // A right code with the wrong password is the server's refusal, in words.
    let msg = invalid(
        rig.core
            .enable_two_step("wrong-pass".into(), setup.otpauth_url.clone(), code_at(&setup.secret, 0))
            .unwrap_err(),
    );
    assert_eq!(msg, "The current password is not right.");
    assert!(!rig.core.account_security().unwrap().two_step);

    rig.core.enable_two_step(ACCOUNT_PASSWORD.into(), setup.otpauth_url.clone(), code_at(&setup.secret, 0)).unwrap();
    assert!(rig.core.account_security().unwrap().two_step);
    assert_eq!(rig.server.manage(|m| m.two_step_url()), Some(setup.otpauth_url));
}

#[test]
fn a_code_of_the_step_before_or_after_is_taken() {
    for offset in [-1, 1] {
        let rig = Rig::new();
        let setup = rig.core.new_two_step().unwrap();
        // The step may turn between making the code and checking it: take the one that holds.
        let code = code_at(&setup.secret, offset);
        rig.core.enable_two_step(ACCOUNT_PASSWORD.into(), setup.otpauth_url, code).unwrap();
        assert!(rig.core.account_security().unwrap().two_step);
    }
    // Two steps off is not.
    let rig = Rig::new();
    let setup = rig.core.new_two_step().unwrap();
    let code = code_at(&setup.secret, 3);
    assert!(rig.core.enable_two_step(ACCOUNT_PASSWORD.into(), setup.otpauth_url, code).is_err());
}

#[test]
fn with_two_step_codes_on_the_server_wants_a_code_for_changes_too() {
    let rig = Rig::new();
    rig.server.manage(|m| m.set_two_step_url(Some("otpauth://totp/x?secret=JBSWY3DPEHPK3PXP")));
    assert!(rig.core.account_security().unwrap().two_step);
    // Without a code to give, the user is told why it cannot be done.
    for msg in [
        invalid(rig.core.disable_two_step(ACCOUNT_PASSWORD.into()).unwrap_err()),
        invalid(rig.core.change_password(ACCOUNT_PASSWORD.into(), "brand-new-pass".into()).unwrap_err()),
    ] {
        assert!(msg.contains("Two-step codes are on"), "{msg}");
    }
    // With the code, through the core as the app uses it, it goes through.
    rig.core
        .change_password_with_code(ACCOUNT_PASSWORD.into(), "brand-new-pass".into(), Some(OTP_CODE.into()))
        .unwrap();
    assert_eq!(rig.server.manage(|m| m.password()), "brand-new-pass");
    // A wrong code or password is said to be one of the two.
    for wrong in [
        rig.core.disable_two_step_with_code("brand-new-pass".into(), Some("111111".into())),
        rig.core.disable_two_step_with_code("wrong".into(), Some(OTP_CODE.into())),
    ] {
        assert_eq!(invalid(wrong.unwrap_err()), "The current password or the two-step code is not right.");
    }
    assert!(rig.core.account_security().unwrap().two_step);
    rig.core.disable_two_step_with_code("brand-new-pass".into(), Some(OTP_CODE.into())).unwrap();
    assert!(!rig.core.account_security().unwrap().two_step);
}

#[test]
fn two_step_codes_are_replaced_with_the_old_code() {
    let rig = Rig::new();
    rig.server.manage(|m| m.set_two_step_url(Some("otpauth://totp/x?secret=JBSWY3DPEHPK3PXP")));
    let setup = rig.core.new_two_step().unwrap();
    let code = code_at(&setup.secret, 0);
    assert!(invalid(
        rig.core.enable_two_step(ACCOUNT_PASSWORD.into(), setup.otpauth_url.clone(), code.clone()).unwrap_err()
    )
    .contains("Two-step codes are on"));
    rig.core
        .enable_two_step_with_code(ACCOUNT_PASSWORD.into(), setup.otpauth_url.clone(), code, Some(OTP_CODE.into()))
        .unwrap();
    assert_eq!(rig.server.manage(|m| m.two_step_url()), Some(setup.otpauth_url));
}

#[test]
fn two_step_codes_are_switched_off() {
    let rig = Rig::new();
    // Off when it is off is fine: the server has nothing to ask for.
    rig.core.disable_two_step(ACCOUNT_PASSWORD.into()).unwrap();
    assert_eq!(invalid(rig.core.disable_two_step("wrong".into()).unwrap_err()), "The current password is not right.");
    assert!(matches!(rig.core.disable_two_step(String::new()), Err(Error::Invalid { .. })));
}

#[test]
fn an_app_password_is_made_listed_and_deleted() {
    let rig = Rig::new();
    let at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64 + 86_400;
    let made = rig.core.create_app_password("  phone  ".into(), Some(at)).unwrap();
    assert!(made.secret.starts_with("app_"));
    let plain = rig.core.create_app_password("tablet".into(), None).unwrap();
    let list = rig.core.account_security().unwrap().app_passwords;
    assert_eq!(list.len(), 2);
    let phone = list.iter().find(|c| c.id == made.id).unwrap();
    assert_eq!((phone.description.as_str(), phone.expires_at), ("phone", Some(at)));
    assert_eq!(list.iter().find(|c| c.id == plain.id).unwrap().expires_at, None);
    assert!(rig.core.account_security().unwrap().api_keys.is_empty());

    rig.core.delete_app_password(made.id.clone()).unwrap();
    assert_eq!(rig.core.account_security().unwrap().app_passwords.len(), 1);
    // Gone already is as good as deleted.
    rig.core.delete_app_password(made.id).unwrap();
}

#[test]
fn an_api_key_is_made_with_the_accounts_rights_and_deleted() {
    let rig = Rig::new();
    let made = rig.core.create_api_key("backup".into(), None).unwrap();
    assert!(made.secret.starts_with("API_"));
    let keys = rig.core.account_security().unwrap().api_keys;
    assert_eq!((keys.len(), keys[0].description.as_str()), (1, "backup"));
    let (_, _, rights) = rig.server.manage(|m| m.credential(true, &made.id)).unwrap();
    assert_eq!(rights, json!({ "@type": "Inherit" }));
    rig.core.delete_api_key(made.id.clone()).unwrap();
    assert!(rig.core.account_security().unwrap().api_keys.is_empty());
    rig.core.delete_api_key(made.id).unwrap();
}

#[test]
fn a_sign_in_with_fewer_rights_than_the_account_makes_a_key_with_the_rights_it_has() {
    let rig = Rig::new();
    rig.server.manage(|m| m.inherit_refused = true);
    for made in
        [rig.core.create_api_key("k".into(), None).unwrap(), rig.core.create_app_password("a".into(), None).unwrap()]
    {
        assert!(!made.secret.is_empty());
    }
    let sent = rig.server.manage(|m| m.create_permissions.clone());
    // Each was first asked for with all the account's rights, then with the ones the sign-in has.
    assert_eq!(sent.len(), 4);
    assert_eq!(sent[0].1, json!({ "@type": "Inherit" }));
    assert_eq!(sent[1].1["@type"], "Replace");
    assert_eq!(sent[1].1["permissions"]["sysApiKeyGet"], true);
    assert_eq!(sent[2].1, json!({ "@type": "Inherit" }));
    assert_eq!(sent[3].1["@type"], "Replace");
}

#[test]
fn what_cannot_be_made_is_refused_in_words() {
    let rig = Rig::new();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
    // Checked before the server is asked.
    assert!(invalid(rig.core.create_app_password("  ".into(), None).unwrap_err()).contains("name"));
    assert!(invalid(rig.core.create_api_key("x".into(), Some(now - 10)).unwrap_err()).contains("still to come"));
    assert!(invalid(rig.core.create_api_key("x".into(), Some(now)).unwrap_err()).contains("still to come"));
    assert_eq!(rig.requests("x:AppPassword/set") + rig.requests("x:ApiKey/set"), 0);
    // The server's limit says how many it allows.
    for i in 0..5 {
        rig.core.create_app_password(format!("p{i}"), None).unwrap();
        rig.core.create_api_key(format!("k{i}"), None).unwrap();
    }
    assert_eq!(
        invalid(rig.core.create_app_password("one more".into(), None).unwrap_err()),
        "You have reached the limit of 5 app passwords."
    );
    assert_eq!(
        invalid(rig.core.create_api_key("one more".into(), None).unwrap_err()),
        "You have reached the limit of 5 API keys."
    );
    assert_eq!(rig.core.account_security().unwrap().app_passwords.len(), 5);
}

#[test]
fn a_passkey_is_renamed_and_deleted() {
    let rig = Rig::new();
    let id = rig.server.manage(|m| m.add_passkey("Pixel", 1_700_000_000));
    rig.core.rename_passkey(id.clone(), "  Work phone ".into()).unwrap();
    assert_eq!(rig.server.manage(|m| m.passkeys()), vec![(id.clone(), "Work phone".to_string())]);
    assert_eq!(rig.core.account_security().unwrap().passkeys[0].name, "Work phone");
    // Refused before the server is asked.
    assert!(matches!(rig.core.rename_passkey(id.clone(), "  ".into()), Err(Error::Invalid { .. })));
    assert!(matches!(rig.core.rename_passkey(id.clone(), "x".repeat(65)), Err(Error::Invalid { .. })));
    rig.core.rename_passkey(id.clone(), "x".repeat(64)).unwrap();
    rig.core.delete_passkey(id.clone()).unwrap();
    assert!(rig.core.account_security().unwrap().passkeys.is_empty());
    rig.core.delete_passkey(id.clone()).unwrap();
    assert!(invalid(rig.core.rename_passkey(id, "late".into()).unwrap_err()).contains("no longer exists"));
}

#[test]
fn an_app_is_revoked_but_the_apps_own_sign_in_is_not() {
    let rig = Rig::new();
    // The grants are in use, so the server has them as refreshed: `pending` is set on both.
    let other = rig.server.manage(|m| m.add_grant("other-client", Some("Other app"), &["mail"], true));
    let apps = rig.core.account_security().unwrap().connected_apps;
    let own = apps.iter().find(|a| a.is_this_app).unwrap().id.clone();
    assert_eq!(apps.len(), 2);

    let msg = invalid(rig.core.revoke_connected_app(own.clone()).unwrap_err());
    assert!(msg.contains("Sign out"), "{msg}");
    assert!(rig.server.manage(|m| m.grant_ids()).contains(&own), "the sign-in is still there");

    rig.core.revoke_connected_app(other.clone()).unwrap();
    assert_eq!(rig.server.manage(|m| m.grant_ids()), vec![own]);
    // Gone already is as good as revoked.
    rig.core.revoke_connected_app(other).unwrap();
}

#[test]
fn the_security_settings_are_refused_in_words_without_the_scope() {
    let rig = Rig::with(Edition::Licensed, true, false);
    for e in [
        rig.core.change_password("a".into(), "bbbbbbbb".into()).unwrap_err(),
        rig.core.create_app_password("x".into(), None).unwrap_err(),
        rig.core.delete_api_key("k1".into()).unwrap_err(),
        rig.core.rename_passkey("p".into(), "n".into()).unwrap_err(),
        rig.core.revoke_connected_app("g".into()).unwrap_err(),
    ] {
        assert!(invalid(e).contains("Sign in again"));
    }
}

#[test]
fn nothing_works_signed_out() {
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
    let _guard = rt.enter();
    let dir = tempfile::tempdir().unwrap();
    let core =
        Core::new(dir.path().join("c.db").to_string_lossy().into_owned(), Arc::new(MemorySecrets::default())).unwrap();
    assert!(matches!(core.account_features(), Err(Error::SignInNeeded { .. })));
    assert!(matches!(core.masked_addresses(), Err(Error::SignInNeeded { .. })));
    assert!(matches!(core.account_security(), Err(Error::SignInNeeded { .. })));
    assert!(matches!(core.new_two_step(), Err(Error::SignInNeeded { .. })));
}

// ---- the real thing ----

const NOTE: &str = "kweb app test, delete me";

/// Only the kind of an error: its text may hold the server's address or words of the server.
fn kind_of(e: &Error) -> &'static str {
    match e {
        Error::SignInNeeded { .. } => "sign-in needed",
        Error::Temporary { .. } => "temporary",
        Error::Store { .. } => "store",
        Error::Invalid { .. } => "invalid",
    }
}

/// Against the server named by KWEB_REAL_URL, with the API key in the file KWEB_REAL_KEY_FILE as
/// the bearer token. Makes one masked address, switches it off and on, deletes it, and deletes
/// it also when something fails. Prints counts, booleans and the kind of an error, never an address, the key or an error's text. An address whose creation was not answered is found by its note and deleted too.
/// Run: `KWEB_REAL_URL=... KWEB_REAL_KEY_FILE=... cargo test -p kweb-core --test manage -- --ignored --nocapture`
#[test]
#[ignore = "needs a real server that offers masked addresses"]
fn masked_addresses_on_a_real_server() {
    let (Ok(url), Ok(key_file)) = (std::env::var("KWEB_REAL_URL"), std::env::var("KWEB_REAL_KEY_FILE")) else {
        panic!("set KWEB_REAL_URL and KWEB_REAL_KEY_FILE");
    };
    let key = std::fs::read_to_string(PathBuf::from(key_file)).expect("the key file cannot be read");
    let key = key.trim().to_string();
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
    let conn =
        rt.block_on(Connection::connect(&url, &key)).unwrap_or_else(|e| panic!("connecting failed: {}", kind_of(&e)));

    let features = rt.block_on(conn.features(false)).unwrap_or_else(|e| panic!("features failed: {}", kind_of(&e)));
    println!(
        "masked_email={} security={} needs_sign_in={}",
        features.masked_email, features.security, features.needs_sign_in
    );
    assert!(features.masked_email, "this server does not offer masked addresses");

    let before = rt.block_on(conn.masked_addresses()).unwrap_or_else(|e| panic!("list failed: {}", kind_of(&e)));
    println!("before: {} addresses, limit known: {}", before.addresses.len(), before.limit.is_some());

    let mut made: Option<String> = None;
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // Whether it failed or not, an address of the test may exist: the cleanup below looks.
        let new = match rt.block_on(conn.create_masked_address(NOTE, "", "kwebtest")) {
            Ok(new) => new,
            Err(e) => panic!("create failed: {}", kind_of(&e)),
        };
        made = Some(new.id.clone());
        println!(
            "created: enabled={} has_note={} has_time={}",
            new.enabled,
            !new.description.is_empty(),
            new.created_at.is_some()
        );
        assert!(new.enabled && new.email.starts_with("kwebtest"));

        let list = rt.block_on(conn.masked_addresses()).unwrap_or_else(|e| panic!("list failed: {}", kind_of(&e)));
        println!("after create: {} addresses", list.addresses.len());
        assert_eq!(list.addresses.len(), before.addresses.len() + 1);
        assert!(list.addresses.iter().any(|a| a.id == new.id));

        for on in [false, true] {
            if let Err(e) = rt.block_on(conn.update_masked_address(&new.id, on, None)) {
                panic!("switch failed: {}", kind_of(&e));
            }
            let list = rt.block_on(conn.masked_addresses()).unwrap_or_else(|e| panic!("list failed: {}", kind_of(&e)));
            let now = list.addresses.iter().find(|a| a.id == new.id).map(|a| a.enabled);
            println!("switched to {on}: reads back {now:?}");
            assert_eq!(now, Some(on));
        }

        if let Err(e) = rt.block_on(conn.delete_masked_address(&new.id)) {
            panic!("delete failed: {}", kind_of(&e));
        }
        made = None;
        let list = rt.block_on(conn.masked_addresses()).unwrap_or_else(|e| panic!("list failed: {}", kind_of(&e)));
        println!("after delete: {} addresses", list.addresses.len());
        assert!(!list.addresses.iter().any(|a| a.id == new.id));
        assert_eq!(list.addresses.len(), before.addresses.len());
    }));
    // Whatever happened above, nothing of the test is left behind: also an address whose
    // creation was not answered is found by its note and by not having been there before.
    let known: Vec<&str> = before.addresses.iter().map(|a| a.id.as_str()).collect();
    let mut leftovers: Vec<String> = made.into_iter().collect();
    if let Ok(list) = rt.block_on(conn.masked_addresses()) {
        for a in &list.addresses {
            if a.description == NOTE && !known.contains(&a.id.as_str()) && !leftovers.contains(&a.id) {
                leftovers.push(a.id.clone());
            }
        }
    }
    for id in leftovers {
        let gone = rt.block_on(conn.delete_masked_address(&id)).is_ok();
        println!("cleanup of a leftover: deleted={gone}");
    }
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}
