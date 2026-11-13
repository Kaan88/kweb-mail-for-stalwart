//! Notification bookkeeping: what is shown, what the user swiped away.

use super::messages::{query_headers, COLUMNS};
use super::Store;
use crate::model::MessageHeader;
use crate::Result;

impl Store {
    /// Messages that should have a notification: in `inbox_id`, unread, received at or after
    /// `since`, and not dismissed. Newest first.
    pub fn notifiable(&self, account_id: &str, inbox_id: &str, since: i64) -> Result<Vec<MessageHeader>> {
        self.read(|c| {
            let sql = format!(
                "SELECT {COLUMNS} FROM messages m
                 JOIN message_mailboxes mm ON mm.account_id = m.account_id AND mm.message_id = m.id
                 WHERE m.account_id = ?1 AND mm.mailbox_id = ?2 AND m.is_unread = 1 AND m.received_at >= ?3
                   AND NOT EXISTS (SELECT 1 FROM notifications_dismissed d
                                   WHERE d.account_id = m.account_id AND d.message_id = m.id)
                 ORDER BY m.received_at DESC, m.id DESC"
            );
            query_headers(c, &sql, rusqlite::params![account_id, inbox_id, since], account_id)
        })
    }

    /// `(message_id, posted_at)` of what the core last told Android to show.
    pub fn shown_list(&self, account_id: &str) -> Result<Vec<(String, i64)>> {
        self.read(|c| {
            let mut stmt = c.prepare(
                "SELECT message_id, posted_at FROM notifications_shown WHERE account_id = ?1
                 ORDER BY posted_at, message_id",
            )?;
            let rows = stmt.query_map([account_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    /// Adding an id that is already shown updates its `posted_at`.
    pub fn shown_add(&self, account_id: &str, message_id: &str, posted_at: i64) -> Result<()> {
        self.write(|c| {
            c.execute(
                "INSERT INTO notifications_shown (account_id, message_id, posted_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT(account_id, message_id) DO UPDATE SET posted_at = ?3",
                rusqlite::params![account_id, message_id, posted_at],
            )?;
            Ok(())
        })
    }

    pub fn shown_remove(&self, account_id: &str, message_id: &str) -> Result<()> {
        self.write(|c| {
            c.execute(
                "DELETE FROM notifications_shown WHERE account_id = ?1 AND message_id = ?2",
                [account_id, message_id],
            )?;
            Ok(())
        })
    }

    /// The user swiped the notification away: never post it again, and it is no longer shown.
    pub fn dismissed_add(&self, account_id: &str, message_id: &str) -> Result<()> {
        self.write(|c| {
            c.execute(
                "INSERT OR IGNORE INTO notifications_dismissed (account_id, message_id) VALUES (?1, ?2)",
                [account_id, message_id],
            )?;
            c.execute(
                "DELETE FROM notifications_shown WHERE account_id = ?1 AND message_id = ?2",
                [account_id, message_id],
            )?;
            Ok(())
        })
    }

    pub fn is_dismissed(&self, account_id: &str, message_id: &str) -> Result<bool> {
        self.read(|c| {
            Ok(c.query_row(
                "SELECT EXISTS (SELECT 1 FROM notifications_dismissed WHERE account_id = ?1 AND message_id = ?2)",
                [account_id, message_id],
                |r| r.get(0),
            )?)
        })
    }

    /// Drops dismissed rows whose message is gone or no longer unread: they can never
    /// matter again (a message turned unread again is a fresh reason to notify).
    /// Returns how many rows were removed.
    pub fn prune_dismissed(&self, account_id: &str) -> Result<usize> {
        self.write(|c| {
            Ok(c.execute(
                "DELETE FROM notifications_dismissed WHERE account_id = ?1 AND NOT EXISTS (
                     SELECT 1 FROM messages m
                     WHERE m.account_id = notifications_dismissed.account_id
                       AND m.id = notifications_dismissed.message_id AND m.is_unread = 1)",
                [account_id],
            )?)
        })
    }
}
