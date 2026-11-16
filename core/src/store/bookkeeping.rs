//! Notification bookkeeping: what is shown, what the user swiped away.

use super::mailboxes::QUIET_ROLES;
use super::messages::{query_headers, COLUMNS};
use super::Store;
use crate::model::{Address, MessageHeader};
use crate::notify::AddressMode;
use crate::Result;
use std::collections::HashMap;

impl Store {
    /// The notification choices per receiving address, by lower-case address. An address with
    /// no entry notifies.
    pub fn address_modes(&self, account_id: &str) -> Result<HashMap<String, AddressMode>> {
        self.read(|c| {
            let mut stmt = c.prepare("SELECT address, mode FROM address_notify WHERE account_id = ?1")?;
            let rows =
                stmt.query_map([account_id], |r| Ok((r.get::<_, String>(0)?, AddressMode::from_code(r.get(1)?))))?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    /// Records the choice for `address` (already lower case); `Notify` removes the row.
    pub fn set_address_mode(&self, account_id: &str, address: &str, mode: AddressMode) -> Result<()> {
        self.write(|c| {
            if mode == AddressMode::Notify {
                c.execute("DELETE FROM address_notify WHERE account_id = ?1 AND address = ?2", [account_id, address])?;
            } else {
                c.execute(
                    "INSERT INTO address_notify (account_id, address, mode) VALUES (?1, ?2, ?3)
                     ON CONFLICT(account_id, address) DO UPDATE SET mode = ?3",
                    rusqlite::params![account_id, address, mode.code()],
                )?;
            }
            Ok(())
        })
    }

    /// The ids of up to `limit` cached messages whose other recipients (Cc, delivery headers) were
    /// never fetched, newest first.
    pub fn receivers_missing(&self, account_id: &str, limit: usize) -> Result<Vec<String>> {
        self.read(|c| {
            let mut stmt = c.prepare(
                "SELECT id FROM messages WHERE account_id = ?1 AND recv_json IS NULL
                 ORDER BY received_at DESC, id DESC LIMIT ?2",
            )?;
            let rows = stmt.query_map(rusqlite::params![account_id, limit as i64], |r| r.get(0))?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    /// Records the other recipients of messages (an empty list is an answer too). A message that is not
    /// cached any more is skipped.
    pub fn set_receivers(&self, account_id: &str, list: &[(String, Vec<Address>)]) -> Result<()> {
        self.write(|c| {
            let mut stmt = c.prepare_cached("UPDATE messages SET recv_json = ?3 WHERE account_id = ?1 AND id = ?2")?;
            for (id, addresses) in list {
                let json = serde_json::to_string(addresses).map_err(|e| crate::Error::store(e.to_string()))?;
                stmt.execute(rusqlite::params![account_id, id, json])?;
            }
            Ok(())
        })
    }

    /// The other recipients (Cc and delivery addresses) of these messages; a message whose headers are not in
    /// yet has no entry.
    pub fn receivers_of(&self, account_id: &str, ids: &[String]) -> Result<HashMap<String, Vec<Address>>> {
        self.read(|c| {
            let mut stmt = c.prepare_cached(
                "SELECT recv_json FROM messages WHERE account_id = ?1 AND id = ?2 AND recv_json IS NOT NULL",
            )?;
            let mut out = HashMap::new();
            for id in ids {
                let json: Option<String> =
                    rusqlite::OptionalExtension::optional(stmt.query_row([account_id, id.as_str()], |r| r.get(0)))?;
                if let Some(json) = json {
                    out.insert(id.clone(), serde_json::from_str(&json).unwrap_or_default());
                }
            }
            Ok(out)
        })
    }

    /// The route (see `notify::Route`) each shown notification was last posted on, by message id.
    pub fn shown_routes(&self, account_id: &str) -> Result<HashMap<String, String>> {
        self.read(|c| {
            let mut stmt = c.prepare("SELECT message_id, route FROM notifications_shown WHERE account_id = ?1")?;
            let rows = stmt.query_map([account_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    pub fn set_shown_route(&self, account_id: &str, message_id: &str, route: &str) -> Result<()> {
        self.write(|c| {
            c.execute(
                "UPDATE notifications_shown SET route = ?3 WHERE account_id = ?1 AND message_id = ?2",
                [account_id, message_id, route],
            )?;
            Ok(())
        })
    }

    /// Messages that should have a notification: unread, not dismissed, and either in
    /// `inbox_id` and received at or after `since`, or in a folder switched on for
    /// notifications and received at or after `since` and after it was switched on. A message in
    /// both is listed once. Newest first.
    pub fn notifiable(&self, account_id: &str, inbox_id: &str, since: i64) -> Result<Vec<MessageHeader>> {
        self.read(|c| {
            let sql = format!(
                "SELECT {COLUMNS} FROM messages m
                 WHERE m.account_id = ?1 AND m.is_unread = 1
                   AND NOT EXISTS (SELECT 1 FROM notifications_dismissed d
                                   WHERE d.account_id = m.account_id AND d.message_id = m.id)
                   AND EXISTS (
                       SELECT 1 FROM message_mailboxes mm
                       LEFT JOIN mailbox_notify n ON n.account_id = mm.account_id AND n.mailbox_id = mm.mailbox_id
                       LEFT JOIN mailboxes b ON b.account_id = mm.account_id AND b.id = mm.mailbox_id
                       WHERE mm.account_id = m.account_id AND mm.message_id = m.id
                         AND ((mm.mailbox_id = ?2 AND m.received_at >= ?3)
                              OR (n.mailbox_id IS NOT NULL AND m.received_at >= MAX(?3, n.since)
                                  AND COALESCE(b.role, '') NOT IN ({QUIET_ROLES})))
                   )
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
