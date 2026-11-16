//! M9 stage 3: signing and encrypting what the user sends.
//!
//! The rules kept here:
//! - The finished message is made once, when the user taps Send (or schedules), in the user's own call and before
//!   anything is stored as "sending" or handed to the server. Every later try uploads the same bytes
//!   (`compose::flush::make_copy`), so a retry never makes a second, different message.
//! - Anything that is missing (a key, a passphrase, a recipient's key) stops the call before that: nothing goes to
//!   the server, the draft is untouched, and the error says in plain words what is missing. A message that was
//!   asked to be encrypted is never sent as plain text.
//! - Nothing of the message, a key or a passphrase goes into an error or a log. A typed passphrase is held in a
//!   zeroizing buffer for the call and kept only when the user chose "keep unlocked".

use zeroize::Zeroizing;

use super::{crypto::pass_name, Core};
use crate::{
    compose,
    crypto::{
        build::{self, Addr, BuildError, Encrypt, File, Headers, Protect, Signer},
        openpgp,
        plan::{self, Ask, PlanError, Resolved},
        types::KeyKind,
    },
    jmap::MAX_RAW_BYTES,
    model::{Address, Draft, MessageBody, Protection},
    store::StoredKey,
    Error, Result,
};

/// The key in the account's key/value table for "sign new messages by default".
const SIGN_DEFAULT_KEY: &str = "crypto_sign_default";

/// What the compose screen needs to know before the user sends a protected message.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct SendCheck {
    /// Which kind of keys would be used (None when nothing is asked for or nothing can be done).
    pub protocol: Option<KeyKind>,
    /// Everything asked for can be done, as far as keys go (a passphrase may still be needed: see `needs_passphrase`).
    pub ready: bool,
    /// Why not, in plain words.
    pub problem: Option<String>,
    /// The recipients that have no key (when that is the problem).
    pub missing: Vec<String>,
    /// The label of the own key that needs its passphrase before it can sign.
    pub needs_passphrase: Option<String>,
    /// Things to read, such as a key that has not been checked.
    pub warnings: Vec<String>,
    /// The text quotes a message that was encrypted, and this one is not encrypted.
    pub quotes_decrypted_unencrypted: bool,
    /// Bcc recipients are present and the message is encrypted: nobody can see from the packets who they are.
    pub bcc_hidden: bool,
}

/// A passphrase typed for one send, and whether to keep it.
#[derive(Default)]
pub struct Typed {
    pub passphrase: Option<Zeroizing<String>>,
    pub keep: bool,
}

fn addr(a: &Address) -> Addr {
    Addr { name: a.name.clone(), email: a.email.trim().to_string() }
}

impl Core {
    fn recipients_of(draft: &Draft) -> (Vec<String>, bool) {
        let has_bcc = !draft.bcc.is_empty();
        let all = draft.to.iter().chain(&draft.cc).chain(&draft.bcc).map(|a| a.email.trim().to_string()).collect();
        (all, has_bcc)
    }

    fn resolve_for(
        &self,
        account_id: &str,
        draft: &Draft,
        keys: &[StoredKey],
    ) -> std::result::Result<Resolved, PlanError> {
        let identities = self.store.identities_list(account_id).unwrap_or_default();
        let sender = identities.iter().find(|i| i.id == draft.identity_id).map(|i| i.email.clone()).unwrap_or_default();
        let (recipients, has_bcc) = Self::recipients_of(draft);
        let seen = |a: &str| self.store.crypto_first_seen(account_id, a).ok().flatten();
        let ask = Ask {
            sender: &sender,
            recipients: &recipients,
            has_bcc,
            sign: draft.protection.sign,
            encrypt: draft.protection.encrypt,
            now: super::now(),
        };
        plan::resolve(keys, &ask, &seen)
    }

