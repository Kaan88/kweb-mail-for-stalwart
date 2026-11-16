//! From the raw message to what the screen shows: find the signed or encrypted entity, check
//! the signature, and say honestly what that check covers.
//!
//! Pure: bytes and the keyring in, a [`CryptoView`] out. The caller does the network and the
//! database, so every rule here can be tested without either.

use mail_parser::{MessageParser, PartKind};

pub use super::mime::TypeTree;
use super::{
    mime::{self, ArmorKind, Found, Node, Protocol},
    openpgp::{self, KeyInfo, Verdict as PgpVerdict},
    smime::{self, Verdict as SmimeVerdict},
    types::*,
};

/// A public key or certificate of the keyring, as the database holds it.
#[derive(Debug, Clone)]
pub struct KeyRow {
    pub id: String,
    pub kind: KeyKind,
    pub label: String,
    pub emails: Vec<String>,
    pub fingerprint: String,
    /// The OpenPGP transferable public key (binary) or the DER certificate.
    pub material: Vec<u8>,
}

/// What the analysis needs besides the message.
pub struct Context<'a> {
    pub now: i64,
    pub keys: &'a [KeyRow],
    /// The fingerprint first seen for a From address (lower case), if any.
    pub first_seen: &'a dyn Fn(&str) -> Option<String>,
    /// The user's private keys, loaded for this call only.
    pub secrets: &'a [SecretKey],
    /// A passphrase the user just typed.
    pub passphrase: Option<&'a str>,
}

/// A file inside a decrypted or opaque message, held in memory for the host to save on request.
pub struct HeldPart {
    pub name: String,
    pub mime_type: String,
    pub bytes: zeroize::Zeroizing<Vec<u8>>,
}

pub struct Outcome {
    pub view: CryptoView,
    /// Remember this fingerprint as the first one seen for the address (trust on first use).
    pub record_first_seen: Option<(String, String)>,
    pub held: Vec<HeldPart>,
    /// The key a typed passphrase opened, so the host can keep it unlocked if the user chose to.
    pub unlocked_with: Option<String>,
    /// The keys found in the message, with their bytes, for importing: `(offer, material)`.
    pub offered: Vec<(KeyOffer, Vec<u8>)>,
}

/// The first address of the From header, lower case.
pub fn from_address(raw: &[u8]) -> String {
    MessageParser::default()
        .parse_headers(raw)
        .and_then(|m| m.from().and_then(|list| list.first().and_then(|a| a.address().map(str::to_ascii_lowercase))))
        .unwrap_or_default()
}

fn sent_at(raw: &[u8]) -> Option<i64> {
    MessageParser::default().parse_headers(raw).and_then(|m| m.date()).map(|d| d.to_timestamp())
}

/// Judges the kind of mail from the media types of its structure alone: the top type and, for
/// a multipart, the types below it (`(media type, children)`). `None` for plain mail.
pub fn kind_from_types(root: &TypeTree) -> CryptoKind {
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        match node.media.as_str() {
            "multipart/signed" => {
                let second = node.children.get(1).map(|c| c.media.as_str()).unwrap_or_default();
                if second == "application/pgp-signature" {
                    return CryptoKind::PgpSigned;
                }
                if mime::is_pkcs7_signature(second) {
                    return CryptoKind::SmimeSigned;
                }
            }
            "multipart/encrypted" => {
                if node.children.iter().any(|c| c.media == "application/pgp-encrypted") {
                    return CryptoKind::PgpEncrypted;
                }
            }
            m if mime::is_pkcs7_mime(m) => return CryptoKind::SmimeOpaque,
            _ => {}
        }
        // Reading order: the first child is looked at first.
        stack.extend(node.children.iter().rev());
    }
    CryptoKind::None
}

/// Inline OpenPGP in a plain text body.
pub fn inline_kind(text: &str) -> Option<CryptoKind> {
    mime::find_armor(text).filter(|b| b.kind != ArmorKind::PublicKey).map(|_| CryptoKind::PgpInline)
}

