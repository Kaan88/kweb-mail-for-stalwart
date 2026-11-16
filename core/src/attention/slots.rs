//! Free time, worked out from the cache: the chips of "Find a slot" and the list of "Free time".

use super::{Clock, TimeSlot};
use crate::model::Occurrence;

/// An event that takes the user's time: timed, not a task, not declined, with a length.
pub fn takes_time(e: &Occurrence) -> bool {
    !e.is_task && !e.all_day && e.my_status.as_deref() != Some("declined") && e.ends_at > e.starts_at
}

/// The busy spans of the events, sorted and merged.
pub fn busy(entries: &[Occurrence], skip: Option<(&str, i64)>) -> Vec<(i64, i64)> {
    let mut spans: Vec<(i64, i64)> = entries
        .iter()
        .filter(|e| takes_time(e) && skip.is_none_or(|(id, start)| !(e.event_id == id && e.starts_at == start)))
        .map(|e| (e.starts_at, e.ends_at))
        .collect();
    spans.sort_unstable();
    let mut merged: Vec<(i64, i64)> = Vec::new();
    for (s, e) in spans {
        match merged.last_mut() {
            Some(last) if s <= last.1 => last.1 = last.1.max(e),
            _ => merged.push((s, e)),
        }
    }
    merged
}

fn is_free(busy: &[(i64, i64)], start: i64, end: i64) -> bool {
    !busy.iter().any(|&(s, e)| s < end && e > start)
}

/// Free times of `len_secs` on the day of `day_ts` and the next day, 08:00 to 19:00, on the half
/// hour, never in the past: at most `max` chips, in time order.
pub fn free_slots(clock: &Clock, busy: &[(i64, i64)], len_secs: i64, day_ts: i64, max: usize) -> Vec<TimeSlot> {
    let len = len_secs.max(15 * 60);
    let mut out = Vec::new();
    for add_days in 0..2 {
        let day = clock.local(day_ts).date() + chrono::Duration::days(add_days);
        let open = clock.ts(day.and_hms_opt(8, 0, 0).unwrap_or_default());
        let close = clock.ts(day.and_hms_opt(19, 0, 0).unwrap_or_default());
        let mut start = open;
        while start + len <= close {
            if start > clock.now && is_free(busy, start, start + len) {
                out.push(TimeSlot { start, end: start + len });
                // Slots an hour apart at least, so the chips are different choices.
                start += (len).max(3600);
                continue;
            }
            start += 30 * 60;
        }
    }
    out.truncate(max);
    out
}

/// The gaps of a day between 09:00 and 18:00 (from now on for today) that are at least ten
/// minutes long.
pub fn gaps(clock: &Clock, busy: &[(i64, i64)], day_ts: i64) -> Vec<TimeSlot> {
    let open = clock.at_hour(day_ts, 9).max(clock.now);
    let close = clock.at_hour(day_ts, 18);
    let mut out = Vec::new();
    let mut cursor = open;
    for &(s, e) in busy {
        if e <= cursor {
            continue;
        }
        if s >= close {
            break;
        }
        if s > cursor && s - cursor >= 600 {
            out.push(TimeSlot { start: cursor, end: s });
        }
        cursor = cursor.max(e);
    }
    if close > cursor && close - cursor >= 600 {
        out.push(TimeSlot { start: cursor, end: close });
    }
    out
}
