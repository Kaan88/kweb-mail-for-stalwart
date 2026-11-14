//! Drafts, sender identities and the address book.
//!
//! A draft row is the user's text (`Draft` as JSON) plus what the sending machinery needs:
//! where it is in its life, whether the server copy is current, and the ids a reply must
//! carry. The user's calls change a row in one statement or one transaction; the pass reads a
//! row, works on the network without any lock, and writes back conditionally (see
//! [`Store::draft_saved`]), so an edit made meanwhile is never lost.

use std::time::Instant;

use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};

use super::Store;
use crate::{
    model::{Address, Attachment, Draft, Identity, PendingSend},
    Error, Result,
};

/// Where a draft is in its life.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DraftState {
    Draft,
    /// Held back until `send_at`; the user may still take it back.
    Sending,
    /// The request that sends it was started. If its answer was lost, the pass finds out from
    /// the server copy whether it went.
    Submitting,
}

impl DraftState {
    fn column(self) -> &'static str {
        match self {
            DraftState::Draft => "draft",
            DraftState::Sending => "sending",
            DraftState::Submitting => "submitting",
        }
    }

    fn from_column(text: &str) -> DraftState {
        match text {
            "sending" => DraftState::Sending,
            "submitting" => DraftState::Submitting,
            _ => DraftState::Draft,
        }
    }
}

/// The Message-IDs (without angle brackets) a message that answers another carries.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Threading {
    pub in_reply_to: Vec<String>,
    pub references: Vec<String>,
}

/// A stored draft with what the `Draft` does not carry.
#[derive(Debug, Clone, PartialEq)]
pub struct DraftRow {
    /// `server_id` and `send_error` are the columns' values.
    pub draft: Draft,
    pub state: DraftState,
    pub send_at: Option<i64>,
    /// The server copy is not current.
    pub dirty: bool,
    pub discarded: bool,
    /// Counts the user's edits.
    pub rev: i64,
    /// `None` until looked up.
    pub threading: Option<Threading>,
}

const COLUMNS: &str = "draft, server_id, send_error, state, send_at, dirty, discarded, rev, threading";

fn json_error(e: serde_json::Error) -> Error {
    Error::store(e.to_string())
}

/// An unreadable row (written by another version of the app) is skipped: `None`.
fn from_row(r: &Row) -> rusqlite::Result<Option<DraftRow>> {
    let json: String = r.get(0)?;
    let Ok(mut draft) = serde_json::from_str::<Draft>(&json) else { return Ok(None) };
    draft.server_id = r.get(1)?;
    draft.send_error = r.get(2)?;
    let threading: Option<String> = r.get(8)?;
    Ok(Some(DraftRow {
        draft,
        state: DraftState::from_column(&r.get::<_, String>(3)?),
        send_at: r.get(4)?,
        dirty: r.get(5)?,
        discarded: r.get(6)?,
        rev: r.get(7)?,
        threading: threading.and_then(|t| serde_json::from_str(&t).ok()),
    }))
}

fn load(c: &Connection, account_id: &str, id: &str) -> Result<Option<DraftRow>> {
    let sql = format!("SELECT {COLUMNS} FROM drafts WHERE account_id = ?1 AND id = ?2");
    Ok(c.query_row(&sql, [account_id, id], from_row).optional()?.flatten())
}

/// The rows of the account that meet `condition` (SQL of ours, never user input).
fn load_all(c: &Connection, account_id: &str, condition: &str, order: &str) -> Result<Vec<DraftRow>> {
    let sql = format!("SELECT {COLUMNS} FROM drafts WHERE account_id = ?1 AND {condition} ORDER BY {order}");
    let rows = c.prepare(&sql)?.query_map([account_id], from_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows.into_iter().flatten().collect())
}

/// What a write of the user's text does to the rest of the row.
struct Write<'a> {
    state: DraftState,
    send_at: Option<i64>,
    /// `Some` replaces the stored error (a send clears it); `None` keeps it.
    send_error: Option<Option<&'a str>>,
}