struct Parsed {
    key: pgp::composed::SignedPublicKey,
    info: KeyInfo,
}

fn parse_keyring(keys: &[KeyRow]) -> (Vec<Parsed>, Vec<Vec<u8>>) {
    let (mut pgp_keys, mut certs) = (Vec::new(), Vec::new());
    for entry in keys {
        match entry.kind {
            KeyKind::Pgp => {
                if let Ok(mut parsed) = openpgp::parse_public(&entry.material) {
                    if let Some(key) = parsed.pop() {
                        let info = openpgp::info(&key);
                        pgp_keys.push(Parsed { key, info });
                    }
                }
            }
            KeyKind::Smime => {
                certs.push(entry.material.clone());
            }
        }
    }
    (pgp_keys, certs)
}

/// Analyses the raw message. `raw` is the message as it was received.
pub fn analyze(raw: &[u8], ctx: &Context<'_>) -> Outcome {
    let from = from_address(raw);
    let mut out = Outcome {
        view: CryptoView::plain(CryptoKind::None),
        record_first_seen: None,
        held: Vec::new(),
        unlocked_with: None,
        offered: Vec::new(),
    };
    out.view.from = from.clone();
    let Some(root) = mime::parse(raw) else {
        out.view.detail = Some("the message could not be read".into());
        return out;
    };
    offers_in(raw, &root, ctx, &mut out);
    match mime::find(&root) {
        Some((node, Found::Signed { protocol }, whole)) => {
            out.view.covers_whole_message = whole;
            if !whole {
                out.view.notes.push(CryptoNote::OnlyPartCovered);
            }
            let signed = mime::canonical_crlf(&raw[node.children[0].whole.clone()]).into_owned();
            let signature = node.children[1].decoded_body(raw);
            match protocol {
                Protocol::Pgp => {
                    out.view.kind = CryptoKind::PgpSigned;
                    let (parsed, _) = parse_keyring(ctx.keys);
                    let keys: Vec<_> = parsed.iter().map(|p| p.key.clone()).collect();
                    let verdict = openpgp::verify_detached(&signature, &signed, &keys, ctx.now);
                    apply_pgp(&mut out, verdict, &parsed, ctx, sent_at(raw));
                }
                Protocol::Smime => {
                    out.view.kind = CryptoKind::SmimeSigned;
                    let (_, certs) = parse_keyring(ctx.keys);
                    let result = smime::verify(&signature, Some(&signed), &certs, ctx.now);
                    apply_smime(&mut out, result, ctx, sent_at(raw));
                }
            }
        }
        Some((node, Found::Pkcs7 { .. }, whole)) => {
            out.view.kind = CryptoKind::SmimeOpaque;
            out.view.covers_whole_message = whole;
            let der = node.decoded_body(raw);
            match smime::cms_kind(&der) {
                smime::CmsKind::Signed => {
                    let (_, certs) = parse_keyring(ctx.keys);
                    let result = smime::verify(&der, None, &certs, ctx.now);
                    let content = result.content.clone();
                    apply_smime(&mut out, result, ctx, sent_at(raw));
                    if let Some(inner) = content {
                        set_content(&mut out, &inner, true);
                    }
                }
                smime::CmsKind::Enveloped => open_smime(&mut out, &der, ctx),
                smime::CmsKind::AuthEnveloped => {
                    out.view.encryption = EncryptionState::Unsupported;
                    out.view.notes.push(CryptoNote::NotSearchable);
                    out.view.detail =
                        Some("authenticated enveloped data (AES-GCM), which this app does not open yet".into());
                }
                smime::CmsKind::Other => {
                    out.view.signature = SignatureState::Unreadable;
                    out.view.detail = Some("the S/MIME data could not be read".into());
                }
            }
        }
        Some((node, Found::PgpEncrypted, whole)) => {
            out.view.covers_whole_message = whole;
            let data = node.children[1].decoded_body(raw);
            open_pgp(&mut out, &data, ctx, CryptoKind::PgpEncrypted, true);
        }
        None => inline_pgp(raw, &root, ctx, &mut out, &from),
    }
    sync_offers(&mut out);
    out
}

