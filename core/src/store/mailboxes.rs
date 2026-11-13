//! Mailbox rows.

use super::Store;
use crate::model::Mailbox;
use crate::Result;
use rusqlite::{params, Connection, OptionalExtension, Row};

const COLUMNS: &str = "id, name, role, parent_id, sort_order, total_emails, unread_emails";

fn from_row(r: &Row) -> rusqlite::Result<Mailbox> {
    Ok(Mailbox {
        id: r.get(0)?,
        name: r.get(1)?,
        role: r.get(2)?,
        parent_id: r.get(3)?,
        sort_order: r.get(4)?,
        total_emails: r.get(5)?,
        unread_emails: r.get(6)?,
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
            for id in ids {
                stmt.execute([account_id, id])?;
            }
            Ok(())
        })
    }

    /// Ordered by `sort_order`, then name.
    pub fn list_mailboxes(&self, account_id: &str) -> Result<Vec<Mailbox>> {
        self.read(|c| {
            let sql = format!("SELECT {COLUMNS} FROM mailboxes WHERE account_id = ?1 ORDER BY sort_order, name, id");
            let mut stmt = c.prepare(&sql)?;
            let rows = stmt.query_map([account_id], from_row)?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    /// `role` in lower case, e.g. "inbox".
    pub fn mailbox_by_role(&self, account_id: &str, role: &str) -> Result<Option<Mailbox>> {
        self.read(|c| {
            let sql = format!("SELECT {COLUMNS} FROM mailboxes WHERE account_id = ?1 AND role = ?2 LIMIT 1");
            Ok(c.query_row(&sql, [account_id, role], from_row).optional()?)
        })
    }

    pub fn get_mailbox(&self, account_id: &str, id: &str) -> Result<Option<Mailbox>> {
        self.read(|c| {
            let sql = format!("SELECT {COLUMNS} FROM mailboxes WHERE account_id = ?1 AND id = ?2");
            Ok(c.query_row(&sql, [account_id, id], from_row).optional()?)
        })
    }

    /// Full resync: drops every mailbox of the account and stores `boxes`, in one transaction.
    pub fn replace_mailboxes(&self, account_id: &str, boxes: &[Mailbox]) -> Result<()> {
        self.write(|c| {
            c.execute("DELETE FROM mailboxes WHERE account_id = ?1", [account_id])?;
            upsert(c, account_id, boxes)
        })
    }
}