/// Stores the user's version of a draft, marks it for the server and counts the edit. The
/// stored `server_id` stays: the pass owns it, and the caller's copy may be older. A draft that
/// was thrown away or is already on its way out cannot be edited.
///
/// It never makes a row: every draft is born in `draft_insert` (a new message, a reply, an
/// opened server draft). A draft the cache does not have was sent or thrown away, and a late
/// save or a second tap on Send with the screen's old copy must not bring it back, or the
/// message would go out twice.
fn write_user_draft(c: &Connection, account_id: &str, draft: &Draft, now: i64, w: Write) -> Result<()> {
    let Some(row) = load(c, account_id, &draft.id)? else {
        return Err(Error::invalid("this draft is gone: it was sent or thrown away"));
    };
    if row.discarded {
        return Err(Error::invalid("this draft was thrown away"));
    }
    if row.state != DraftState::Draft {
        return Err(Error::invalid("this message is being sent: undo the send to change it"));
    }
    let existing = Some(row);
    let mut stored = draft.clone();
    stored.updated_at = now;
    let json = serde_json::to_string(&stored).map_err(json_error)?;
    let error = match w.send_error {
        Some(new) => new.map(str::to_string),
        None => existing.as_ref().and_then(|row| row.draft.send_error.clone()),
    };
    c.execute(
        "INSERT INTO drafts (account_id, id, draft, send_error, updated_at, state, send_at, dirty, rev)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1, 1)
         ON CONFLICT(account_id, id) DO UPDATE SET draft = ?3, send_error = ?4, updated_at = ?5, state = ?6,
             send_at = ?7, dirty = 1, rev = rev + 1",
        params![account_id, draft.id, json, error, now, w.state.column(), w.send_at],
    )?;
    Ok(())
}

impl Store {
    /// Stores a draft that was just made (a reply, a forward, an opened server draft). The server
    /// copy is current when `dirty` is false (an opened one) and due when it is true.
    pub fn draft_insert(
        &self,
        account_id: &str,
        draft: &Draft,
        threading: Option<&Threading>,
        dirty: bool,
    ) -> Result<()> {
        let json = serde_json::to_string(draft).map_err(json_error)?;
        let threading = threading.map(serde_json::to_string).transpose().map_err(json_error)?;
        self.write(|c| {
            c.execute(
                "INSERT INTO drafts (account_id, id, draft, server_id, updated_at, dirty, threading)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![account_id, draft.id, json, draft.server_id, draft.updated_at, dirty, threading],
            )?;
            Ok(())
        })
    }

    /// The user's edit. Returns the draft as stored.
    pub fn draft_save(&self, account_id: &str, draft: &Draft, now: i64) -> Result<Draft> {
        self.write(|c| {
            let w = Write { state: DraftState::Draft, send_at: None, send_error: None };
            write_user_draft(c, account_id, draft, now, w)?;
            Ok(load(c, account_id, &draft.id)?.expect("just written").draft)
        })
    }

    /// Stores the draft as handed over for sending at `send_at`. One that is already on its way
    /// stays as it is and its send is returned: a second tap must not move the time.
    pub fn draft_send(&self, account_id: &str, draft: &Draft, send_at: i64, now: i64) -> Result<PendingSend> {
        self.write(|c| {
            if let Some(row) = load(c, account_id, &draft.id)? {
                if !row.discarded && row.state != DraftState::Draft {
                    return Ok(pending(&row));
                }
            }
            let w = Write { state: DraftState::Sending, send_at: Some(send_at), send_error: Some(None) };
            write_user_draft(c, account_id, draft, now, w)?;
            Ok(pending(&load(c, account_id, &draft.id)?.expect("just written")))
        })
    }

    /// A draft that is not thrown away, in any state.
    pub fn draft_get(&self, account_id: &str, id: &str) -> Result<Option<DraftRow>> {
        self.read(|c| Ok(load(c, account_id, id)?.filter(|row| !row.discarded)))
    }

