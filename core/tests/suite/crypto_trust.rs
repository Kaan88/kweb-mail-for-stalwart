//! M9 stage 4 through the `Core` object against the mock server: keys learned from the Autocrypt header of mail
//! the user opened, kept with their source and trusted for nothing until the user says so.

use std::sync::Arc;

use base64::{engine::general_purpose::STANDARD, Engine};
use kweb_core::{
    crypto::{
        openpgp,
        types::{CryptoKind, KeyOrigin},
    },
    ffi::{Core, Shade},
    secrets::MemorySecrets,
};
use tokio::runtime::Runtime;

use crate::support::*;

fn fx(name: &str) -> Vec<u8> {
    let path = format!("{}/tests/fixtures/crypto/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

struct Rig {
    core: Arc<Core>,
    server: MockServer,
    _rt: Runtime,
    _dir: tempfile::TempDir,
}

impl Rig {
    fn new() -> Rig {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        let server = rt.block_on(MockServer::start());
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("cache.db").to_string_lossy().into_owned();
        let core = Core::new(db_path, Arc::new(MemorySecrets::default())).unwrap();
        let url = core.begin_sign_in(server.url()).unwrap();
        let browser = rt.spawn(async move { reqwest::get(url).await.map(|r| r.status()) });
        core.finish_sign_in().unwrap();
        assert!(rt.block_on(browser).unwrap().unwrap().is_success());
        core.check_once(Shade::Unknown).unwrap();
        Rig { core, server, _rt: rt, _dir: dir }
    }

    /// A plain mail from `from` with these `Autocrypt` header values (nothing signed or encrypted about it).
    fn mail_with(&self, from: &str, headers: &[String]) -> String {
        let id = self.server.deliver("Sender", from, "Hi", "x");
        self.server.set_bodies(&id, Some("hello"), None);
        let pairs: Vec<(&str, &str)> = headers.iter().map(|h| ("Autocrypt", h.as_str())).collect();
        self.server.set_delivery_headers(&id, &pairs);
        id
    }
}

/// The header value a program would send for the key in `file`, claiming `addr`.
fn header(addr: &str, file: &str) -> String {
    let key = openpgp::parse_public(&fx(file)).unwrap().remove(0);
    let bytes = openpgp::public_bytes(&key).unwrap();
    format!("addr={addr}; prefer-encrypt=mutual; keydata={}", STANDARD.encode(bytes))
}

fn learned(rig: &Rig) -> Vec<kweb_core::crypto::types::KeyEntry> {
    rig.core.crypto_keys().unwrap().into_iter().filter(|k| k.origin == KeyOrigin::Autocrypt).collect()
}

#[test]
fn a_key_in_the_autocrypt_header_is_kept_with_its_source_when_the_mail_is_opened_and_only_then() {
    let rig = Rig::new();
    let id = rig.mail_with("bob@example.test", &[header("bob@example.test", "bob.pub.asc")]);
    // Off: nothing is read.
    assert_eq!(rig.core.crypto_probe(id.clone()).unwrap(), CryptoKind::None);
    assert!(learned(&rig).is_empty());
    rig.core.set_crypto_enabled(true).unwrap();
    assert_eq!(rig.core.crypto_probe(id.clone()).unwrap(), CryptoKind::None);
    let keys = learned(&rig);
    assert_eq!(keys.len(), 1);
    let key = &keys[0];
    assert_eq!(key.origin_detail, "bob@example.test");
    assert_eq!(key.emails, ["bob@example.test"]);
    assert!(!key.verified && key.changed_for.is_empty() && !key.has_secret);
    assert!(key.added_at > 0, "the day it was first seen");
    // Opening it again changes nothing.
    rig.core.crypto_probe(id).unwrap();
    assert_eq!(learned(&rig).len(), 1);
}

#[test]
fn a_header_that_is_not_the_senders_or_not_one_is_not_believed() {
    let rig = Rig::new();
    rig.core.set_crypto_enabled(true).unwrap();
    // The header names another address than the From address.
    let other = rig.mail_with("eve@example.test", &[header("bob@example.test", "bob.pub.asc")]);
    // Two headers are no header.
    let twice = rig.mail_with(
        "bob@example.test",
        &[header("bob@example.test", "bob.pub.asc"), header("bob@example.test", "mallory.pub.asc")],
    );
    // An attribute that must be understood and is not.
    let critical = rig.mail_with("bob@example.test", &["addr=bob@example.test; type=9; keydata=AQID".to_string()]);
    let junk = rig.mail_with("bob@example.test", &["addr=bob@example.test; keydata=AQIDBA==".to_string()]);
    for id in [other, twice, critical, junk] {
        rig.core.crypto_probe(id).unwrap();
    }
    assert!(learned(&rig).is_empty(), "{:?}", learned(&rig));
}

#[test]
fn a_second_key_for_an_address_is_a_change_and_the_first_one_stays_the_one_used() {
    let rig = Rig::new();
    rig.core.set_crypto_enabled(true).unwrap();
    rig.core.crypto_probe(rig.mail_with("bob@example.test", &[header("bob@example.test", "bob.pub.asc")])).unwrap();
    // Later the "same" sender offers a different key.
    rig.core.crypto_probe(rig.mail_with("bob@example.test", &[header("bob@example.test", "mallory.pub.asc")])).unwrap();
    let keys = learned(&rig);
    assert_eq!(keys.len(), 2, "both are shown");
    let first = keys.iter().find(|k| k.fingerprint.starts_with("4B2498E6")).expect("bob's key");
    let second = keys.iter().find(|k| k.fingerprint.starts_with("067DC482")).expect("the new key");
    assert!(first.changed_for.is_empty(), "the first seen is not a change");
    assert_eq!(second.changed_for, ["bob@example.test"], "the new one is");
    // The user can compare by hand and say so; nothing else does.
    rig.core.crypto_set_verified(first.id.clone(), true).unwrap();
    assert!(rig.core.crypto_keys().unwrap().iter().find(|k| k.id == first.id).unwrap().verified);
    rig.core.crypto_set_verified(first.id.clone(), false).unwrap();
    assert!(!rig.core.crypto_keys().unwrap().iter().find(|k| k.id == first.id).unwrap().verified);
    // Only the user's own removing of the first key lets another one become the first seen.
    rig.core.crypto_remove_key(first.id.clone()).unwrap();
    assert_eq!(learned(&rig).len(), 1);
    assert!(learned(&rig)[0].changed_for.is_empty());
}

#[test]
fn a_key_the_user_imports_by_hand_replaces_the_weaker_source() {
    let rig = Rig::new();
    rig.core.set_crypto_enabled(true).unwrap();
    rig.core.crypto_probe(rig.mail_with("bob@example.test", &[header("bob@example.test", "bob.pub.asc")])).unwrap();
    assert_eq!(learned(&rig).len(), 1);
    rig.core.crypto_import(fx("bob.pub.asc"), KeyOrigin::File).unwrap();
    assert!(learned(&rig).is_empty());
    assert_eq!(rig.core.crypto_keys().unwrap().iter().filter(|k| k.origin == KeyOrigin::File).count(), 1);
}

#[test]
fn a_directory_lookup_is_refused_for_anything_but_a_plain_address_and_while_the_feature_is_off() {
    let rig = Rig::new();
    assert!(rig.core.crypto_wkd_lookup("bob@example.test".into()).is_err(), "off");
    rig.core.set_crypto_enabled(true).unwrap();
    for bad in ["bob@localhost", "bob@127.0.0.1", "not an address"] {
        let e = rig.core.crypto_wkd_lookup(bad.into()).unwrap_err();
        assert!(
            format!("{e}").contains("cannot be looked up") || format!("{e:?}").contains("cannot be looked up"),
            "{bad}: {e:?}"
        );
    }
}
