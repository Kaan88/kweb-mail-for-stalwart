//! M9 through the `Core` object against the mock server: the switch, the keyring, and the status
//! of signed mail whose raw bytes (written by GnuPG and OpenSSL) the mock serves as the blob.

use std::sync::Arc;

use crate::support::*;
use kweb_core::{
    crypto::types::{CryptoKind, CryptoNote, EncryptionState, KeyKind, KeyOrigin, PrivateFile, SignatureState},
    ffi::{Core, Shade},
    secrets::{MemorySecrets, SecretStore},
    Error,
};
use tokio::runtime::Runtime;

fn fx(name: &str) -> Vec<u8> {
    let path = format!("{}/tests/fixtures/crypto/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

struct Rig {
    core: Arc<Core>,
    server: MockServer,
    secrets: Arc<dyn SecretStore>,
    _rt: Runtime,
    dir: tempfile::TempDir,
}

impl Rig {
    fn new() -> Rig {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        let server = rt.block_on(MockServer::start());
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("cache.db").to_string_lossy().into_owned();
        let secrets: Arc<dyn SecretStore> = Arc::new(MemorySecrets::default());
        let core = Core::new(db_path, secrets.clone()).unwrap();
        let url = core.begin_sign_in(server.url()).unwrap();
        let browser = rt.spawn(async move { reqwest::get(url).await.map(|r| r.status()) });
        core.finish_sign_in().unwrap();
        assert!(rt.block_on(browser).unwrap().unwrap().is_success());
        core.check_once(Shade::Unknown).unwrap();
        Rig { core, server, secrets, _rt: rt, dir }
    }

    /// A message whose raw bytes are the fixture, in the Inbox.
    fn deliver_raw(&self, fixture: &str) -> String {
        let id = self.server.deliver("Sender", "sender@example.test", "Hi", "x");
        self.server.set_raw(&id, &fx(fixture));
        id
    }

    fn enable(&self) {
        self.core.set_crypto_enabled(true).unwrap();
    }
}

#[test]
fn the_feature_is_off_until_it_is_switched_on_and_then_does_nothing_for_plain_mail() {
    let rig = Rig::new();
    assert!(!rig.core.crypto_enabled().unwrap());
    let id = rig.deliver_raw("pgp-signed.eml");
    assert_eq!(rig.core.crypto_probe(id.clone()).unwrap(), CryptoKind::None, "off: nothing is asked");
    assert!(matches!(rig.core.crypto_open(id.clone()), Err(Error::Invalid { .. })));
    rig.enable();
    assert_eq!(rig.core.crypto_probe(id).unwrap(), CryptoKind::PgpSigned);
    // Mail that is neither signed nor encrypted has no status and costs one small request.
    let plain = rig.server.deliver("Sender", "sender@example.test", "Plain", "x");
    rig.server.set_bodies(&plain, Some("hello"), None);
    assert_eq!(rig.core.crypto_probe(plain).unwrap(), CryptoKind::None);
}

#[test]
fn a_signed_message_is_checked_against_the_keyring_through_the_core() {
    let rig = Rig::new();
    rig.enable();
    let id = rig.deliver_raw("pgp-signed.eml");
    // No key yet: nothing can be said.
    assert_eq!(rig.core.crypto_open(id.clone()).unwrap().signature, SignatureState::NoKey);
    let added = rig.core.crypto_import(fx("alice.pub.asc"), KeyOrigin::File).unwrap();
    assert_eq!(added.len(), 1);
    assert_eq!(added[0].kind, KeyKind::Pgp);
    assert_eq!(added[0].emails, vec!["alice@example.test".to_string()]);
    assert!(!added[0].has_secret);
    let view = rig.core.crypto_open(id.clone()).unwrap();
    assert_eq!(view.signature, SignatureState::Verified);
    assert_eq!(view.signer.unwrap().fingerprint, added[0].fingerprint);
    assert!(view.covers_whole_message);
    // The same again (the first fingerprint is remembered and matches).
    assert_eq!(rig.core.crypto_open(id.clone()).unwrap().signature, SignatureState::Verified);
    // Take the key away and it is unknown again.
    rig.core.crypto_remove_key(added[0].id.clone()).unwrap();
    assert!(rig.core.crypto_keys().unwrap().is_empty());
    assert_eq!(rig.core.crypto_open(id).unwrap().signature, SignatureState::NoKey);
}

#[test]
fn a_message_changed_on_the_server_is_invalid_and_one_for_another_sender_is_flagged() {
    let rig = Rig::new();
    rig.enable();
    rig.core.crypto_import(fx("alice.pub.asc"), KeyOrigin::File).unwrap();
    let raw = String::from_utf8(fx("pgp-signed.eml")).unwrap();
    let tampered = rig.server.deliver("Sender", "sender@example.test", "T", "x");
    rig.server.set_raw(&tampered, raw.replace("this is signed", "this is SIGNED").as_bytes());
    assert_eq!(rig.core.crypto_open(tampered).unwrap().signature, SignatureState::Invalid);
    let spoofed = rig.server.deliver("Sender", "sender@example.test", "S", "x");
    rig.server
        .set_raw(&spoofed, raw.replace("From: Alice <alice@example.test>", "From: Alice <alice@evil.test>").as_bytes());
    assert_eq!(rig.core.crypto_open(spoofed).unwrap().signature, SignatureState::OtherAddress);
}

#[test]
fn a_key_in_a_message_is_offered_and_only_added_when_the_user_says_so() {
    let rig = Rig::new();
    rig.enable();
    let id = rig.deliver_raw("pgp-keys-attached.eml");
    let view = rig.core.crypto_open(id.clone()).unwrap();
    assert_eq!(view.offers.len(), 1);
    assert!(rig.core.crypto_keys().unwrap().is_empty(), "never imported by itself");
    let entry = rig.core.crypto_import_offer(id, view.offers[0].id.clone()).unwrap();
    assert_eq!(entry.origin, KeyOrigin::Mail);
    assert_eq!(entry.origin_detail, "alice@example.test");
    assert_eq!(rig.core.crypto_keys().unwrap().len(), 1);
}

#[test]
fn an_smime_certificate_from_the_message_becomes_a_checked_one_only_after_it_is_imported() {
    let rig = Rig::new();
    rig.enable();
    let id = rig.deliver_raw("smime-signed.eml");
    let before = rig.core.crypto_open(id.clone()).unwrap();
    assert_eq!(before.signature, SignatureState::CertificateNotChecked);
    assert!(before.notes.contains(&CryptoNote::SmimeNoRevocationCheck));
    rig.core.crypto_import_offer(id.clone(), before.offers[0].id.clone()).unwrap();
    let after = rig.core.crypto_open(id).unwrap();
    assert_eq!(after.signature, SignatureState::Verified);
    assert!(after.signer.unwrap().in_keyring);
}

#[test]
fn what_is_not_a_key_is_refused_in_plain_words() {
    let rig = Rig::new();
    let refused = rig.core.crypto_import(b"hello".to_vec(), KeyOrigin::Pasted).unwrap_err();
    assert!(matches!(&refused, Error::Invalid { msg } if msg.contains("not an OpenPGP key")), "{refused:?}");
    let big = rig.core.crypto_import(vec![b'a'; 2 << 20], KeyOrigin::File).unwrap_err();
    assert!(matches!(big, Error::Invalid { .. }));
    // A private key is not a public one and is not taken as such.
    assert!(rig.core.crypto_import(fx("alice.sec.asc"), KeyOrigin::File).is_err());
    // Public imports are listed with where they came from.
    rig.core.crypto_import(fx("carol.crt.pem"), KeyOrigin::File).unwrap();
    rig.core.crypto_import(fx("bob.pub.asc"), KeyOrigin::Pasted).unwrap();
    let keys = rig.core.crypto_keys().unwrap();
    assert_eq!(keys.len(), 2);
    assert!(keys.iter().any(|k| k.kind == KeyKind::Smime && k.origin == KeyOrigin::File));
    assert!(keys.iter().any(|k| k.kind == KeyKind::Pgp && k.origin == KeyOrigin::Pasted));
    rig.core.crypto_forget_all().unwrap();
    assert!(rig.core.crypto_keys().unwrap().is_empty());
    assert!(!rig.core.crypto_enabled().unwrap());
}

#[test]
fn the_calls_need_a_signed_in_account() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cache.db").to_string_lossy().into_owned();
    let core = Core::new(path, Arc::new(MemorySecrets::default())).unwrap();
    assert!(matches!(core.crypto_enabled(), Err(Error::SignInNeeded { .. })));
    assert!(matches!(core.crypto_keys(), Err(Error::SignInNeeded { .. })));
    assert!(matches!(core.crypto_probe("m".into()), Err(Error::SignInNeeded { .. })));
}

// ---- opening encrypted mail (stage 2) -------------------------------------------------------

const SECRET_SENTENCE: &str = "Secret hello, signed and sealed.";

/// Every byte of the cache database (and its write-ahead log) as text, to look for what must not be there.
fn cache_bytes(rig: &Rig) -> Vec<u8> {
    let mut all = Vec::new();
    for name in ["cache.db", "cache.db-wal"] {
        if let Ok(bytes) = std::fs::read(rig.dir.path().join(name)) {
            all.extend(bytes);
        }
    }
    all
}

fn contains(haystack: &[u8], needle: &str) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle.as_bytes())
}