    /// Makes the finished, signed or encrypted message for `draft`. Nothing is stored or sent here.
    pub(super) fn protected_message(&self, account_id: &str, draft: &Draft, typed: &Typed) -> Result<Vec<u8>> {
        if !self.crypto_enabled()? {
            return Err(Error::invalid(
                "Encryption is switched off for this account: turn off Sign and Encrypt for this message, or switch it on in Settings.",
            ));
        }
        let identities = self.store.identities_list(account_id)?;
        let identity = identities
            .iter()
            .find(|i| i.id == draft.identity_id)
            .ok_or_else(|| Error::invalid("choose an address to send from"))?;
        let keys = self.store.crypto_keys(account_id)?;
        let resolved = self.resolve_for(account_id, draft, &keys).map_err(|e| Error::invalid(e.text()))?;
        let secrets = self.secret_keys(account_id)?;
        let to_error = |e: BuildError| Error::invalid(e.text());

        // The signer: the own key's private part, unlocked with the typed or the kept passphrase.
        let mut used_passphrase: Option<(String, Zeroizing<String>)> = None;
        let pgp_secret;
        let pgp_pass: Zeroizing<String>;
        let signer = match &resolved.signer {
            None => None,
            Some(id) => {
                let secret = secrets
                    .iter()
                    .find(|s| &s.id == id)
                    .ok_or_else(|| Error::invalid("The private key for this address is missing from this phone."))?;
                match secret.kind {
                    KeyKind::Pgp => {
                        let key = openpgp::parse_secret(&secret.material)
                            .ok()
                            .and_then(|mut k| k.pop())
                            .ok_or_else(|| Error::invalid("The private key could not be read."))?;
                        let pass = if openpgp::is_locked(&key) {
                            let candidates = typed
                                .passphrase
                                .as_ref()
                                .map(|p| p.to_string())
                                .into_iter()
                                .chain(secret.stored_passphrase.as_ref().map(|p| p.to_string()));
                            let mut opened = None;
                            for c in candidates {
                                if openpgp::passphrase_opens(&key, &c) {
                                    opened = Some(c);
                                    break;
                                }
                            }
                            match opened {
                                Some(p) => Zeroizing::new(p),
                                None if typed.passphrase.is_some() => {
                                    return Err(to_error(BuildError::WrongPassphrase))
                                }
                                None => return Err(to_error(BuildError::NeedPassphrase)),
                            }
                        } else {
                            Zeroizing::new(String::new())
                        };
                        if typed.keep && typed.passphrase.as_deref().is_some_and(|t| *t == *pass) && !pass.is_empty() {
                            used_passphrase = Some((secret.id.clone(), pass.clone()));
                        }
                        pgp_secret = key;
                        pgp_pass = pass;
                        Some(Signer::Pgp { key: &pgp_secret, passphrase: pgp_pass.as_str() })
                    }
                    KeyKind::Smime => Some(Signer::Smime { cert: &secret.cert, pkcs8: &secret.material }),
                }
            }
        };

        // The recipients: each distinct key once, and always the sender's own.
        let mut ids: Vec<&str> = resolved.recipients.iter().map(|(_, id)| id.as_str()).collect();
        if let Some(own) = &resolved.own {
            ids.push(own.as_str());
        }
        ids.sort_unstable();
        ids.dedup();
        let pgp_keys: Vec<pgp::composed::SignedPublicKey>;
        let smime_certs: Vec<Vec<u8>>;
        let encrypt = if draft.protection.encrypt {
            let material: Vec<&StoredKey> =
                ids.iter().filter_map(|id| keys.iter().find(|k| k.entry.id.as_str() == *id)).collect();
            match resolved.protocol {
                KeyKind::Pgp => {
                    pgp_keys = material
                        .iter()
                        .filter_map(|k| {
                            openpgp::parse_public(&k.material)
                                .ok()
                                .and_then(|mut v| (!v.is_empty()).then(|| v.remove(0)))
                        })
                        .collect();
                    if pgp_keys.len() != material.len() {
                        return Err(Error::invalid("A recipient's key could not be read."));
                    }
                    Some(Encrypt::Pgp { keys: pgp_keys.iter().collect(), hide: !draft.bcc.is_empty() })
                }
                KeyKind::Smime => {
                    smime_certs = material.iter().map(|k| k.material.clone()).collect();
                    Some(Encrypt::Smime { certs: smime_certs.iter().map(Vec::as_slice).collect() })
                }
            }
        } else {
            None
        };

        // The files (their bytes are on the server as blobs) and the threading headers of an answer.
        let (files, threading) = {
            let row = &self.store.draft_get(account_id, &draft.id)?;
            let store = &self.store;
            let attachments = &draft.attachments;
            self.with_connection(|conn| async move {
                let mut files = Vec::new();
                let mut total: u64 = 0;
                for a in attachments {
                    let bytes = conn.raw_message(&a.blob_id, MAX_RAW_BYTES).await.map_err(|e| match e {
                        Error::Invalid { .. } => Error::invalid("An attached file is too large to protect."),
                        other => other,
                    })?;
                    total += bytes.len() as u64;
                    if total > MAX_RAW_BYTES {
                        return Err(Error::invalid("The attached files are too large to sign or encrypt."));
                    }
                    files.push(File { name: a.name.clone(), mime_type: a.mime_type.clone(), bytes });
                }
                let threading = match row {
                    Some(row) => compose::threading_of(&conn, store, account_id, row).await?,
                    None => Default::default(),
                };
                Ok((files, threading))
            })?
        };

        let headers = Headers {
            from: Addr { name: identity.name.clone(), email: identity.email.clone() },
            to: draft.to.iter().map(addr).collect(),
            cc: draft.cc.iter().map(addr).collect(),
            subject: draft.subject.clone(),
            date: super::now(),
            message_id: build::new_message_id(&identity.email),
            in_reply_to: threading.in_reply_to,
            references: threading.references,
        };
        let protect = Protect { sign: signer, encrypt, autocrypt: None, now: super::now() };
        let raw = build::build(&headers, &draft.body, &files, &protect).map_err(to_error)?;
        // The passphrase is kept only after it did its work.
        if let Some((key_id, pass)) = used_passphrase {
            self.secrets
                .set(pass_name(&key_id), pass.to_string())
                .map_err(|_| Error::store("the phone's protected storage did not keep the passphrase"))?;
        }
        Ok(raw)
    }

