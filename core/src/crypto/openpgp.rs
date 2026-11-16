//! OpenPGP with rPGP: reading keys, checking signatures and (stage 2) decrypting.
//!
//! Nothing here touches the store or the network. Every function takes bytes and returns
//! plain results, so the rules can be tested without a server. Secret material is read into
//! rPGP's own types (which wipe themselves) and never leaves this module as text.

use std::io::{Cursor, Read};

use pgp::{
    composed::{
        CleartextSignedMessage, Deserializable, DetachedSignature, Message, SignedPublicKey, SignedPublicSubKey,
        SignedSecretKey,
    },
    types::{KeyDetails, Password, VerifyingKey},
};
use zeroize::Zeroizing;

/// What the keyring and the UI need to know about one OpenPGP certificate.
#[derive(Debug, Clone, PartialEq)]
pub struct KeyInfo {
    /// The primary key's fingerprint, upper-case hex.
    pub fingerprint: String,
    pub user_ids: Vec<String>,
    /// The addresses of the user ids, lower case.
    pub emails: Vec<String>,
    pub created: i64,
    /// When the key stops being valid (from its latest self-signature), if it does.
    pub expires: Option<i64>,
    pub revoked: bool,
}

pub fn hex_upper(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect()
}

fn is_armored(data: &[u8]) -> bool {
    let start = data.iter().position(|b| !b.is_ascii_whitespace()).unwrap_or(0);
    data[start..].starts_with(b"-----BEGIN")
}

/// The address of a user id such as `Name <name@example.org>` or a bare address.
pub fn address_of_user_id(id: &str) -> Option<String> {
    let inner = match (id.rfind('<'), id.rfind('>')) {
        (Some(open), Some(close)) if open < close => &id[open + 1..close],
        _ => id.trim(),
    };
    let inner = inner.trim().to_ascii_lowercase();
    (inner.contains('@') && !inner.contains(char::is_whitespace)).then_some(inner)
}

/// The public keys in `data` (armored or binary). Keys whose self-signatures do not check out
/// are refused: a key with a forged binding is not a key to trust.
pub fn parse_public(data: &[u8]) -> Result<Vec<SignedPublicKey>, String> {
    let keys: Vec<SignedPublicKey> = if is_armored(data) {
        let (iter, _) =
            SignedPublicKey::from_armor_many(Cursor::new(data.to_vec())).map_err(|_| "not an OpenPGP key")?;
        iter.collect::<Result<_, _>>().map_err(|_| "not an OpenPGP key")?
    } else {
        SignedPublicKey::from_bytes_many(Cursor::new(data.to_vec()))
            .map_err(|_| "not an OpenPGP key")?
            .collect::<Result<_, _>>()
            .map_err(|_| "not an OpenPGP key")?
    };
    if keys.is_empty() {
        return Err("no OpenPGP key in it".into());
    }
    for key in &keys {
        key.verify_bindings().map_err(|_| "the key's own signatures do not check out")?;
    }
    Ok(keys)
}

/// The secret keys in `data` (armored or binary).
pub fn parse_secret(data: &[u8]) -> Result<Vec<SignedSecretKey>, String> {
    let keys: Vec<SignedSecretKey> = if is_armored(data) {
        let (iter, _) =
            SignedSecretKey::from_armor_many(Cursor::new(data.to_vec())).map_err(|_| "not an OpenPGP key")?;
        iter.collect::<Result<_, _>>().map_err(|_| "not an OpenPGP key")?
    } else {
        SignedSecretKey::from_bytes_many(Cursor::new(data.to_vec()))
            .map_err(|_| "not an OpenPGP key")?
            .collect::<Result<_, _>>()
            .map_err(|_| "not an OpenPGP key")?
    };
    if keys.is_empty() {
        return Err("no OpenPGP secret key in it".into());
    }
    Ok(keys)
}

pub fn public_bytes(key: &SignedPublicKey) -> Result<Vec<u8>, String> {
    use pgp::ser::Serialize;
    key.to_bytes().map_err(|_| "the key could not be written".to_string())
}

pub fn info(key: &SignedPublicKey) -> KeyInfo {
    let user_ids: Vec<String> =
        key.details.users.iter().filter_map(|u| std::str::from_utf8(u.id.id()).ok().map(str::to_string)).collect();
    let mut emails: Vec<String> = user_ids.iter().filter_map(|u| address_of_user_id(u)).collect();
    emails.dedup();
    let created = i64::from(key.primary_key.created_at().as_secs());
    // The latest self-signature decides about the expiry.
    let latest = key
        .details
        .users
        .iter()
        .flat_map(|u| u.signatures.iter())
        .chain(key.details.direct_signatures.iter())
        .filter(|s| s.created().is_some())
        .max_by_key(|s| s.created().map(|t| t.as_secs()));
    let expires = latest.and_then(|s| s.key_expiration_time()).map(|d| created + i64::from(d.as_secs()));
    KeyInfo {
        fingerprint: hex_upper(key.primary_key.fingerprint().as_bytes()),
        user_ids,
        emails,
        created,
        expires,
        revoked: !key.details.revocation_signatures.is_empty(),
    }
}

