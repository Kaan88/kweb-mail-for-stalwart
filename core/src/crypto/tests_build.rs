//! Writing signed and encrypted mail (stage 3), read back by this program's own stage 1 and 2 code and,
//! where GnuPG and OpenSSL are on this machine, by those too (`tests_external`).

use super::{
    build::{self, Addr, BuildError, Encrypt, File, Headers, Protect, Signer},
    openpgp,
    plan::{self, Ask, PlanError},
    smime,
    tests::{cert_row, fx, now, pgp_row},
    tests_decrypt::{carol_secret, open, pgp_secret, text_of},
    types::*,
};
use crate::store::StoredKey;

fn headers(from: (&str, &str), to: (&str, &str)) -> Headers {
    Headers {
        from: Addr { name: from.0.into(), email: from.1.into() },
        to: vec![Addr { name: to.0.into(), email: to.1.into() }],
        cc: Vec::new(),
        subject: "Plans for Tuesday".into(),
        date: 1_767_323_045,
        message_id: "abc123@example.test".into(),
        in_reply_to: vec!["parent@example.test".into()],
        references: vec!["root@example.test".into(), "parent@example.test".into()],
    }
}

fn alice_secret() -> pgp::composed::SignedSecretKey {
    openpgp::parse_secret(&fx("alice.sec.asc")).unwrap().remove(0)
}

fn bob_public() -> pgp::composed::SignedPublicKey {
    openpgp::parse_public(&fx("bob.pub.asc")).unwrap().remove(0)
}

fn alice_public() -> pgp::composed::SignedPublicKey {
    openpgp::parse_public(&fx("alice.pub.asc")).unwrap().remove(0)
}

fn carol_cert() -> Vec<u8> {
    smime::parse_certificates(&fx("carol.crt.pem")).remove(0)
}

fn carol_pkcs8() -> zeroize::Zeroizing<Vec<u8>> {
    smime::import_pkcs12(&fx("carol.p12"), "carol-pass").unwrap().1
}

fn file() -> File {
    File {
        name: "plan.txt".into(),
        mime_type: "text/plain".into(),
        bytes: b"the attached plan\r\nline two\r\n".to_vec(),
    }
}

fn pgp_sign(alice: &pgp::composed::SignedSecretKey) -> Option<Signer<'_>> {
    Some(Signer::Pgp { key: alice, passphrase: "" })
}

// ---- OpenPGP ------------------------------------------------------------------------------------

#[test]
fn a_pgp_signed_message_is_verified_by_stage_one_and_keeps_its_headers() {
    let alice = alice_secret();
    let h = headers(("Alice", "alice@example.test"), ("Bob", "bob@example.test"));
    let protect = Protect { sign: pgp_sign(&alice), encrypt: None, autocrypt: None, now: now() };
    let raw = build::build(&h, "Hello Bob, a signed note with ünïcode.\n", &[file()], &protect).unwrap();
    let text = String::from_utf8_lossy(&raw).into_owned();
    assert!(text.contains("Subject: Plans for Tuesday\r\n"), "{text}");
    assert!(text.contains("In-Reply-To: <parent@example.test>\r\n"));
    assert!(text.contains("multipart/signed"));
    assert!(!text.contains("Bcc"));
    let out = open(&raw, &[pgp_row("alice.pub.asc")], &[], None);
    assert_eq!(out.view.kind, CryptoKind::PgpSigned);
    assert_eq!(out.view.signature, SignatureState::Verified, "{:?}", out.view);
    assert!(out.view.covers_whole_message);
    // The files are in the signed part: a change to the attachment breaks the signature.
    let tampered = String::from_utf8(raw.clone()).unwrap().replacen("Hello Bob", "Hello Bab", 1);
    // (quoted-printable keeps ASCII words readable)
    let out = open(tampered.as_bytes(), &[pgp_row("alice.pub.asc")], &[], None);
    assert_eq!(out.view.signature, SignatureState::Invalid);
}