#[test]
fn a_private_key_is_imported_into_the_secret_store_only_and_opens_mail() {
    let rig = Rig::new();
    rig.enable();
    assert_eq!(rig.core.crypto_inspect_private(fx("bob.sec.asc")), PrivateFile::OpenPgp);
    assert_eq!(rig.core.crypto_inspect_private(fx("carol.p12")), PrivateFile::Pkcs12);
    assert_eq!(rig.core.crypto_inspect_private(b"nothing".to_vec()), PrivateFile::Unknown);
    let bob = rig.core.crypto_import_private(fx("bob.sec.asc"), None).unwrap().remove(0);
    assert!(bob.has_secret && bob.needs_passphrase && !bob.unlocked);
    assert_eq!(bob.origin, KeyOrigin::Own);
    // The private part is in the secret store; the cache holds public material only.
    let in_store = rig.secrets.get(format!("crypto.secret.{}", bob.id.replace(':', "."))).unwrap();
    assert!(in_store.is_some());
    let cache = cache_bytes(&rig);
    assert!(!contains(&cache, "PRIVATE KEY"), "no armored private key in the cache");
    let id = rig.deliver_raw("pgp-encrypted-bob.eml");
    // Locked: it asks. Wrong: it says so. Right: it opens.
    assert_eq!(rig.core.crypto_open(id.clone()).unwrap().encryption, EncryptionState::NeedPassphrase);
    let wrong = rig.core.crypto_open_with(id.clone(), Some("nope".into()), false).unwrap();
    assert_eq!(wrong.encryption, EncryptionState::WrongPassphrase);
    let opened = rig.core.crypto_open_with(id.clone(), Some("bob-pass".into()), false).unwrap();
    assert_eq!(opened.encryption, EncryptionState::Decrypted);
    assert!(opened.content.unwrap().text.unwrap().contains("Secret hello"));
    // Not kept unlocked: asks again, and nothing was stored for the passphrase.
    assert_eq!(rig.core.crypto_open(id.clone()).unwrap().encryption, EncryptionState::NeedPassphrase);
    assert!(rig.secrets.get(format!("crypto.pass.{}", bob.id.replace(':', "."))).unwrap().is_none());
}

