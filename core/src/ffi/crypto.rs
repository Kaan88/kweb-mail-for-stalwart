//! The encryption feature (M9) as the host sees it: a switch, a keyring and the status of one message.
//!
//! The rules kept here:
//! - Off by default, and off costs nothing: no function but the switch and the keyring does any
//!   work while it is off, and sync never looks at a message for it.
//! - A message is only fetched whole after its structure said it is signed or encrypted.
//! - Nothing of a signed or encrypted message (its text, the signer's mail, a passphrase) goes
//!   into an error, a log or the database. The keyring holds public material only.

use std::time::{SystemTime, UNIX_EPOCH};

use super::Core;
use crate::{
    crypto::{
        analyze::{self, KeyRow},
        openpgp, smime,
        types::{CryptoKind, CryptoView, KeyEntry, KeyKind, KeyOffer, KeyOrigin},
    },
    jmap::MAX_RAW_BYTES,
    store::StoredKey,
    Error, Result,
};

/// The key in the account's key/value table that says the feature is on.
const ENABLED_KEY: &str = "crypto_enabled";
/// The most a key file or pasted key may be.
const MAX_KEY_BYTES: usize = 1 << 20;

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn entry_of_pgp(key: &pgp::composed::SignedPublicKey, origin: KeyOrigin, detail: &str) -> Result<StoredKey> {
    let info = openpgp::info(key);
    let material = openpgp::public_bytes(key).map_err(Error::invalid)?;
    Ok(StoredKey {
        entry: KeyEntry {
            id: format!("pgp:{}", info.fingerprint),
            kind: KeyKind::Pgp,
            label: info.user_ids.first().cloned().unwrap_or_else(|| info.fingerprint.clone()),
            emails: info.emails,
            fingerprint: info.fingerprint,
            origin,
            origin_detail: detail.to_string(),
            added_at: now(),
            has_secret: false,
            needs_passphrase: false,
            unlocked: false,
            expires_at: info.expires,
        },
        material,
    })
}

fn entry_of_cert(der: &[u8], origin: KeyOrigin, detail: &str) -> Result<StoredKey> {
    let info = smime::cert_info(der).map_err(Error::invalid)?;
    Ok(StoredKey {
        entry: KeyEntry {
            id: format!("x509:{}", info.fingerprint),
            kind: KeyKind::Smime,
            label: info.subject,
            emails: info.emails,
            fingerprint: info.fingerprint,
            origin,
            origin_detail: detail.to_string(),
            added_at: now(),
            has_secret: false,
            needs_passphrase: false,
            unlocked: false,
            expires_at: Some(info.not_after),
        },
        material: der.to_vec(),
    })
}

/// Keeps `key`, but never lets a public import weaken a key that has a private part: the entry
/// that exists keeps its origin, its date and its private part, and only gets the new public material.
fn merge(existing: Option<StoredKey>, mut new: StoredKey) -> StoredKey {
    if let Some(old) = existing {
        new.entry.origin = old.entry.origin;
        new.entry.origin_detail = old.entry.origin_detail;
        new.entry.added_at = old.entry.added_at;
        new.entry.has_secret = old.entry.has_secret;
        new.entry.needs_passphrase = old.entry.needs_passphrase;
    }
    new
}

#[uniffi::export]
impl Core {
    /// Whether reading signed and encrypted mail is switched on for this account (default: off).
    pub fn crypto_enabled(&self) -> Result<bool> {
        Ok(self.store.kv_get(&self.account_id()?, ENABLED_KEY)?.as_deref() == Some("1"))
    }

    /// Switches the feature on or off. Off leaves the keys where they are; [`Core::crypto_forget_all`]
    /// removes them.
    pub fn set_crypto_enabled(&self, on: bool) -> Result<()> {
        let account_id = self.account_id()?;
        if on {
            self.store.kv_set(&account_id, ENABLED_KEY, "1")?;
        } else {
            self.store.kv_delete(&account_id, ENABLED_KEY)?;
        }
        self.crypto.forget();
        Ok(())
    }

    /// Every key and certificate of the keyring: the user's own first.
    pub fn crypto_keys(&self) -> Result<Vec<KeyEntry>> {
        let account_id = self.account_id()?;
        Ok(self.store.crypto_keys(&account_id)?.into_iter().map(|k| k.entry).collect())
    }

    /// Adds the public keys or certificates in `data`: an armored or binary OpenPGP key, or one
    /// or more PEM or DER certificates. Nothing is trusted by being added except that signatures
    /// made by it are checked against it. Returns what was added.
    pub fn crypto_import(&self, data: Vec<u8>, origin: KeyOrigin) -> Result<Vec<KeyEntry>> {
        let account_id = self.account_id()?;
        if data.len() > MAX_KEY_BYTES {
            return Err(Error::invalid("That file is too large to be a key."));
        }
        let mut found: Vec<StoredKey> = Vec::new();
        if let Ok(keys) = openpgp::parse_public(&data) {
            for key in &keys {
                found.push(entry_of_pgp(key, origin, "")?);
            }
        } else {
            for der in smime::parse_certificates(&data) {
                found.push(entry_of_cert(&der, origin, "")?);
            }
        }
        if found.is_empty() {
            return Err(Error::invalid("That is not an OpenPGP key or an S/MIME certificate."));
        }
        let mut added = Vec::new();
        for key in found {
            let key = merge(self.store.crypto_key(&account_id, &key.entry.id)?, key);
            self.store.put_crypto_key(&account_id, &key)?;
            added.push(key.entry);
        }
        Ok(added)
    }

