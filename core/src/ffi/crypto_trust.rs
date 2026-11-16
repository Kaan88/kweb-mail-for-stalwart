//! M9 stage 4: where keys come from beyond a file the user picked: the Autocrypt header of a mail the user
//! opened, and a Web Key Directory lookup the user asked for. Both are trust on first use:
//! - a key is stored with its source and the day it was first seen, and never replaces another one;
//! - the first key seen for an address is remembered, a different one afterwards is shown as a change;
//! - such a key is used to encrypt (with a warning) but a good signature from it is "first seen", never
//!   "verified", until the user marks the key verified themselves.
//!
//! Nothing here runs by itself in the background: Autocrypt is read from the one request that already asks for
//! a message's structure when the user opens it, and a lookup is one explicit call per address.

use super::{
    crypto::{entry_of_pgp, now},
    Core,
};
use crate::{
    crypto::{
        autocrypt, build, openpgp,
        types::{KeyEntry, KeyOrigin},
        wkd,
    },
    jmap::Structure,
    store::StoredKey,
    Error, Result,
};

/// How many keys learned from outside are kept for one address; more are ignored (a sender cannot fill the keyring).
const MAX_LEARNED_PER_ADDRESS: usize = 5;
/// How many keys learned from outside are kept in all.
const MAX_LEARNED: usize = 500;

/// What a lookup found.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct WkdResult {
    /// The keys of the address that were found (new ones and ones the keyring already had).
    pub keys: Vec<KeyEntry>,
    /// The hosts that were asked, in order: the recipient's provider learns of the lookup from them.
    pub asked: Vec<String>,
}

impl Core {
    /// Keeps a key learned from `origin` for `address`, unless the keyring has it already, the user has a private
    /// key for the address (their own mail), or too many were learned. Returns the new entry. The first fingerprint
    /// seen for the address is remembered by this.
    pub(super) fn learn_key(
        &self,
        account_id: &str,
        key: &pgp::composed::SignedPublicKey,
        origin: KeyOrigin,
        detail: &str,
        address: &str,
    ) -> Result<Option<StoredKey>> {
        let address = address.trim().to_ascii_lowercase();
        let mut stored = entry_of_pgp(key, origin, detail)?;
        let keys = self.store.crypto_keys(account_id)?;
        if keys.iter().any(|k| k.entry.id == stored.entry.id) {
            return Ok(None);
        }
        if keys.iter().any(|k| k.entry.has_secret && k.entry.emails.iter().any(|e| e.eq_ignore_ascii_case(&address))) {
            return Ok(None);
        }
        let learned = |k: &&StoredKey| matches!(k.entry.origin, KeyOrigin::Autocrypt | KeyOrigin::Wkd);
        let for_address = keys
            .iter()
            .filter(learned)
            .filter(|k| k.entry.emails.iter().any(|e| e.eq_ignore_ascii_case(&address)))
            .count();
        if for_address >= MAX_LEARNED_PER_ADDRESS || keys.iter().filter(learned).count() >= MAX_LEARNED {
            return Ok(None);
        }
        if !build::pgp_can_encrypt(key, now()) {
            return Ok(None);
        }
        // The address the sender claimed is the one the key is kept for, even if the key itself does not name it.
        if !stored.entry.emails.iter().any(|e| e.eq_ignore_ascii_case(&address)) {
            stored.entry.emails.push(address.clone());
        }
        self.store.put_crypto_key(account_id, &stored)?;
        self.store.put_crypto_first_seen(account_id, &address, &stored.entry.fingerprint, now())?;
        Ok(Some(stored))
    }

    /// Reads what the opened message's headers already gave (see `jmap::Structure`): one `Autocrypt` header whose
    /// address is the one From address. A failure here never disturbs opening the message.
    pub(super) fn learn_autocrypt(&self, account_id: &str, structure: &Structure) {
        let (Some(header), [from]) = (structure.autocrypt.first(), structure.from.as_slice()) else { return };
        if structure.autocrypt.len() != 1 {
            return;
        }
        let Some(header) = autocrypt::parse(header) else { return };
        if !header.addr.eq_ignore_ascii_case(from.trim()) {
            return;
        }
        let Ok(mut keys) = openpgp::parse_public(&header.keydata) else { return };
        // Level 1 allows one key: more than one is not a valid header.
        if keys.len() != 1 {
            return;
        }
        let key = keys.remove(0);
        // A key that cannot be kept (a full database, say) is simply not kept: opening the message goes on.
        let _ = self.learn_key(account_id, &key, KeyOrigin::Autocrypt, &header.addr, &header.addr);
    }
}

#[uniffi::export]
impl Core {
    /// Looks for a recipient's OpenPGP key at their mail provider (Web Key Directory), because the user asked
    /// for it. The provider's web server is asked for a file named after the address, so the provider learns that
    /// someone looked for this address. HTTPS only, no redirects, a small answer. Keys found are kept with their source and
    /// are not trusted for anything but encrypting, with a warning, until the user marks them verified.
    pub fn crypto_wkd_lookup(&self, email: String) -> Result<WkdResult> {
        let account_id = self.account_id()?;
        if !self.crypto_enabled()? {
            return Err(Error::invalid("Encryption is switched off."));
        }
        let target = wkd::Target::parse(&email)
            .ok_or_else(|| Error::invalid("That address cannot be looked up: it is not a plain internet address."))?;
        let at = now();
        let found = self.rt.block_on(wkd::lookup(&target, |url| async move { wkd::fetch(&url, true).await }, at));
        if found.keys.is_empty() {
            return Err(Error::invalid(wkd::nothing_text(&target, &found.asked)));
        }
        let address = target.address();
        let mut keys = Vec::new();
        for key in &found.keys {
            match self.learn_key(&account_id, key, KeyOrigin::Wkd, &found.host, &address)? {
                Some(stored) => keys.push(stored.entry),
                None => {
                    let id = format!("pgp:{}", openpgp::info(key).fingerprint);
                    if let Some(existing) = self.store.crypto_key(&account_id, &id)? {
                        keys.push(existing.entry);
                    }
                }
            }
        }
        if keys.is_empty() {
            return Err(Error::invalid("A key was found, but it cannot be used here."));
        }
        Ok(WkdResult { keys, asked: found.asked })
    }
}
