//! Dates for the cli: UTC times by hand, and the agenda's local times through chrono-tz.

use chrono::{DateTime, Days, NaiveDate, TimeZone};
use chrono_tz::Tz;

/// `YYYY-MM-DD HH:MM` in UTC for Unix seconds.
pub fn utc(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let in_day = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}", in_day / 3600, in_day % 3600 / 60)
}

/// The zone to show the agenda in: the `TZ` value if it names an IANA zone (a leading `:` or
/// a zoneinfo path is tolerated), else UTC.
pub fn zone_from_env(tz: Option<&str>) -> Tz {
    let name = tz.unwrap_or_default().trim_start_matches(':');
    let name = name.rsplit_once("zoneinfo/").map_or(name, |(_, zone)| zone);
    name.parse().unwrap_or(Tz::UTC)
}

/// The instant the wall clock in `zone` first shows midnight of `date`. A midnight that does
/// not exist (the clock jumped over it) is read as the same wall time in UTC.
fn midnight(date: NaiveDate, zone: Tz) -> i64 {
    let local = date.and_hms_opt(0, 0, 0).expect("midnight exists");
    zone.from_local_datetime(&local).earliest().map_or_else(|| local.and_utc().timestamp(), |t| t.timestamp())
}

/// `[from, to)` in Unix seconds for `days` whole calendar days starting with the day `now`
/// falls on in `zone`.
pub fn span(now: i64, days: u32, zone: Tz) -> (i64, i64) {
    let today = DateTime::from_timestamp(now, 0).expect("a sane clock").with_timezone(&zone).date_naive();
    let end = today.checked_add_days(Days::new(u64::from(days))).expect("a sane span");
    (midnight(today, zone), midnight(end, zone))
}

/// `YYYY-MM-DD` of Unix seconds in `zone`.
pub fn day(secs: i64, zone: Tz) -> String {
    local(secs, zone, "%Y-%m-%d")
}

/// `HH:MM` of Unix seconds in `zone`.
pub fn clock(secs: i64, zone: Tz) -> String {
    local(secs, zone, "%H:%M")
}

fn local(secs: i64, zone: Tz, format: &str) -> String {
    match DateTime::from_timestamp(secs, 0) {
        Some(t) => t.with_timezone(&zone).format(format).to_string(),
        None => "?".into(),
    }
}

/// Days since 1970-01-01 to (year, month, day), proleptic Gregorian (Howard Hinnant's algorithm).
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_epoch() {
        assert_eq!(utc(0), "1970-01-01 00:00");
    }

    #[test]
    fn known_dates() {
        assert_eq!(utc(951_782_400), "2000-02-29 00:00");
        assert_eq!(utc(1_709_251_199), "2024-02-29 23:59");
        assert_eq!(utc(1_709_251_200), "2024-03-01 00:00");
        assert_eq!(utc(1_767_225_540), "2025-12-31 23:59");
        assert_eq!(utc(1_767_225_600), "2026-01-01 00:00");
    }

    #[test]
    fn the_zone_comes_from_tz_or_is_utc() {
        assert_eq!(zone_from_env(Some("Europe/Berlin")), Tz::Europe__Berlin);
        assert_eq!(zone_from_env(Some(":Europe/Berlin")), Tz::Europe__Berlin);
        assert_eq!(zone_from_env(Some("/usr/share/zoneinfo/Asia/Tokyo")), Tz::Asia__Tokyo);
        assert_eq!(zone_from_env(Some("Not/AZone")), Tz::UTC);
        assert_eq!(zone_from_env(Some("")), Tz::UTC);
        assert_eq!(zone_from_env(None), Tz::UTC);
    }

    #[test]
    fn the_span_starts_at_local_midnight() {
        // 2026-01-02 23:30 UTC is already 2026-01-03 in Berlin.
        let now = 1_767_396_600;
        assert_eq!(span(now, 7, Tz::UTC), (1_767_312_000, 1_767_312_000 + 7 * 86_400));
        let (from, to) = span(now, 1, Tz::Europe__Berlin);
        assert_eq!(utc(from), "2026-01-02 23:00");
        assert_eq!(utc(to), "2026-01-03 23:00");
    }

    #[test]
    fn a_span_over_a_clock_change_counts_calendar_days() {
        // Berlin went to summer time on 2026-03-29: that day has 23 hours.
        let now = 1_774_700_000; // 2026-03-28 12:13 UTC
        let (from, to) = span(now, 2, Tz::Europe__Berlin);
        assert_eq!(utc(from), "2026-03-27 23:00");
        assert_eq!(utc(to), "2026-03-29 22:00");
    }

    #[test]
    fn day_and_clock_follow_the_zone() {
        assert_eq!(day(1_767_396_600, Tz::UTC), "2026-01-02");
        assert_eq!(day(1_767_396_600, Tz::Europe__Berlin), "2026-01-03");
        assert_eq!(clock(1_767_396_600, Tz::Europe__Berlin), "00:30");
    }

    #[test]
    fn before_the_epoch() {
        assert_eq!(utc(-1), "1969-12-31 23:59");
        assert_eq!(utc(-86_400 * 365), "1969-01-01 00:00");
    }
}
