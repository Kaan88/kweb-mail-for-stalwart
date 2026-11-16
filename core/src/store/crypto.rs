//! The keyring of the encryption feature: other people's public keys and certificates, the user's
//! own (public part only; the private part is in the host's secret store), and the fingerprint
//! first seen for each address. Nothing here is ever a secret.

use rusqlite::{params, OptionalExtension};

use super::Store;
use crate::{
    crypto::types::{KeyEntry, KeyKind, KeyOrigin},
    Error, Result,
};

/// A keyring row with its public material.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredKey {
    pub entry: KeyEntry,
    /// An OpenPGP transferable public key (binary) or a DER certificate.
    pub material: Vec<u8>,
}

fn kind_of(code: &str) -> KeyKind {
    if code == "x509" {
        KeyKind::Smime
    } else {
        KeyKind::Pgp
    }
}

fn read_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<StoredKey> {
    let emails: String = r.get(3)?;
    Ok(StoredKey {
        entry: KeyEntry {
            id: r.get(0)?,
            kind: kind_of(&r.get::<_, String>(1)?),
            label: r.get(2)?,
            emails: serde_json::from_str(&emails).unwrap_or_default(),
            fingerprint: r.get(4)?,
            origin: KeyOrigin::from_code(&r.get::<_, String>(5)?),
            origin_detail: r.get(6)?,
            added_at: r.get(7)?,
            has_secret: r.get::<_, i64>(9)? != 0,
            needs_passphrase: r.get::<_, i64>(10)? != 0,
            unlocked: false,
            expires_at: r.get(11)?,
        },
        material: r.get(8)?,
    })
}

const COLUMNS: &str = "id, kind, label, emails, fingerprint, origin, origin_detail, added_at, material, has_secret, needs_passphrase, expires_at";

