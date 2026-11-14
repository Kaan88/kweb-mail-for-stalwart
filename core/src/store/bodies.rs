//! Cached message bodies and the per-mailbox windows.

use super::Store;
use crate::model::MessageBody;
use crate::{Error, Result};
use rusqlite::{params, OptionalExtension};

impl Store {
    /// Keeps the body of a cached message (replacing an older copy). Returns false, and stores
    /// nothing, when the message itself is not cached: a body without a header could never be
    /// listed or cleaned up.
    pub fn put_body(&self, account_id: &str, body: &MessageBody) -> Result<bool> {
        let json = serde_json::to_string(body).map_err(|e| Error::store(e.to_string()))?;
        self.write(|c| {
            let cached = c
                .query_row(
                    "SELECT 1 FROM messages WHERE account_id = ?1 AND id = ?2",
                    [account_id, &body.message_id],
                    |_| Ok(()),
                )
                .optional()?
                .is_some();
            if cached {
                c.execute(
                    "INSERT INTO message_bodies (account_id, message_id, body) VALUES (?1, ?2, ?3)
                     ON CONFLICT(account_id, message_id) DO UPDATE SET body = ?3",
                    params![account_id, body.message_id, json],
                )?;
            }
            Ok(cached)
        })
    }

    /// An unreadable row (written by another version of the app) counts as no body: it is
    /// fetched again.
    pub fn get_body(&self, account_id: &str, message_id: &str) -> Result<Option<MessageBody>> {
        self.read(|c| {
            let json: Option<String> = c
                .query_row(
                    "SELECT body FROM message_bodies WHERE account_id = ?1 AND message_id = ?2",
                    [account_id, message_id],
                    |r| r.get(0),
                )
                .optional()?;
            Ok(json.and_then(|json| serde_json::from_str(&json).ok()))
        })
    }

    /// Among the `newest_n` newest messages of the mailbox, the ones with no cached body,
    /// newest first.
    pub fn ids_without_body(&self, account_id: &str, mailbox_id: &str, newest_n: u32) -> Result<Vec<String>> {
        self.read(|c| {
            let mut stmt = c.prepare(
                "SELECT t.id FROM (
                     SELECT m.id, m.received_at FROM messages m
                     JOIN message_mailboxes mm ON mm.account_id = m.account_id AND mm.message_id = m.id
                     WHERE m.account_id = ?1 AND mm.mailbox_id = ?2
                     ORDER BY m.received_at DESC, m.id DESC LIMIT ?3
                 ) t
                 WHERE NOT EXISTS (SELECT 1 FROM message_bodies b WHERE b.account_id = ?1 AND b.message_id = t.id)
                 ORDER BY t.received_at DESC, t.id DESC",
            )?;
            let ids = stmt.query_map(params![account_id, mailbox_id, newest_n], |r| r.get(0))?;
            Ok(ids.collect::<rusqlite::Result<_>>()?)
        })
    }

    // ---- windows ----

    /// How far back the cache holds the mailbox without gaps: a `received_at`. `None`: all of it.
    pub fn get_window(&self, account_id: &str, mailbox_id: &str) -> Result<Option<i64>> {
        self.read(|c| {
            Ok(c.query_row(
                "SELECT oldest FROM mailbox_windows WHERE account_id = ?1 AND mailbox_id = ?2",
                [account_id, mailbox_id],
                |r| r.get(0),
            )
            .optional()?)
        })
    }

    pub fn set_window(&self, account_id: &str, mailbox_id: &str, oldest: i64) -> Result<()> {
        self.write(|c| {
            c.execute(
                "INSERT INTO mailbox_windows (account_id, mailbox_id, oldest) VALUES (?1, ?2, ?3)
                 ON CONFLICT(account_id, mailbox_id) DO UPDATE SET oldest = ?3",
                params![account_id, mailbox_id, oldest],
            )?;
            Ok(())
        })
    }

    /// The whole mailbox is cached.
    pub fn clear_window(&self, account_id: &str, mailbox_id: &str) -> Result<()> {
        self.write(|c| {
            c.execute(
                "DELETE FROM mailbox_windows WHERE account_id = ?1 AND mailbox_id = ?2",
                [account_id, mailbox_id],
            )?;
            Ok(())
        })
    }
}