    /// The protection a new draft starts with: signing when the account's default says so and the sending address has
    /// a private key; encrypting when it answers a message that was encrypted.
    pub(super) fn starting_protection(
        &self,
        account_id: &str,
        identity_id: &str,
        encrypted_source: bool,
    ) -> Protection {
        if !self.crypto_enabled().unwrap_or(false) {
            return Protection::default();
        }
        let sign_by_default = self.store.kv_get(account_id, SIGN_DEFAULT_KEY).ok().flatten().as_deref() == Some("1");
        let own =
            self.store.identities_list(account_id).ok().and_then(|ids| ids.into_iter().find(|i| i.id == identity_id));
        let has_key = own.is_some_and(|i| {
            let email = i.email.to_ascii_lowercase();
            self.store.crypto_keys(account_id).ok().is_some_and(|keys| {
                keys.iter()
                    .any(|k| k.entry.has_secret && k.entry.emails.iter().any(|e| e.to_ascii_lowercase() == email))
            })
        });
        Protection { sign: sign_by_default && has_key, encrypt: encrypted_source && has_key, quotes_decrypted: false }
    }

    /// The body of a message to answer, when it was encrypted: its decrypted text, so the quote is the text and not
    /// the ciphertext. `(body, was_encrypted, text_was_decrypted)`. Needs no passphrase beyond a kept one; without
    /// it nothing is quoted.
    pub(super) fn answer_source(&self, message_id: &str, mut body: MessageBody) -> (MessageBody, bool, bool) {
        if !self.crypto_enabled().unwrap_or(false) {
            return (body, false, false);
        }
        match self.crypto_probe(message_id.to_string()) {
            Ok(crate::crypto::types::CryptoKind::None) | Err(_) => return (body, false, false),
            Ok(_) => {}
        }
        let Ok(outcome) = self.analyse(message_id, None) else { return (body, false, false) };
        use crate::crypto::types::EncryptionState;
        match outcome.view.encryption {
            EncryptionState::NotEncrypted => (body, false, false),
            EncryptionState::Decrypted => {
                let text = outcome.view.content.as_ref().map(|c| match (&c.text, &c.html) {
                    (Some(t), _) if !t.trim().is_empty() => t.clone(),
                    (_, Some(h)) => compose::rules::html_to_text(h),
                    _ => String::new(),
                });
                body.text = text;
                body.html = None;
                (body, true, true)
            }
            _ => {
                body.text = Some(String::new());
                body.html = None;
                (body, true, false)
            }
        }
    }
}