/// A key that may have made a signature: the primary key or one of its subkeys.
enum Candidate<'a> {
    Primary(&'a SignedPublicKey),
    Sub(&'a SignedPublicSubKey),
}

impl Candidate<'_> {
    fn fingerprint(&self) -> Vec<u8> {
        match self {
            Candidate::Primary(k) => k.primary_key.fingerprint().as_bytes().to_vec(),
            Candidate::Sub(k) => k.key.fingerprint().as_bytes().to_vec(),
        }
    }

    fn key_id(&self) -> Vec<u8> {
        match self {
            Candidate::Primary(k) => k.primary_key.legacy_key_id().as_ref().to_vec(),
            Candidate::Sub(k) => k.key.legacy_key_id().as_ref().to_vec(),
        }
    }
}

fn candidates(key: &SignedPublicKey) -> Vec<Candidate<'_>> {
    let mut out = vec![Candidate::Primary(key)];
    out.extend(key.public_subkeys.iter().map(Candidate::Sub));
    out
}

/// Who made a signature that checked out.
#[derive(Debug, Clone, PartialEq)]
pub struct Signer {
    /// Which of the keys given (by position) it was.
    pub key_index: usize,
    /// The primary fingerprint of that key.
    pub fingerprint: String,
    /// When the signature says it was made.
    pub made_at: Option<i64>,
    /// The signature carries an expiry that has passed.
    pub signature_expired: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    /// The signature is right for the message and made by one of the keys.
    Valid(Signer),
    /// One of the keys made it (its id matches) but the signature does not fit the message.
    Invalid { key_index: usize },
    /// None of the keys is the signer's. `issuer` is the signer's id when the signature gives one.
    NoKey { issuer: Option<String> },
    /// Not a signature this program can read.
    Unreadable,
}

fn issuer_matches(sig: &pgp::packet::Signature, cand: &Candidate<'_>) -> bool {
    let fingerprints = sig.issuer_fingerprint();
    if !fingerprints.is_empty() {
        let fp = cand.fingerprint();
        return fingerprints.iter().any(|f| f.as_bytes() == fp.as_slice());
    }
    let ids = sig.issuer_key_id();
    if !ids.is_empty() {
        let id = cand.key_id();
        return ids.iter().any(|i| i.as_ref() == id.as_slice());
    }
    true
}

fn issuer_text(sig: &pgp::packet::Signature) -> Option<String> {
    sig.issuer_fingerprint()
        .first()
        .map(|f| hex_upper(f.as_bytes()))
        .or_else(|| sig.issuer_key_id().first().map(|i| hex_upper(i.as_ref())))
}

fn signer_of(sig: &pgp::packet::Signature, key_index: usize, key: &SignedPublicKey, now: i64) -> Signer {
    let made_at = sig.created().map(|t| i64::from(t.as_secs()));
    let signature_expired = match (made_at, sig.signature_expiration_time()) {
        (Some(at), Some(valid_for)) => at + i64::from(valid_for.as_secs()) < now,
        _ => false,
    };
    Signer { key_index, fingerprint: hex_upper(key.primary_key.fingerprint().as_bytes()), made_at, signature_expired }
}

/// Checks a detached signature (the `application/pgp-signature` part) over `content`.
pub fn verify_detached(signature: &[u8], content: &[u8], keys: &[SignedPublicKey], now: i64) -> Verdict {
    let parsed = if is_armored(signature) {
        DetachedSignature::from_armor_single(Cursor::new(signature.to_vec())).map(|(s, _)| s)
    } else {
        DetachedSignature::from_bytes(Cursor::new(signature.to_vec()))
    };
    let Ok(detached) = parsed else { return Verdict::Unreadable };
    let sig = &detached.signature;
    let mut matched = None;
    for (index, key) in keys.iter().enumerate() {
        for cand in candidates(key).iter().filter(|c| issuer_matches(sig, c)) {
            matched = Some(index);
            let outcome = match cand {
                Candidate::Primary(k) => sig.verify(*k, content),
                Candidate::Sub(k) => sig.verify(*k, content),
            };
            if outcome.is_ok() {
                return Verdict::Valid(signer_of(sig, index, key, now));
            }
        }
    }
    match matched {
        Some(key_index) => Verdict::Invalid { key_index },
        None => Verdict::NoKey { issuer: issuer_text(sig) },
    }
}