/// A private key the user imported, as the core hands it to the analysis for one call. The
/// material is wiped when it is dropped.
pub struct SecretKey {
    pub id: String,
    pub kind: KeyKind,
    pub label: String,
    pub fingerprint: String,
    /// OpenPGP: the binary secret key (still locked if it was). S/MIME: the PKCS#8 key.
    pub material: zeroize::Zeroizing<Vec<u8>>,
    /// S/MIME: the certificate (DER) the key belongs to.
    pub cert: Vec<u8>,
    /// The passphrase the user chose to keep on this phone for the key.
    pub stored_passphrase: Option<zeroize::Zeroizing<String>>,
}

/// Opens an OpenPGP message with the user's keys. `mime`: the plaintext is a MIME entity (PGP/MIME);
/// otherwise it is text (inline). The decrypted text is shown on its own; it is never merged with the
/// outer message, and nothing is opened unless the packet carries an integrity check.
fn open_pgp(out: &mut Outcome, data: &[u8], ctx: &Context<'_>, kind: CryptoKind, mime: bool) {
    out.view.kind = kind;
    out.view.notes.push(CryptoNote::NotSearchable);
    let mut secrets: Vec<(&SecretKey, pgp::composed::SignedSecretKey)> = Vec::new();
    for s in ctx.secrets.iter().filter(|s| s.kind == KeyKind::Pgp) {
        if let Some(key) = openpgp::parse_secret(&s.material).ok().and_then(|mut k| k.pop()) {
            secrets.push((s, key));
        }
    }
    let Some(ids) = openpgp::recipient_ids(data) else {
        out.view.encryption = EncryptionState::Failed;
        return;
    };
    let candidates: Vec<_> = secrets.iter().filter(|(_, k)| openpgp::is_recipient(k, &ids)).collect();
    if candidates.is_empty() {
        out.view.encryption = EncryptionState::NeedKey;
        return;
    }
    // Which passphrase goes to which key: the one just typed, else the one kept for the key.
    let mut unlockable: Vec<(&pgp::composed::SignedSecretKey, String)> = Vec::new();
    let (mut asked_for, mut wrong, mut typed_for) = (None, None, None);
    for (s, key) in &candidates {
        if !openpgp::is_locked(key) {
            unlockable.push((key, String::new()));
            continue;
        }
        let typed = ctx.passphrase.map(str::to_string);
        let kept = s.stored_passphrase.as_ref().map(|p| p.to_string());
        match typed.into_iter().chain(kept).find(|p| openpgp::passphrase_opens(key, p)) {
            Some(p) => {
                if ctx.passphrase == Some(p.as_str()) {
                    typed_for = Some(s.id.clone());
                }
                unlockable.push((key, p));
            }
            None if ctx.passphrase.is_some() => wrong = wrong.or(Some(s.label.clone())),
            None => asked_for = asked_for.or(Some(s.label.clone())),
        }
    }
    if unlockable.is_empty() {
        match (wrong, asked_for) {
            (Some(label), _) => {
                out.view.encryption = EncryptionState::WrongPassphrase;
                out.view.passphrase_for = Some(label);
            }
            (None, label) => {
                out.view.encryption = EncryptionState::NeedPassphrase;
                out.view.passphrase_for = label;
            }
        }
        return;
    }
    let (parsed, _) = parse_keyring(ctx.keys);
    let public: Vec<_> = parsed.iter().map(|p| p.key.clone()).collect();
    let unlock: Vec<openpgp::Unlockable<'_>> =
        unlockable.iter().map(|(key, pass)| openpgp::Unlockable { key, passphrase: pass }).collect();
    match openpgp::decrypt(data, &unlock, &public, ctx.now) {
        Ok(done) => {
            out.view.encryption = EncryptionState::Decrypted;
            out.unlocked_with = typed_for;
            let from = out.view.from.clone();
            if let Some(verdict) = done.inner {
                out.view.notes.push(CryptoNote::SignedInsideEncryption);
                apply_pgp(out, verdict, &parsed, ctx, None);
            }
            if mime {
                inner_entity(out, &done.plaintext, ctx, &from, false);
            } else {
                let text = String::from_utf8_lossy(&done.plaintext).into_owned();
                out.view.content = Some(CryptoContent { html: None, text: Some(text), attachments: Vec::new() });
            }
        }
        Err(openpgp::DecryptError::NoKey) => out.view.encryption = EncryptionState::NeedKey,
        Err(openpgp::DecryptError::WrongPassphrase) => out.view.encryption = EncryptionState::WrongPassphrase,
        Err(openpgp::DecryptError::NoIntegrity) => out.view.encryption = EncryptionState::NoIntegrity,
        Err(openpgp::DecryptError::Unsupported) => {
            out.view.encryption = EncryptionState::Unsupported;
            out.view.detail = Some("an encryption packet this app does not open".into());
        }
        Err(openpgp::DecryptError::Unreadable) => out.view.encryption = EncryptionState::Failed,
    }
}