    /// The server copy of a draft that is not thrown away: the draft that was opened from it.
    pub fn draft_by_server_id(&self, account_id: &str, server_id: &str) -> Result<Option<Draft>> {
        self.read(|c| {
            let sql =
                format!("SELECT {COLUMNS} FROM drafts WHERE account_id = ?1 AND server_id = ?2 AND discarded = 0");
            Ok(c.query_row(&sql, [account_id, server_id], from_row).optional()?.flatten().map(|row| row.draft))
        })
    }

    /// The drafts that are not waiting to be sent, newest first.
    pub fn drafts_list(&self, account_id: &str) -> Result<Vec<Draft>> {
        self.read(|c| {
            let rows = load_all(c, account_id, "state = 'draft' AND discarded = 0", "updated_at DESC, id")?;
            Ok(rows.into_iter().map(|row| row.draft).collect())
        })
    }

    /// The messages that are held back or on their way, in the order they leave.
    pub fn pending_sends(&self, account_id: &str) -> Result<Vec<PendingSend>> {
        self.read(|c| {
            let rows = load_all(c, account_id, "state != 'draft' AND discarded = 0", "send_at, id")?;
            Ok(rows.iter().map(pending).collect())
        })
    }

    /// Throws a draft away: it is gone from every list at once and the pass removes its server
    /// copy and then the row. A draft that does not exist is already gone. One whose send
    /// request is under way cannot be stopped.
    pub fn draft_discard(&self, account_id: &str, id: &str) -> Result<()> {
        self.write(|c| {
            match load(c, account_id, id)? {
                Some(row) if row.state == DraftState::Submitting => {
                    return Err(Error::invalid("this message is already being sent"));
                }
                Some(_) => {
                    c.execute("UPDATE drafts SET discarded = 1 WHERE account_id = ?1 AND id = ?2", [account_id, id])?;
                }
                None => {}
            }
            Ok(())
        })
    }

    /// Takes a held-back message back as a draft. Only while it is still held back; the update
    /// is one statement, so it and the pass starting the send cannot both win.
    pub fn draft_undo_send(&self, account_id: &str, id: &str) -> Result<Draft> {
        self.write(|c| {
            let changed = c.execute(
                "UPDATE drafts SET state = 'draft', send_at = NULL
                 WHERE account_id = ?1 AND id = ?2 AND state = 'sending' AND discarded = 0",
                [account_id, id],
            )?;
            if changed == 0 {
                return Err(Error::invalid("this message cannot be taken back: it was sent, or is not being sent"));
            }
            Ok(load(c, account_id, id)?.expect("just updated").draft)
        })
    }

    /// Adds an uploaded file to a draft that is still being written.
    pub fn draft_add_attachment(&self, account_id: &str, id: &str, file: Attachment, now: i64) -> Result<Draft> {
        self.write(|c| {
            let row =
                load(c, account_id, id)?.filter(|row| !row.discarded).ok_or_else(|| Error::invalid("no such draft"))?;
            let mut draft = row.draft;
            draft.attachments.push(file);
            let w = Write { state: DraftState::Draft, send_at: None, send_error: None };
            write_user_draft(c, account_id, &draft, now, w)?;
            Ok(load(c, account_id, id)?.expect("just written").draft)
        })
    }

    // ---- what the pass uses ----

    /// Thrown-away drafts: `(id, server id)`.
    pub fn drafts_discarded(&self, account_id: &str) -> Result<Vec<(String, Option<String>)>> {
        self.read(|c| {
            let rows = load_all(c, account_id, "discarded = 1", "updated_at, id")?;
            Ok(rows.into_iter().map(|row| (row.draft.id, row.draft.server_id)).collect())
        })
    }

    /// Drafts being written whose server copy is not current, oldest edit first.
    pub fn drafts_to_save(&self, account_id: &str) -> Result<Vec<DraftRow>> {
        self.read(|c| load_all(c, account_id, "state = 'draft' AND dirty = 1 AND discarded = 0", "updated_at, id"))
    }

