//! Message headers and their mailbox membership.

use super::Store;
use crate::model::{Address, MessageHeader};
use crate::Result;
use rusqlite::{params, Connection, OptionalExtension, Row};
use std::collections::HashSet;

/// Column list shared by every query that returns headers; `mailbox_ids` is filled separately.
/// Callers that join must qualify the table as `m`.
pub(super) const COLUMNS: &str = "m.id, m.thread_id, m.from_name, m.from_email, m.subject, m.preview, \
     m.received_at, m.is_unread, m.is_flagged, m.has_attachment, m.size, m.to_json";

pub(super) fn from_row(r: &Row) -> rusqlite::Result<MessageHeader> {
    Ok(MessageHeader {
        id: r.get(0)?,
        thread_id: r.get(1)?,
        mailbox_ids: Vec::new(),
        from_name: r.get(2)?,
        from_email: r.get(3)?,
        // An unreadable value is no recipients: the next sync writes it again.
        to: serde_json::from_str::<Vec<Address>>(&r.get::<_, String>(11)?).unwrap_or_default(),
        subject: r.get(4)?,
        preview: r.get(5)?,
        received_at: r.get(6)?,
        is_unread: r.get(7)?,
        is_flagged: r.get(8)?,
        has_attachment: r.get(9)?,
        size: r.get::<_, i64>(10)? as u64,
    })
}

