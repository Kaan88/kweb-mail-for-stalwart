//! Opening encrypted mail, against messages written by GnuPG and OpenSSL (`dev/make_crypto_fixtures.py`).

use zeroize::Zeroizing;

use super::{
    analyze::*,
    openpgp, smime,
    tests::{cert_row, fx, now, pgp_row},
    types::*,
};

const BOB_PASSPHRASE: &str = "bob-pass";
const CAROL_PASSPHRASE: &str = "carol-pass";

fn pgp_secret(file: &str, id: &str, stored: Option<&str>) -> SecretKey {
    let key = openpgp::parse_secret(&fx(file)).unwrap().remove(0);
    SecretKey {
        id: id.into(),
        kind: KeyKind::Pgp,
        label: id.into(),
        fingerprint: openpgp::fingerprint_of_secret(&key),
        material: openpgp::secret_bytes(&key).unwrap(),
        cert: Vec::new(),
        stored_passphrase: stored.map(|p| Zeroizing::new(p.to_string())),
    }
}

fn carol_secret() -> SecretKey {
    let (cert, pkcs8) = smime::import_pkcs12(&fx("carol.p12"), CAROL_PASSPHRASE).unwrap();
    SecretKey {
        id: "carol".into(),
        kind: KeyKind::Smime,
        label: "Carol".into(),
        fingerprint: String::new(),
        material: pkcs8,
        cert,
        stored_passphrase: None,
    }
}

fn open(raw: &[u8], keys: &[KeyRow], secrets: &[SecretKey], passphrase: Option<&str>) -> Outcome {
    let lookup = |_: &str| None;
    analyze(raw, &Context { now: now(), keys, first_seen: &lookup, secrets, passphrase })
}

fn text_of(out: &Outcome) -> String {
    let c = out.view.content.as_ref().expect("opened content");
    format!("{}{}", c.text.clone().unwrap_or_default(), c.html.clone().unwrap_or_default())
}

// ---- OpenPGP -------------------------------------------------------------------------------

#[test]
fn a_locked_key_asks_for_its_passphrase_and_says_when_it_is_wrong() {
    let raw = fx("pgp-encrypted-bob.eml");
    let bob = pgp_secret("bob.sec.asc", "bob", None);
    let asked = open(&raw, &[], std::slice::from_ref(&bob), None);
    assert_eq!(asked.view.kind, CryptoKind::PgpEncrypted);
    assert_eq!(asked.view.encryption, EncryptionState::NeedPassphrase);
    assert_eq!(asked.view.passphrase_for.as_deref(), Some("bob"));
    assert!(asked.view.content.is_none(), "nothing is shown before it is opened");
    let wrong = open(&raw, &[], std::slice::from_ref(&bob), Some("not it"));
    assert_eq!(wrong.view.encryption, EncryptionState::WrongPassphrase);
    assert!(wrong.view.content.is_none());
    let right = open(&raw, &[], std::slice::from_ref(&bob), Some(BOB_PASSPHRASE));
    assert_eq!(right.view.encryption, EncryptionState::Decrypted);
    assert!(text_of(&right).contains("Secret hello"), "{}", text_of(&right));
    assert_eq!(right.unlocked_with.as_deref(), Some("bob"));
    assert!(right.view.notes.contains(&CryptoNote::NotSearchable));
    assert!(!right.view.remote_content_blocked || right.view.kind != CryptoKind::PgpEncrypted);
}

#[test]
fn a_kept_passphrase_opens_the_message_without_asking() {
    let bob = pgp_secret("bob.sec.asc", "bob", Some(BOB_PASSPHRASE));
    let out = open(&fx("pgp-encrypted-bob.eml"), &[], std::slice::from_ref(&bob), None);
    assert_eq!(out.view.encryption, EncryptionState::Decrypted);
    // A kept passphrase that is stale does not open it, and the user is asked again.
    let stale = pgp_secret("bob.sec.asc", "bob", Some("old"));
    assert_eq!(
        open(&fx("pgp-encrypted-bob.eml"), &[], &[stale], None).view.encryption,
        EncryptionState::NeedPassphrase
    );
}