#[test]
fn a_pgp_message_is_encrypted_for_the_recipient_and_the_sender_and_both_can_read_it() {
    let alice = alice_secret();
    let (bob_pub, alice_pub) = (bob_public(), alice_public());
    let h = headers(("Alice", "alice@example.test"), ("Bob", "bob@example.test"));
    let protect = Protect {
        sign: None,
        encrypt: Some(Encrypt::Pgp { keys: vec![&bob_pub, &alice_pub], hide: false }),
        autocrypt: None,
        now: now(),
    };
    let raw = build::build(&h, "Secret plans for the week\n", &[file()], &protect).unwrap();
    let text = String::from_utf8_lossy(&raw).into_owned();
    assert!(text.contains("multipart/encrypted"));
    assert!(!text.contains("Secret plans"), "the text is not in the clear");
    assert!(!text.contains("the attached plan"));
    assert!(text.contains("Subject: Plans for Tuesday"), "the subject stays in the clear, as the screens say");
    // Bob (locked key) opens it with his passphrase.
    let bob = pgp_secret("bob.sec.asc", "bob", None);
    let out = open(&raw, &[], std::slice::from_ref(&bob), Some("bob-pass"));
    assert_eq!(out.view.encryption, EncryptionState::Decrypted, "{:?}", out.view);
    assert!(text_of(&out).contains("Secret plans for the week"));
    let c = out.view.content.as_ref().unwrap();
    assert_eq!(c.attachments.len(), 1, "the file is inside the encryption");
    assert_eq!(c.attachments[0].name, "plan.txt");
    // The sender reads her own sent copy.
    let own = pgp_secret("alice.sec.asc", "alice", None);
    let out = open(&raw, &[], std::slice::from_ref(&own), None);
    assert_eq!(out.view.encryption, EncryptionState::Decrypted);
    // Somebody else cannot.
    let mallory = pgp_secret("mallory.sec.asc", "mallory", None);
    let out = open(&raw, &[], std::slice::from_ref(&mallory), Some("x"));
    assert_ne!(out.view.encryption, EncryptionState::Decrypted);
    let _ = alice;
}

#[test]
fn sign_then_encrypt_puts_the_signature_inside_and_it_is_checked_on_opening() {
    let alice = alice_secret();
    let (bob_pub, alice_pub) = (bob_public(), alice_public());
    let h = headers(("Alice", "alice@example.test"), ("Bob", "bob@example.test"));
    let protect = Protect {
        sign: pgp_sign(&alice),
        encrypt: Some(Encrypt::Pgp { keys: vec![&bob_pub, &alice_pub], hide: false }),
        autocrypt: None,
        now: now(),
    };
    let raw = build::build(&h, "Signed and sealed\n", &[], &protect).unwrap();
    let bob = pgp_secret("bob.sec.asc", "bob", None);
    let out = open(&raw, &[pgp_row("alice.pub.asc")], std::slice::from_ref(&bob), Some("bob-pass"));
    assert_eq!(out.view.encryption, EncryptionState::Decrypted);
    assert!(out.view.notes.contains(&CryptoNote::SignedInsideEncryption));
    assert_eq!(out.view.signature, SignatureState::Verified, "{:?}", out.view);
    assert!(text_of(&out).contains("Signed and sealed"));
}

#[test]
fn a_bcc_recipient_is_not_named_by_the_message_or_its_packets_when_hidden() {
    let (bob_pub, alice_pub) = (bob_public(), alice_public());
    let h = headers(("Alice", "alice@example.test"), ("Carl", "carl@example.test"));
    let make = |hide: bool| {
        let protect = Protect {
            sign: None,
            encrypt: Some(Encrypt::Pgp { keys: vec![&bob_pub, &alice_pub], hide }),
            autocrypt: None,
            now: now(),
        };
        build::build(&h, "for bob too\n", &[], &protect).unwrap()
    };
    let armored_ids = |raw: &[u8]| {
        let text = String::from_utf8_lossy(raw).into_owned();
        let start = text.find("-----BEGIN PGP MESSAGE-----").unwrap();
        let end = text.find("-----END PGP MESSAGE-----").unwrap() + "-----END PGP MESSAGE-----".len();
        openpgp::recipient_ids(&text.as_bytes()[start..end]).unwrap()
    };
    let hidden = make(true);
    assert!(armored_ids(&hidden).is_empty(), "the packets do not say whose keys they are for");
    assert!(!String::from_utf8_lossy(&hidden).to_ascii_lowercase().contains("bob@example.test"));
    let named = make(false);
    assert_eq!(armored_ids(&named).len(), 2);
    // Hidden mail still opens: the keys are tried one by one.
    let bob = pgp_secret("bob.sec.asc", "bob", None);
    let out = open(&hidden, &[], std::slice::from_ref(&bob), Some("bob-pass"));
    assert_eq!(out.view.encryption, EncryptionState::Decrypted);
}

