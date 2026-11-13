//! Dates without a date library: the cli only prints UTC times.

/// `YYYY-MM-DD HH:MM` in UTC for Unix seconds.
pub fn utc(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let in_day = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}", in_day / 3600, in_day % 3600 / 60)
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
    fn before_the_epoch() {
        assert_eq!(utc(-1), "1969-12-31 23:59");
        assert_eq!(utc(-86_400 * 365), "1969-01-01 00:00");
    }
}
