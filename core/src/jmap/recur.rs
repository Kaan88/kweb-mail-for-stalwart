//! The dates of a repeating event, worked out here.
//!
//! Stalwart 1.1 does not expand repeating events in `CalendarEvent/query` (see `calendars`), so
//! the core does it from the event's `recurrenceRules`. Everything is in wall-clock time of the
//! event's own zone, as JSCalendar has it; the caller turns the dates into instants.
//!
//! Covered: daily, weekly, monthly and yearly rules with `interval`, `count`, `until`, `byDay`
//! (with an ordinal in monthly and yearly rules), `byMonthDay`, `byMonth` and, in monthly and
//! yearly rules, `bySetPosition` ("the last Friday"). A rule that says more than that
//! (`byYearDay`, `byWeekNo`, time parts, finer frequencies, another calendar, skip mode or first
//! day of the week) is not guessed at: [`expand`] returns `None`.

use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, Weekday};
use serde_json::Value;

use super::clock::parse_local;

/// The most dates one rule produces, so that a rule nobody ends cannot run away.
const MAX_DATES: usize = 20_000;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Frequency {
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

#[derive(Debug)]
struct Rule {
    frequency: Frequency,
    interval: i64,
    /// A weekday and, in a monthly or yearly rule, which one of the month (`-1` is the last).
    by_day: Vec<(Weekday, Option<i32>)>,
    /// Days of the month; negative ones count from its end.
    by_month_day: Vec<i32>,
    by_month: Vec<u32>,
    /// Which of the dates a month (monthly rule) or year (yearly rule) holds are used; `-1` is the last.
    by_set_position: Vec<i32>,
    count: Option<usize>,
    until: Option<NaiveDateTime>,
}

/// The start times of the instances of `rules` (a JSON array of JSCalendar `RecurrenceRule`s)
/// from `start` up to and including `limit`, ascending. `start` is always the first one.
/// `None` when a rule is one this module does not follow.
pub fn expand(rules: &Value, start: NaiveDateTime, limit: NaiveDateTime) -> Option<Vec<NaiveDateTime>> {
    let mut dates = vec![start.date()];
    for rule in rules.as_array()? {
        let rule = parse_rule(rule)?;
        let mut own = generate(&rule, start.date(), limit.date());
        own.push(start.date());
        own.sort();
        own.dedup();
        own.truncate(rule.count.unwrap_or(usize::MAX));
        let time = start.time();
        own.retain(|d| rule.until.is_none_or(|until| d.and_time(time) <= until));
        dates.extend(own);
    }
    dates.sort();
    dates.dedup();
    Some(dates.into_iter().map(|d| d.and_time(start.time())).filter(|at| *at <= limit).collect())
}

fn parse_rule(rule: &Value) -> Option<Rule> {
    const KNOWN: [&str; 12] = [
        "@type",
        "frequency",
        "interval",
        "count",
        "until",
        "byDay",
        "byMonthDay",
        "byMonth",
        "bySetPosition",
        "rscale",
        "skip",
        "firstDayOfWeek",
    ];
    let fields = rule.as_object()?;
    if fields.keys().any(|k| !KNOWN.contains(&k.as_str())) {
        return None;
    }
    // The defaults of the draft: the Gregorian calendar, a day that does not exist is left out,
    // weeks start on Monday.
    let default_only = |key: &str, only: &str| fields.get(key).is_none_or(|v| v == only);
    if !(default_only("rscale", "gregorian") && default_only("skip", "omit") && default_only("firstDayOfWeek", "mo")) {
        return None;
    }
    let frequency = match fields.get("frequency")?.as_str()? {
        "daily" => Frequency::Daily,
        "weekly" => Frequency::Weekly,
        "monthly" => Frequency::Monthly,
        "yearly" => Frequency::Yearly,
        _ => return None,
    };
    let interval = fields.get("interval").map_or(Some(1), Value::as_i64).filter(|n| *n >= 1)?;
    let by_day = match fields.get("byDay") {
        None => Vec::new(),
        Some(days) => days.as_array()?.iter().map(by_day_entry).collect::<Option<Vec<_>>>()?,
    };
    let by_month_day = match fields.get("byMonthDay") {
        None => Vec::new(),
        Some(days) => {
            days.as_array()?.iter().map(|d| d.as_i64().and_then(|n| i32::try_from(n).ok())).collect::<Option<_>>()?
        }
    };
    let by_month = match fields.get("byMonth") {
        None => Vec::new(),
        Some(months) => months.as_array()?.iter().map(|m| m.as_str()?.parse().ok()).collect::<Option<_>>()?,
    };
    let by_set_position: Vec<i32> = match fields.get("bySetPosition") {
        None => Vec::new(),
        Some(all) => {
            let all = all.as_array()?;
            all.iter()
                .map(|n| n.as_i64().and_then(|n| i32::try_from(n).ok()).filter(|n| *n != 0))
                .collect::<Option<_>>()?
        }
    };
    // A position picks among the dates of a month or a year only.
    if !by_set_position.is_empty() && !matches!(frequency, Frequency::Monthly | Frequency::Yearly) {
        return None;
    }
    // An ordinal only means something against a month or a year.
    let ordinals = by_day.iter().any(|(_, nth)| nth.is_some());
    let whole_year_weekday = frequency == Frequency::Yearly && by_month.is_empty() && !by_day.is_empty();
    let whole_year_day = frequency == Frequency::Yearly && by_month.is_empty() && !by_month_day.is_empty();
    if (ordinals && matches!(frequency, Frequency::Daily | Frequency::Weekly)) || whole_year_weekday || whole_year_day {
        return None;
    }
    let count = match fields.get("count") {
        None => None,
        Some(n) => Some(usize::try_from(n.as_u64()?).ok()?),
    };
    let until = match fields.get("until") {
        None => None,
        Some(text) => Some(parse_local(text.as_str()?)?),
    };
    Some(Rule { frequency, interval, by_day, by_month_day, by_month, by_set_position, count, until })
}

fn by_day_entry(entry: &Value) -> Option<(Weekday, Option<i32>)> {
    let day = match entry.get("day")?.as_str()? {
        "mo" => Weekday::Mon,
        "tu" => Weekday::Tue,
        "we" => Weekday::Wed,
        "th" => Weekday::Thu,
        "fr" => Weekday::Fri,
        "sa" => Weekday::Sat,
        "su" => Weekday::Sun,
        _ => return None,
    };
    let nth = match entry.get("nthOfPeriod") {
        None => None,
        Some(n) => Some(i32::try_from(n.as_i64()?).ok()?).filter(|n| *n != 0),
    };
    Some((day, nth))
}

/// The dates of one rule after `start` up to `last`, ascending, without `start` itself.
fn generate(rule: &Rule, start: NaiveDate, last: NaiveDate) -> Vec<NaiveDate> {
    let mut out = Vec::new();
    for k in 0.. {
        let Some((period_start, found)) = period(rule, start, k) else { break };
        if period_start > last || out.len() > MAX_DATES {
            break;
        }
        out.extend(found.into_iter().filter(|d| *d > start && *d <= last));
    }
    out
}

/// The first day of the k-th period of the rule and the dates the rule picks in it, ascending.
/// `None` when the calendar ends.
fn period(rule: &Rule, start: NaiveDate, k: i64) -> Option<(NaiveDate, Vec<NaiveDate>)> {
    let step = rule.interval.checked_mul(k)?;
    let month_ok = |d: &NaiveDate| rule.by_month.is_empty() || rule.by_month.contains(&d.month());
    let month_day_ok = |d: &NaiveDate| {
        rule.by_month_day.is_empty() || rule.by_month_day.iter().any(|n| month_day(d.year(), d.month(), *n) == Some(*d))
    };
    match rule.frequency {
        Frequency::Daily => {
            let day = start.checked_add_signed(Duration::try_days(step)?)?;
            let weekday_ok = rule.by_day.is_empty() || rule.by_day.iter().any(|(w, _)| *w == day.weekday());
            let found = [day].into_iter().filter(|d| weekday_ok && month_ok(d) && month_day_ok(d)).collect();
            Some((day, found))
        }
        Frequency::Weekly => {
            let monday = start - Duration::days(i64::from(start.weekday().num_days_from_monday()));
            let week = monday.checked_add_signed(Duration::try_weeks(step)?)?;
            let weekdays: Vec<Weekday> = if rule.by_day.is_empty() {
                vec![start.weekday()]
            } else {
                rule.by_day.iter().map(|(w, _)| *w).collect()
            };
            let mut found: Vec<NaiveDate> =
                weekdays.iter().map(|w| week + Duration::days(i64::from(w.num_days_from_monday()))).collect();
            found.retain(|d| month_ok(d) && month_day_ok(d));
            found.sort();
            Some((week, found))
        }
        Frequency::Monthly => {
            let (year, month) = shift_month(start.year(), start.month(), step)?;
            let first = NaiveDate::from_ymd_opt(year, month, 1)?;
            let found = if month_ok(&first) { month_dates(rule, year, month, start.day()) } else { Vec::new() };
            Some((first, pick_positions(found, &rule.by_set_position)))
        }
        Frequency::Yearly => {
            let year = i32::try_from(i64::from(start.year()).checked_add(step)?).ok()?;
            let months = if rule.by_month.is_empty() { vec![start.month()] } else { rule.by_month.clone() };
            let mut found: Vec<NaiveDate> =
                months.into_iter().flat_map(|m| month_dates(rule, year, m, start.day())).collect();
            found.sort();
            Some((NaiveDate::from_ymd_opt(year, 1, 1)?, pick_positions(found, &rule.by_set_position)))
        }
    }
}

/// The dates at the given positions (1 is the first, -1 the last) of an ascending set; the set
/// itself when no position is given.
fn pick_positions(found: Vec<NaiveDate>, positions: &[i32]) -> Vec<NaiveDate> {
    if positions.is_empty() {
        return found;
    }
    let at = |n: &i32| match n.signum() {
        1 => found.get(*n as usize - 1),
        _ => found.len().checked_sub(n.unsigned_abs() as usize).and_then(|i| found.get(i)),
    };
    let mut out: Vec<NaiveDate> = positions.iter().filter_map(at).copied().collect();
    out.sort();
    out.dedup();
    out
}

fn shift_month(year: i32, month: u32, by: i64) -> Option<(i32, u32)> {
    let index = i64::from(year) * 12 + i64::from(month - 1) + by;
    Some((i32::try_from(index.div_euclid(12)).ok()?, index.rem_euclid(12) as u32 + 1))
}

/// The day `n` of a month: from its start for a positive `n`, from its end for a negative one.
/// `None` when the month has no such day.
fn month_day(year: i32, month: u32, n: i32) -> Option<NaiveDate> {
    let length = days_in_month(year, month)? as i32;
    match n {
        1.. if n <= length => NaiveDate::from_ymd_opt(year, month, n as u32),
        ..=-1 if -n <= length => NaiveDate::from_ymd_opt(year, month, (length + n + 1) as u32),
        _ => None,
    }
}

fn days_in_month(year: i32, month: u32) -> Option<u32> {
    let next = if month == 12 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(year, month + 1, 1)
    }?;
    Some(next.pred_opt()?.day())
}

