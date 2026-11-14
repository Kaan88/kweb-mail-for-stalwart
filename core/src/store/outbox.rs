//! Actions queued while offline. Stored as a kind string, a message id and, for a move, the
//! target mailbox, so a new kind needs no schema change.

use super::Store;
use crate::model::Action;
use crate::Result;

/// What the queue holds: an action, or a spam report. The report is not an `Action` because it
/// sets keywords as well as moving the message, and `Action` is part of the app's interface.
#[derive(Debug, Clone, PartialEq)]
pub enum Job {
    Action(Action),
    /// `is_spam`: to Junk with `$junk`; otherwise back to the Inbox with `$notjunk`.
    Spam {
        message_id: String,
        is_spam: bool,
    },
}

impl Job {
    pub fn message_id(&self) -> &str {
        match self {
            Job::Action(action) => action_message_id(action),
            Job::Spam { message_id, .. } => message_id,
        }
    }
}

fn action_message_id(action: &Action) -> &str {
    match action {
        Action::MarkRead { message_id }
        | Action::MarkUnread { message_id }
        | Action::Archive { message_id }
        | Action::Delete { message_id }
        | Action::Destroy { message_id }
        | Action::Flag { message_id }
        | Action::Unflag { message_id }
        | Action::Move { message_id, .. } => message_id,
    }
}

/// `(kind, message id, target mailbox)`.
fn job_columns(job: &Job) -> (&'static str, &str, Option<&str>) {
    match job {
        Job::Action(action) => to_columns(action),
        Job::Spam { message_id, is_spam: true } => ("spam", message_id, None),
        Job::Spam { message_id, is_spam: false } => ("not_spam", message_id, None),
    }
}

/// `(kind, message id, target mailbox)`.
fn to_columns(action: &Action) -> (&'static str, &str, Option<&str>) {
    match action {
        Action::MarkRead { message_id } => ("mark_read", message_id, None),
        Action::MarkUnread { message_id } => ("mark_unread", message_id, None),
        Action::Archive { message_id } => ("archive", message_id, None),
        Action::Delete { message_id } => ("delete", message_id, None),
        Action::Destroy { message_id } => ("destroy", message_id, None),
        Action::Flag { message_id } => ("flag", message_id, None),
        Action::Unflag { message_id } => ("unflag", message_id, None),
        Action::Move { message_id, mailbox_id } => ("move", message_id, Some(mailbox_id)),
    }
}

/// `None` for a kind this version does not know, and for a move that lost its target.
fn job_from_columns(kind: &str, message_id: String, target: Option<String>) -> Option<Job> {
    match kind {
        "spam" => Some(Job::Spam { message_id, is_spam: true }),
        "not_spam" => Some(Job::Spam { message_id, is_spam: false }),
        _ => from_columns(kind, message_id, target).map(Job::Action),
    }
}

/// `None` for a kind this version does not know, and for a move that lost its target.
fn from_columns(kind: &str, message_id: String, target: Option<String>) -> Option<Action> {
    Some(match kind {
        "mark_read" => Action::MarkRead { message_id },
        "mark_unread" => Action::MarkUnread { message_id },
        "archive" => Action::Archive { message_id },
        "delete" => Action::Delete { message_id },
        "destroy" => Action::Destroy { message_id },
        "flag" => Action::Flag { message_id },
        "unflag" => Action::Unflag { message_id },
        "move" => Action::Move { message_id, mailbox_id: target? },
        _ => return None,
    })
}

impl Store {
    /// Returns the row id, which stays stable until the action is marked done.
    pub fn outbox_enqueue(&self, account_id: &str, action: &Action) -> Result<i64> {
        self.outbox_enqueue_job(account_id, &Job::Action(action.clone()))
    }

    /// Like [`Store::outbox_enqueue`], for anything the queue holds.
    pub fn outbox_enqueue_job(&self, account_id: &str, job: &Job) -> Result<i64> {
        let (kind, message_id, target) = job_columns(job);
        self.write(|c| {
            c.execute(
                "INSERT INTO outbox (account_id, kind, message_id, target) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![account_id, kind, message_id, target],
            )?;
            Ok(c.last_insert_rowid())
        })
    }

    /// `(id, action, attempts)`, oldest first. A row with a kind this version does not know
    /// (written by a newer app), or a move without a target, is skipped rather than blocking
    /// the queue.
    pub fn outbox_pending(&self, account_id: &str) -> Result<Vec<(i64, Action, u32)>> {
        let jobs = self.outbox_jobs(account_id)?;
        Ok(jobs
            .into_iter()
            .filter_map(|(id, job, attempts)| match job {
                Job::Action(action) => Some((id, action, attempts)),
                Job::Spam { .. } => None,
            })
            .collect())
    }

    /// Everything queued, as [`Store::outbox_pending`] but with the spam reports too: `(id, job,
    /// attempts)`, oldest first.
    pub fn outbox_jobs(&self, account_id: &str) -> Result<Vec<(i64, Job, u32)>> {
        self.read(|c| {
            let mut stmt = c.prepare(
                "SELECT id, kind, message_id, attempts, target FROM outbox WHERE account_id = ?1 ORDER BY id",
            )?;
            let rows = stmt.query_map([account_id], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, u32>(3)?,
                    r.get(4)?,
                ))
            })?;
            let mut out = Vec::new();
            for row in rows {
                let (id, kind, message_id, attempts, target) = row?;
                if let Some(job) = job_from_columns(&kind, message_id, target) {
                    out.push((id, job, attempts));
                }
            }
            Ok(out)
        })
    }

    /// The server has the action: remove it from the queue.
    pub fn outbox_done(&self, id: i64) -> Result<()> {
        self.write(|c| {
            c.execute("DELETE FROM outbox WHERE id = ?1", [id])?;
            Ok(())
        })
    }

    /// Counts one more failed try and keeps the last error text for the Delivery check screen.
    pub fn outbox_failed(&self, id: i64, error: &str) -> Result<()> {
        self.write(|c| {
            c.execute(
                "UPDATE outbox SET attempts = attempts + 1, last_error = ?2 WHERE id = ?1",
                rusqlite::params![id, error],
            )?;
            Ok(())
        })
    }

    /// The last error recorded for a queued action, if any.
    pub fn outbox_last_error(&self, id: i64) -> Result<Option<String>> {
        use rusqlite::OptionalExtension;
        self.read(|c| {
            Ok(c.query_row("SELECT last_error FROM outbox WHERE id = ?1", [id], |r| r.get(0)).optional()?.flatten())
        })
    }
}
