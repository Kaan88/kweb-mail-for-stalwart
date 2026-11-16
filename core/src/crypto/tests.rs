//! Checks against messages written by GnuPG and OpenSSL (`dev/make_crypto_fixtures.py`), so the
//! core is tested against what real programs produce and not against its own output.

use super::{analyze::*, openpgp, smime, types::*};

pub(super) fn fx(name: &str) -> Vec<u8> {
    let path = format!("{}/tests/fixtures/crypto/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("fixture {path}: {e}"))
}

pub(super) fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64
}

pub(super) fn pgp_row(file: &str) -> KeyRow {
    let key = openpgp::parse_public(&fx(file)).unwrap().remove(0);
    let info = openpgp::info(&key);
    KeyRow {
        id: format!("pgp:{}", info.fingerprint),
        kind: KeyKind::Pgp,
        label: info.user_ids.first().cloned().unwrap_or_default(),
        emails: info.emails.clone(),
        fingerprint: info.fingerprint.clone(),
        material: openpgp::public_bytes(&key).unwrap(),
    }
}

pub(super) fn cert_row(file: &str) -> KeyRow {
    let der = smime::parse_certificates(&fx(file)).remove(0);
    let info = smime::cert_info(&der).unwrap();
    KeyRow {
        id: format!("x509:{}", info.fingerprint),
        kind: KeyKind::Smime,
        label: info.subject,
        emails: info.emails,
        fingerprint: info.fingerprint,
        material: der,
    }
}

fn check(raw: &[u8], keys: &[KeyRow], first: Option<&str>) -> Outcome {
    let first = first.map(str::to_string);
    let lookup = move |_: &str| first.clone();
    analyze(raw, &Context { now: now(), keys, first_seen: &lookup, secrets: &[], passphrase: None })
}

fn replace(raw: &[u8], from: &str, to: &str) -> Vec<u8> {
    let text = String::from_utf8(raw.to_vec()).unwrap();
    assert!(text.contains(from), "{from} not in the message");
    text.replacen(from, to, 1).into_bytes()
}

// ---- OpenPGP -------------------------------------------------------------------------------

#[test]
fn a_pgp_mime_signature_from_a_known_key_with_the_same_address_is_verified() {
    let raw = fx("pgp-signed.eml");
    let alice = pgp_row("alice.pub.asc");
    let out = check(&raw, std::slice::from_ref(&alice), None);
    assert_eq!(out.view.kind, CryptoKind::PgpSigned);
    assert_eq!(out.view.signature, SignatureState::Verified);
    assert_eq!(out.view.signer.as_ref().unwrap().fingerprint, alice.fingerprint);
    assert!(out.view.covers_whole_message);
    // The first key seen for the address is remembered.
    assert_eq!(out.record_first_seen, Some(("alice@example.test".into(), alice.fingerprint)));
}

#[test]
fn a_changed_body_makes_the_signature_invalid() {
    let raw = replace(&fx("pgp-signed.eml"), "this is signed", "this is SIGNED");
    let out = check(&raw, &[pgp_row("alice.pub.asc")], None);
    assert_eq!(out.view.signature, SignatureState::Invalid);
    assert_eq!(out.record_first_seen, None);
}

#[test]
fn a_valid_signature_from_another_address_than_from_is_not_good_news() {
    let raw = replace(&fx("pgp-signed.eml"), "From: Alice <alice@example.test>", "From: Alice <alice@evil.test>");
    let out = check(&raw, &[pgp_row("alice.pub.asc")], None);
    assert_eq!(out.view.signature, SignatureState::OtherAddress);
    assert_eq!(out.record_first_seen, None, "nothing is learned from a mismatch");
}

#[test]
fn an_unknown_signing_key_says_so_and_nothing_more() {
    let out = check(&fx("pgp-signed.eml"), &[pgp_row("mallory.pub.asc")], None);
    assert_eq!(out.view.signature, SignatureState::NoKey);
    assert!(out.view.signer.is_none());
    let none = check(&fx("pgp-signed.eml"), &[], None);
    assert_eq!(none.view.signature, SignatureState::NoKey);
}

#[test]
fn another_key_than_the_first_seen_for_the_address_is_a_warning() {
    let alice = pgp_row("alice.pub.asc");
    let same = check(&fx("pgp-signed.eml"), std::slice::from_ref(&alice), Some(&alice.fingerprint));
    assert_eq!(same.view.signature, SignatureState::Verified);
    assert_eq!(same.record_first_seen, None);
    let other = check(&fx("pgp-signed.eml"), &[alice], Some("AAAA"));
    assert_eq!(other.view.signature, SignatureState::KeyChanged);
    assert_eq!(other.record_first_seen, None, "the first key is never replaced");
}

