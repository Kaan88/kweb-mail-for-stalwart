//! Conversation lists, a thread's messages and search over the cached headers.

use super::messages::{query_headers, COLUMNS};
use super::Store;
use crate::model::{ListFilter, MessageHeader, ThreadRow};
use crate::Result;
use rusqlite::params;
use std::collections::HashMap;

/// The cached messages of one mailbox with the key of their thread (a message without a
/// thread id is its own thread). Takes `?1` = account id and `?2` = mailbox id.
const CANDIDATES: &str = "
    candidates AS (
        SELECT m.id, m.from_name, m.from_email, m.received_at, m.is_unread, m.is_flagged, m.has_attachment,
               CASE WHEN m.thread_id = '' THEN 'm:' || m.id ELSE 't:' || m.thread_id END AS k
        FROM messages m
        JOIN message_mailboxes mm ON mm.account_id = m.account_id AND mm.message_id = m.id
        WHERE m.account_id = ?1 AND mm.mailbox_id = ?2
    )";

/// Where the mailbox's window ends; the smallest number when the whole mailbox is cached.
const WINDOW: &str = "COALESCE(
    (SELECT oldest FROM mailbox_windows w WHERE w.account_id = ?1 AND w.mailbox_id = ?2), -9223372036854775808)";

fn filter_code(filter: ListFilter) -> u32 {
    match filter {
        ListFilter::All => 0,
        ListFilter::Unread => 1,
        ListFilter::Flagged => 2,
        ListFilter::Attachments => 3,
    }
}

/// What one thread of the page is made of, from the members query.
#[derive(Default)]
struct Members {
    message_ids: Vec<String>,
    senders: Vec<String>,
}

/// Escapes `%`, `_` and `\` so the text is matched literally by `LIKE ... ESCAPE '\'`.
fn like_pattern(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('%');
    for c in text.chars() {
        if matches!(c, '%' | '_' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('%');
    out
}

impl Store {
    /// The conversations of a mailbox, newest first by their newest message (ties by id, so
    /// paging is stable). A conversation is listed when its newest message is inside the
    /// mailbox's window: older ones would leave gaps in the list. A listed row is made of every
    /// cached message of the thread in this mailbox, also those older than the window, so an
    /// action on the row reaches all of them. `filter` is applied to the whole thread before
    /// `limit` and `offset`.
    pub fn list_threads(
        &self,
        account_id: &str,
        mailbox_id: &str,
        filter: ListFilter,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<ThreadRow>> {
        self.read(|c| {
            // The newest message of each thread of the page, and what the whole thread adds up to.
            let page_sql = format!(
                "WITH {CANDIDATES},
                 ranked AS (
                    SELECT k, id, received_at,
                           ROW_NUMBER() OVER (PARTITION BY k ORDER BY received_at DESC, id DESC) AS rn,
                           SUM(is_unread) OVER (PARTITION BY k) AS unread,
                           MAX(is_flagged) OVER (PARTITION BY k) AS flagged,
                           MAX(has_attachment) OVER (PARTITION BY k) AS attached
                    FROM candidates
                 )
                 SELECT k, id, unread, flagged, attached FROM ranked
                 WHERE rn = 1 AND received_at >= {WINDOW} AND CASE ?3 WHEN 1 THEN unread > 0 WHEN 2 THEN flagged = 1
                                          WHEN 3 THEN attached = 1 ELSE 1 END
                 ORDER BY received_at DESC, id DESC LIMIT ?4 OFFSET ?5"
            );
            let page: Vec<(String, String, u32, bool, bool)> = c
                .prepare(&page_sql)?
                .query_map(params![account_id, mailbox_id, filter_code(filter), limit, offset], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
                })?
                .collect::<rusqlite::Result<_>>()?;
            if page.is_empty() {
                return Ok(Vec::new());
            }

            let keys = serde_json::to_string(&page.iter().map(|p| &p.0).collect::<Vec<_>>())
                .map_err(|e| crate::Error::store(e.to_string()))?;
            let newest_ids = serde_json::to_string(&page.iter().map(|p| &p.1).collect::<Vec<_>>())
                .map_err(|e| crate::Error::store(e.to_string()))?;

            // Every member of those threads, oldest first.
            let members_sql = format!(
                "WITH {CANDIDATES}
                 SELECT k, id, from_name, from_email FROM candidates
                 WHERE k IN (SELECT value FROM json_each(?3)) ORDER BY received_at, id"
            );
            let mut members: HashMap<String, Members> = HashMap::new();
            let mut stmt = c.prepare(&members_sql)?;
            let rows = stmt.query_map(params![account_id, mailbox_id, keys], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?))
            })?;
            for row in rows {
                let (key, id, name, email) = row?;
                let entry = members.entry(key).or_default();
                entry.message_ids.push(id);
                let sender = if name.is_empty() { email } else { name };
                if !sender.is_empty() && !entry.senders.contains(&sender) {
                    entry.senders.push(sender);
                }
            }

            let newest_sql = format!(
                "SELECT {COLUMNS} FROM messages m WHERE m.account_id = ?1 AND m.id IN (SELECT value FROM json_each(?2))"
            );
            let mut newest: HashMap<String, MessageHeader> =
                query_headers(c, &newest_sql, params![account_id, newest_ids], account_id)?
                    .into_iter()
                    .map(|h| (h.id.clone(), h))
                    .collect();

            Ok(page
                .into_iter()
                .filter_map(|(key, id, unread, flagged, attached)| {
                    let Members { message_ids, senders } = members.remove(&key)?;
                    Some(ThreadRow {
                        newest: newest.remove(&id)?,
                        message_ids,
                        unread_count: unread,
                        is_flagged: flagged,
                        has_attachment: attached,
                        senders,
                    })
                })
                .collect())
        })
    }

    /// Every cached message of a thread, in any mailbox, oldest first. An empty thread id
    /// (a message the server gave none) has no thread: nothing.
    pub fn thread_messages(&self, account_id: &str, thread_id: &str) -> Result<Vec<MessageHeader>> {
        if thread_id.is_empty() {
            return Ok(Vec::new());
        }
        self.read(|c| {
            let sql = format!(
                "SELECT {COLUMNS} FROM messages m WHERE m.account_id = ?1 AND m.thread_id = ?2
                 ORDER BY m.received_at, m.id"
            );
            query_headers(c, &sql, [account_id, thread_id], account_id)
        })
    }

    /// Messages whose sender, subject or preview contains `query`, newest first, in one
    /// mailbox or in all. Upper and lower case match only for the letters A to Z (SQLite's
    /// `LIKE`); the server's search is the thorough one.
    pub fn search_messages(
        &self,
        account_id: &str,
        query: &str,
        mailbox_id: Option<&str>,
        limit: u32,
    ) -> Result<Vec<MessageHeader>> {
        self.read(|c| {
            let sql = format!(
                "SELECT {COLUMNS} FROM messages m
                 WHERE m.account_id = ?1
                   AND (?3 IS NULL OR EXISTS (SELECT 1 FROM message_mailboxes mm
                        WHERE mm.account_id = m.account_id AND mm.message_id = m.id AND mm.mailbox_id = ?3))
                   AND (m.subject LIKE ?2 ESCAPE '\\' OR m.from_name LIKE ?2 ESCAPE '\\'
                        OR m.from_email LIKE ?2 ESCAPE '\\' OR m.preview LIKE ?2 ESCAPE '\\')
                 ORDER BY m.received_at DESC, m.id DESC LIMIT ?4"
            );
            query_headers(c, &sql, params![account_id, like_pattern(query), mailbox_id, limit], account_id)
        })
    }
}
