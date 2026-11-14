//! Wall-clock times, time zones and ISO 8601 durations as JSCalendar writes them.
//!
//! The zone rules are chrono-tz's; nothing here knows what a zone does.

use chrono::{DateTime, Duration, LocalResult, NaiveDate, NaiveDateTime, Offset, TimeZone};
use chrono_tz::Tz;

/// The only form the app writes a local date-time in.
pub const LOCAL_FORMAT: &str = "%Y-%m-%dT%H:%M:%S";

/// A local date-time as a server may have it: `2026-11-12T10:00:00`, with a fraction of a
/// second, with a trailing `Z` (read as the same wall-clock time) or as a bare date (midnight).
pub fn parse_local(text: &str) -> Option<NaiveDateTime> {
    let text = text.strip_suffix('Z').unwrap_or(text);
    if text.contains('T') {
        NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S%.f").ok()
    } else {
        NaiveDate::parse_from_str(text, "%Y-%m-%d").ok()?.and_hms_opt(0, 0, 0)
    }
}

/// A local date-time in exactly the form the app writes (`YYYY-MM-DDTHH:MM:SS`).
pub fn parse_strict(text: &str) -> Option<NaiveDateTime> {
    NaiveDateTime::parse_from_str(text, LOCAL_FORMAT).ok().filter(|_| text.len() == 19)
}

pub fn format_local(at: NaiveDateTime) -> String {
    at.format(LOCAL_FORMAT).to_string()
}

/// An IANA zone by name.
pub fn zone(name: &str) -> Option<Tz> {
    name.parse().ok()
}

/// The instant (Unix seconds) at which the wall clock in `zone` shows `local`. A time that
/// happens twice (the clock went back) is the first one; a time that never happens (the clock
/// jumped forward) is read with the offset from before the jump, as RFC 5545 says.
pub fn unix(local: NaiveDateTime, zone: Tz) -> i64 {
    match zone.from_local_datetime(&local) {
        LocalResult::Single(at) | LocalResult::Ambiguous(at, _) => at.timestamp(),
        LocalResult::None => {
            let before = zone.offset_from_local_datetime(&(local - Duration::days(1))).earliest();
            let offset = before.map_or(0, |o| i64::from(o.fix().local_minus_utc()));
            local.and_utc().timestamp() - offset
        }
    }
}

/// The wall-clock time in `zone` at an instant.
pub fn local_naive(at: i64, zone: Tz) -> NaiveDateTime {
    DateTime::from_timestamp(at, 0).unwrap_or_default().with_timezone(&zone).naive_local()
}

/// The wall-clock date in `zone` at an instant.
pub fn local_date(at: i64, zone: Tz) -> NaiveDate {
    local_naive(at, zone).date()
}

/// The seconds an ISO 8601 duration with weeks, days, hours, minutes and seconds stands for
/// (`PT1H30M`, `P1D`, `P2W`, `P1DT2H`). `None` for anything else, years and months included.
pub fn parse_duration(text: &str) -> Option<u64> {
    let (days, time) = parse_duration_split(text)?;
    days.checked_add(time)
}

/// A duration as `(seconds in whole weeks and days, seconds in hours, minutes and seconds)`.
/// The first part is a number of calendar days (a day is not always 24 hours on a wall clock);
/// the second is exact time.
pub fn parse_duration_split(text: &str) -> Option<(u64, u64)> {
    let rest = text.strip_prefix('P')?;
    let (date, time) = match rest.split_once('T') {
        Some((date, time)) => (date, Some(time)),
        None => (rest, None),
    };
    let (days, day_parts) = components(date, &[('W', 604_800), ('D', 86_400)])?;
    let (secs, time_parts) = match time {
        Some(time) => components(time, &[('H', 3600), ('M', 60), ('S', 1)]).filter(|(_, parts)| *parts > 0)?,
        None => (0, 0),
    };
    // "P" is not a duration, and the whole must fit.
    (day_parts + time_parts > 0 && days.checked_add(secs).is_some()).then_some((days, secs))
}

/// `(seconds, how many parts)` of a run such as `1H30M`; the units come in the order of `table`,
/// each at most once.
fn components(mut text: &str, table: &[(char, u64)]) -> Option<(u64, usize)> {
    let (mut total, mut parts, mut next) = (0u64, 0, 0);
    while !text.is_empty() {
        let digits = text.find(|c: char| !c.is_ascii_digit()).filter(|&n| n > 0)?;
        let unit = text[digits..].chars().next()?;
        let index = table.iter().position(|(u, _)| *u == unit).filter(|&i| i >= next)?;
        total = total.checked_add(text[..digits].parse::<u64>().ok()?.checked_mul(table[index].1)?)?;
        (parts, next) = (parts + 1, index + 1);
        text = &text[digits + unit.len_utf8()..];
    }
    Some((total, parts))
}