    /// Messages to send now: held back and due, or with a send under way that has to be settled.
    pub fn drafts_to_send(&self, account_id: &str, now: i64) -> Result<Vec<DraftRow>> {
        self.read(|c| {
            let due = format!("discarded = 0 AND (state = 'submitting' OR (state = 'sending' AND send_at <= {now}))");
            load_all(c, account_id, &due, "send_at, id")
        })
    }

    pub fn draft_delete(&self, account_id: &str, id: &str) -> Result<()> {
        self.write(|c| {
            c.execute("DELETE FROM drafts WHERE account_id = ?1 AND id = ?2", [account_id, id])?;
            Ok(())
        })
    }

    /// A server copy was made. It is remembered whatever happened to the row meanwhile; the row
    /// counts as saved only when it was not edited since `rev` was read.
    pub fn draft_saved(&self, account_id: &str, id: &str, server_id: &str, rev: i64) -> Result<()> {
        self.write(|c| {
            c.execute(
                "UPDATE drafts SET server_id = ?3, dirty = CASE WHEN rev = ?4 THEN 0 ELSE dirty END
                 WHERE account_id = ?1 AND id = ?2",
                params![account_id, id, server_id, rev],
            )?;
            Ok(())
        })
    }

    /// The server will not take a copy of this version of the draft. The text is safe here, so
    /// the row counts as saved until the user changes it, and the pass does not try again.
    pub fn draft_give_up(&self, account_id: &str, id: &str, rev: i64) -> Result<()> {
        self.write(|c| {
            c.execute(
                "UPDATE drafts SET dirty = 0 WHERE account_id = ?1 AND id = ?2 AND rev = ?3",
                params![account_id, id, rev],
            )?;
            Ok(())
        })
    }

    /// The server's copy is gone: the next save makes a new one, and a send under way starts
    /// again from the beginning.
    pub fn draft_copy_lost(&self, account_id: &str, id: &str) -> Result<()> {
        self.write(|c| {
            c.execute(
                "UPDATE drafts SET server_id = NULL, dirty = 1,
                     state = CASE WHEN state = 'submitting' THEN 'sending' ELSE state END
                 WHERE account_id = ?1 AND id = ?2",
                [account_id, id],
            )?;
            Ok(())
        })
    }

    pub fn draft_set_threading(&self, account_id: &str, id: &str, threading: &Threading) -> Result<()> {
        let json = serde_json::to_string(threading).map_err(json_error)?;
        self.write(|c| {
            c.execute(
                "UPDATE drafts SET threading = ?3 WHERE account_id = ?1 AND id = ?2",
                params![account_id, id, json],
            )?;
            Ok(())
        })
    }

    /// Marks the send as started. False when it was taken back, or is already under way.
    pub fn draft_begin_submit(&self, account_id: &str, id: &str, rev: i64, copy: &str) -> Result<bool> {
        self.write(|c| {
            // Only the very version the pass read and copied: an undo, an edit and a new send
            // made while the copy was on its way must not get the old text sent.
            let changed = c.execute(
                "UPDATE drafts SET state = 'submitting'
                 WHERE account_id = ?1 AND id = ?2 AND state = 'sending' AND discarded = 0
                   AND rev = ?3 AND dirty = 0 AND server_id = ?4",
                params![account_id, id, rev, copy],
            )?;
            Ok(changed > 0)
        })
    }

    /// The server refused the message for good: it is a draft again, with the reason.
    pub fn draft_refused(&self, account_id: &str, id: &str, error: &str) -> Result<()> {
        self.write(|c| {
            c.execute(
                "UPDATE drafts SET state = 'draft', send_at = NULL, send_error = ?3, dirty = 1
                 WHERE account_id = ?1 AND id = ?2",
                [account_id, id, error],
            )?;
            Ok(())
        })
    }

    // ---- identities ----

    /// The identities as the server last gave them.
    pub fn identities_list(&self, account_id: &str) -> Result<Vec<Identity>> {
        self.read(|c| {
            let mut stmt = c.prepare("SELECT identity FROM identities WHERE account_id = ?1 ORDER BY position")?;
            let rows = stmt.query_map([account_id], |r| r.get::<_, String>(0))?;
            let mut out = Vec::new();
            for json in rows {
                out.extend(serde_json::from_str(&json?).ok());
            }
            Ok(out)
        })
    }

