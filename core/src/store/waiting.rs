//! What the user did that still waits for a pass, and when a pass is next useful for it.

use super::Store;
use crate::Result;

/// How long after its send time a message that was handed to the server is looked at: by then
/// the server has let it go, and the pass brings the row up to date. (Every `held` row is
/// counted, whether it is the undo window or a send-later: one extra pass is cheap.)
const HELD_GRACE_MS: i64 = 3_000;

impl Store {
    /// Milliseconds until a pass is useful for something the user did, from the cache alone:
    /// `Some(0)` when something is due now, `None` when nothing waits. See
    /// `Core::work_waiting`.
    pub fn work_waiting(&self, account_id: &str, now_ms: i64) -> Result<Option<u64>> {
        // A queued action that this version can run.
        if !self.outbox_jobs(account_id)?.is_empty() {
            return Ok(Some(0));
        }
        // A thrown-away draft whose server copy is still to be destroyed (a held message is
        // taken back first, or it leaves at its time all the same).
        if !self.drafts_discarded_rows(account_id)?.is_empty() {
            return Ok(Some(0));
        }
        let rows: Vec<(String, Option<i64>, bool)> = self.read(|c| {
            let mut stmt =
                c.prepare("SELECT state, send_at, dirty FROM drafts WHERE account_id = ?1 AND discarded = 0")?;
            let rows = stmt.query_map([account_id], |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, i64>(2)? != 0)))?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })?;
        let mut soonest: Option<i64> = None;
        let mut want = |at_ms: i64| soonest = Some(soonest.map_or(at_ms, |s| s.min(at_ms)));
        for (state, send_at, dirty) in rows {
            let send_ms = send_at.map(|s| s.saturating_mul(1000));
            match state.as_str() {
                // A draft whose server copy is not the latest.
                "draft" if dirty => want(now_ms),
                // Handed to the server, or being: a pass settles it.
                "submitting" | "scheduling" | "schedule_submitting" => want(now_ms),
                // Held here: it leaves with the first pass after its time. (No time: now.)
                "sending" => want(send_ms.unwrap_or(now_ms)),
                // Held by the server for the undo window: the pass only updates the row.
                "held" => want(send_ms.map_or(now_ms, |s| s.saturating_add(HELD_GRACE_MS))),
                _ => {}
            }
        }
        Ok(soonest.map(|at| (at - now_ms).max(0) as u64))
    }
}