#[test]
fn a_message_for_someone_else_says_there_is_no_key() {
    let alice = pgp_secret("alice.sec.asc", "alice", None);
    let out = open(&fx("pgp-encrypted-bob.eml"), &[], &[alice], None);
    assert_eq!(out.view.encryption, EncryptionState::NeedKey);
    assert_eq!(open(&fx("pgp-encrypted-bob.eml"), &[], &[], None).view.encryption, EncryptionState::NeedKey);
}

#[test]
fn a_message_signed_and_then_encrypted_shows_who_signed_it() {
    let bob = pgp_secret("bob.sec.asc", "bob", Some(BOB_PASSPHRASE));
    let raw = fx("pgp-encrypted-signed-bob.eml");
    let known = open(&raw, &[pgp_row("alice.pub.asc")], std::slice::from_ref(&bob), None);
    assert_eq!(known.view.encryption, EncryptionState::Decrypted);
    assert_eq!(known.view.signature, SignatureState::Verified);
    assert!(known.view.notes.contains(&CryptoNote::SignedInsideEncryption));
    assert!(text_of(&known).contains("signed and sealed"));
    // Without Alice's key the signature cannot be judged, and nothing is claimed.
    let unknown = open(&raw, &[], std::slice::from_ref(&bob), None);
    assert_eq!(unknown.view.encryption, EncryptionState::Decrypted);
    assert_eq!(unknown.view.signature, SignatureState::NoKey);
    // A signature from another address than From is flagged, not trusted.
    let spoofed = String::from_utf8(raw).unwrap().replacen(
        "From: Alice <alice@example.test>",
        "From: Alice <alice@evil.test>",
        1,
    );
    let other = open(spoofed.as_bytes(), &[pgp_row("alice.pub.asc")], std::slice::from_ref(&bob), None);
    assert_eq!(other.view.signature, SignatureState::OtherAddress);
}

#[test]
fn inline_encrypted_text_is_opened_as_text() {
    let bob = pgp_secret("bob.sec.asc", "bob", Some(BOB_PASSPHRASE));
    let out = open(&fx("pgp-inline-encrypted.eml"), &[], &[bob], None);
    assert_eq!(out.view.kind, CryptoKind::PgpInline);
    assert_eq!(out.view.encryption, EncryptionState::Decrypted);
    assert_eq!(out.view.content.unwrap().text.as_deref().map(str::trim), Some("Inline secret."));
}

#[test]
fn the_files_of_an_opened_message_are_held_in_memory_and_its_html_is_the_decrypted_part_alone() {
    let bob = pgp_secret("bob.sec.asc", "bob", Some(BOB_PASSPHRASE));
    let out = open(&fx("pgp-encrypted-attachment.eml"), &[], &[bob], None);
    assert_eq!(out.view.encryption, EncryptionState::Decrypted);
    let content = out.view.content.as_ref().unwrap();
    assert!(content.html.as_deref().unwrap().contains("Secret <b>html</b>"));
    assert_eq!(content.attachments.len(), 1);
    assert_eq!(content.attachments[0].name, "note.txt");
    assert_eq!(&**out.held[0].bytes, b"The attached note.");
}

/// The binary form of the message in an armored fixture.
fn binary_of(eml: &str) -> Vec<u8> {
    let text = String::from_utf8(fx(eml)).unwrap();
    let start = text.find("-----BEGIN PGP MESSAGE-----").unwrap();
    let end = text.find("-----END PGP MESSAGE-----").unwrap();
    let body: String = text[start..end]
        .lines()
        .skip(1)
        .skip_while(|l| !l.trim().is_empty())
        .filter(|l| !l.starts_with('=') && !l.trim().is_empty())
        .collect();
    crate::crypto::mime::base64_lenient(body.as_bytes()).unwrap()
}

fn pgp_mime_with(binary: &[u8]) -> Vec<u8> {
    use base64::{engine::general_purpose::STANDARD, Engine};
    format!(
        "From: Alice <alice@example.test>\r\nMIME-Version: 1.0\r\nContent-Type: multipart/encrypted; protocol=\"application/pgp-encrypted\"; boundary=\"E\"\r\n\r\n--E\r\nContent-Type: application/pgp-encrypted\r\n\r\nVersion: 1\r\n--E\r\nContent-Type: application/octet-stream\r\nContent-Transfer-Encoding: base64\r\n\r\n{}\r\n--E--\r\n",
        STANDARD.encode(binary)
    )
    .into_bytes()
}

