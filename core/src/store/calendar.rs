//! The calendar list and the agenda entries, kept so both can be shown offline.

use rusqlite::params;

use super::Store;
use crate::{
    model::{Calendar, Occurrence},
    Error, Result,
};

/// Whether an entry shows in the span `[from, to)`. An entry with no length is a point in time.
const OVERLAPS: &str = "starts_at < ?3 AND max(ends_at, starts_at + 1) > ?2";

fn json_error(e: serde_json::Error) -> Error {
    Error::store(e.to_string())
}

impl Store {
    /// The calendars as the server last gave them, in its order.
    pub fn calendars_list(&self, account_id: &str) -> Result<Vec<Calendar>> {
        self.read(|c| {
            let mut stmt = c.prepare("SELECT calendar FROM calendars WHERE account_id = ?1 ORDER BY position")?;
            let rows = stmt.query_map([account_id], |r| r.get::<_, String>(0))?;
            let mut out = Vec::new();
            for json in rows {
                // An unreadable row (written by another version of the app) is skipped.
                out.extend(serde_json::from_str(&json?).ok());
            }
            Ok(out)
        })
    }

    pub fn calendars_replace(&self, account_id: &str, calendars: &[Calendar]) -> Result<()> {
        self.write(|c| {
            c.execute("DELETE FROM calendars WHERE account_id = ?1", [account_id])?;
            let mut insert = c.prepare("INSERT INTO calendars (account_id, position, calendar) VALUES (?1, ?2, ?3)")?;
            for (position, calendar) in calendars.iter().enumerate() {
                let json = serde_json::to_string(calendar).map_err(json_error)?;
                insert.execute(params![account_id, position as i64, json])?;
            }
            Ok(())
        })
    }

    /// Replaces what is cached for the span `[from, to)` with `entries`, which are everything
    /// the server has there: an entry that is gone from the server goes from the cache too.
    pub fn agenda_replace(&self, account_id: &str, from: i64, to: i64, entries: &[Occurrence]) -> Result<()> {
        self.write(|c| {
            c.execute(
                &format!("DELETE FROM agenda_entries WHERE account_id = ?1 AND {OVERLAPS}"),
                params![account_id, from, to],
            )?;
            let mut insert = c.prepare(
                "INSERT OR REPLACE INTO agenda_entries
                     (account_id, event_id, recurrence_id, starts_at, ends_at, occurrence)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?;
            for e in entries {
                let json = serde_json::to_string(e).map_err(json_error)?;
                let recurrence_id = e.recurrence_id.as_deref().unwrap_or_default();
                insert.execute(params![account_id, e.event_id, recurrence_id, e.starts_at, e.ends_at, json])?;
            }
            Ok(())
        })
    }

    /// The cached entries of the span `[from, to)`, in no particular order.
    pub fn agenda_list(&self, account_id: &str, from: i64, to: i64) -> Result<Vec<Occurrence>> {
        self.read(|c| {
            let sql = format!("SELECT occurrence FROM agenda_entries WHERE account_id = ?1 AND {OVERLAPS}");
            let mut stmt = c.prepare(&sql)?;
            let rows = stmt.query_map(params![account_id, from, to], |r| r.get::<_, String>(0))?;
            let mut out = Vec::new();
            for json in rows {
                out.extend(serde_json::from_str(&json?).ok());
            }
            Ok(out)
        })
    }

    /// Drops every cached entry of an event (all its occurrences), so that a copy the user
    /// just changed or deleted is not shown from the cache.
    pub fn agenda_forget_event(&self, account_id: &str, event_id: &str) -> Result<()> {
        self.write(|c| {
            c.execute("DELETE FROM agenda_entries WHERE account_id = ?1 AND event_id = ?2", [account_id, event_id])?;
            Ok(())
        })
    }
}