#[test]
fn a_locked_signing_key_needs_its_passphrase_and_a_wrong_one_is_told() {
    let bob = openpgp::parse_secret(&fx("bob.sec.asc")).unwrap().remove(0);
    let h = headers(("Bob", "bob@example.test"), ("Alice", "alice@example.test"));
    let with = |pass: &'static str| {
        let protect = Protect {
            sign: Some(Signer::Pgp { key: &bob, passphrase: pass }),
            encrypt: None,
            autocrypt: None,
            now: now(),
        };
        build::build(&h, "x\n", &[], &protect)
    };
    assert_eq!(with("").unwrap_err(), BuildError::NeedPassphrase);
    assert_eq!(with("nope").unwrap_err(), BuildError::WrongPassphrase);
    let raw = with("bob-pass").unwrap();
    assert_eq!(open(&raw, &[pgp_row("bob.pub.asc")], &[], None).view.signature, SignatureState::Verified);
    // No error says anything of the passphrase or the message.
    for e in [BuildError::NeedPassphrase, BuildError::WrongPassphrase] {
        assert!(!e.text().contains("bob-pass"));
    }
}

#[test]
fn a_revoked_key_is_not_encrypted_to() {
    let rex = openpgp::parse_public(&fx("rex-revoked.pub.asc")).unwrap().remove(0);
    assert!(!build::pgp_can_encrypt(&rex, now()));
    let alice_pub = alice_public();
    let h = headers(("Alice", "alice@example.test"), ("Rex", "rex@example.test"));
    let protect = Protect {
        sign: None,
        encrypt: Some(Encrypt::Pgp { keys: vec![&rex, &alice_pub], hide: false }),
        autocrypt: None,
        now: now(),
    };
    assert!(matches!(build::build(&h, "x\n", &[], &protect), Err(BuildError::NoEncryptionKey(_))));
}

// ---- S/MIME -------------------------------------------------------------------------------------

#[test]
fn an_smime_signed_message_is_verified_by_stage_one() {
    let pkcs8 = carol_pkcs8();
    let cert = carol_cert();
    let h = headers(("Carol", "carol@example.test"), ("Bob", "bob@example.test"));
    let protect = Protect {
        sign: Some(Signer::Smime { cert: &cert, pkcs8: &pkcs8 }),
        encrypt: None,
        autocrypt: None,
        now: now(),
    };
    let raw = build::build(&h, "Signed by Carol\n", &[file()], &protect).unwrap();
    let out = open(&raw, &[cert_row("carol.crt.pem")], &[], None);
    assert_eq!(out.view.kind, CryptoKind::SmimeSigned);
    assert_eq!(out.view.signature, SignatureState::Verified, "{:?}", out.view);
    // Without her certificate in the keyring it is only "certificate not checked".
    let out = open(&raw, &[], &[], None);
    assert_eq!(out.view.signature, SignatureState::CertificateNotChecked);
    let tampered = String::from_utf8(raw).unwrap().replacen("Signed by Carol", "Signed by Carob", 1);
    assert_eq!(
        open(tampered.as_bytes(), &[cert_row("carol.crt.pem")], &[], None).view.signature,
        SignatureState::Invalid
    );
}