#[test]
fn a_revoked_key_is_reported_even_when_the_signature_fits() {
    let mut rex = pgp_row("rex.pub.asc");
    let revoked = openpgp::parse_public(&fx("rex-revoked.pub.asc")).unwrap().remove(0);
    assert!(openpgp::info(&revoked).revoked);
    rex.material = openpgp::public_bytes(&revoked).unwrap();
    let out = check(&fx("pgp-signed-rex.eml"), &[rex], None);
    assert_eq!(out.view.signature, SignatureState::KeyRevoked);
}

#[test]
fn a_clear_signed_text_is_checked_and_only_the_signed_text_is_shown() {
    let out = check(&fx("pgp-clearsigned.eml"), &[pgp_row("alice.pub.asc")], None);
    assert_eq!(out.view.kind, CryptoKind::PgpInline);
    assert_eq!(out.view.signature, SignatureState::Verified);
    let text = out.view.content.as_ref().and_then(|c| c.text.clone()).unwrap();
    assert!(text.contains("Hello Bob, this is clear-signed."), "{text}");
    // Text added around the block is not covered, so it is not shown and the status says so.
    let around = replace(
        &fx("pgp-clearsigned.eml"),
        "-----BEGIN PGP SIGNED MESSAGE-----",
        "Click here: evil\r\n-----BEGIN PGP SIGNED MESSAGE-----",
    );
    let out = check(&around, &[pgp_row("alice.pub.asc")], None);
    assert_eq!(out.view.signature, SignatureState::Verified);
    assert!(!out.view.covers_whole_message);
    assert!(out.view.notes.contains(&CryptoNote::UnsignedTextHidden));
    assert!(!out.view.content.unwrap().text.unwrap().contains("evil"));
    let tampered = replace(&fx("pgp-clearsigned.eml"), "this is clear-signed", "this is clear-forged");
    assert_eq!(check(&tampered, &[pgp_row("alice.pub.asc")], None).view.signature, SignatureState::Invalid);
}

#[test]
fn a_signed_part_inside_a_wider_message_covers_only_itself() {
    let signed = fx("pgp-signed.eml");
    let text = String::from_utf8(signed).unwrap();
    let at = text.find("Content-Type: multipart/signed").unwrap();
    let entity = &text[at..];
    let raw = format!(
        "From: Alice <alice@example.test>\r\nMIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=\"outer\"\r\n\r\n--outer\r\nContent-Type: text/plain\r\n\r\nUnsigned text next to it\r\n--outer\r\n{entity}\r\n--outer--\r\n"
    );
    let out = check(raw.as_bytes(), &[pgp_row("alice.pub.asc")], None);
    assert_eq!(out.view.signature, SignatureState::Verified);
    assert!(!out.view.covers_whole_message);
    assert!(out.view.notes.contains(&CryptoNote::OnlyPartCovered));
}

#[test]
fn a_key_attached_to_a_message_is_offered_and_not_imported() {
    let out = check(&fx("pgp-keys-attached.eml"), &[], None);
    assert_eq!(out.view.offers.len(), 1);
    assert_eq!(out.view.offers[0].emails, vec!["alice@example.test".to_string()]);
    assert!(!out.view.offers[0].already_known);
    let known = check(&fx("pgp-keys-attached.eml"), &[pgp_row("alice.pub.asc")], None);
    assert!(known.view.offers[0].already_known);
}

#[test]
fn ordinary_mail_has_no_status() {
    let raw = b"From: a@x.test\r\nContent-Type: text/plain\r\n\r\nhello\r\n";
    let out = check(raw, &[], None);
    assert_eq!(out.view.kind, CryptoKind::None);
    assert_eq!(out.view.signature, SignatureState::NotSigned);
    assert_eq!(out.view.encryption, EncryptionState::NotEncrypted);
}

// ---- S/MIME --------------------------------------------------------------------------------

#[test]
fn an_smime_signature_from_an_imported_certificate_is_verified() {
    let carol = cert_row("carol.crt.pem");
    let out = check(&fx("smime-signed.eml"), std::slice::from_ref(&carol), None);
    assert_eq!(out.view.kind, CryptoKind::SmimeSigned);
    assert_eq!(out.view.signature, SignatureState::Verified);
    assert!(out.view.signer.as_ref().unwrap().in_keyring);
    assert_eq!(out.record_first_seen, Some(("carol@example.test".into(), carol.fingerprint)));
    assert!(out.view.notes.contains(&CryptoNote::SmimeNoRevocationCheck));
}