#[test]
fn a_message_with_no_integrity_packet_is_refused_and_nothing_of_it_is_shown() {
    let bob = pgp_secret("bob.sec.asc", "bob", Some(BOB_PASSPHRASE));
    let mut binary = binary_of("pgp-encrypted-bob.eml");
    // The first packet is the (old-format) key packet; the data packet follows it.
    assert_eq!(binary[0] & 0xC0, 0x80);
    let at = 2 + usize::from(binary[1]);
    assert_eq!(binary[at], 0xd2, "a new-format packet of tag 18 (data with integrity protection)");
    // Tag 9 is the old data packet that has no integrity check.
    binary[at] = 0xc9;
    let out = open(&pgp_mime_with(&binary), &[], &[bob], None);
    assert_eq!(out.view.encryption, EncryptionState::NoIntegrity);
    assert!(out.view.content.is_none());
}

#[test]
fn changed_ciphertext_is_never_shown() {
    let bob = pgp_secret("bob.sec.asc", "bob", Some(BOB_PASSPHRASE));
    let mut binary = binary_of("pgp-encrypted-bob.eml");
    let last = binary.len() - 5;
    binary[last] ^= 0x40;
    let out = open(&pgp_mime_with(&binary), &[], &[bob], None);
    assert_ne!(out.view.encryption, EncryptionState::Decrypted);
    assert!(out.view.content.is_none());
}

// ---- S/MIME --------------------------------------------------------------------------------

#[test]
fn smime_enveloped_data_is_opened_and_shown_alone_with_remote_content_off() {
    let carol = carol_secret();
    for fixture in ["smime-encrypted.eml", "smime-encrypted-aes128.eml"] {
        let out = open(&fx(fixture), &[cert_row("carol.crt.pem")], std::slice::from_ref(&carol), None);
        assert_eq!(out.view.encryption, EncryptionState::Decrypted, "{fixture}");
        assert!(text_of(&out).contains("Hello, this is S/MIME signed."), "{fixture}");
        assert!(out.view.remote_content_blocked, "{fixture}");
        assert!(out.view.notes.contains(&CryptoNote::RemoteContentBlocked));
    }
}

#[test]
fn smime_without_the_key_or_in_a_newer_form_says_so() {
    assert_eq!(open(&fx("smime-encrypted.eml"), &[], &[], None).view.encryption, EncryptionState::NeedKey);
    let gcm = open(&fx("smime-encrypted-gcm.eml"), &[], &[carol_secret()], None);
    assert_eq!(gcm.view.encryption, EncryptionState::Unsupported);
    assert!(gcm.view.detail.unwrap().contains("AES-GCM"));
}

#[test]
fn an_smime_message_signed_and_then_encrypted_shows_the_signature_inside() {
    let out = open(&fx("smime-signed-encrypted.eml"), &[], &[carol_secret()], None);
    assert_eq!(out.view.encryption, EncryptionState::Decrypted);
    // Erin's certificate came with the message and is not in the keyring: not called trusted.
    assert_eq!(out.view.signature, SignatureState::CertificateNotChecked);
    assert!(out.view.remote_content_blocked);
    assert!(text_of(&out).contains("Hello, this is S/MIME signed."));
    assert!(!text_of(&out).contains("pkcs7"), "the signature part is not shown as content");
    let known = open(&fx("smime-signed-encrypted.eml"), &[cert_row("erin.crt.pem")], &[carol_secret()], None);
    assert_eq!(known.view.signature, SignatureState::Verified);
}

#[test]
fn pkcs12_files_are_read_with_their_passphrase_and_wrong_ones_are_refused() {
    assert!(smime::import_pkcs12(&fx("carol.p12"), CAROL_PASSPHRASE).is_ok());
    assert_eq!(smime::import_pkcs12(&fx("carol.p12"), "nope").unwrap_err(), smime::P12Error::WrongPassphrase);
    // An older file (3DES) is read too, or refused plainly; it never panics.
    let legacy = smime::import_pkcs12(&fx("carol-legacy.p12"), CAROL_PASSPHRASE);
    assert!(legacy.is_ok() || legacy.unwrap_err() == smime::P12Error::WrongPassphrase);
}