/// Opens S/MIME enveloped data. Its encryption (CBC) cannot tell a changed message from the original, so the
/// result is shown alone with remote content off, whatever the user's setting is.
fn open_smime(out: &mut Outcome, der: &[u8], ctx: &Context<'_>) {
    out.view.kind = CryptoKind::SmimeOpaque;
    out.view.notes.push(CryptoNote::NotSearchable);
    let keys: Vec<smime::Private<'_>> = ctx
        .secrets
        .iter()
        .filter(|s| s.kind == KeyKind::Smime)
        .map(|s| smime::Private { cert: &s.cert, pkcs8: &s.material })
        .collect();
    match smime::decrypt(der, &keys) {
        Ok(plain) => {
            out.view.encryption = EncryptionState::Decrypted;
            let from = out.view.from.clone();
            inner_entity(out, &plain, ctx, &from, true);
        }
        Err(smime::DecryptFail::NoKey) => out.view.encryption = EncryptionState::NeedKey,
        Err(smime::DecryptFail::Unsupported(why)) => {
            out.view.encryption = EncryptionState::Unsupported;
            out.view.detail = Some(why);
        }
        Err(smime::DecryptFail::Failed) => out.view.encryption = EncryptionState::Failed,
    }
}

/// The decrypted MIME entity: look for a signature inside it (a signed message that was encrypted), and show its
/// content alone. Only what was signed is shown for a signed entity.
fn inner_entity(out: &mut Outcome, plain: &[u8], ctx: &Context<'_>, from: &str, block_remote: bool) {
    // The entity has no From of its own: the outer message's is lent to it, so the signer is compared with it.
    let mut lent = format!("From: {from}\r\n").into_bytes();
    lent.extend_from_slice(plain);
    let inner_ctx = Context { secrets: &[], passphrase: None, ..*ctx };
    let inner = analyze(&lent, &inner_ctx);
    let signed_inside = inner.view.signature != SignatureState::NotSigned;
    let payload: Vec<u8> = match mime::parse(plain) {
        // The signed part of a signed entity, not the signature around it.
        Some(root) if signed_inside && inner.view.kind != CryptoKind::SmimeOpaque && root.children.len() == 2 => {
            plain[root.children[0].whole.clone()].to_vec()
        }
        _ => plain.to_vec(),
    };
    if signed_inside {
        out.view.signature = inner.view.signature;
        out.view.signer = inner.view.signer;
        out.record_first_seen = out.record_first_seen.take().or(inner.record_first_seen);
        out.view.notes.push(CryptoNote::SignedInsideEncryption);
        for n in inner.view.notes {
            if !out.view.notes.contains(&n) {
                out.view.notes.push(n);
            }
        }
        out.offered.extend(inner.offered);
    }
    let payload = match (&inner.view.content, inner.view.kind) {
        // Opaque signed data inside: its content came out of the signed data.
        (Some(_), CryptoKind::SmimeOpaque) => None,
        _ => Some(payload),
    };
    match payload {
        Some(bytes) => set_content(out, &bytes, block_remote),
        None => {
            out.view.content = inner.view.content;
            out.held = inner.held;
            if block_remote {
                out.view.remote_content_blocked = true;
                out.view.notes.push(CryptoNote::RemoteContentBlocked);
            }
        }
    }
    sync_offers(out);
}