#[test]
fn an_smime_certificate_that_only_came_with_the_message_is_not_called_trusted() {
    let out = check(&fx("smime-signed.eml"), &[], None);
    assert_eq!(out.view.signature, SignatureState::CertificateNotChecked);
    assert!(!out.view.signer.as_ref().unwrap().in_keyring);
    // The certificate is offered; nothing is imported by itself.
    assert_eq!(out.view.offers.len(), 1);
    assert_eq!(out.view.offers[0].kind, KeyKind::Smime);
    assert_eq!(out.view.offers[0].emails, vec!["carol@example.test".to_string()]);
}

#[test]
fn a_changed_smime_body_is_invalid() {
    let raw = replace(&fx("smime-signed.eml"), "S/MIME signed.", "S/MIME forged..");
    assert_eq!(check(&raw, &[cert_row("carol.crt.pem")], None).view.signature, SignatureState::Invalid);
}

#[test]
fn an_smime_signature_from_another_address_than_from_is_flagged() {
    let raw =
        replace(&fx("smime-signed.eml"), "From: Carol Example <carol@example.test>", "From: Carol <carol@evil.test>");
    assert_eq!(check(&raw, &[], None).view.signature, SignatureState::OtherAddress);
}

#[test]
fn an_smime_certificate_that_changed_for_the_address_is_a_warning() {
    let out = check(&fx("smime-signed.eml"), &[], Some("0000"));
    assert_eq!(out.view.signature, SignatureState::KeyChanged);
}

#[test]
fn an_ecdsa_smime_signature_is_checked_too() {
    let out = check(&fx("smime-signed-ec.eml"), &[cert_row("erin.crt.pem")], None);
    assert_eq!(out.view.signature, SignatureState::Verified);
}

#[test]
fn a_certificate_that_had_expired_when_it_signed_is_reported() {
    let out = check(&fx("smime-signed-old.eml"), &[cert_row("olga.crt.pem")], None);
    // OpenSSL writes the signing time of now; the certificate ran out in 2020.
    assert_eq!(out.view.signature, SignatureState::KeyExpired);
}

#[test]
fn a_sha1_signature_is_not_accepted() {
    let out = check(&fx("smime-signed-sha1.eml"), &[cert_row("carol.crt.pem")], None);
    assert_eq!(out.view.signature, SignatureState::Unsupported);
}

#[test]
fn opaque_signed_smime_is_shown_from_the_signed_content_alone_with_remote_content_off() {
    let out = check(&fx("smime-opaque.eml"), &[cert_row("carol.crt.pem")], None);
    assert_eq!(out.view.kind, CryptoKind::SmimeOpaque);
    assert_eq!(out.view.signature, SignatureState::Verified);
    let content = out.view.content.as_ref().expect("content comes from the signed data");
    assert!(content.text.as_deref().unwrap().contains("Hello, this is S/MIME signed."));
    assert!(out.view.remote_content_blocked);
    assert!(out.view.notes.contains(&CryptoNote::RemoteContentBlocked));
}

#[test]
fn types_alone_tell_signed_and_encrypted_mail_from_plain_mail() {
    let t = |media: &str, kids: &[&str]| TypeTree {
        media: media.into(),
        children: kids.iter().map(|k| TypeTree { media: (*k).into(), children: Vec::new() }).collect(),
    };
    assert_eq!(
        kind_from_types(&t("multipart/signed", &["text/plain", "application/pgp-signature"])),
        CryptoKind::PgpSigned
    );
    assert_eq!(
        kind_from_types(&t("multipart/signed", &["multipart/mixed", "application/x-pkcs7-signature"])),
        CryptoKind::SmimeSigned
    );
    assert_eq!(
        kind_from_types(&t("multipart/encrypted", &["application/pgp-encrypted", "application/octet-stream"])),
        CryptoKind::PgpEncrypted
    );
    assert_eq!(kind_from_types(&t("application/pkcs7-mime", &[])), CryptoKind::SmimeOpaque);
    assert_eq!(kind_from_types(&t("multipart/mixed", &["text/plain", "image/png"])), CryptoKind::None);
    assert_eq!(kind_from_types(&t("multipart/signed", &["text/plain", "image/png"])), CryptoKind::None);
    let nested = TypeTree {
        media: "multipart/mixed".into(),
        children: vec![t("text/plain", &[]), t("multipart/signed", &["text/plain", "application/pgp-signature"])],
    };
    assert_eq!(kind_from_types(&nested), CryptoKind::PgpSigned);
    assert_eq!(inline_kind("-----BEGIN PGP MESSAGE-----\nx\n-----END PGP MESSAGE-----"), Some(CryptoKind::PgpInline));
    assert_eq!(inline_kind("just text"), None);
}