    /// Removes a key or certificate, and what was remembered about its fingerprint.
    pub fn crypto_remove_key(&self, id: String) -> Result<()> {
        let account_id = self.account_id()?;
        self.store.delete_crypto_key(&account_id, &id)?;
        Ok(())
    }

    /// Removes every key and certificate and what was remembered, and switches the feature off.
    pub fn crypto_forget_all(&self) -> Result<()> {
        let account_id = self.account_id()?;
        self.store.clear_crypto(&account_id)?;
        self.set_crypto_enabled(false)
    }

    /// Whether the message is signed or encrypted, judged from its structure alone (network, one
    /// small request, and only while the feature is on). `None` for nearly all mail. The answer is
    /// kept for the session, so asking again costs nothing.
    pub fn crypto_probe(&self, message_id: String) -> Result<CryptoKind> {
        if !self.crypto_enabled()? {
            return Ok(CryptoKind::None);
        }
        if let Some(kind) = self.crypto.kind(&message_id) {
            return Ok(kind);
        }
        let id = &message_id;
        let structure = self.with_connection(|conn| async move { conn.message_structure(id).await })?;
        let mut kind = structure.as_ref().map_or(CryptoKind::None, |s| analyze::kind_from_types(&s.tree));
        if kind == CryptoKind::None && structure.is_some() {
            // Armored text in the body: look at the text the app has anyway.
            let body = self.message_body(message_id.clone())?;
            if body.text.as_deref().is_some_and(|t| t.contains("-----BEGIN PGP")) {
                kind = analyze::inline_kind(body.text.as_deref().unwrap_or_default()).unwrap_or(CryptoKind::None);
            }
        }
        self.crypto.set_kind(&message_id, kind);
        Ok(kind)
    }

    /// Fetches the message whole, checks its signature and says what it covers (network). The
    /// text of the answer is never stored.
    pub fn crypto_open(&self, message_id: String) -> Result<CryptoView> {
        Ok(self.analyse(&message_id)?.view)
    }

    /// Adds a key found in a message (an `offer` of [`Core::crypto_open`]) to the keyring.
    pub fn crypto_import_offer(&self, message_id: String, offer_id: String) -> Result<KeyEntry> {
        let account_id = self.account_id()?;
        let outcome = self.analyse(&message_id)?;
        let (offer, material): &(KeyOffer, Vec<u8>) = outcome
            .offered
            .iter()
            .find(|(o, _)| o.id == offer_id)
            .ok_or_else(|| Error::invalid("That key is no longer in the message."))?;
        let from = outcome.view.from.clone();
        let stored = match offer.kind {
            KeyKind::Pgp => {
                let key = openpgp::parse_public(material).map_err(Error::invalid)?.remove(0);
                entry_of_pgp(&key, KeyOrigin::Mail, &from)?
            }
            KeyKind::Smime => entry_of_cert(material, KeyOrigin::Mail, &from)?,
        };
        let stored = merge(self.store.crypto_key(&account_id, &stored.entry.id)?, stored);
        self.store.put_crypto_key(&account_id, &stored)?;
        Ok(stored.entry)
    }
}

impl Core {
    /// Fetches and analyses a message. Records the first fingerprint seen for the sender.
    pub(super) fn analyse(&self, message_id: &str) -> Result<analyze::Outcome> {
        let account_id = self.account_id()?;
        if !self.crypto_enabled()? {
            return Err(Error::invalid("Encryption is switched off."));
        }
        let id = message_id;
        let raw = self.with_connection(|conn| async move {
            let structure =
                conn.message_structure(id).await?.ok_or_else(|| Error::invalid("the server has no such message"))?;
            if structure.size > MAX_RAW_BYTES {
                return Err(Error::invalid("the message is too large to check"));
            }
            conn.raw_message(&structure.blob_id, MAX_RAW_BYTES).await
        });
        let raw = match raw {
            Ok(raw) => raw,
            Err(Error::Invalid { msg }) if msg.contains("too large") => {
                let mut view = CryptoView::plain(CryptoKind::None);
                view.signature = crate::crypto::types::SignatureState::Unreadable;
                view.detail = Some(msg);
                return Ok(analyze::Outcome { view, record_first_seen: None, held: Vec::new(), offered: Vec::new() });
            }
            Err(e) => return Err(e),
        };
        let keys = self.key_rows(&account_id)?;
        let store = &self.store;
        let seen = |address: &str| store.crypto_first_seen(&account_id, address).ok().flatten();
        let outcome = analyze::analyze(&raw, &analyze::Context { now: now(), keys: &keys, first_seen: &seen });
        if let Some((address, fingerprint)) = &outcome.record_first_seen {
            self.store.put_crypto_first_seen(&account_id, address, fingerprint, now())?;
        }
        self.crypto.set_kind(message_id, outcome.view.kind);
        Ok(outcome)
    }

    fn key_rows(&self, account_id: &str) -> Result<Vec<KeyRow>> {
        Ok(self
            .store
            .crypto_keys(account_id)?
            .into_iter()
            .map(|k| KeyRow {
                id: k.entry.id,
                kind: k.entry.kind,
                label: k.entry.label,
                emails: k.entry.emails,
                fingerprint: k.entry.fingerprint,
                material: k.material,
            })
            .collect())
    }
}