#[test]
fn an_smime_message_is_encrypted_and_signed_inside_and_the_owner_opens_it() {
    let pkcs8 = carol_pkcs8();
    let cert = carol_cert();
    let h = headers(("Carol", "carol@example.test"), ("Carol", "carol@example.test"));
    let seal = |sign: bool| {
        let protect = Protect {
            sign: sign.then_some(Signer::Smime { cert: &cert, pkcs8: &pkcs8 }),
            encrypt: Some(Encrypt::Smime { certs: vec![cert.as_slice()] }),
            autocrypt: None,
            now: now(),
        };
        build::build(&h, "Sealed for Carol\n", &[file()], &protect).unwrap()
    };
    for sign in [false, true] {
        let raw = seal(sign);
        assert!(!String::from_utf8_lossy(&raw).contains("Sealed for Carol"));
        let carol = carol_secret();
        let out = open(&raw, &[cert_row("carol.crt.pem")], std::slice::from_ref(&carol), None);
        assert_eq!(out.view.encryption, EncryptionState::Decrypted, "{:?}", out.view);
        assert!(text_of(&out).contains("Sealed for Carol"));
        assert_eq!(out.view.content.as_ref().unwrap().attachments.len(), 1);
        if sign {
            assert_eq!(out.view.signature, SignatureState::Verified, "{:?}", out.view);
        }
    }
}

// ---- which keys ---------------------------------------------------------------------------------

fn stored(file: &str, secret: bool, kind: KeyKind, origin: KeyOrigin) -> StoredKey {
    let row = if kind == KeyKind::Pgp { pgp_row(file) } else { cert_row(file) };
    StoredKey {
        entry: KeyEntry {
            id: row.id,
            kind,
            label: row.label,
            emails: row.emails,
            fingerprint: row.fingerprint,
            origin,
            origin_detail: String::new(),
            added_at: 10,
            has_secret: secret,
            needs_passphrase: false,
            unlocked: false,
            expires_at: None,
            verified: false,
            changed_for: Vec::new(),
        },
        material: row.material,
    }
}

fn ask<'a>(recipients: &'a [String], sign: bool, encrypt: bool, bcc: bool) -> Ask<'a> {
    Ask { sender: "alice@example.test", recipients, has_bcc: bcc, sign, encrypt, now: now() }
}

#[test]
fn the_protocol_follows_the_keys_of_the_sender_and_all_recipients() {
    let keys = vec![
        stored("alice.pub.asc", true, KeyKind::Pgp, KeyOrigin::Own),
        stored("bob.pub.asc", false, KeyKind::Pgp, KeyOrigin::File),
        stored("carol.crt.pem", false, KeyKind::Smime, KeyOrigin::File),
    ];
    let none = |_: &str| None;
    let to = vec!["bob@example.test".to_string()];
    let ok = plan::resolve(&keys, &ask(&to, true, true, false), &none).unwrap();
    assert_eq!(ok.protocol, KeyKind::Pgp);
    assert_eq!(ok.signer.as_deref(), Some(keys[0].entry.id.as_str()));
    assert_eq!(ok.recipients.len(), 1);
    // Sign only needs only the sender's key.
    let to = vec!["nobody@example.test".to_string()];
    assert_eq!(plan::resolve(&keys, &ask(&to, true, false, false), &none).unwrap().protocol, KeyKind::Pgp);
    // A recipient without a key: exactly who is named.
    let to = vec!["bob@example.test".to_string(), "nobody@example.test".to_string(), "other@example.test".to_string()];
    match plan::resolve(&keys, &ask(&to, false, true, false), &none) {
        Err(PlanError::Missing { protocol: KeyKind::Pgp, emails }) => {
            assert_eq!(emails, vec!["nobody@example.test", "other@example.test"]);
        }
        other => panic!("{other:?}"),
    }
    // The sender without any private key for the address.
    let none_own = vec![stored("bob.pub.asc", false, KeyKind::Pgp, KeyOrigin::File)];
    assert_eq!(plan::resolve(&none_own, &ask(&to, true, false, false), &none), Err(PlanError::NoOwnKey));
    // Nothing asked for.
    assert_eq!(plan::resolve(&keys, &ask(&to, false, false, false), &none), Err(PlanError::NothingAsked));
}

