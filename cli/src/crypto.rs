//! `kweb crypto ...`: the encryption feature (M9) from the desktop, enough to try it by hand
//! against a server. Never prints a key, a passphrase or the text of an encrypted message
//! unless asked to (`decrypt`).

use kweb_core::{
    crypto::types::{
        CryptoKind, CryptoNote, CryptoView, EncryptionState, KeyEntry, KeyKind, KeyOrigin, SignatureState,
    },
    ffi::Core,
};

#[derive(Debug, Clone, PartialEq)]
pub enum Crypto {
    Status,
    On,
    Off,
    Keys,
    Import {
        path: String,
    },
    Remove {
        id: String,
    },
    /// Fetch the message, check its signature, say what it covers.
    Check {
        id: String,
    },
    /// Add a key the message carries to the keyring.
    Take {
        id: String,
        offer: String,
    },
    /// Add a private key (OpenPGP secret key or PKCS#12 file). A PKCS#12 passphrase comes from KWEB_PASSPHRASE.
    ImportPrivate {
        path: String,
    },
    /// Open an encrypted message: the passphrase of the key comes from KWEB_PASSPHRASE; `--keep` keeps the key
    /// unlocked; `--show` prints the decrypted text; `--save-to <dir>` writes its files.
    Decrypt {
        id: String,
        keep: bool,
        show: bool,
        save_to: Option<String>,
    },
    /// Delete the passphrase kept for a key.
    Lock {
        id: String,
    },
}

/// The environment variable the passphrase is read from (never an argument, which shell history would keep).
pub const PASSPHRASE_VARIABLE: &str = "KWEB_PASSPHRASE";

const USAGE: &str = "crypto takes: status | on | off | keys | import <file> | rm <key-id> | check <message-id> | take <message-id> <key-id> | import-private <file> | decrypt <message-id> [--keep] [--show] [--save-to <dir>] | lock <key-id>";

pub fn parse(rest: &[String]) -> Result<Crypto, String> {
    let words: Vec<&str> = rest.iter().map(String::as_str).collect();
    match words.as_slice() {
        ["status"] => Ok(Crypto::Status),
        ["on"] => Ok(Crypto::On),
        ["off"] => Ok(Crypto::Off),
        ["keys"] => Ok(Crypto::Keys),
        ["import", path] => Ok(Crypto::Import { path: (*path).into() }),
        ["rm", id] => Ok(Crypto::Remove { id: (*id).into() }),
        ["check", id] => Ok(Crypto::Check { id: (*id).into() }),
        ["take", id, offer] => Ok(Crypto::Take { id: (*id).into(), offer: (*offer).into() }),
        ["import-private", path] => Ok(Crypto::ImportPrivate { path: (*path).into() }),
        ["lock", id] => Ok(Crypto::Lock { id: (*id).into() }),
        ["decrypt", id, flags @ ..] => {
            let (mut keep, mut show, mut save_to) = (false, false, None);
            let mut flags = flags.iter();
            while let Some(flag) = flags.next() {
                match *flag {
                    "--keep" => keep = true,
                    "--show" => show = true,
                    "--save-to" => save_to = Some(flags.next().ok_or("--save-to takes a folder")?.to_string()),
                    other => return Err(format!("unknown option {other}")),
                }
            }
            Ok(Crypto::Decrypt { id: (*id).into(), keep, show, save_to })
        }
        _ => Err(USAGE.into()),
    }
}

pub fn run(core: &Core, what: Crypto) -> Result<(), String> {
    let e = |e: kweb_core::Error| e.to_string();
    match what {
        Crypto::Status => {
            let on = core.crypto_enabled().map_err(e)?;
            println!("Encryption (S/MIME and OpenPGP): {}", if on { "on" } else { "off" });
            println!("{} keys and certificates", core.crypto_keys().map_err(e)?.len());
        }
        Crypto::On => core.set_crypto_enabled(true).map_err(e)?,
        Crypto::Off => core.set_crypto_enabled(false).map_err(e)?,
        Crypto::Keys => {
            for k in core.crypto_keys().map_err(e)? {
                println!("{}", key_line(&k));
            }
        }
        Crypto::Import { path } => {
            let data = std::fs::read(&path).map_err(|x| format!("cannot read {path}: {x}"))?;
            for k in core.crypto_import(data, KeyOrigin::File).map_err(e)? {
                println!("added: {}", key_line(&k));
            }
        }
        Crypto::Remove { id } => core.crypto_remove_key(id).map_err(e)?,
        Crypto::Check { id } => {
            let kind = core.crypto_probe(id.clone()).map_err(e)?;
            if kind == CryptoKind::None {
                println!("Not signed or encrypted (or the feature is off).");
                return Ok(());
            }
            print_view(&core.crypto_open(id).map_err(e)?);
        }
        Crypto::Take { id, offer } => {
            println!("added: {}", key_line(&core.crypto_import_offer(id, offer).map_err(e)?));
        }
        Crypto::ImportPrivate { path } => {
            let data = std::fs::read(&path).map_err(|x| format!("cannot read {path}: {x}"))?;
            let passphrase = std::env::var(PASSPHRASE_VARIABLE).ok();
            for k in core.crypto_import_private(data, passphrase).map_err(e)? {
                println!("added: {}", key_line(&k));
            }
        }
        Crypto::Lock { id } => core.crypto_forget_passphrase(id).map_err(e)?,
        Crypto::Decrypt { id, keep, show, save_to } => {
            let passphrase = std::env::var(PASSPHRASE_VARIABLE).ok();
            let view = core.crypto_open_with(id.clone(), passphrase, keep).map_err(e)?;
            print_view(&view);
            let Some(content) = &view.content else { return Ok(()) };
            if show {
                println!();
                println!("{}", content.text.clone().or_else(|| content.html.clone()).unwrap_or_default());
            }
            if let Some(dir) = save_to {
                for file in &content.attachments {
                    let name = if file.name.is_empty() {
                        format!("file-{}", file.index)
                    } else {
                        file.name.replace(['/', '\\'], "_")
                    };
                    let path = std::path::Path::new(&dir).join(name);
                    core.crypto_save_attachment(id.clone(), file.index, path.to_string_lossy().into_owned())
                        .map_err(e)?;
                    println!("saved: {}", path.display());
                }
            }
        }
    }
    Ok(())
}