fn inline_pgp(raw: &[u8], _root: &Node, ctx: &Context<'_>, out: &mut Outcome, _from: &str) {
    let Some(message) = MessageParser::default().parse(raw) else { return };
    let Some(text) = message.body_text(0) else { return };
    let Some(block) = mime::find_armor(&text) else { return };
    match block.kind {
        ArmorKind::Signed => {
            out.view.kind = CryptoKind::PgpInline;
            let (parsed, _) = parse_keyring(ctx.keys);
            let keys: Vec<_> = parsed.iter().map(|p| p.key.clone()).collect();
            let Some(clear) = openpgp::verify_cleartext(&text[block.range.clone()], &keys, ctx.now) else {
                out.view.signature = SignatureState::Unreadable;
                return;
            };
            apply_pgp(out, clear.verdict, &parsed, ctx, sent_at(raw));
            let outside = text[..block.range.start].trim().len() + text[block.range.end..].trim().len();
            if outside > 0 {
                out.view.notes.push(CryptoNote::UnsignedTextHidden);
                out.view.covers_whole_message = false;
            }
            out.view.content = Some(CryptoContent { html: None, text: Some(clear.text), attachments: Vec::new() });
        }
        ArmorKind::Message => open_pgp(out, text[block.range.clone()].as_bytes(), ctx, CryptoKind::PgpInline, false),
        ArmorKind::PublicKey => {}
    }
}

/// The text between two keys' notions: does the address list name the From address.
fn names(emails: &[String], from: &str) -> bool {
    !from.is_empty() && emails.iter().any(|e| e.eq_ignore_ascii_case(from))
}

fn apply_pgp(out: &mut Outcome, verdict: PgpVerdict, parsed: &[Parsed], ctx: &Context<'_>, date: Option<i64>) {
    let from = out.view.from.clone();
    match verdict {
        PgpVerdict::Valid(signer) => {
            let p = &parsed[signer.key_index];
            let at = signer.made_at.or(date).unwrap_or(ctx.now);
            out.view.signer = Some(Signer {
                label: p.info.user_ids.first().cloned().unwrap_or_else(|| p.info.fingerprint.clone()),
                emails: p.info.emails.clone(),
                fingerprint: p.info.fingerprint.clone(),
                kind: KeyKind::Pgp,
                in_keyring: true,
                signed_at: signer.made_at,
            });
            out.view.signature = if signer.signature_expired {
                out.view.notes.push(CryptoNote::SignatureExpired);
                SignatureState::Invalid
            } else if p.info.revoked {
                SignatureState::KeyRevoked
            } else if p.info.expires.is_some_and(|e| at > e) {
                SignatureState::KeyExpired
            } else if !names(&p.info.emails, &from) {
                SignatureState::OtherAddress
            } else {
                first_seen(out, ctx, &from, &p.info.fingerprint, SignatureState::Verified)
            };
        }
        PgpVerdict::Invalid { key_index } => {
            let p = &parsed[key_index];
            out.view.signature = SignatureState::Invalid;
            out.view.signer = Some(Signer {
                label: p.info.user_ids.first().cloned().unwrap_or_default(),
                emails: p.info.emails.clone(),
                fingerprint: p.info.fingerprint.clone(),
                kind: KeyKind::Pgp,
                in_keyring: true,
                signed_at: None,
            });
        }
        PgpVerdict::NoKey { issuer } => {
            out.view.signature = SignatureState::NoKey;
            out.view.detail = issuer;
        }
        PgpVerdict::Unreadable => out.view.signature = SignatureState::Unreadable,
    }
}