#[test]
fn keeping_a_key_unlocked_stores_the_passphrase_in_the_secret_store_and_forgetting_asks_again() {
    let rig = Rig::new();
    rig.enable();
    let bob = rig.core.crypto_import_private(fx("bob.sec.asc"), None).unwrap().remove(0);
    let id = rig.deliver_raw("pgp-encrypted-bob.eml");
    rig.core.crypto_open_with(id.clone(), Some("bob-pass".into()), true).unwrap();
    assert!(rig.core.crypto_keys().unwrap().iter().any(|k| k.id == bob.id && k.unlocked));
    assert_eq!(rig.core.crypto_open(id.clone()).unwrap().encryption, EncryptionState::Decrypted);
    rig.core.crypto_forget_passphrase(bob.id.clone()).unwrap();
    assert_eq!(rig.core.crypto_open(id).unwrap().encryption, EncryptionState::NeedPassphrase);
    assert!(!rig.core.crypto_keys().unwrap().iter().any(|k| k.unlocked));
}

#[test]
fn decrypted_text_never_reaches_the_cache_and_files_come_from_memory() {
    let rig = Rig::new();
    rig.enable();
    rig.core.crypto_import(fx("alice.pub.asc"), KeyOrigin::File).unwrap();
    rig.core.crypto_import_private(fx("bob.sec.asc"), None).unwrap();
    let signed = rig.deliver_raw("pgp-encrypted-signed-bob.eml");
    let view = rig.core.crypto_open_with(signed, Some("bob-pass".into()), true).unwrap();
    assert_eq!(view.signature, SignatureState::Verified);
    let with_file = rig.deliver_raw("pgp-encrypted-attachment.eml");
    let view = rig.core.crypto_open(with_file.clone()).unwrap();
    let file = &view.content.as_ref().unwrap().attachments[0];
    let target = rig.dir.path().join("note.txt");
    rig.core.crypto_save_attachment(with_file.clone(), file.index, target.to_string_lossy().into_owned()).unwrap();
    assert_eq!(std::fs::read(&target).unwrap(), b"The attached note.");
    // Nothing of the plaintext is in the database or its log, or in the keyring.
    let cache = cache_bytes(&rig);
    for secret in [SECRET_SENTENCE, "The attached note", "Secret <b>html"] {
        assert!(!contains(&cache, secret), "{secret} found in the cache");
    }
    // Switching the feature off forgets the held files.
    rig.core.set_crypto_enabled(false).unwrap();
    assert!(rig.core.crypto_save_attachment(with_file, 0, target.to_string_lossy().into_owned()).is_err());
}

