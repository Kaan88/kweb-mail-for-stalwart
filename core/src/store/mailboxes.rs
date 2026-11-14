//! Mailbox rows.

use super::Store;
use crate::model::Mailbox;
use crate::Result;
use rusqlite::{params, Connection, OptionalExtension, Row};

/// Roles whose mail never notifies: a folder with one of them cannot be switched on.
pub(super) const QUIET_ROLES: &str = "'drafts', 'sent', 'trash', 'junk'";

/// The Inbox always notifies; any other folder does when it has a row in `mailbox_notify` and
/// no quiet role. Callers name the mailbox table `m`.
const COLUMNS: &str = "m.id, m.name, m.role, m.parent_id, m.sort_order, m.total_emails, m.unread_emails,
     (COALESCE(m.role, '') = 'inbox' OR (n.mailbox_id IS NOT NULL AND COALESCE(m.role, '') NOT IN ('drafts', 'sent', 'trash', 'junk')))";

const FROM: &str = "mailboxes m LEFT JOIN mailbox_notify n ON n.account_id = m.account_id AND n.mailbox_id = m.id";

fn from_row(r: &Row) -> rusqlite::Result<Mailbox> {
    Ok(Mailbox {
        id: r.get(0)?,
        name: r.get(1)?,
        role: r.get(2)?,
        parent_id: r.get(3)?,
        sort_order: r.get(4)?,
        total_emails: r.get(5)?,
        unread_emails: r.get(6)?,
        notify: r.get(7)?,
    })
}

fn upsert(c: &Connection, account_id: &str, boxes: &[Mailbox]) -> Result<()> {
    let mut stmt = c.prepare_cached(
        "INSERT INTO mailboxes (account_id, id, name, role, parent_id, sort_order, total_emails, unread_emails)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(account_id, id) DO UPDATE SET name = ?3, role = ?4, parent_id = ?5,
             sort_order = ?6, total_emails = ?7, unread_emails = ?8",
    )?;
    for m in boxes {
        stmt.execute(params![
            account_id,
            m.id,
            m.name,
            m.role,
            m.parent_id,
            m.sort_order,
            m.total_emails,
            m.unread_emails
        ])?;
    }
    Ok(())
}

impl Store {
    pub fn upsert_mailboxes(&self, account_id: &str, boxes: &[Mailbox]) -> Result<()> {
        self.write(|c| upsert(c, account_id, boxes))
    }

    pub fn delete_mailboxes(&self, account_id: &str, ids: &[String]) -> Result<()> {
        self.write(|c| {
            let mut stmt = c.prepare_cached("DELETE FROM mailboxes WHERE account_id = ?1 AND id = ?2")?;
            // The window goes with its mailbox: a later mailbox with the same id starts clean.
            let mut window =
                c.prepare_cached("DELETE FROM mailbox_windows WHERE account_id = ?1 AND mailbox_id = ?2")?;
            // So does the choice to notify: a later folder with the same id starts switched off.
            let mut notify =
                c.prepare_cached("DELETE FROM mailbox_notify WHERE account_id = ?1 AND mailbox_id = ?2")?;
            for id in ids {
                stmt.execute([account_id, id])?;
                window.execute([account_id, id])?;
                notify.execute([account_id, id])?;
            }
            Ok(())
        })
    }