#[uniffi::export]
impl Core {
    /// Whether new messages are signed by default (only meaningful while the encryption feature is on).
    pub fn crypto_sign_default(&self) -> Result<bool> {
        Ok(self.store.kv_get(&self.account_id()?, SIGN_DEFAULT_KEY)?.as_deref() == Some("1"))
    }

    pub fn set_crypto_sign_default(&self, on: bool) -> Result<()> {
        let account_id = self.account_id()?;
        if on {
            self.store.kv_set(&account_id, SIGN_DEFAULT_KEY, "1")
        } else {
            self.store.kv_delete(&account_id, SIGN_DEFAULT_KEY)
        }
    }

    /// What signing and encrypting this draft would use, and what is missing. From the keyring only: no
    /// network, no passphrase asked, nothing changed.
    pub fn crypto_send_check(&self, draft: Draft) -> Result<SendCheck> {
        let account_id = self.account_id()?;
        let mut check = SendCheck {
            protocol: None,
            ready: true,
            problem: None,
            missing: Vec::new(),
            needs_passphrase: None,
            warnings: Vec::new(),
            quotes_decrypted_unencrypted: draft.protection.quotes_decrypted && !draft.protection.encrypt,
            bcc_hidden: false,
        };
        if !draft.protection.any() {
            return Ok(check);
        }
        if !self.crypto_enabled()? {
            check.ready = false;
            check.problem = Some("Encryption is switched off for this account.".into());
            return Ok(check);
        }
        let keys = self.store.crypto_keys(&account_id)?;
        match self.resolve_for(&account_id, &draft, &keys) {
            Ok(resolved) => {
                check.protocol = Some(resolved.protocol);
                check.warnings = resolved.warnings.clone();
                check.bcc_hidden = draft.protection.encrypt && !draft.bcc.is_empty();
                if let Some(id) = &resolved.signer {
                    if let Some(key) = keys.iter().find(|k| &k.entry.id == id) {
                        let kept = self.secrets.get(pass_name(id)).ok().flatten().is_some_and(|p| !p.is_empty());
                        if key.entry.needs_passphrase && !kept {
                            check.needs_passphrase = Some(key.entry.label.clone());
                        }
                    }
                }
            }
            Err(e) => {
                check.ready = false;
                if let PlanError::Missing { emails, .. } = &e {
                    check.missing = emails.clone();
                }
                check.problem = Some(e.text());
            }
        }
        Ok(check)
    }

    /// [`Core::send`] for a message that is signed or encrypted, with the passphrase of the signing key when it is
    /// locked and not kept. With `keep_unlocked` the passphrase (once it opened the key) is kept on this phone.
    /// The message is made first; if that fails nothing has happened to the draft. See the module notes.
    pub fn send_protected(
        &self,
        draft: Draft,
        delay_secs: u32,
        passphrase: Option<String>,
        keep_unlocked: bool,
    ) -> Result<crate::model::PendingSend> {
        let typed = Typed { passphrase: passphrase.map(Zeroizing::new), keep: keep_unlocked };
        self.send_with(draft, delay_secs, &typed)
    }

    /// [`Core::schedule_send`] for a message that is signed or encrypted; see [`Core::send_protected`].
    pub fn schedule_send_protected(
        &self,
        draft: Draft,
        send_at: i64,
        passphrase: Option<String>,
        keep_unlocked: bool,
    ) -> Result<crate::model::ScheduledSend> {
        let typed = Typed { passphrase: passphrase.map(Zeroizing::new), keep: keep_unlocked };
        self.schedule_with(draft, send_at, &typed)
    }
}