#[test]
fn removing_a_key_or_signing_out_deletes_its_private_part() {
    let rig = Rig::new();
    rig.enable();
    let bob = rig.core.crypto_import_private(fx("bob.sec.asc"), None).unwrap().remove(0);
    let carol = rig.core.crypto_import_private(fx("carol.p12"), Some("carol-pass".into())).unwrap().remove(0);
    assert_eq!(carol.kind, KeyKind::Smime);
    assert!(rig.core.crypto_import_private(fx("carol.p12"), Some("wrong".into())).is_err());
    let name = |id: &str| format!("crypto.secret.{}", id.replace(':', "."));
    assert!(rig.secrets.get(name(&bob.id)).unwrap().is_some());
    rig.core.crypto_remove_key(bob.id.clone()).unwrap();
    assert!(rig.secrets.get(name(&bob.id)).unwrap().is_none());
    assert!(rig.secrets.get(name(&carol.id)).unwrap().is_some());
    rig.core.sign_out().unwrap();
    assert!(rig.secrets.get(name(&carol.id)).unwrap().is_none(), "the private part goes with the account");
}

#[test]
fn smime_mail_opens_with_an_imported_p12_and_stays_off_the_internet() {
    let rig = Rig::new();
    rig.enable();
    rig.core.crypto_import_private(fx("carol.p12"), Some("carol-pass".into())).unwrap();
    let id = rig.deliver_raw("smime-encrypted.eml");
    let view = rig.core.crypto_open(id).unwrap();
    assert_eq!(view.encryption, EncryptionState::Decrypted);
    assert!(view.remote_content_blocked);
    assert!(view.content.unwrap().text.unwrap().contains("S/MIME signed"));
}