impl Store {
    /// Every key of the keyring, own keys first, then by label.
    pub fn crypto_keys(&self, account_id: &str) -> Result<Vec<StoredKey>> {
        self.read(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {COLUMNS} FROM crypto_keys WHERE account_id = ?1 ORDER BY has_secret DESC, label COLLATE NOCASE, id"
            ))?;
            let rows = stmt.query_map([account_id], read_row)?.collect::<rusqlite::Result<_>>()?;
            Ok(rows)
        })
    }

    pub fn crypto_key(&self, account_id: &str, id: &str) -> Result<Option<StoredKey>> {
        self.read(|c| {
            Ok(c.query_row(
                &format!("SELECT {COLUMNS} FROM crypto_keys WHERE account_id = ?1 AND id = ?2"),
                [account_id, id],
                read_row,
            )
            .optional()?)
        })
    }

    /// Adds a key, or replaces the one with the same id (the same fingerprint).
    pub fn put_crypto_key(&self, account_id: &str, key: &StoredKey) -> Result<()> {
        let e = &key.entry;
        let emails = serde_json::to_string(&e.emails).map_err(|e| Error::store(e.to_string()))?;
        self.write(|c| {
            c.execute(
                "INSERT INTO crypto_keys (account_id, id, kind, label, emails, fingerprint, origin, origin_detail,
                                          added_at, material, has_secret, needs_passphrase, expires_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
                 ON CONFLICT(account_id, id) DO UPDATE SET
                     label = ?4, emails = ?5, origin = ?7, origin_detail = ?8, material = ?10,
                     has_secret = ?11, needs_passphrase = ?12, expires_at = ?13",
                params![
                    account_id,
                    e.id,
                    e.kind.code(),
                    e.label,
                    emails,
                    e.fingerprint,
                    e.origin.code(),
                    e.origin_detail,
                    e.added_at,
                    key.material,
                    e.has_secret as i64,
                    e.needs_passphrase as i64,
                    e.expires_at,
                ],
            )?;
            Ok(())
        })
    }

    /// Removes a key, and what was remembered about its fingerprint (so that a new key for the
    /// same address is the first one seen again, which only the user's own deleting allows).
    pub fn delete_crypto_key(&self, account_id: &str, id: &str) -> Result<bool> {
        self.write(|c| {
            let fingerprint: Option<String> = c
                .query_row(
                    "SELECT fingerprint FROM crypto_keys WHERE account_id = ?1 AND id = ?2",
                    [account_id, id],
                    |r| r.get(0),
                )
                .optional()?;
            let removed = c.execute("DELETE FROM crypto_keys WHERE account_id = ?1 AND id = ?2", [account_id, id])?;
            if let Some(fingerprint) = fingerprint {
                c.execute(
                    "DELETE FROM crypto_first_seen WHERE account_id = ?1 AND fingerprint = ?2",
                    [account_id, &fingerprint],
                )?;
            }
            Ok(removed > 0)
        })
    }

    /// The fingerprint first seen with this address, if any.
    pub fn crypto_first_seen(&self, account_id: &str, address: &str) -> Result<Option<String>> {
        self.read(|c| {
            Ok(c.query_row(
                "SELECT fingerprint FROM crypto_first_seen WHERE account_id = ?1 AND address = ?2",
                [account_id, &address.to_ascii_lowercase()],
                |r| r.get(0),
            )
            .optional()?)
        })
    }

    /// Remembers the first fingerprint seen with an address. An address that has one keeps it.
    pub fn put_crypto_first_seen(&self, account_id: &str, address: &str, fingerprint: &str, at: i64) -> Result<()> {
        self.write(|c| {
            c.execute(
                "INSERT OR IGNORE INTO crypto_first_seen (account_id, address, fingerprint, seen_at) VALUES (?1, ?2, ?3, ?4)",
                params![account_id, address.to_ascii_lowercase(), fingerprint, at],
            )?;
            Ok(())
        })
    }

    /// Every key and what was remembered goes (the user switched the feature off and chose to forget).
    pub fn clear_crypto(&self, account_id: &str) -> Result<()> {
        self.write(|c| {
            c.execute("DELETE FROM crypto_keys WHERE account_id = ?1", [account_id])?;
            c.execute("DELETE FROM crypto_first_seen WHERE account_id = ?1", [account_id])?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Account;

    fn store() -> Store {
        let st = Store::open_in_memory().unwrap();
        st.save_account(&Account {
            id: "a".into(),
            server_url: "https://x.test".into(),
            username: "u".into(),
            signed_in_at: 0,
        })
        .unwrap();
        st
    }

    fn key(id: &str, label: &str, secret: bool) -> StoredKey {
        StoredKey {
            entry: KeyEntry {
                id: id.into(),
                kind: KeyKind::Pgp,
                label: label.into(),
                emails: vec!["a@x.test".into()],
                fingerprint: id.trim_start_matches("pgp:").into(),
                origin: KeyOrigin::File,
                origin_detail: String::new(),
                added_at: 5,
                has_secret: secret,
                needs_passphrase: secret,
                unlocked: false,
                expires_at: None,
            },
            material: vec![1, 2, 3],
        }
    }

    #[test]
    fn keys_are_kept_listed_replaced_and_removed_with_what_was_seen_of_them() {
        let st = store();
        st.put_crypto_key("a", &key("pgp:BB", "bee", false)).unwrap();
        st.put_crypto_key("a", &key("pgp:AA", "ay", true)).unwrap();
        let ids: Vec<_> = st.crypto_keys("a").unwrap().into_iter().map(|k| k.entry.id).collect();
        assert_eq!(ids, ["pgp:AA", "pgp:BB"], "own keys first");
        st.put_crypto_key("a", &key("pgp:BB", "renamed", false)).unwrap();
        assert_eq!(st.crypto_key("a", "pgp:BB").unwrap().unwrap().entry.label, "renamed");
        st.put_crypto_first_seen("a", "Alice@X.test", "BB", 9).unwrap();
        st.put_crypto_first_seen("a", "alice@x.test", "CC", 10).unwrap();
        assert_eq!(st.crypto_first_seen("a", "alice@x.test").unwrap().as_deref(), Some("BB"), "the first one stays");
        assert!(st.delete_crypto_key("a", "pgp:BB").unwrap());
        assert_eq!(st.crypto_first_seen("a", "alice@x.test").unwrap(), None);
        assert!(!st.delete_crypto_key("a", "pgp:BB").unwrap());
    }

    #[test]
    fn nothing_of_one_account_is_seen_from_another() {
        let st = store();
        st.save_account(&Account {
            id: "b".into(),
            server_url: "https://y.test".into(),
            username: "v".into(),
            signed_in_at: 0,
        })
        .unwrap();
        st.put_crypto_key("a", &key("pgp:AA", "ay", false)).unwrap();
        st.put_crypto_first_seen("a", "x@x.test", "AA", 1).unwrap();
        assert!(st.crypto_keys("b").unwrap().is_empty());
        assert_eq!(st.crypto_first_seen("b", "x@x.test").unwrap(), None);
    }

    #[test]
    fn the_migration_is_additive() {
        use crate::store::schema;
        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        let before = schema::MIGRATIONS.len() - 1;
        assert_eq!(schema::migrate(&mut conn, &schema::MIGRATIONS[..before]).unwrap(), before);
        assert_eq!(schema::migrate(&mut conn, schema::MIGRATIONS).unwrap(), 1);
    }
}