fn key_line(k: &KeyEntry) -> String {
    let kind = match k.kind {
        KeyKind::Pgp => "OpenPGP",
        KeyKind::Smime => "S/MIME",
    };
    let own = if k.has_secret { " [own]" } else { "" };
    format!("{} {kind} {} <{}> {}{own}", k.id, k.label, k.emails.join(", "), k.fingerprint)
}

pub fn signature_words(s: SignatureState) -> &'static str {
    match s {
        SignatureState::NotSigned => "not signed",
        SignatureState::Verified => "valid signature; the signer's address is the From address",
        SignatureState::KeyChanged => "valid signature, but the key is not the one first seen for this address",
        SignatureState::CertificateNotChecked => {
            "signed; the certificate came with the message and is not checked against an authority"
        }
        SignatureState::OtherAddress => "valid signature, but from a different address than From",
        SignatureState::NoKey => "cannot check: no key or certificate for the signer",
        SignatureState::Invalid => "INVALID: the message was changed or the signature is false",
        SignatureState::KeyExpired => "valid signature, but the key had expired",
        SignatureState::KeyRevoked => "valid signature, but the key was revoked",
        SignatureState::Unsupported => "cannot check: a kind of signature this app does not read",
        SignatureState::Unreadable => "cannot check: the signature could not be read",
    }
}

pub fn encryption_words(s: EncryptionState) -> &'static str {
    match s {
        EncryptionState::NotEncrypted => "not encrypted",
        EncryptionState::Decrypted => "decrypted",
        EncryptionState::NeedKey => "encrypted; none of your keys opens it",
        EncryptionState::NeedPassphrase => "encrypted; the key needs its passphrase",
        EncryptionState::WrongPassphrase => "encrypted; that passphrase does not open the key",
        EncryptionState::NoIntegrity => "encrypted without protection against change: not opened",
        EncryptionState::Unsupported => "encrypted in a way this app cannot open",
        EncryptionState::Failed => "encrypted; it could not be opened",
    }
}

fn note_words(n: CryptoNote) -> &'static str {
    match n {
        CryptoNote::OnlyPartCovered => "parts outside the signed or encrypted part are not covered",
        CryptoNote::RemoteContentBlocked => "remote content stays off for this message",
        CryptoNote::UnsignedTextHidden => "text around the signed block was left out",
        CryptoNote::SignatureExpired => "the signature itself had expired",
        CryptoNote::SignedInsideEncryption => "signed inside the encryption",
        CryptoNote::NotSearchable => "encrypted mail cannot be searched on the server",
        CryptoNote::SmimeNoRevocationCheck => "S/MIME: no chain or revocation check",
    }
}

pub fn print_view(v: &CryptoView) {
    println!("kind: {:?}", v.kind);
    println!("from: {}", v.from);
    println!("signature: {}", signature_words(v.signature));
    if let Some(s) = &v.signer {
        let place = if s.in_keyring { "in your keyring" } else { "only in the message" };
        println!("signer: {} <{}> {} ({place})", s.label, s.emails.join(", "), s.fingerprint);
    }
    println!("encryption: {}", encryption_words(v.encryption));
    println!("covers: {}", if v.covers_whole_message { "the whole message" } else { "only part of the message" });
    for n in &v.notes {
        println!("note: {}", note_words(*n));
    }
    if let Some(d) = &v.detail {
        println!("detail: {d}");
    }
    for o in &v.offers {
        let known = if o.already_known { " (already yours)" } else { "" };
        println!("key in message: {} {} <{}> {}{known}", o.id, o.label, o.emails.join(", "), o.fingerprint);
    }
    if let Some(c) = &v.content {
        println!("content: {} attachments", c.attachments.len());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(words: &[&str]) -> Result<Crypto, String> {
        parse(&words.iter().map(|w| w.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn the_commands_are_read() {
        assert_eq!(p(&["status"]), Ok(Crypto::Status));
        assert_eq!(p(&["check", "e1"]), Ok(Crypto::Check { id: "e1".into() }));
        assert_eq!(
            p(&["decrypt", "e1", "--keep", "--save-to", "out"]),
            Ok(Crypto::Decrypt { id: "e1".into(), keep: true, show: false, save_to: Some("out".into()) })
        );
        assert!(p(&["decrypt", "e1", "--save-to"]).is_err());
        assert!(p(&["decrypt", "e1", "--passphrase", "x"]).is_err(), "a passphrase is never an argument");
        assert!(p(&[]).is_err());
    }
}
