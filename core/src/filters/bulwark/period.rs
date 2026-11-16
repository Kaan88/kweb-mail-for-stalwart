//! Active periods of a rule: Bulwark's `lib/sieve/period.ts`. A rule acts only on mail that
//! arrives from `activeFrom` until `activeUntil`, both UTC instants, compiled to RFC 5260
//! `currentdate` tests.

use chrono::{Datelike, Duration, NaiveDate, Timelike};

/// What a period needs from the server.
pub const PERIOD_REQUIRES: [&str; 2] = ["date", "relational"];

pub fn supports_periods(extensions: &[String]) -> bool {
    PERIOD_REQUIRES.iter().all(|e| extensions.iter().any(|x| x == e))
}

fn digits(b: &[u8], from: usize, n: usize) -> Option<u32> {
    let part = b.get(from..from + n)?;
    if !part.iter().all(u8::is_ascii_digit) {
        return None;
    }
    part.iter().try_fold(0u32, |acc, d| Some(acc * 10 + u32::from(d - b'0')))
}

/// The boundary as UTC `("yyyy-mm-dd", "hh:mm:ss")`, or `None` when it is not a moment with
/// its zone spelled out (`^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}(:\d{2}(.\d{1,3})?)?(Z|[+-]\d{2}:\d{2})$`)
/// or its UTC year does not fit four digits.
pub fn utc_parts(boundary: &str) -> Option<(String, String)> {
    let b = boundary.as_bytes();
    let (y, mo, d) = (digits(b, 0, 4)?, digits(b, 5, 2)?, digits(b, 8, 2)?);
    if b.get(4) != Some(&b'-') || b.get(7) != Some(&b'-') || b.get(10) != Some(&b'T') {
        return None;
    }
    let (h, mi) = (digits(b, 11, 2)?, digits(b, 14, 2)?);
    if b.get(13) != Some(&b':') {
        return None;
    }
    let mut at = 16;
    let mut s = 0;
    if b.get(at) == Some(&b':') {
        s = digits(b, at + 1, 2)?;
        at += 3;
        if b.get(at) == Some(&b'.') {
            let n = b[at + 1..].iter().take_while(|c| c.is_ascii_digit()).count();
            if !(1..=3).contains(&n) {
                return None;
            }
            at += 1 + n;
        }
    }
    let offset_minutes: i64 = match b.get(at..)? {
        [b'Z'] => 0,
        [sign @ (b'+' | b'-'), ..] if b.len() == at + 6 && b[at + 3] == b':' => {
            let (oh, om) = (digits(b, at + 1, 2)?, digits(b, at + 4, 2)?);
            if oh > 23 || om > 59 {
                return None;
            }
            let minutes = i64::from(oh * 60 + om);
            if *sign == b'-' {
                -minutes
            } else {
                minutes
            }
        }
        _ => return None,
    };
    if h > 23 || mi > 59 || s > 59 {
        return None;
    }
    let local = NaiveDate::from_ymd_opt(y as i32, mo, d)?.and_hms_opt(h, mi, s)?;
    let utc = local.checked_sub_signed(Duration::minutes(offset_minutes))?;
    if !(0..=9999).contains(&utc.year()) {
        return None;
    }
    Some((
        format!("{:04}-{:02}-{:02}", utc.year(), utc.month(), utc.day()),
        format!("{:02}:{:02}:{:02}", utc.hour(), utc.minute(), utc.second()),
    ))
}

pub fn is_period_boundary(value: &str) -> bool {
    utc_parts(value).is_some()
}

const NOW: &str = "currentdate :zone \"+0000\"";

fn at_or_after(date: &str, time: &str) -> String {
    format!(
        "anyof({NOW} :value \"gt\" \"date\" \"{date}\", allof({NOW} :is \"date\" \"{date}\", {NOW} :value \"ge\" \"time\" \"{time}\"))"
    )
}

fn at_or_before(date: &str, time: &str) -> String {
    format!(
        "anyof({NOW} :value \"lt\" \"date\" \"{date}\", allof({NOW} :is \"date\" \"{date}\", {NOW} :value \"le\" \"time\" \"{time}\"))"
    )
}

/// The tests that confine a rule to its period: none without one, `None` when a boundary is
/// unusable (the rule must then be dropped, not run without its period).
pub fn period_tests(from: Option<&str>, until: Option<&str>) -> Option<Vec<String>> {
    let mut tests = Vec::new();
    if let Some(from) = from {
        let (date, time) = utc_parts(from)?;
        tests.push(at_or_after(&date, &time));
    }
    if let Some(until) = until {
        let (date, time) = utc_parts(until)?;
        tests.push(at_or_before(&date, &time));
    }
    Some(tests)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boundaries_need_a_zone_and_four_digit_utc_years() {
        assert!(is_period_boundary("2026-10-05T06:00:00.000Z"));
        assert!(is_period_boundary("2026-10-05T08:00+02:00"));
        assert!(!is_period_boundary("2026-10-05T08:00"));
        assert!(!is_period_boundary("2026-10-05"));
        assert!(!is_period_boundary("2026-13-45T08:00:00Z"));
        assert!(!is_period_boundary(""));
        assert!(!is_period_boundary("9999-12-31T23:59:59-01:00"));
        assert!(is_period_boundary("9999-12-31T23:59:59Z"));
        assert!(!is_period_boundary("2026-10-05T06:00:00.1234Z"));
    }

    #[test]
    fn the_tests_are_bulwarks_written_in_utc() {
        assert_eq!(
            period_tests(Some("2026-10-05T08:00:00+02:00"), Some("2026-10-16T16:00:00.000Z")).unwrap(),
            vec![
                "anyof(currentdate :zone \"+0000\" :value \"gt\" \"date\" \"2026-10-05\", allof(currentdate :zone \"+0000\" :is \"date\" \"2026-10-05\", currentdate :zone \"+0000\" :value \"ge\" \"time\" \"06:00:00\"))",
                "anyof(currentdate :zone \"+0000\" :value \"lt\" \"date\" \"2026-10-16\", allof(currentdate :zone \"+0000\" :is \"date\" \"2026-10-16\", currentdate :zone \"+0000\" :value \"le\" \"time\" \"16:00:00\"))",
            ]
        );
        assert_eq!(period_tests(None, None).unwrap(), Vec::<String>::new());
        assert!(period_tests(Some("2026-10-05T08:00"), None).is_none());
        assert!(period_tests(Some("2026-10-05T06:00:00Z"), Some("soon")).is_none());
    }
}
