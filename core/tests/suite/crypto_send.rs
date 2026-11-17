//! M9 stage 3 through the `Core` object against the mock server: signed and encrypted sending. The rule under test
//! is the project's: a message is never sent twice and never lost, and a message that was asked to be protected is
//! never sent as plain text; whatever is missing stops the send before anything reaches the server.

use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::support::{MockServer, Refusal};
use kweb_core::{
    crypto::types::{KeyKind, KeyOrigin},
    ffi::{Core, Shade},
    model::{Address, Draft, DraftKind, Protection},
    secrets::MemorySecrets,
    store::Store,
    Error,
};
use tokio::runtime::Runtime;

fn fx(name: &str) -> Vec<u8> {
    let path = format!("{}/tests/fixtures/crypto/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

struct Rig {
    // Field order is drop order: the core goes first, the mock and its runtime last.
    core: Arc<Core>,
    server: MockServer,
    rt: Runtime,
    db_path: String,
    _dir: tempfile::TempDir,
}

impl Rig {
    /// Signed in as `identity` (an address with one identity) and synced once.
    fn new(identity: (&str, &str)) -> Rig {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        let server = rt.block_on(MockServer::start());
        server.set_identities(&[("i1", identity.0, identity.1, "")]);
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("cache.db").to_string_lossy().into_owned();
        let core = Core::new(db_path.clone(), Arc::new(MemorySecrets::default())).unwrap();
        let url = core.begin_sign_in_with_redirect(server.url(), "ovh.kweb.mail:/oauth/callback".into()).unwrap();
        core.complete_sign_in(server.authorize(&url).unwrap()).unwrap();
        core.set_crypto_enabled(true).unwrap();
        let rig = Rig { core, server, rt, db_path, _dir: dir };
        rig.sync();
        rig
    }

    fn alice() -> Rig {
        let rig = Rig::new(("Alice", "alice@example.test"));
        rig.core.crypto_import_private(fx("alice.sec.asc"), None).unwrap();
        rig
    }

    fn sync(&self) {
        self.core.check_once(Shade::Unknown).expect("a pass");
    }

    fn store(&self) -> Store {
        Store::open(std::path::Path::new(&self.db_path)).unwrap()
    }

    fn account(&self) -> String {
        self.core.account().unwrap().id
    }

    fn import_public(&self, file: &str) {
        self.core.crypto_import(fx(file), KeyOrigin::File).unwrap();
    }

    /// A message to Bob, with the protection asked for.
    fn note(&self, sign: bool, encrypt: bool) -> Draft {
        let mut draft = self.core.new_draft(DraftKind::New).unwrap();
        draft.to = vec![Address { name: "Bob".into(), email: "bob@example.test".into() }];
        draft.subject = "Plans".into();
        draft.body = "Hello Bob, the plans are in the attachment".into();
        draft.protection = Protection { sign, encrypt, quotes_decrypted: false };
        draft
    }
}

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64
}

fn is_invalid<T>(r: &Result<T, Error>) -> bool {
    matches!(r, Err(Error::Invalid { .. }))
}

fn text_of(e: &Error) -> String {
    e.to_string()
}

#[test]
fn an_encrypted_send_reaches_the_server_only_as_the_finished_message_and_without_the_bcc() {
    let rig = Rig::alice();
    rig.import_public("bob.pub.asc");
    rig.import_public("mallory.pub.asc");
    let mut draft = rig.note(true, true);
    draft.bcc = vec![Address { name: String::new(), email: "mallory@example.test".into() }];
    let check = rig.core.crypto_send_check(draft.clone()).unwrap();
    assert!(check.ready && check.bcc_hidden, "{check:?}");
    assert_eq!(check.protocol, Some(KeyKind::Pgp));
    rig.core.send_protected(draft.clone(), 3600, None, false).unwrap();
    let held = rig.server.held();
    assert_eq!(held.len(), 1, "held by the server for the undo");
    // The envelope names everyone, the Bcc included.
    assert!(held[0].recipients.iter().any(|r| r == "mallory@example.test"), "{:?}", held[0].recipients);
    assert!(held[0].recipients.iter().any(|r| r == "bob@example.test"));
    let raw = String::from_utf8_lossy(&rig.server.raw_of(&held[0].email_id).expect("an imported message")).into_owned();
    assert!(raw.contains("multipart/encrypted"), "{raw}");
    assert!(!raw.contains("Hello Bob"), "no plain text on the server");
    // No Bcc header. (Not a plain search for "bcc": the base64 ciphertext holds those letters now and then.)
    assert!(!raw.lines().any(|l| l.to_ascii_lowercase().starts_with("bcc:")), "{raw}");
    assert!(!raw.to_ascii_lowercase().contains("mallory"), "the Bcc is nowhere in the message");
    // Exactly the bytes that were stored with the row.
    let stored = rig.store().draft_raw(&rig.account(), &draft.id).unwrap().expect("stored with the row");
    assert_eq!(stored, raw.as_bytes());
}

#[test]
fn a_recipient_without_a_key_stops_the_send_before_anything_is_stored_or_uploaded() {
    let rig = Rig::alice();
    let draft = rig.note(true, true);
    let check = rig.core.crypto_send_check(draft.clone()).unwrap();
    assert!(!check.ready);
    assert_eq!(check.missing, vec!["bob@example.test"]);
    let result = rig.core.send_protected(draft.clone(), 3600, None, false);
    assert!(is_invalid(&result), "{result:?}");
    assert!(text_of(&result.unwrap_err()).contains("bob@example.test"));
    assert!(rig.server.file_uploads().is_empty(), "nothing was uploaded");
    assert!(rig.server.ids_in("drafts").is_empty());
    assert!(rig.server.held().is_empty() && rig.server.submissions().is_empty());
    // The draft is as it was: still a draft, no error recorded, nothing waiting to go.
    assert!(rig.core.pending_sends().unwrap().is_empty());
    let kept = rig.core.draft(draft.id.clone()).unwrap().expect("the draft is kept");
    assert_eq!(kept.send_error, None);
    assert!(rig.store().draft_raw(&rig.account(), &draft.id).unwrap().is_none());
    // The plain `send` does not go around it either.
    assert!(is_invalid(&rig.core.send(draft, 0)));
    assert!(rig.server.file_uploads().is_empty());
}

#[test]
fn a_retry_after_a_failed_upload_sends_the_same_bytes_once() {
    let rig = Rig::alice();
    rig.import_public("bob.pub.asc");
    let draft = rig.note(true, true);
    let sent_before = rig.server.ids_in("sent");
    rig.server.fail_next_imports(1);
    rig.core.send_protected(draft.clone(), 0, None, false).unwrap();
    let stored = rig.store().draft_raw(&rig.account(), &draft.id).unwrap().unwrap();
    // The first pass fails at the server; the message is still on its way (never lost), nothing went out.
    let _ = rig.core.check_once(Shade::Unknown);
    assert!(rig.server.submissions().is_empty() && rig.server.held().is_empty());
    assert_eq!(rig.core.pending_sends().unwrap().len(), 1);
    // The next one carries it.
    rig.sync();
    rig.sync();
    let sent = rig.server.submissions();
    assert_eq!(sent.len(), 1, "sent exactly once");
    assert!(sent[0].recipients.iter().any(|r| r == "bob@example.test"));
    assert_eq!(rig.server.raw_of(&sent[0].email_id).unwrap(), stored, "the very bytes made when the user sent it");
    let mut expected = sent_before;
    expected.push(sent[0].email_id.clone());
    assert_eq!(rig.server.ids_in("sent"), expected);
    assert!(rig.server.ids_in("drafts").is_empty(), "no second copy");
    assert!(rig.core.pending_sends().unwrap().is_empty());
}

#[test]
fn a_held_protected_send_retried_after_a_failed_hand_over_is_one_held_message_and_can_be_undone() {
    let rig = Rig::alice();
    rig.import_public("bob.pub.asc");
    let draft = rig.note(false, true);
    // The copy is made, the submission fails once: the message waits here and is held on the next pass.
    rig.server.set_submission_refusal(Some(Refusal::Temporary));
    rig.core.send_protected(draft.clone(), 3600, None, false).unwrap();
    assert!(rig.server.held().is_empty(), "the hand-over failed");
    assert_eq!(rig.core.pending_sends().unwrap().len(), 1, "never lost");
    rig.server.set_submission_refusal(None);
    rig.sync();
    assert_eq!(rig.server.held().len(), 1);
    rig.sync();
    assert_eq!(rig.server.held().len(), 1, "never held twice");
    let copy = rig.server.held()[0].email_id.clone();
    let back = rig.core.undo_send(draft.id.clone()).unwrap();
    assert!(back.protection.encrypt, "the draft is back as the user wrote it, plain, still to be encrypted");
    assert_eq!(back.body, draft.body);
    assert!(rig.server.held().is_empty() && rig.server.submissions().is_empty());
    assert!(rig.store().draft_raw(&rig.account(), &draft.id).unwrap().is_none(), "the finished bytes are dropped");
    // The finished copy that was put back in Drafts is removed: a draft to be encrypted is never kept on the
    // server, and the plain text is nowhere there.
    rig.sync();
    assert!(rig.server.ids_in("drafts").is_empty(), "{:?} {copy}", rig.server.ids_in("drafts"));
}

#[test]
fn a_locked_key_needs_its_passphrase_before_anything_happens() {
    let rig = Rig::new(("Bob", "bob@example.test"));
    let imported = rig.core.crypto_import_private(fx("bob.sec.asc"), None).unwrap().remove(0);
    rig.import_public("alice.pub.asc");
    let draft = {
        let mut d = rig.note(true, false);
        d.to = vec![Address { name: String::new(), email: "alice@example.test".into() }];
        d
    };
    let check = rig.core.crypto_send_check(draft.clone()).unwrap();
    assert!(check.ready);
    assert_eq!(check.needs_passphrase.as_deref(), Some(imported.label.as_str()));
    let missing = rig.core.send_protected(draft.clone(), 3600, None, false);
    assert!(is_invalid(&missing) && text_of(&missing.unwrap_err()).contains("passphrase"));
    let wrong = rig.core.send_protected(draft.clone(), 3600, Some("nope".into()), false);
    assert!(is_invalid(&wrong) && text_of(&wrong.unwrap_err()).contains("does not open"));
    assert!(rig.server.file_uploads().is_empty() && rig.core.pending_sends().unwrap().is_empty());
    assert!(!rig.core.crypto_keys().unwrap().iter().any(|k| k.unlocked), "a wrong try keeps nothing");
    // The right one, kept: the key is unlocked from now on and the next message needs no asking.
    rig.core.send_protected(draft.clone(), 3600, Some("bob-pass".into()), true).unwrap();
    assert!(rig.core.crypto_keys().unwrap().iter().any(|k| k.id == imported.id && k.unlocked));
    assert!(rig.core.crypto_send_check(draft).unwrap().needs_passphrase.is_none());
    assert_eq!(rig.server.held().len(), 1);
}

#[test]
fn a_draft_to_be_encrypted_is_never_kept_on_the_server_as_plain_text() {
    let rig = Rig::alice();
    rig.import_public("bob.pub.asc");
    let mut draft = rig.note(false, false);
    // Plain: the server keeps a backup copy, as before.
    rig.core.save_draft(draft.clone()).unwrap();
    rig.sync();
    assert_eq!(rig.server.ids_in("drafts").len(), 1);
    // Encrypt switched on: the copy is removed and none is made while it is on.
    draft.protection.encrypt = true;
    rig.core.save_draft(draft.clone()).unwrap();
    rig.sync();
    assert!(rig.server.ids_in("drafts").is_empty(), "no plain copy of what is to be encrypted");
    rig.sync();
    assert!(rig.server.ids_in("drafts").is_empty());
    assert_eq!(rig.core.draft(draft.id.clone()).unwrap().unwrap().body, draft.body, "the text is kept on the phone");
    // Off again: saved as before.
    draft.protection.encrypt = false;
    rig.core.save_draft(draft).unwrap();
    rig.sync();
    // A reply that quotes decrypted text stays off the server too, even with Encrypt off.
    let mut quoting = rig.note(false, false);
    quoting.protection.quotes_decrypted = true;
    rig.core.save_draft(quoting).unwrap();
    rig.sync();
    assert_eq!(rig.server.ids_in("drafts").len(), 1, "only the first draft has a copy");

    assert_eq!(rig.server.ids_in("drafts").len(), 1);
}

#[test]
fn a_message_that_asked_for_protection_is_never_sent_as_plain_text_even_if_the_core_is_bypassed() {
    let rig = Rig::alice();
    rig.import_public("bob.pub.asc");
    let draft = rig.note(false, true);
    let sent_before = rig.server.ids_in("sent");
    // Hand it over as a plain send behind the core's back (an older build, a bug): the pass must refuse it.
    let store = rig.store();
    store.draft_send(&rig.account(), &draft, now(), now()).unwrap();
    rig.sync();
    rig.sync();
    assert!(rig.server.submissions().is_empty() && rig.server.held().is_empty(), "nothing was sent");
    assert_eq!(rig.server.ids_in("sent"), sent_before);
    assert!(rig.server.ids_in("drafts").is_empty(), "and no plain copy was made");
    let back = rig.core.draft(draft.id.clone()).unwrap().expect("a draft again");
    assert!(back.send_error.as_deref().is_some_and(|e| e.contains("not sent")), "{:?}", back.send_error);
}

#[test]
fn a_scheduled_protected_message_is_held_as_finished_bytes_and_leaves_no_row() {
    let rig = Rig::alice();
    rig.import_public("bob.pub.asc");
    let draft = rig.note(true, true);
    let held = rig.core.schedule_send_protected(draft.clone(), now() + 3600, None, false).unwrap();
    assert_eq!(held.subject, "Plans");
    let server_held = rig.server.held();
    assert_eq!(server_held.len(), 1);
    let raw = String::from_utf8_lossy(&rig.server.raw_of(&server_held[0].email_id).unwrap()).into_owned();
    assert!(raw.contains("multipart/encrypted") && !raw.contains("Hello Bob"));
    assert!(rig.core.draft(draft.id.clone()).unwrap().is_none(), "the server holds it; the row is gone");
    rig.sync();
    assert_eq!(rig.server.held().len(), 1, "built once, never again");
    // A missing key stops a schedule just the same, before the row is written.
    let mut other = rig.note(false, true);
    other.to = vec![Address { name: String::new(), email: "nobody@example.test".into() }];
    assert!(is_invalid(&rig.core.schedule_send_protected(other.clone(), now() + 3600, None, false)));
    assert_eq!(rig.server.held().len(), 1);
    assert!(rig.core.draft(other.id).unwrap().is_some_and(|d| d.send_error.is_none()));
}

#[test]
fn signing_only_sends_a_signed_message_whose_text_the_server_can_still_read() {
    let rig = Rig::alice();
    let draft = rig.note(true, false);
    rig.core.send_protected(draft, 3600, None, false).unwrap();
    let held = rig.server.held();
    let raw = String::from_utf8_lossy(&rig.server.raw_of(&held[0].email_id).unwrap()).into_owned();
    assert!(raw.contains("multipart/signed") && raw.contains("application/pgp-signature"));
    assert!(raw.contains("Hello Bob"), "signed, not encrypted");
    assert!(raw.contains("Subject: Plans"));
}

#[test]
fn the_switch_off_and_a_missing_own_key_are_said_in_words() {
    let rig = Rig::new(("Alice", "alice@example.test"));
    let draft = rig.note(true, false);
    let check = rig.core.crypto_send_check(draft.clone()).unwrap();
    assert!(!check.ready && check.problem.as_deref().is_some_and(|p| p.contains("private key")), "{check:?}");
    assert!(is_invalid(&rig.core.send_protected(draft.clone(), 3600, None, false)));
    rig.core.set_crypto_enabled(false).unwrap();
    let check = rig.core.crypto_send_check(draft.clone()).unwrap();
    assert!(!check.ready && check.problem.as_deref().is_some_and(|p| p.contains("switched off")));
    assert!(is_invalid(&rig.core.send_protected(draft, 3600, None, false)));
    assert!(rig.server.file_uploads().is_empty());
}

#[test]
fn new_messages_start_signed_when_the_account_says_so_and_the_address_has_a_key() {
    let rig = Rig::alice();
    assert!(!rig.core.new_draft(DraftKind::New).unwrap().protection.sign);
    rig.core.set_crypto_sign_default(true).unwrap();
    assert!(rig.core.crypto_sign_default().unwrap());
    assert!(rig.core.new_draft(DraftKind::New).unwrap().protection.sign);
    let plain = Rig::new(("Alice", "alice@example.test"));
    plain.core.set_crypto_sign_default(true).unwrap();
    assert!(!plain.core.new_draft(DraftKind::New).unwrap().protection.sign, "no key for the address: not offered");
    let _ = &plain.rt;
}

#[test]
fn a_failed_import_leaves_the_message_waiting_here_and_it_is_not_lost() {
    let rig = Rig::alice();
    rig.import_public("bob.pub.asc");
    let draft = rig.note(false, true);
    rig.server.fail_next_imports(1);
    rig.core.send_protected(draft.clone(), 3600, None, false).unwrap();
    assert!(rig.server.held().is_empty() && rig.server.ids_in("drafts").is_empty());
    assert_eq!(rig.core.pending_sends().unwrap().len(), 1);
    assert!(rig.store().draft_raw(&rig.account(), &draft.id).unwrap().is_some(), "the finished bytes are kept");
    // Taken back before it left: a draft again, plain, with Encrypt still on.
    let back = rig.core.undo_send(draft.id.clone()).unwrap();
    assert!(back.protection.encrypt);
    assert!(rig.store().draft_raw(&rig.account(), &draft.id).unwrap().is_none());
}

#[test]
fn a_signed_message_carries_our_public_key_in_an_autocrypt_header_and_nothing_private() {
    let rig = Rig::alice();
    let draft = rig.note(true, false);
    rig.core.send_protected(draft, 3600, None, false).unwrap();
    let held = rig.server.held();
    let raw = rig.server.raw_of(&held[0].email_id).unwrap();
    let text = String::from_utf8_lossy(&raw).into_owned();
    let (head, _) = text.split_once("\r\n\r\n").unwrap();
    let unfolded = head.replace("\r\n ", "");
    let line = unfolded.lines().find(|l| l.starts_with("Autocrypt:")).expect("an Autocrypt header");
    let header = kweb_core::crypto::autocrypt::parse(line.trim_start_matches("Autocrypt:")).expect("a readable header");
    assert_eq!(header.addr, "alice@example.test");
    let keys = kweb_core::crypto::openpgp::parse_public(&header.keydata).expect("a public key");
    assert!(keys[0].to_armored_string(Default::default()).unwrap().contains("PUBLIC KEY BLOCK"));
    assert!(kweb_core::crypto::openpgp::parse_secret(&header.keydata).is_err(), "no private part is sent");
    assert_eq!(
        kweb_core::crypto::openpgp::info(&keys[0]).fingerprint,
        rig.core.crypto_keys().unwrap().iter().find(|k| k.has_secret).unwrap().fingerprint
    );
}

#[test]
fn a_key_from_autocrypt_encrypts_with_a_warning_until_the_user_marks_it_verified() {
    let rig = Rig::alice();
    // Bob's key arrives only in the header of a mail the user opened.
    let key = kweb_core::crypto::openpgp::parse_public(&fx("bob.pub.asc")).unwrap().remove(0);
    let bytes = kweb_core::crypto::openpgp::public_bytes(&key).unwrap();
    let value = format!(
        "addr=bob@example.test; keydata={}",
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, bytes)
    );
    let id = rig.server.deliver("Bob", "bob@example.test", "Hi", "x");
    rig.server.set_bodies(&id, Some("hello"), None);
    rig.server.set_delivery_headers(&id, &[("Autocrypt", &value)]);
    rig.core.crypto_probe(id).unwrap();
    let draft = rig.note(false, true);
    let check = rig.core.crypto_send_check(draft.clone()).unwrap();
    assert!(check.ready, "{check:?}");
    assert!(
        check.warnings.iter().any(|w| w.contains("Autocrypt") && w.contains("not been checked")),
        "{:?}",
        check.warnings
    );
    let bob = rig.core.crypto_keys().unwrap().into_iter().find(|k| k.origin == KeyOrigin::Autocrypt).unwrap();
    rig.core.crypto_set_verified(bob.id, true).unwrap();
    let check = rig.core.crypto_send_check(draft).unwrap();
    assert!(check.ready && check.warnings.is_empty(), "{check:?}");
}