/// The text of a clear-signed message and how its signature stands.
pub struct Cleartext {
    pub text: String,
    pub verdict: Verdict,
}

/// Checks a `-----BEGIN PGP SIGNED MESSAGE-----` block. The text returned is what the signature
/// covers (dash-escapes removed), never what surrounds the block.
pub fn verify_cleartext(armored: &str, keys: &[SignedPublicKey], now: i64) -> Option<Cleartext> {
    let (message, _) = CleartextSignedMessage::from_string(armored).ok()?;
    let text = message.text().to_string();
    let signatures = message.signatures();
    let mut matched = None;
    for (index, key) in keys.iter().enumerate() {
        for cand in candidates(key) {
            if !signatures.iter().any(|s| issuer_matches(s, &cand)) {
                continue;
            }
            matched = Some(index);
            let outcome = match &cand {
                Candidate::Primary(k) => message.verify(*k),
                Candidate::Sub(k) => message.verify(*k),
            };
            if let Ok(sig) = outcome {
                return Some(Cleartext { text, verdict: Verdict::Valid(signer_of(sig, index, key, now)) });
            }
        }
    }
    let verdict = match matched {
        Some(key_index) => Verdict::Invalid { key_index },
        None => Verdict::NoKey { issuer: signatures.first().and_then(issuer_text) },
    };
    Some(Cleartext { text, verdict })
}

// ---- decrypting -------------------------------------------------------------------------------

/// A secret key with the passphrase to unlock it (empty when it has none).
pub struct Unlockable<'a> {
    pub key: &'a SignedSecretKey,
    pub passphrase: &'a str,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DecryptError {
    /// The message is not for any of the keys.
    NoKey,
    /// It is for a key given, but the passphrase did not unlock it.
    WrongPassphrase,
    /// The message has no integrity protection (a packet an attacker could change undetected), or
    /// the protection did not check out. Nothing of it is shown.
    NoIntegrity,
    /// A kind of encryption this program does not open (vendor extensions).
    Unsupported,
    Unreadable,
}

/// The plaintext of a decrypted message, wiped when dropped, and who signed it inside.
pub struct Decrypted {
    pub plaintext: Zeroizing<Vec<u8>>,
    /// The signature inside the encryption, checked against the keys given.
    pub inner: Option<Verdict>,
}