    pub fn identities_replace(&self, account_id: &str, identities: &[Identity]) -> Result<()> {
        self.write(|c| {
            c.execute("DELETE FROM identities WHERE account_id = ?1", [account_id])?;
            let mut insert =
                c.prepare("INSERT INTO identities (account_id, position, identity) VALUES (?1, ?2, ?3)")?;
            for (position, identity) in identities.iter().enumerate() {
                let json = serde_json::to_string(identity).map_err(json_error)?;
                insert.execute(params![account_id, position as i64, json])?;
            }
            Ok(())
        })
    }

    // ---- address book ----

    /// `(name, address)` in the server's order.
    pub fn contacts_list(&self, account_id: &str) -> Result<Vec<Address>> {
        self.read(|c| {
            let mut stmt = c.prepare("SELECT name, email FROM contacts WHERE account_id = ?1 ORDER BY position")?;
            let rows = stmt.query_map([account_id], |r| Ok(Address { name: r.get(0)?, email: r.get(1)? }))?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    /// Replaces the address book. An address that comes twice (in any case) is kept once, with
    /// the name of its first card.
    pub fn contacts_replace(&self, account_id: &str, contacts: &[Address]) -> Result<()> {
        self.write(|c| {
            c.execute("DELETE FROM contacts WHERE account_id = ?1", [account_id])?;
            let mut insert = c.prepare(
                "INSERT OR IGNORE INTO contacts (account_id, email, name, position) VALUES (?1, ?2, ?3, ?4)",
            )?;
            for (position, contact) in contacts.iter().enumerate() {
                insert.execute(params![account_id, contact.email, contact.name, position as i64])?;
            }
            Ok(())
        })
    }

    /// The people in the newest `scan` cached messages, as the senders and then the recipients
    /// of each message, newest message first. An address may come more than once.
    pub fn people_in_mail(&self, account_id: &str, scan: u32) -> Result<Vec<Address>> {
        self.read(|c| {
            let mut stmt = c.prepare(
                "SELECT from_name, from_email, to_json FROM messages WHERE account_id = ?1
                 ORDER BY received_at DESC, id DESC LIMIT ?2",
            )?;
            let rows = stmt.query_map(params![account_id, scan], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
            })?;
            let mut out = Vec::new();
            for row in rows {
                let (name, email, to_json) = row?;
                if !email.is_empty() {
                    out.push(Address { name, email });
                }
                out.extend(serde_json::from_str::<Vec<Address>>(&to_json).unwrap_or_default());
            }
            Ok(out)
        })
    }

    // ---- the hourly refresh of identities and address book ----

    /// Whether the identities and the address book are due: never fetched in this process, or
    /// not for [`DIRECTORY_EVERY`].
    pub fn directory_due(&self, now: Instant) -> bool {
        directory_due(*self.directory.lock().unwrap_or_else(|e| e.into_inner()), now)
    }

    /// A refresh worked. A failed one is not recorded, so the next pass tries again.
    pub fn directory_done(&self, now: Instant) {
        *self.directory.lock().unwrap_or_else(|e| e.into_inner()) = Some(now);
    }
}

/// How often a running process fetches the identities and the address book again.
pub const DIRECTORY_EVERY: std::time::Duration = std::time::Duration::from_secs(3600);

fn directory_due(last: Option<Instant>, now: Instant) -> bool {
    last.is_none_or(|at| now.saturating_duration_since(at) >= DIRECTORY_EVERY)
}

fn pending(row: &DraftRow) -> PendingSend {
    PendingSend {
        draft_id: row.draft.id.clone(),
        subject: row.draft.subject.clone(),
        send_at: row.send_at.unwrap_or(0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn the_directory_is_due_first_and_then_hourly() {
        let start = Instant::now();
        assert!(directory_due(None, start));
        assert!(!directory_due(Some(start), start + Duration::from_secs(3599)));
        assert!(directory_due(Some(start), start + DIRECTORY_EVERY));
    }
}