/// The shortest ISO 8601 form of a length of time: `PT45M`, `PT1H30M`, `P1D`, `P1DT2H`.
pub fn format_duration(secs: u64) -> String {
    if secs == 0 {
        return "PT0S".into();
    }
    let (days, rest) = (secs / 86_400, secs % 86_400);
    let mut out = String::from("P");
    if days > 0 {
        out.push_str(&format!("{days}D"));
    }
    if rest > 0 {
        out.push('T');
        for (n, unit) in [(rest / 3600, 'H'), (rest % 3600 / 60, 'M'), (rest % 60, 'S')] {
            if n > 0 {
                out.push_str(&format!("{n}{unit}"));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local(text: &str) -> NaiveDateTime {
        parse_strict(text).unwrap()
    }

    #[test]
    fn local_times_as_servers_write_them() {
        let noon = local("2026-11-12T12:00:00");
        assert_eq!(parse_local("2026-11-12T12:00:00"), Some(noon));
        assert_eq!(parse_local("2026-11-12T12:00:00Z"), Some(noon));
        assert_eq!(parse_local("2026-11-12T12:00:00.250"), Some(noon + Duration::milliseconds(250)));
        assert_eq!(parse_local("2026-11-12"), Some(local("2026-11-12T00:00:00")));
        for bad in ["", "tomorrow", "2026-13-01T00:00:00", "2026-11-12T25:00:00", "2026-11-12T10:00"] {
            assert_eq!(parse_local(bad), None, "{bad}");
        }
    }

    #[test]
    fn the_strict_form_is_one_exact_shape() {
        assert!(parse_strict("2026-11-12T10:00:00").is_some());
        for bad in
            ["2026-11-12", "2026-11-12T10:00", "2026-11-12T10:00:00Z", "2026-11-12T10:00:00.5", "2026-1-2T1:00:00"]
        {
            assert_eq!(parse_strict(bad), None, "{bad}");
        }
        assert_eq!(format_local(local("2026-01-02T03:04:05")), "2026-01-02T03:04:05");
    }

    #[test]
    fn an_instant_from_a_wall_clock_time() {
        let (utc, istanbul) = (zone("UTC").unwrap(), zone("Europe/Istanbul").unwrap());
        // The server reported this very event as starting at 07:00 UTC.
        assert_eq!(unix(local("2026-11-16T10:00:00"), istanbul), unix(local("2026-11-16T07:00:00"), utc));
        let new_york = zone("America/New_York").unwrap();
        // Winter (UTC-5) and summer (UTC-4).
        assert_eq!(unix(local("2026-01-15T12:00:00"), new_york), unix(local("2026-01-15T17:00:00"), utc));
        assert_eq!(unix(local("2026-07-15T12:00:00"), new_york), unix(local("2026-07-15T16:00:00"), utc));
        assert_eq!(zone("Mars/Base"), None);
        assert_eq!(zone("Europe/Istanbul "), None);
    }

    #[test]
    fn a_time_the_clock_skips_or_repeats() {
        let utc = zone("UTC").unwrap();
        let new_york = zone("America/New_York").unwrap();
        // 2026-03-08: 02:00 became 03:00. 02:30 never happened; the offset from before the jump
        // (UTC-5) puts it at 07:30 UTC, which is 03:30 on the clock.
        assert_eq!(unix(local("2026-03-08T02:30:00"), new_york), unix(local("2026-03-08T07:30:00"), utc));
        // 2026-11-01: 02:00 became 01:00. 01:30 happened twice; the first (UTC-4) is taken.
        assert_eq!(unix(local("2026-11-01T01:30:00"), new_york), unix(local("2026-11-01T05:30:00"), utc));
    }

    #[test]
    fn the_wall_clock_date_at_an_instant() {
        let istanbul = zone("Europe/Istanbul").unwrap();
        let at = unix(local("2026-11-16T23:30:00"), zone("UTC").unwrap());
        assert_eq!(local_date(at, zone("UTC").unwrap()).to_string(), "2026-11-16");
        assert_eq!(local_date(at, istanbul).to_string(), "2026-11-17");
    }

    #[test]
    fn durations_are_read() {
        let cases = [
            ("PT0S", 0),
            ("PT15M", 900),
            ("PT1H30M", 5400),
            ("P1D", 86_400),
            ("P2W", 1_209_600),
            ("P1DT2H", 93_600),
            ("P1W2DT3H4M5S", 604_800 + 172_800 + 10_800 + 245),
        ];
        for (text, secs) in cases {
            assert_eq!(parse_duration(text), Some(secs), "{text}");
        }
        for bad in
            ["", "P", "PT", "1H", "PT1", "PT1.5S", "P1Y", "P1M", "PT1M1H", "P1DT", "PT-5M", "-PT5M", "pt5m", "PT1H1H"]
        {
            assert_eq!(parse_duration(bad), None, "{bad}");
        }
    }

    #[test]
    fn durations_are_written_short_and_read_back() {
        let cases = [
            (0, "PT0S"),
            (45, "PT45S"),
            (900, "PT15M"),
            (3600, "PT1H"),
            (5400, "PT1H30M"),
            (86_400, "P1D"),
            (3 * 86_400, "P3D"),
            (93_600, "P1DT2H"),
            (3661, "PT1H1M1S"),
        ];
        for (secs, text) in cases {
            assert_eq!(format_duration(secs), text);
            assert_eq!(parse_duration(text), Some(secs));
        }
    }
}