/// Applies trust on first use: the first key seen for an address is remembered, a different one
/// afterwards is a warning.
fn first_seen(
    out: &mut Outcome,
    ctx: &Context<'_>,
    from: &str,
    fingerprint: &str,
    good: SignatureState,
) -> SignatureState {
    match (ctx.first_seen)(from) {
        None => {
            out.record_first_seen = Some((from.to_string(), fingerprint.to_string()));
            good
        }
        Some(first) if first.eq_ignore_ascii_case(fingerprint) => good,
        Some(_) => SignatureState::KeyChanged,
    }
}

fn apply_smime(out: &mut Outcome, result: smime::Signed, ctx: &Context<'_>, date: Option<i64>) {
    let from = out.view.from.clone();
    out.view.notes.push(CryptoNote::SmimeNoRevocationCheck);
    // Remote content never loads in S/MIME mail: its encryption has no integrity protection, and a
    // signed one is shown from the signed bytes alone.
    for cert in &result.embedded {
        add_cert_offer(out, ctx, cert);
    }
    match result.verdict {
        SmimeVerdict::Valid { cert, from_keyring, signed_at, .. } => {
            let at = signed_at.or(date).unwrap_or(ctx.now);
            out.view.signer = Some(Signer {
                label: cert.subject.clone(),
                emails: cert.emails.clone(),
                fingerprint: cert.fingerprint.clone(),
                kind: KeyKind::Smime,
                in_keyring: from_keyring,
                signed_at,
            });
            out.view.signature = if !smime::valid_at(&cert, at) {
                SignatureState::KeyExpired
            } else if !names(&cert.emails, &from) {
                SignatureState::OtherAddress
            } else {
                let good = if from_keyring { SignatureState::Verified } else { SignatureState::CertificateNotChecked };
                first_seen(out, ctx, &from, &cert.fingerprint, good)
            };
        }
        SmimeVerdict::Invalid => out.view.signature = SignatureState::Invalid,
        SmimeVerdict::NoCert => out.view.signature = SignatureState::NoKey,
        SmimeVerdict::Unsupported(why) => {
            out.view.signature = SignatureState::Unsupported;
            out.view.detail = Some(why);
        }
        SmimeVerdict::Unreadable => out.view.signature = SignatureState::Unreadable,
    }
}

fn add_cert_offer(out: &mut Outcome, ctx: &Context<'_>, der: &[u8]) {
    let Ok(info) = smime::cert_info(der) else { return };
    let id = format!("x509:{}", info.fingerprint);
    if out.offered.iter().any(|(o, _)| o.id == id) {
        return;
    }
    let already_known = ctx.keys.iter().any(|k| k.id == id);
    out.offered.push((
        KeyOffer {
            id,
            kind: KeyKind::Smime,
            label: info.subject,
            emails: info.emails,
            fingerprint: info.fingerprint,
            already_known,
        },
        der.to_vec(),
    ));
}

/// Public keys attached to the message (`application/pgp-keys`) are offered, never imported by themselves.
fn offers_in(raw: &[u8], root: &Node, ctx: &Context<'_>, out: &mut Outcome) {
    let mut all = Vec::new();
    root.walk(&mut all);
    for node in all.into_iter().filter(|n| n.ctype.is("application/pgp-keys")) {
        let body = node.decoded_body(raw);
        let Ok(keys) = openpgp::parse_public(&body) else { continue };
        for key in keys {
            let info = openpgp::info(&key);
            let id = format!("pgp:{}", info.fingerprint);
            if out.offered.iter().any(|(o, _)| o.id == id) {
                continue;
            }
            let Ok(material) = openpgp::public_bytes(&key) else { continue };
            let already_known = ctx.keys.iter().any(|k| k.id == id);
            out.offered.push((
                KeyOffer {
                    id,
                    kind: KeyKind::Pgp,
                    label: info.user_ids.first().cloned().unwrap_or_default(),
                    emails: info.emails,
                    fingerprint: info.fingerprint,
                    already_known,
                },
                material,
            ));
        }
    }
    sync_offers(out);
}

fn sync_offers(out: &mut Outcome) {
    out.view.offers = out.offered.iter().map(|(o, _)| o.clone()).collect();
}