/// Decrypts an OpenPGP message (armored or binary). Messages that are not protected against
/// change (the old "symmetrically encrypted data" packet) are refused, not decrypted.
pub fn decrypt(
    data: &[u8],
    secrets: &[Unlockable<'_>],
    verify_with: &[SignedPublicKey],
    now: i64,
) -> Result<Decrypted, DecryptError> {
    let message = if is_armored(data) {
        Message::from_armor(Cursor::new(data.to_vec())).map(|(m, _)| m)
    } else {
        Message::from_bytes(Cursor::new(data.to_vec()))
    }
    .map_err(|_| DecryptError::Unreadable)?;
    if !message.is_encrypted() {
        return Err(DecryptError::Unreadable);
    }
    // Encrypted data that carries no integrity check is never opened: an attacker could change it.
    if let Message::Encrypted { edata, .. } = &message {
        match edata {
            pgp::composed::Edata::SymEncryptedProtectedData { .. } => {}
            pgp::composed::Edata::SymEncryptedData { .. } => return Err(DecryptError::NoIntegrity),
            _ => return Err(DecryptError::Unsupported),
        }
    }
    let passwords: Vec<Password> = secrets.iter().map(|s| Password::from(s.passphrase)).collect();
    let keys: Vec<&SignedSecretKey> = secrets.iter().map(|s| s.key).collect();
    let passwords_ref: Vec<&Password> = passwords.iter().collect();
    let decrypted = message.decrypt_with_keys(passwords_ref, keys).map_err(classify_decrypt_error)?;
    let mut decrypted = decrypted.decompress().map_err(|_| DecryptError::Unreadable)?;
    let is_signed = decrypted.is_signed();
    let mut plaintext = Zeroizing::new(Vec::new());
    // The integrity check of the old format is made as the data is read: a failure shows here.
    decrypted.read_to_end(&mut plaintext).map_err(|_| DecryptError::NoIntegrity)?;
    let inner = if is_signed { Some(verify_inner(&decrypted, verify_with, now)) } else { None };
    Ok(Decrypted { plaintext, inner })
}

fn verify_inner(message: &Message<'_>, keys: &[SignedPublicKey], now: i64) -> Verdict {
    let mut cands: Vec<(usize, &SignedPublicKey, Candidate<'_>)> = Vec::new();
    for (i, key) in keys.iter().enumerate() {
        for cand in candidates(key) {
            cands.push((i, key, cand));
        }
    }
    let refs: Vec<&dyn VerifyingKey> = cands
        .iter()
        .map(|(_, _, c)| match c {
            Candidate::Primary(k) => *k as &dyn VerifyingKey,
            Candidate::Sub(k) => *k as &dyn VerifyingKey,
        })
        .collect();
    let Ok(results) = message.verify_nested(&refs) else { return Verdict::Unreadable };
    for ((index, key, _), result) in cands.iter().zip(results) {
        if let pgp::composed::VerificationResult::Valid(sig) = result {
            return Verdict::Valid(signer_of(&sig, *index, key, now));
        }
    }
    Verdict::NoKey { issuer: None }
}

fn classify_decrypt_error(e: pgp::errors::Error) -> DecryptError {
    let text = e.to_string().to_ascii_lowercase();
    if text.contains("missing key") || text.contains("no matching") {
        DecryptError::NoKey
    } else if text.contains("unsupported") && text.contains("sym") || text.contains("legacy") || text.contains("mdc") {
        DecryptError::NoIntegrity
    } else if text.contains("password")
        || text.contains("passphrase")
        || text.contains("unlock")
        || text.contains("s2k")
    {
        DecryptError::WrongPassphrase
    } else {
        DecryptError::Unreadable
    }
}

/// Whether the key is protected by a passphrase.
pub fn is_locked(key: &SignedSecretKey) -> bool {
    matches!(key.primary_key.secret_params(), pgp::types::SecretParams::Encrypted(_))
}

/// Whether `passphrase` opens the key (a key without a passphrase opens with any).
pub fn passphrase_opens(key: &SignedSecretKey, passphrase: &str) -> bool {
    let password = Password::from(passphrase);
    key.primary_key.unlock(&password, |_, _| Ok(())).is_ok_and(|inner| inner.is_ok())
}

/// The ids of the keys a message is encrypted for (key ids or fingerprints, raw bytes). An empty
/// list means the message hides its recipients (wildcard) and every key has to be tried.
pub fn recipient_ids(data: &[u8]) -> Option<Vec<Vec<u8>>> {
    let message = if is_armored(data) {
        Message::from_armor(Cursor::new(data.to_vec())).map(|(m, _)| m)
    } else {
        Message::from_bytes(Cursor::new(data.to_vec()))
    }
    .ok()?;
    let Message::Encrypted { esk, .. } = &message else { return None };
    let mut ids = Vec::new();
    for e in esk {
        if let pgp::composed::Esk::PublicKeyEncryptedSessionKey(p) = e {
            if let Ok(id) = p.id() {
                if !id.is_wildcard() {
                    ids.push(id.as_ref().to_vec());
                }
            } else if let Ok(Some(fp)) = p.fingerprint() {
                ids.push(fp.as_bytes().to_vec());
            }
        }
    }
    Some(ids)
}

/// Whether the secret key is one of the recipients: its primary key or a subkey has one of the ids.
pub fn is_recipient(key: &SignedSecretKey, ids: &[Vec<u8>]) -> bool {
    if ids.is_empty() {
        return true;
    }
    let mut mine: Vec<Vec<u8>> =
        vec![key.primary_key.legacy_key_id().as_ref().to_vec(), key.primary_key.fingerprint().as_bytes().to_vec()];
    for sub in &key.secret_subkeys {
        mine.push(sub.key.legacy_key_id().as_ref().to_vec());
        mine.push(sub.key.fingerprint().as_bytes().to_vec());
    }
    for sub in &key.public_subkeys {
        mine.push(sub.key.legacy_key_id().as_ref().to_vec());
        mine.push(sub.key.fingerprint().as_bytes().to_vec());
    }
    ids.iter().any(|id| mine.contains(id))
}

/// The public part of a secret key, for the keyring.
pub fn public_of(key: &SignedSecretKey) -> SignedPublicKey {
    SignedPublicKey::from(key.clone())
}

pub fn secret_bytes(key: &SignedSecretKey) -> Result<Zeroizing<Vec<u8>>, String> {
    use pgp::ser::Serialize;
    key.to_bytes().map(Zeroizing::new).map_err(|_| "the key could not be written".to_string())
}

pub fn fingerprint_of_secret(key: &SignedSecretKey) -> String {
    hex_upper(key.primary_key.fingerprint().as_bytes())
}