/// Runs a header query and attaches the mailbox ids to each row.
pub(super) fn query_headers(
    c: &Connection,
    sql: &str,
    params: impl rusqlite::Params,
    account_id: &str,
) -> Result<Vec<MessageHeader>> {
    let mut list: Vec<MessageHeader> = c.prepare(sql)?.query_map(params, from_row)?.collect::<rusqlite::Result<_>>()?;
    let mut stmt = c.prepare_cached(
        "SELECT mailbox_id FROM message_mailboxes WHERE account_id = ?1 AND message_id = ?2 ORDER BY mailbox_id",
    )?;
    for m in &mut list {
        m.mailbox_ids = stmt.query_map([account_id, &m.id], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
    }
    Ok(list)
}

fn set_mailboxes(c: &Connection, account_id: &str, message_id: &str, mailbox_ids: &[String]) -> Result<()> {
    c.execute("DELETE FROM message_mailboxes WHERE account_id = ?1 AND message_id = ?2", [account_id, message_id])?;
    let mut ins = c.prepare_cached(
        "INSERT OR IGNORE INTO message_mailboxes (account_id, message_id, mailbox_id) VALUES (?1, ?2, ?3)",
    )?;
    for mb in mailbox_ids {
        ins.execute([account_id, message_id, mb])?;
    }
    Ok(())
}

fn upsert(c: &Connection, account_id: &str, msgs: &[MessageHeader]) -> Result<()> {
    let mut stmt = c.prepare_cached(
        "INSERT INTO messages (account_id, id, thread_id, from_name, from_email, subject, preview,
             received_at, is_unread, is_flagged, has_attachment, size, to_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
         ON CONFLICT(account_id, id) DO UPDATE SET thread_id = ?3, from_name = ?4, from_email = ?5,
             subject = ?6, preview = ?7, received_at = ?8, is_unread = ?9, is_flagged = ?10,
             has_attachment = ?11, size = ?12, to_json = ?13",
    )?;
    for m in msgs {
        let to_json = serde_json::to_string(&m.to).map_err(|e| crate::Error::store(e.to_string()))?;
        stmt.execute(params![
            account_id,
            m.id,
            m.thread_id,
            m.from_name,
            m.from_email,
            m.subject,
            m.preview,
            m.received_at,
            m.is_unread,
            m.is_flagged,
            m.has_attachment,
            m.size as i64,
            to_json
        ])?;
        set_mailboxes(c, account_id, &m.id, &m.mailbox_ids)?;
    }
    Ok(())
}

impl Store {
    /// Inserts or updates; the mailbox membership of each message is replaced by `mailbox_ids`.
    pub fn upsert_messages(&self, account_id: &str, msgs: &[MessageHeader]) -> Result<()> {
        self.write(|c| upsert(c, account_id, msgs))
    }

    /// Deletes only what the server said is gone; nothing is deleted just for having no mailbox.
    pub fn delete_messages(&self, account_id: &str, ids: &[String]) -> Result<()> {
        self.write(|c| {
            let mut stmt = c.prepare_cached("DELETE FROM messages WHERE account_id = ?1 AND id = ?2")?;
            for id in ids {
                stmt.execute([account_id, id])?;
            }
            Ok(())
        })
    }

    pub fn get_message(&self, account_id: &str, id: &str) -> Result<Option<MessageHeader>> {
        self.read(|c| {
            let sql = format!("SELECT {COLUMNS} FROM messages m WHERE m.account_id = ?1 AND m.id = ?2");
            Ok(query_headers(c, &sql, [account_id, id], account_id)?.pop())
        })
    }

    /// Newest first (ties broken by id so paging is stable).
    pub fn list_messages(
        &self,
        account_id: &str,
        mailbox_id: &str,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<MessageHeader>> {
        self.read(|c| {
            let sql = format!(
                "SELECT {COLUMNS} FROM messages m
                 JOIN message_mailboxes mm ON mm.account_id = m.account_id AND mm.message_id = m.id
                 WHERE m.account_id = ?1 AND mm.mailbox_id = ?2
                 ORDER BY m.received_at DESC, m.id DESC LIMIT ?3 OFFSET ?4"
            );
            query_headers(c, &sql, params![account_id, mailbox_id, limit, offset], account_id)
        })
    }

    pub fn count_unread(&self, account_id: &str, mailbox_id: &str) -> Result<u32> {
        self.read(|c| {
            Ok(c.query_row(
                "SELECT COUNT(*) FROM messages m
                 JOIN message_mailboxes mm ON mm.account_id = m.account_id AND mm.message_id = m.id
                 WHERE m.account_id = ?1 AND mm.mailbox_id = ?2 AND m.is_unread = 1",
                [account_id, mailbox_id],
                |r| r.get(0),
            )?)
        })
    }

    /// Returns false when the message is not cached (nothing changed).
    pub fn set_unread(&self, account_id: &str, id: &str, unread: bool) -> Result<bool> {
        self.update_flag("is_unread", account_id, id, unread)
    }

    /// Returns false when the message is not cached (nothing changed).
    pub fn set_flagged(&self, account_id: &str, id: &str, flagged: bool) -> Result<bool> {
        self.update_flag("is_flagged", account_id, id, flagged)
    }

    fn update_flag(&self, column: &str, account_id: &str, id: &str, value: bool) -> Result<bool> {
        self.write(|c| {
            // `column` is one of our two literals, never user input.
            let sql = format!("UPDATE messages SET {column} = ?3 WHERE account_id = ?1 AND id = ?2");
            Ok(c.execute(&sql, params![account_id, id, value])? > 0)
        })
    }

    /// Replaces the mailbox membership. Returns false when the message is not cached.
    pub fn set_message_mailboxes(&self, account_id: &str, id: &str, mailbox_ids: &[String]) -> Result<bool> {
        self.write(|c| {
            let exists = c
                .query_row("SELECT 1 FROM messages WHERE account_id = ?1 AND id = ?2", [account_id, id], |_| Ok(()))
                .optional()?
                .is_some();
            if exists {
                set_mailboxes(c, account_id, id, mailbox_ids)?;
            }
            Ok(exists)
        })
    }

    /// Full resync: stores `msgs` and drops the account's other messages, in one transaction.
    /// A message that is in `msgs` keeps its cached body. Shown and dismissed notification rows
    /// are kept: they describe what the user saw.
    pub fn replace_messages(&self, account_id: &str, msgs: &[MessageHeader]) -> Result<()> {
        self.write(|c| {
            let keep: HashSet<&str> = msgs.iter().map(|m| m.id.as_str()).collect();
            let cached: Vec<String> = c
                .prepare("SELECT id FROM messages WHERE account_id = ?1")?
                .query_map([account_id], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?;
            let mut delete = c.prepare_cached("DELETE FROM messages WHERE account_id = ?1 AND id = ?2")?;
            for id in cached.iter().filter(|id| !keep.contains(id.as_str())) {
                delete.execute([account_id, id])?;
            }
            upsert(c, account_id, msgs)
        })
    }
}