/// Makes the content of a view from a MIME entity: the HTML and text it has and its files. The
/// files are kept in memory in `out.held`.
pub fn set_content(out: &mut Outcome, entity: &[u8], block_remote: bool) {
    let (content, held) = content_of(entity);
    out.view.content = Some(content);
    out.held = held;
    if block_remote {
        out.view.remote_content_blocked = true;
        out.view.notes.push(CryptoNote::RemoteContentBlocked);
    }
    sync_offers(out);
}

fn escape_html(text: &str, out: &mut String) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
}

/// The HTML, text and files of a MIME entity.
fn content_of(entity: &[u8]) -> (CryptoContent, Vec<HeldPart>) {
    let Some(message) = MessageParser::default().parse(entity) else {
        return (CryptoContent { html: None, text: None, attachments: Vec::new() }, Vec::new());
    };
    let (mut html, mut text) = (String::new(), String::new());
    let mut have_html = false;
    for part in message.html_body() {
        match part.kind() {
            PartKind::Html => {
                if let Some(h) = part.text() {
                    html.push_str(&h);
                    have_html = true;
                }
            }
            PartKind::Text => {
                if let Some(t) = part.text() {
                    html.push_str("<pre style=\"white-space:pre-wrap;font:inherit\">");
                    escape_html(&t, &mut html);
                    html.push_str("</pre>");
                }
            }
            _ => {}
        }
    }
    let mut have_text = false;
    for part in message.text_body() {
        if matches!(part.kind(), PartKind::Text) {
            if let Some(t) = part.text() {
                if have_text {
                    text.push('\n');
                }
                text.push_str(&t);
                have_text = true;
            }
        }
    }
    let mut attachments = Vec::new();
    let mut held = Vec::new();
    // A file is a leaf the server's rules list as an attachment (whatever its type, a text file included) or a
    // non-text part of a body (a picture).
    let mut seen_binary = |part: mail_parser::MessagePart<'_>, held: &mut Vec<HeldPart>, listed: bool| {
        let is_file = matches!(part.kind(), PartKind::Binary | PartKind::InlineBinary | PartKind::Message(_))
            || (listed && !matches!(part.kind(), PartKind::Multipart));
        if !is_file {
            return;
        }
        let mime_type = part
            .content_type()
            .map(|c| format!("{}/{}", c.ctype(), c.subtype().unwrap_or("octet-stream")).to_ascii_lowercase())
            .unwrap_or_else(|| "application/octet-stream".into());
        let bytes = part.decoded().into_owned();
        let name = part.attachment_name().unwrap_or_default().to_string();
        let content_id = part.content_id().map(|c| c.trim_matches(['<', '>']).to_string()).filter(|c| !c.is_empty());
        let is_inline = content_id.is_some() && mime_type.starts_with("image/");
        attachments.push(CryptoAttachment {
            index: held.len() as u32,
            name: name.clone(),
            mime_type: mime_type.clone(),
            size: bytes.len() as u64,
            content_id,
            is_inline,
        });
        held.push(HeldPart { name, mime_type, bytes: zeroize::Zeroizing::new(bytes) });
    };
    for part in message.html_body().chain(message.text_body()) {
        seen_binary(part, &mut held, false);
    }
    for part in message.attachments() {
        seen_binary(part, &mut held, true);
    }
    // Pictures listed in both bodies are one file.
    let mut seen = std::collections::HashSet::new();
    let mut keep = Vec::new();
    for (a, h) in attachments.into_iter().zip(held) {
        let key = (a.name.clone(), a.content_id.clone(), a.size);
        if seen.insert(key) {
            keep.push((a, h));
        }
    }
    let (attachments, held): (Vec<_>, Vec<_>) = keep
        .into_iter()
        .enumerate()
        .map(|(i, (mut a, h))| {
            a.index = i as u32;
            (a, h)
        })
        .unzip();
    (CryptoContent { html: have_html.then_some(html), text: have_text.then_some(text), attachments }, held)
}
