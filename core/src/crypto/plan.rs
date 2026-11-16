//! M9 stage 3: which protocol and which keys a message is signed and encrypted with.
//!
//! Pure: the keyring, the addresses and the time in, a decision out. The decision is made again
//! when the message is sent, from the keyring as it is then; what the compose screen shows is the
//! same function, so the screen and the send cannot disagree.

use super::{
    build::pgp_can_encrypt,
    openpgp, smime,
    types::{KeyKind, KeyOrigin},
};
use crate::store::StoredKey;

/// What was asked for and what the keys allow.
#[derive(Debug, Clone, PartialEq)]
pub struct Resolved {
    pub protocol: KeyKind,
    /// The id of the own key that signs (when signing was asked for).
    pub signer: Option<String>,
    /// The id of the own key that is always added as a recipient (when encrypting).
    pub own: Option<String>,
    /// For each recipient address (lower case), the id of the key used.
    pub recipients: Vec<(String, String)>,
    /// Things the user should read: a second key for the same address, a key that is only trusted on first use.
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PlanError {
    /// The sender has no private key of the kind needed (none at all, or none for this address).
    NoOwnKey,
    /// These recipients (addresses) have no usable key for the protocol that could otherwise be used.
    Missing { protocol: KeyKind, emails: Vec<String> },
    /// S/MIME names every recipient in the message, so a Bcc recipient would be revealed.
    BccWithSmime,
    /// Neither sign nor encrypt was asked for.
    NothingAsked,
}

impl PlanError {
    pub fn text(&self) -> String {
        match self {
            PlanError::NoOwnKey => {
                "You have no private key for this sending address. Import one in the Encryption settings.".into()
            }
            PlanError::Missing { emails, .. } => {
                format!("No key to encrypt to for: {}. Add their key, or turn Encrypt off.", emails.join(", "))
            }
            PlanError::BccWithSmime => {
                "S/MIME would show who is in Bcc. Remove the Bcc recipients, or use OpenPGP keys.".into()
            }
            PlanError::NothingAsked => "Neither signing nor encrypting was asked for.".into(),
        }
    }
}

fn lower(s: &str) -> String {
    s.trim().to_ascii_lowercase()
}

fn has_email(key: &StoredKey, address: &str) -> bool {
    key.entry.emails.iter().any(|e| lower(e) == address)
}

fn usable_to_encrypt(key: &StoredKey, now: i64) -> bool {
    match key.entry.kind {
        KeyKind::Pgp => openpgp::parse_public(&key.material)
            .ok()
            .and_then(|mut k| (!k.is_empty()).then(|| k.remove(0)))
            .is_some_and(|k| pgp_can_encrypt(&k, now)),
        KeyKind::Smime => smime::cert_info(&key.material).is_ok_and(|info| smime::valid_at(&info, now)),
    }
}

/// The key to encrypt to for `address`: the one first seen for it, else the best of several (verified by the
/// user, then newest). Returns the key and a warning when the choice was not clear.
fn key_for<'a>(
    keys: &'a [StoredKey],
    kind: KeyKind,
    address: &str,
    first_seen: Option<&str>,
    now: i64,
) -> Option<(&'a StoredKey, Option<String>)> {
    let mut found: Vec<&StoredKey> =
        keys.iter().filter(|k| k.entry.kind == kind && has_email(k, address) && usable_to_encrypt(k, now)).collect();
    if found.is_empty() {
        return None;
    }
    found.sort_by_key(|k| (!(k.entry.has_secret), !k.entry.verified, std::cmp::Reverse(k.entry.added_at)));
    let chosen = found
        .iter()
        .find(|k| first_seen.is_some_and(|f| f == k.entry.fingerprint))
        .or_else(|| found.first())
        .copied()?;
    let warning = (found.len() > 1).then(|| {
        format!(
            "There is more than one key for {address}; the one with fingerprint {} is used.",
            chosen.entry.fingerprint
        )
    });
    Some((chosen, warning))
}

/// What a message asks for.
pub struct Ask<'a> {
    pub sender: &'a str,
    /// Every address the message goes to (To, Cc and Bcc).
    pub recipients: &'a [String],
    /// One of them is a Bcc.
    pub has_bcc: bool,
    pub sign: bool,
    pub encrypt: bool,
    pub now: i64,
}

/// Chooses the protocol and the keys. `first_seen` gives the fingerprint first seen for an address.
pub fn resolve(
    keys: &[StoredKey],
    ask: &Ask<'_>,
    first_seen: &dyn Fn(&str) -> Option<String>,
) -> Result<Resolved, PlanError> {
    let Ask { has_bcc, sign, encrypt, now, .. } = *ask;
    if !sign && !encrypt {
        return Err(PlanError::NothingAsked);
    }
    let sender = lower(ask.sender);
    let mut addresses: Vec<String> = Vec::new();
    for r in ask.recipients {
        let r = lower(r);
        if !r.is_empty() && !addresses.contains(&r) {
            addresses.push(r);
        }
    }
    let mut best_missing: Option<(KeyKind, Vec<String>)> = None;
    let mut bcc_blocked = false;
    for kind in [KeyKind::Pgp, KeyKind::Smime] {
        let own: Option<&StoredKey> =
            keys.iter().find(|k| k.entry.kind == kind && k.entry.has_secret && has_email(k, &sender));
        let Some(own) = own else { continue };
        if !encrypt {
            return Ok(Resolved {
                protocol: kind,
                signer: Some(own.entry.id.clone()),
                own: None,
                recipients: Vec::new(),
                warnings: Vec::new(),
            });
        }
        if !usable_to_encrypt(own, now) {
            // Our own key cannot take a copy: nothing could be read back from Sent.
            best_missing = Some((kind, vec![sender.clone()]));
            continue;
        }
        let mut chosen = Vec::new();
        let mut missing = Vec::new();
        let mut warnings = Vec::new();
        for address in &addresses {
            if *address == sender {
                continue;
            }
            match key_for(keys, kind, address, first_seen(address).as_deref(), now) {
                Some((key, warning)) => {
                    chosen.push((address.clone(), key.entry.id.clone()));
                    warnings.extend(warning);
                    if matches!(key.entry.origin, KeyOrigin::Autocrypt | KeyOrigin::Wkd) && !key.entry.verified {
                        let source = if key.entry.origin == KeyOrigin::Wkd {
                            "a key directory lookup"
                        } else {
                            "a mail's Autocrypt header"
                        };
                        warnings
                            .push(format!("The key for {address} came from {source} and has not been checked by you."));
                    }
                }
                None => missing.push(address.clone()),
            }
        }
        if !missing.is_empty() {
            if best_missing.as_ref().is_none_or(|(_, m)| missing.len() < m.len()) {
                best_missing = Some((kind, missing));
            }
            continue;
        }
        if kind == KeyKind::Smime && has_bcc {
            bcc_blocked = true;
            continue;
        }
        return Ok(Resolved {
            protocol: kind,
            signer: sign.then(|| own.entry.id.clone()),
            own: Some(own.entry.id.clone()),
            recipients: chosen,
            warnings,
        });
    }
    if bcc_blocked {
        return Err(PlanError::BccWithSmime);
    }
    match best_missing {
        Some((protocol, emails)) => Err(PlanError::Missing { protocol, emails }),
        None => Err(PlanError::NoOwnKey),
    }
}