#[test]
fn smime_cannot_hide_a_bcc_and_is_refused_with_a_reason() {
    let mut carol = stored("carol.crt.pem", true, KeyKind::Smime, KeyOrigin::Own);
    carol.entry.emails = vec!["alice@example.test".into()];
    let keys = vec![carol, stored("erin.crt.pem", false, KeyKind::Smime, KeyOrigin::File)];
    let none = |_: &str| None;
    let to = vec!["erin@example.test".to_string()];
    assert_eq!(plan::resolve(&keys, &ask(&to, true, true, false), &none).unwrap().protocol, KeyKind::Smime);
    assert_eq!(plan::resolve(&keys, &ask(&to, true, true, true), &none), Err(PlanError::BccWithSmime));
}

#[test]
fn a_key_that_came_by_autocrypt_is_used_with_a_warning_and_the_first_seen_one_wins() {
    let mut bob = stored("bob.pub.asc", false, KeyKind::Pgp, KeyOrigin::Autocrypt);
    bob.entry.added_at = 5;
    let alice = stored("alice.pub.asc", true, KeyKind::Pgp, KeyOrigin::Own);
    let keys = vec![alice, bob.clone()];
    let none = |_: &str| None;
    let to = vec!["bob@example.test".to_string()];
    let ok = plan::resolve(&keys, &ask(&to, false, true, false), &none).unwrap();
    assert!(ok.warnings.iter().any(|w| w.contains("Autocrypt")), "{:?}", ok.warnings);
    // A second key for the same address: the one first seen is used, and the warning says so.
    let mut other = stored("mallory.pub.asc", false, KeyKind::Pgp, KeyOrigin::File);
    other.entry.emails = vec!["bob@example.test".into()];
    other.entry.added_at = 99;
    let keys = vec![stored("alice.pub.asc", true, KeyKind::Pgp, KeyOrigin::Own), bob.clone(), other];
    let first = bob.entry.fingerprint.clone();
    let seen = move |_: &str| Some(first.clone());
    let ok = plan::resolve(&keys, &ask(&to, false, true, false), &seen).unwrap();
    assert_eq!(ok.recipients[0].1, bob.entry.id);
    assert!(ok.warnings.iter().any(|w| w.contains("more than one key")));
}

/// Writes messages for programs other than this one to read (`dev/check_crypto_built.py` runs this with
/// `KWEB_CRYPTO_OUT` set and has GnuPG and OpenSSL check them). Does nothing without it.
#[test]
fn messages_for_other_programs_are_written_when_asked() {
    let Ok(dir) = std::env::var("KWEB_CRYPTO_OUT") else { return };
    let (alice, bob_pub, alice_pub) = (alice_secret(), bob_public(), alice_public());
    let h = headers(("Alice", "alice@example.test"), ("Bob", "bob@example.test"));
    let write = |name: &str, protect: Protect<'_>, files: &[File]| {
        let raw = build::build(
            &h,
            "Secret plans for the week
with a second line
",
            files,
            &protect,
        )
        .unwrap();
        std::fs::write(std::path::Path::new(&dir).join(name), raw).unwrap();
    };
    write("pgp-signed.eml", Protect { sign: pgp_sign(&alice), encrypt: None, autocrypt: None, now: now() }, &[file()]);
    write(
        "pgp-encrypted.eml",
        Protect {
            sign: pgp_sign(&alice),
            encrypt: Some(Encrypt::Pgp { keys: vec![&bob_pub, &alice_pub], hide: false }),
            autocrypt: None,
            now: now(),
        },
        &[file()],
    );
    let (cert, pkcs8) = (carol_cert(), carol_pkcs8());
    let h = headers(("Carol", "carol@example.test"), ("Carol", "carol@example.test"));
    let smime = |name: &str, sign: bool, encrypt: bool| {
        let protect = Protect {
            sign: sign.then_some(Signer::Smime { cert: &cert, pkcs8: &pkcs8 }),
            encrypt: encrypt.then(|| Encrypt::Smime { certs: vec![cert.as_slice()] }),
            autocrypt: None,
            now: now(),
        };
        let raw = build::build(
            &h,
            "Secret plans for the week
with a second line
",
            &[file()],
            &protect,
        )
        .unwrap();
        std::fs::write(std::path::Path::new(&dir).join(name), raw).unwrap();
    };
    smime("smime-signed.eml", true, false);
    smime("smime-encrypted.eml", true, true);
}