    /// Ordered by `sort_order`, then name.
    pub fn list_mailboxes(&self, account_id: &str) -> Result<Vec<Mailbox>> {
        self.read(|c| {
            let sql =
                format!("SELECT {COLUMNS} FROM {FROM} WHERE m.account_id = ?1 ORDER BY m.sort_order, m.name, m.id");
            let mut stmt = c.prepare(&sql)?;
            let rows = stmt.query_map([account_id], from_row)?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    /// `role` in lower case, e.g. "inbox".
    pub fn mailbox_by_role(&self, account_id: &str, role: &str) -> Result<Option<Mailbox>> {
        self.read(|c| {
            let sql = format!("SELECT {COLUMNS} FROM {FROM} WHERE m.account_id = ?1 AND m.role = ?2 LIMIT 1");
            Ok(c.query_row(&sql, [account_id, role], from_row).optional()?)
        })
    }

    pub fn get_mailbox(&self, account_id: &str, id: &str) -> Result<Option<Mailbox>> {
        self.read(|c| {
            let sql = format!("SELECT {COLUMNS} FROM {FROM} WHERE m.account_id = ?1 AND m.id = ?2");
            Ok(c.query_row(&sql, [account_id, id], from_row).optional()?)
        })
    }

    /// Full resync: drops every mailbox of the account and stores `boxes`, in one transaction.
    /// The windows and the notify choices of the mailboxes that are still there are kept; the
    /// others go.
    pub fn replace_mailboxes(&self, account_id: &str, boxes: &[Mailbox]) -> Result<()> {
        self.write(|c| {
            c.execute("DELETE FROM mailboxes WHERE account_id = ?1", [account_id])?;
            upsert(c, account_id, boxes)?;
            c.execute(
                "DELETE FROM mailbox_windows WHERE account_id = ?1
                 AND mailbox_id NOT IN (SELECT id FROM mailboxes WHERE account_id = ?1)",
                [account_id],
            )?;
            c.execute(
                "DELETE FROM mailbox_notify WHERE account_id = ?1
                 AND mailbox_id NOT IN (SELECT id FROM mailboxes WHERE account_id = ?1)",
                [account_id],
            )?;
            Ok(())
        })
    }

    /// Switches a folder's notifications on (from `since`, Unix seconds) or off. Switching on a
    /// folder that is already on keeps its original `since`, so mail that came in meanwhile
    /// is not forgotten. A folder the cache does not know is `Ok(false)` and nothing changes.
    pub fn set_mailbox_notify(&self, account_id: &str, mailbox_id: &str, on: bool, since: i64) -> Result<bool> {
        self.write(|c| {
            let known = c
                .query_row(
                    "SELECT 1 FROM mailboxes WHERE account_id = ?1 AND id = ?2",
                    [account_id, mailbox_id],
                    |_| Ok(()),
                )
                .optional()?
                .is_some();
            if !known {
                return Ok(false);
            }
            if on {
                c.execute(
                    "INSERT OR IGNORE INTO mailbox_notify (account_id, mailbox_id, since) VALUES (?1, ?2, ?3)",
                    params![account_id, mailbox_id, since],
                )?;
            } else {
                c.execute(
                    "DELETE FROM mailbox_notify WHERE account_id = ?1 AND mailbox_id = ?2",
                    [account_id, mailbox_id],
                )?;
            }
            Ok(true)
        })
    }

    /// The newest `received_at` among the cached messages of the folder, if it holds any.
    pub fn newest_received_in(&self, account_id: &str, mailbox_id: &str) -> Result<Option<i64>> {
        self.read(|c| {
            Ok(c.query_row(
                "SELECT MAX(m.received_at) FROM message_mailboxes mm
                 JOIN messages m ON m.account_id = mm.account_id AND m.id = mm.message_id
                 WHERE mm.account_id = ?1 AND mm.mailbox_id = ?2",
                [account_id, mailbox_id],
                |r| r.get::<_, Option<i64>>(0),
            )?)
        })
    }

    /// When the folder was switched on, if it is.
    pub fn mailbox_notify_since(&self, account_id: &str, mailbox_id: &str) -> Result<Option<i64>> {
        self.read(|c| {
            Ok(c.query_row(
                "SELECT since FROM mailbox_notify WHERE account_id = ?1 AND mailbox_id = ?2",
                [account_id, mailbox_id],
                |r| r.get(0),
            )
            .optional()?)
        })
    }

    /// A folder was deleted on the server: it goes from the cache with its window and its notify
    /// choice. Its cached messages leave it; one that was only in it goes to `trash` (its mail
    /// was moved there first) or, with no `trash`, is dropped.
    pub fn forget_mailbox(&self, account_id: &str, mailbox_id: &str, trash: Option<&str>) -> Result<()> {
        self.write(|c| {
            let inside: Vec<String> = c
                .prepare("SELECT message_id FROM message_mailboxes WHERE account_id = ?1 AND mailbox_id = ?2")?
                .query_map([account_id, mailbox_id], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?;
            for id in &inside {
                c.execute(
                    "DELETE FROM message_mailboxes WHERE account_id = ?1 AND message_id = ?2 AND mailbox_id = ?3",
                    [account_id, id, mailbox_id],
                )?;
                let left: i64 = c.query_row(
                    "SELECT COUNT(*) FROM message_mailboxes WHERE account_id = ?1 AND message_id = ?2",
                    [account_id, id],
                    |r| r.get(0),
                )?;
                match (left, trash) {
                    (0, Some(trash)) => {
                        c.execute(
                            "INSERT OR IGNORE INTO message_mailboxes (account_id, message_id, mailbox_id)
                             VALUES (?1, ?2, ?3)",
                            [account_id, id, trash],
                        )?;
                    }
                    (0, None) => {
                        c.execute("DELETE FROM messages WHERE account_id = ?1 AND id = ?2", [account_id, id])?;
                    }
                    _ => {}
                }
            }
            for table in ["mailboxes", "mailbox_windows", "mailbox_notify"] {
                // The table name is one of our three literals, never user input.
                let column = if table == "mailboxes" { "id" } else { "mailbox_id" };
                c.execute(
                    &format!("DELETE FROM {table} WHERE account_id = ?1 AND {column} = ?2"),
                    [account_id, mailbox_id],
                )?;
            }
            Ok(())
        })
    }
}