/// What a monthly (or one month of a yearly) rule picks: the days named by `byMonthDay` and
/// `byDay` (both given: those that satisfy both), else the day of the start.
fn month_dates(rule: &Rule, year: i32, month: u32, start_day: u32) -> Vec<NaiveDate> {
    let by_month_day: Vec<NaiveDate> = rule.by_month_day.iter().filter_map(|n| month_day(year, month, *n)).collect();
    let by_day = weekday_dates(year, month, &rule.by_day);
    let mut found = match (rule.by_month_day.is_empty(), rule.by_day.is_empty()) {
        (false, false) => by_month_day.into_iter().filter(|d| by_day.contains(d)).collect(),
        (false, true) => by_month_day,
        (true, false) => by_day,
        (true, true) => NaiveDate::from_ymd_opt(year, month, start_day).into_iter().collect(),
    };
    found.sort();
    found
}

/// The days of a month that the `byDay` entries name.
fn weekday_dates(year: i32, month: u32, by_day: &[(Weekday, Option<i32>)]) -> Vec<NaiveDate> {
    let days: Vec<NaiveDate> = (1..=31).map_while(|d| NaiveDate::from_ymd_opt(year, month, d)).collect();
    let mut out = Vec::new();
    for (weekday, nth) in by_day {
        let same: Vec<NaiveDate> = days.iter().copied().filter(|d| d.weekday() == *weekday).collect();
        match nth {
            None => out.extend(same),
            Some(n) if *n > 0 => out.extend(same.get(*n as usize - 1)),
            Some(n) => out.extend(same.len().checked_sub(n.unsigned_abs() as usize).and_then(|i| same.get(i))),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::jmap::clock::parse_strict;

    fn at(text: &str) -> NaiveDateTime {
        parse_strict(&format!("{text}T09:30:00")).unwrap()
    }

    /// The dates (without the time) of `rule` from `start` to `limit`.
    fn run(rule: Value, start: &str, limit: &str) -> Vec<String> {
        let found = expand(&json!([rule]), at(start), at(limit)).expect("a rule that can be followed");
        found.iter().map(|d| d.date().to_string()).collect()
    }

    #[test]
    fn daily_with_a_count_an_interval_and_an_end() {
        assert_eq!(
            run(json!({"frequency": "daily", "count": 3}), "2026-11-16", "2026-12-31"),
            ["2026-11-16", "2026-11-17", "2026-11-18"]
        );
        assert_eq!(
            run(
                json!({"frequency": "daily", "interval": 2, "until": "2026-11-22T23:59:59"}),
                "2026-11-16",
                "2026-12-31"
            ),
            ["2026-11-16", "2026-11-18", "2026-11-20", "2026-11-22"]
        );
        // The time of the start is kept, and the limit includes the instance on its own day.
        let found = expand(&json!([{"frequency": "daily"}]), at("2026-11-16"), at("2026-11-17")).unwrap();
        assert_eq!(found.last().unwrap().to_string(), "2026-11-17 09:30:00");
    }

    #[test]
    fn weekly_follows_the_weekday_of_the_start_or_the_listed_days() {
        // 2026-11-16 is a Monday.
        assert_eq!(
            run(json!({"frequency": "weekly"}), "2026-11-16", "2026-12-07"),
            ["2026-11-16", "2026-11-23", "2026-11-30", "2026-12-07"]
        );
        // From a Wednesday, every second week: that week's Wednesday (the start) and Friday, then
        // the Monday, Wednesday and Friday of the week after next.
        let days = json!([{"day": "mo"}, {"day": "we"}, {"day": "fr"}]);
        assert_eq!(
            run(json!({"frequency": "weekly", "byDay": days, "interval": 2}), "2026-11-18", "2026-12-12"),
            ["2026-11-18", "2026-11-20", "2026-11-30", "2026-12-02", "2026-12-04"]
        );
    }

    #[test]
    fn monthly_by_the_day_of_the_month_leaves_out_days_a_month_lacks() {
        assert_eq!(
            run(json!({"frequency": "monthly"}), "2026-01-31", "2026-06-30"),
            ["2026-01-31", "2026-03-31", "2026-05-31"]
        );
        assert_eq!(
            run(json!({"frequency": "monthly", "byMonthDay": [1, -1]}), "2026-11-30", "2027-01-31"),
            ["2026-11-30", "2026-12-01", "2026-12-31", "2027-01-01", "2027-01-31"]
        );
    }

    #[test]
    fn monthly_by_an_ordinal_weekday() {
        let second_tuesday = json!([{"day": "tu", "nthOfPeriod": 2}]);
        assert_eq!(
            run(json!({"frequency": "monthly", "byDay": second_tuesday}), "2026-11-10", "2027-01-31"),
            ["2026-11-10", "2026-12-08", "2027-01-12"]
        );
        let last_friday = json!([{"day": "fr", "nthOfPeriod": -1}]);
        assert_eq!(
            run(json!({"frequency": "monthly", "byDay": last_friday}), "2026-11-27", "2027-01-31"),
            ["2026-11-27", "2026-12-25", "2027-01-29"]
        );
        // A month has no fifth Monday: it is left out.
        let fifth_monday = json!([{"day": "mo", "nthOfPeriod": 5}]);
        assert_eq!(
            run(json!({"frequency": "monthly", "byDay": fifth_monday}), "2026-11-30", "2027-03-31"),
            ["2026-11-30", "2027-03-29"]
        );
    }

    #[test]
    fn monthly_and_yearly_by_a_position_in_the_set() {
        // The common exports: "last Friday" is every Friday, the last of them.
        let fridays = json!([{"day": "fr"}]);
        assert_eq!(
            run(json!({"frequency": "monthly", "byDay": fridays, "bySetPosition": [-1]}), "2026-11-27", "2027-02-28"),
            ["2026-11-27", "2026-12-25", "2027-01-29", "2027-02-26"]
        );
        // "Second Tuesday", and the first and last weekday of the month.
        assert_eq!(
            run(
                json!({"frequency": "monthly", "byDay": [{"day": "tu"}], "bySetPosition": [2]}),
                "2026-11-10",
                "2027-01-31"
            ),
            ["2026-11-10", "2026-12-08", "2027-01-12"]
        );
        let weekdays = json!([{"day": "mo"}, {"day": "tu"}, {"day": "we"}, {"day": "th"}, {"day": "fr"}]);
        assert_eq!(
            run(
                json!({"frequency": "monthly", "byDay": weekdays, "bySetPosition": [1, -1]}),
                "2026-11-02",
                "2026-12-31"
            ),
            ["2026-11-02", "2026-11-30", "2026-12-01", "2026-12-31"]
        );
        // A month with no such position is left out. A yearly rule picks among the dates of all
        // the months it names: the last of the Sundays of March and September is in September.
        assert_eq!(
            run(
                json!({"frequency": "monthly", "byDay": [{"day": "mo"}], "bySetPosition": [5]}),
                "2026-11-30",
                "2027-03-31"
            ),
            ["2026-11-30", "2027-03-29"]
        );
        assert_eq!(
            run(
                json!({"frequency": "yearly", "byMonth": ["3", "9"], "byDay": [{"day": "su"}], "bySetPosition": [-1]}),
                "2026-09-27",
                "2028-12-31"
            ),
            ["2026-09-27", "2027-09-26", "2028-09-24"]
        );
    }

    #[test]
    fn yearly_by_the_date_of_the_start_or_the_listed_months() {
        assert_eq!(
            run(json!({"frequency": "yearly"}), "2024-02-29", "2032-12-31"),
            ["2024-02-29", "2028-02-29", "2032-02-29"]
        );
        let first_monday = json!([{"day": "mo", "nthOfPeriod": 1}]);
        assert_eq!(
            run(
                json!({"frequency": "yearly", "byMonth": ["3", "9"], "byDay": first_monday}),
                "2026-09-07",
                "2027-12-31"
            ),
            ["2026-09-07", "2027-03-01", "2027-09-06"]
        );
    }

    #[test]
    fn rules_are_joined_and_the_start_counts_even_when_the_rule_would_skip_it() {
        let rules = json!([{"frequency": "weekly", "count": 2}, {"frequency": "monthly", "count": 2}]);
        let found = expand(&rules, at("2026-11-16"), at("2027-01-31")).unwrap();
        let dates: Vec<String> = found.iter().map(|d| d.date().to_string()).collect();
        assert_eq!(dates, ["2026-11-16", "2026-11-23", "2026-12-16"]);
        // 2026-11-17 is a Tuesday: the rule says Mondays, but the start is the first instance.
        let on_mondays = json!([{"frequency": "weekly", "byDay": [{"day": "mo"}]}]);
        let found = expand(&on_mondays, at("2026-11-17"), at("2026-11-24")).unwrap();
        assert_eq!(found.iter().map(|d| d.date().to_string()).collect::<Vec<_>>(), ["2026-11-17", "2026-11-23"]);
    }

    #[test]
    fn a_rule_that_never_ends_is_cut_at_the_limit() {
        let found = expand(&json!([{"frequency": "daily"}]), at("2000-01-01"), at("2026-11-30")).unwrap();
        let days = (at("2026-11-30").date() - at("2000-01-01").date()).num_days();
        assert_eq!(found.len() as i64, days + 1);
        assert_eq!(found[0], at("2000-01-01"));
    }

    #[test]
    fn rules_that_say_more_than_this_module_follows_are_refused() {
        let start = at("2026-11-16");
        for rule in [
            json!({"frequency": "hourly"}),
            json!({"frequency": "daily", "interval": 0}),
            json!({"frequency": "daily", "bySetPosition": [1]}),
            json!({"frequency": "weekly", "bySetPosition": [1]}),
            json!({"frequency": "monthly", "bySetPosition": [0]}),
            json!({"frequency": "monthly", "bySetPosition": ["1"]}),
            json!({"frequency": "yearly", "byYearDay": [100]}),
            json!({"frequency": "daily", "byHour": [9]}),
            json!({"frequency": "monthly", "skip": "forward"}),
            json!({"frequency": "monthly", "rscale": "hebrew"}),
            json!({"frequency": "weekly", "firstDayOfWeek": "su"}),
            json!({"frequency": "weekly", "byDay": [{"day": "mo", "nthOfPeriod": 1}]}),
            json!({"frequency": "yearly", "byDay": [{"day": "mo"}]}),
            json!({"frequency": "yearly", "byMonthDay": [1]}),
            json!({"frequency": "monthly", "byMonth": ["5L"]}),
            json!({"frequency": "daily", "until": "soon"}),
            json!({"frequency": "weekly", "byDay": [{"day": "xx"}]}),
            json!({}),
        ] {
            assert_eq!(expand(&json!([rule.clone()]), start, at("2026-12-31")), None, "{rule}");
        }
        assert_eq!(expand(&json!("daily"), start, at("2026-12-31")), None);
    }
}
