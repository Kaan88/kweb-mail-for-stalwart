//! A JSCalendar event as rows of the phone's events table, and back as the units the user may
//! have edited. Pure functions.

use chrono::NaiveDateTime;
use chrono_tz::Tz;
use serde_json::{json, Map, Value};

use super::DeviceEvent;
use crate::{
    jmap::{
        clock::{format_local, parse_local, unix, zone},
        jscal,
    },
    model::EventInput,
};

/// The prefix of the sync id of an event the device made and the server has not seen.
pub(super) const PENDING: &str = "~pending/";

/// The parts of an event the device may change and the sync sends back.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(super) struct Proj {
    pub title: String,
    pub description: String,
    pub location: String,
    pub timing: Timing,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(super) struct Timing {
    pub start_ms: i64,
    pub end_ms: i64,
    pub all_day: bool,
    pub zone: String,
}

impl Timing {
    fn same(&self, other: &Timing) -> bool {
        self.start_ms == other.start_ms
            && self.end_ms == other.end_ms
            && self.all_day == other.all_day
            && (self.all_day || self.zone == other.zone)
    }
}

/// The units, to be told apart one by one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Unit {
    Title,
    Description,
    Location,
    Timing,
}

pub(super) const UNITS: [Unit; 4] = [Unit::Title, Unit::Description, Unit::Location, Unit::Timing];

impl Proj {
    pub(super) fn of(row: &DeviceEvent) -> Proj {
        let end = row.end_ms.unwrap_or(row.start_ms);
        Proj {
            title: row.title.clone(),
            description: row.description.clone(),
            location: row.location.clone(),
            timing: Timing { start_ms: row.start_ms, end_ms: end, all_day: row.all_day, zone: row.time_zone.clone() },
        }
    }

    pub(super) fn same(&self, other: &Proj, unit: Unit) -> bool {
        match unit {
            Unit::Title => self.title == other.title,
            Unit::Description => self.description == other.description,
            Unit::Location => self.location == other.location,
            Unit::Timing => self.timing.same(&other.timing),
        }
    }

    /// Takes one unit over from `from`.
    pub(super) fn replace(&mut self, from: &Proj, unit: Unit) {
        match unit {
            Unit::Title => self.title = from.title.clone(),
            Unit::Description => self.description = from.description.clone(),
            Unit::Location => self.location = from.location.clone(),
            Unit::Timing => self.timing = from.timing.clone(),
        }
    }

    pub(super) fn parse(text: &str) -> Option<Proj> {
        serde_json::from_str(text).ok()
    }

    pub(super) fn text(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

/// Copies one unit of `from` (a device row) into `to`.
pub(super) fn copy_unit(to: &mut DeviceEvent, from: &DeviceEvent, unit: Unit) {
    match unit {
        Unit::Title => to.title = from.title.clone(),
        Unit::Description => to.description = from.description.clone(),
        Unit::Location => to.location = from.location.clone(),
        Unit::Timing => {
            to.start_ms = from.start_ms;
            to.end_ms = from.end_ms;
            to.duration = from.duration.clone();
            to.all_day = from.all_day;
            to.time_zone = from.time_zone.clone();
        }
    }
}

/// What an event becomes on the phone.
pub(super) struct Mapped {
    /// The event itself first, then one row per changed occurrence.
    pub rows: Vec<DeviceEvent>,
    /// The phone's edits of it are put back (it repeats, or something in it cannot be shown).
    pub read_only: bool,
}

pub(super) fn event_id(event: &Value) -> Option<&str> {
    event["id"].as_str()
}

/// The sync id of an event of the account `owner`.
pub(super) fn sync_id(owner: &str, id: &str) -> String {
    format!("{owner}/{id}")
}

pub(super) fn exception_sync_id(owner: &str, id: &str, key: &str) -> String {
    format!("{owner}/{id}#{key}")
}

/// `(event id, recurrence key)` of a sync id of the account `owner`; `None` for others' ids and
/// pending ones.
pub(super) fn parse_sync_id<'a>(owner: &str, sync: &'a str) -> Option<(&'a str, Option<&'a str>)> {
    let rest = sync.strip_prefix(owner)?.strip_prefix('/')?;
    match rest.split_once('#') {
        Some((id, key)) => Some((id, Some(key))),
        None => Some((rest, None)),
    }
}

/// The event's start, length and zone as the provider holds them. `device` places a floating time.
pub(super) fn timing_of(input: &EventInput, device: Tz) -> Option<Timing> {
    let local = parse_local(&input.start)?;
    if input.all_day {
        let start = local.date().and_hms_opt(0, 0, 0)?.and_utc().timestamp() * 1000;
        let days = i64::try_from(input.duration_secs.div_ceil(86_400)).ok()?.max(1);
        return Some(Timing { start_ms: start, end_ms: start + days * 86_400_000, all_day: true, zone: "UTC".into() });
    }
    let zone = input.time_zone.as_deref().and_then(zone).unwrap_or(device);
    let start = unix(local, zone) * 1000;
    let end = start + i64::try_from(input.duration_secs).ok()? * 1000;
    Some(Timing { start_ms: start, end_ms: end, all_day: false, zone: zone.name().to_string() })
}

/// The rows of an event of the account `owner` in the calendar `calendar_sync`. `None` for what is
/// not mirrored: a task, or an event with no time that can be read.
pub(super) fn map_event(event: &Value, owner: &str, calendar_sync: &str, device: Tz) -> Option<Mapped> {
    if jscal::is_task(event) {
        return None;
    }
    let id = event_id(event)?;
    let input = jscal::detail(event, &[], &[])?.input;
    let timing = timing_of(&input, device)?;
    let master_sync = sync_id(owner, id);
    let rules = jscal::rules_of(event);
    let repeating = !rules.is_empty();
    let rule = if repeating && !jscal::has_excluding_rules(event) && rules.len() == 1 {
        rrule(rules[0], timing.all_day, zone_of(&timing))
    } else {
        None
    };
    let series = rule.is_some();
    let mut master = base_row(event, &input, &timing, calendar_sync, &master_sync);
    let mut rows = Vec::new();
    if let Some(rule) = rule {
        master.rrule = Some(rule);
        master.end_ms = None;
        let secs = i64::try_from(input.duration_secs).unwrap_or(0);
        master.duration = Some(if timing.all_day {
            format!("P{}D", ((timing.end_ms - timing.start_ms) / 86_400_000).max(1))
        } else {
            format!("P{}S", secs.max(0))
        });
        let mut excluded = Vec::new();
        let mut changed = Vec::new();
        for (key, patch) in event["recurrenceOverrides"].as_object().into_iter().flatten() {
            let Some(at) = parse_local(key) else { continue };
            if patch["excluded"] == true {
                excluded.push(instant_text(at, &timing));
            } else if patch.is_object() {
                changed.push((key.clone(), at, patch));
            }
        }
        excluded.sort();
        if !excluded.is_empty() {
            master.exdate = Some(excluded.join(","));
        }
        rows.push(master);
        for (key, at, patch) in changed {
            if let Some(row) = exception_row(event, &key, at, patch, owner, calendar_sync, &timing, device) {
                rows.push(row);
            }
        }
    } else {
        rows.push(master);
    }
    let _ = series;
    Some(Mapped { rows, read_only: repeating })
}

fn zone_of(timing: &Timing) -> Tz {
    zone(&timing.zone).unwrap_or(Tz::UTC)
}

/// A recurrence id as EXDATE has it: a UTC date-time, or a date for an all-day series.
fn instant_text(at: NaiveDateTime, timing: &Timing) -> String {
    if timing.all_day {
        return at.format("%Y%m%d").to_string();
    }
    let instant = unix(at, zone_of(timing));
    chrono::DateTime::from_timestamp(instant, 0).unwrap_or_default().format("%Y%m%dT%H%M%SZ").to_string()
}

fn base_row(event: &Value, input: &EventInput, timing: &Timing, calendar_sync: &str, sync: &str) -> DeviceEvent {
    DeviceEvent {
        row_id: None,
        sync_id: Some(sync.to_string()),
        calendar_sync_id: calendar_sync.to_string(),
        uid: event["uid"].as_str().map(str::to_string),
        title: input.title.clone(),
        description: input.description.clone(),
        location: input.location.clone(),
        start_ms: timing.start_ms,
        end_ms: Some(timing.end_ms),
        duration: None,
        all_day: timing.all_day,
        time_zone: timing.zone.clone(),
        rrule: None,
        exdate: None,
        status: match event["status"].as_str() {
            Some("tentative") => 0,
            Some("cancelled") => 2,
            _ => 1,
        },
        availability: i32::from(event["freeBusyStatus"] == "free"),
        access_level: match event["privacy"].as_str() {
            Some("public") => 3,
            Some("private") => 2,
            Some("secret") => 1,
            _ => 0,
        },
        original_sync_id: None,
        original_instance_ms: None,
        original_all_day: false,
        dirty: false,
        deleted: false,
        shadow: None,
        baseline: None,
    }
}

#[allow(clippy::too_many_arguments)]
fn exception_row(
    event: &Value,
    key: &str,
    at: NaiveDateTime,
    patch: &Value,
    owner: &str,
    calendar_sync: &str,
    series: &Timing,
    device: Tz,
) -> Option<DeviceEvent> {
    let id = event_id(event)?;
    let mut patched = apply_patch(event, patch);
    let fields = patched.as_object_mut()?;
    fields.remove("recurrenceRule");
    fields.remove("recurrenceRules");
    fields.remove("recurrenceOverrides");
    if patch.get("start").is_none() {
        fields.insert("start".into(), json!(key));
    }
    let input = jscal::detail(&patched, &[], &[])?.input;
    let timing = timing_of(&input, device)?;
    let mut row = base_row(&patched, &input, &timing, calendar_sync, &exception_sync_id(owner, id, key));
    row.original_sync_id = Some(sync_id(owner, id));
    row.original_instance_ms = Some(unix(at, zone_of(series)) * 1000);
    if series.all_day {
        row.original_instance_ms = Some(at.date().and_hms_opt(0, 0, 0)?.and_utc().timestamp() * 1000);
    }
    row.original_all_day = series.all_day;
    Some(row)
}

/// A JSCalendar patch applied to a copy of the object: each key is a path of `/`-separated names, null removes.
pub(super) fn apply_patch(event: &Value, patch: &Value) -> Value {
    let mut out = event.clone();
    for (path, value) in patch.as_object().into_iter().flatten() {
        let parts: Vec<String> = path.split('/').map(|p| p.replace("~1", "/").replace("~0", "~")).collect();
        let Some((last, parents)) = parts.split_last() else { continue };
        let mut at = &mut out;
        for part in parents {
            if !at.is_object() {
                break;
            }
            at = at.as_object_mut().expect("checked").entry(part.clone()).or_insert_with(|| json!({}));
        }
        if let Some(object) = at.as_object_mut() {
            if value.is_null() {
                object.remove(last);
            } else {
                object.insert(last.clone(), value.clone());
            }
        }
    }
    out
}

/// A repeat rule in RFC 5545 form, or `None` for one the provider cannot hold (another calendar
/// than the Gregorian, a skip, a leap month).
pub(super) fn rrule(rule: &Value, all_day: bool, zone: Tz) -> Option<String> {
    let fields: &Map<String, Value> = rule.as_object()?;
    let text = |k: &str| fields.get(k).and_then(Value::as_str);
    if text("rscale").is_some_and(|r| !r.eq_ignore_ascii_case("gregorian")) || text("skip").is_some_and(|s| s != "omit")
    {
        return None;
    }
    let freq = match text("frequency")? {
        "yearly" => "YEARLY",
        "monthly" => "MONTHLY",
        "weekly" => "WEEKLY",
        "daily" => "DAILY",
        "hourly" => "HOURLY",
        "minutely" => "MINUTELY",
        "secondly" => "SECONDLY",
        _ => return None,
    };
    let mut parts = vec![format!("FREQ={freq}")];
    let numbers = |k: &str| -> Option<Vec<String>> {
        let list = fields.get(k)?.as_array()?;
        let out: Vec<String> = list.iter().filter_map(Value::as_i64).map(|n| n.to_string()).collect();
        (!out.is_empty()).then_some(out)
    };
    match fields.get("interval").and_then(Value::as_u64) {
        Some(n) if n > 1 => parts.push(format!("INTERVAL={n}")),
        _ => {}
    }
    if let Some(n) = fields.get("count").and_then(Value::as_u64) {
        parts.push(format!("COUNT={n}"));
    }
    if let Some(until) = text("until").and_then(parse_local) {
        parts.push(if all_day {
            format!("UNTIL={}", until.format("%Y%m%d"))
        } else {
            let at = chrono::DateTime::from_timestamp(unix(until, zone), 0)?;
            format!("UNTIL={}", at.format("%Y%m%dT%H%M%SZ"))
        });
    }
    if let Some(days) = fields.get("byDay").and_then(Value::as_array) {
        let mut out = Vec::new();
        for entry in days {
            let day = entry["day"].as_str()?.to_uppercase();
            let ordinal = entry.get("nthOfPeriod").and_then(Value::as_i64).filter(|n| *n != 0);
            out.push(ordinal.map_or(day.clone(), |n| format!("{n}{day}")));
        }
        if !out.is_empty() {
            parts.push(format!("BYDAY={}", out.join(",")));
        }
    }
    if let Some(months) = fields.get("byMonth").and_then(Value::as_array) {
        let mut out = Vec::new();
        for month in months {
            let text = month.as_str().map(str::to_string).or_else(|| month.as_i64().map(|n| n.to_string()))?;
            // A leap month ("6L") has no RFC 5545 form.
            text.parse::<u32>().ok()?;
            out.push(text);
        }
        if !out.is_empty() {
            parts.push(format!("BYMONTH={}", out.join(",")));
        }
    }
    for (key, name) in [
        ("byMonthDay", "BYMONTHDAY"),
        ("byYearDay", "BYYEARDAY"),
        ("byWeekNo", "BYWEEKNO"),
        ("byHour", "BYHOUR"),
        ("byMinute", "BYMINUTE"),
        ("bySecond", "BYSECOND"),
        ("bySetPosition", "BYSETPOS"),
    ] {
        if let Some(list) = numbers(key) {
            parts.push(format!("{name}={}", list.join(",")));
        }
    }
    if let Some(day) = text("firstDayOfWeek").filter(|d| *d != "mo") {
        parts.push(format!("WKST={}", day.to_uppercase()));
    }
    Some(parts.join(";"))
}

/// The input of an event the device made, for the server. `None` when its time cannot be read or it
/// repeats (a series made on the phone is not sent in this version).
pub(super) fn input_of_new(row: &DeviceEvent, calendar_id: &str, device: Tz) -> Option<EventInput> {
    if row.rrule.is_some() {
        return None;
    }
    let (start, duration, zone_name) = place_of(row, None, device)?;
    Some(EventInput {
        calendar_id: calendar_id.to_string(),
        title: row.title.trim().to_string(),
        location: row.location.trim().to_string(),
        description: row.description.clone(),
        start,
        time_zone: zone_name,
        duration_secs: duration,
        all_day: row.all_day,
        is_task: false,
        repeat: None,
        alert_minutes: Vec::new(),
    })
}

/// A row's time as the server has it: the local start, the length in seconds and the zone name
/// (`None` for a floating time). `current` is the event as the server has it now: its floating time stays floating
/// while the row is in the device's zone, and an all-day event keeps the zone it has.
pub(super) fn place_of(
    row: &DeviceEvent,
    current: Option<&EventInput>,
    device: Tz,
) -> Option<(String, u64, Option<String>)> {
    let end = row.end_ms.unwrap_or(row.start_ms);
    let length = u64::try_from(((end - row.start_ms) / 1000).max(0)).ok()?;
    if row.all_day {
        let date = chrono::DateTime::from_timestamp(row.start_ms / 1000, 0)?.naive_utc().date();
        let days = length.div_ceil(86_400).max(1);
        let zone_name = current.and_then(|c| c.time_zone.clone());
        return Some((format_local(date.and_hms_opt(0, 0, 0)?), days * 86_400, zone_name));
    }
    let zone_of_row = zone(&row.time_zone).unwrap_or(device);
    let floating = current.is_some_and(|c| c.time_zone.is_none()) && zone_of_row == device;
    let local = crate::jmap::clock::local_naive(row.start_ms / 1000, zone_of_row);
    Some((format_local(local), length, (!floating).then(|| zone_of_row.name().to_string())))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEVICE: Tz = chrono_tz::Europe::Istanbul;

    fn event(extra: Value) -> Value {
        let mut base = json!({
            "id": "e1", "uid": "u1", "@type": "Event", "title": "Lunch", "start": "2026-11-20T12:00:00",
            "timeZone": "Europe/Istanbul", "duration": "PT1H", "calendarIds": {"cal1": true},
        });
        for (k, v) in extra.as_object().unwrap() {
            base[k] = v.clone();
        }
        base
    }

    #[test]
    fn a_timed_event_becomes_one_row_with_an_end() {
        let m = map_event(&event(json!({})), "acct", "acct/cal1", DEVICE).unwrap();
        assert!(!m.read_only);
        let row = &m.rows[0];
        assert_eq!(row.sync_id.as_deref(), Some("acct/e1"));
        assert_eq!(row.start_ms, 1_795_165_200_000);
        assert_eq!(row.end_ms, Some(row.start_ms + 3_600_000));
        assert_eq!(row.time_zone, "Europe/Istanbul");
        assert!(row.rrule.is_none() && !row.all_day);
    }

    #[test]
    fn an_all_day_event_is_utc_midnight_for_whole_days() {
        let e = event(
            json!({"start": "2026-11-20T00:00:00", "showWithoutTime": true, "duration": "P2D", "timeZone": null}),
        );
        let row = map_event(&e, "a", "a/c", DEVICE).unwrap().rows.remove(0);
        assert!(row.all_day && row.time_zone == "UTC");
        assert_eq!(row.end_ms, Some(row.start_ms + 2 * 86_400_000));
    }

    #[test]
    fn a_floating_event_is_placed_in_the_device_zone() {
        let e = event(json!({"timeZone": null}));
        let row = map_event(&e, "a", "a/c", DEVICE).unwrap().rows.remove(0);
        assert_eq!(row.time_zone, "Europe/Istanbul");
        assert_eq!(row.start_ms, 1_795_165_200_000);
    }

    #[test]
    fn a_series_keeps_its_rule_exdate_and_exceptions_and_is_read_only() {
        let e = event(json!({
            "recurrenceRule": {"@type": "RecurrenceRule", "frequency": "weekly", "byDay": [{"day": "fr"}], "count": 5},
            "recurrenceOverrides": {
                "2026-11-27T12:00:00": {"excluded": true},
                "2026-12-04T12:00:00": {"title": "Moved lunch", "start": "2026-12-04T13:00:00"},
            },
        }));
        let m = map_event(&e, "a", "a/c", DEVICE).unwrap();
        assert!(m.read_only);
        assert_eq!(m.rows.len(), 2);
        let master = &m.rows[0];
        assert_eq!(master.rrule.as_deref(), Some("FREQ=WEEKLY;COUNT=5;BYDAY=FR"));
        assert_eq!(master.exdate.as_deref(), Some("20261127T090000Z"));
        assert_eq!((master.end_ms, master.duration.as_deref()), (None, Some("P3600S")));
        let exception = &m.rows[1];
        assert_eq!(exception.sync_id.as_deref(), Some("a/e1#2026-12-04T12:00:00"));
        assert_eq!(exception.original_sync_id.as_deref(), Some("a/e1"));
        assert_eq!(exception.title, "Moved lunch");
        assert_eq!(exception.start_ms, 1_796_378_400_000);
        assert_eq!(exception.original_instance_ms, Some(1_796_374_800_000));
        assert!(exception.rrule.is_none());
    }

    #[test]
    fn a_rule_the_provider_cannot_hold_is_shown_once_and_read_only() {
        let e = event(json!({"recurrenceRule": {"frequency": "yearly", "rscale": "hebrew"}}));
        let m = map_event(&e, "a", "a/c", DEVICE).unwrap();
        assert!(m.read_only && m.rows.len() == 1 && m.rows[0].rrule.is_none());
    }

    #[test]
    fn a_task_is_not_mirrored() {
        assert!(map_event(&event(json!({"@type": "Task", "due": "2026-11-20T12:00:00"})), "a", "a/c", DEVICE).is_none());
    }

    #[test]
    fn rules_in_rfc_5545_form() {
        let r = |v: Value| rrule(&v, false, DEVICE);
        assert_eq!(
            r(json!({"frequency": "monthly", "interval": 2, "byDay": [{"day": "mo", "nthOfPeriod": -1}], "until": "2027-01-01T00:00:00"})).as_deref(),
            Some("FREQ=MONTHLY;INTERVAL=2;UNTIL=20261231T210000Z;BYDAY=-1MO")
        );
        assert_eq!(
            rrule(&json!({"frequency": "daily", "until": "2027-01-01T00:00:00"}), true, DEVICE).as_deref(),
            Some("FREQ=DAILY;UNTIL=20270101")
        );
        assert!(r(json!({"frequency": "yearly", "byMonth": ["6L"]})).is_none());
        assert!(r(json!({"frequency": "daily", "skip": "forward"})).is_none());
        assert_eq!(
            r(json!({"frequency": "yearly", "byMonth": ["3"], "byMonthDay": [14], "firstDayOfWeek": "su"})).as_deref(),
            Some("FREQ=YEARLY;BYMONTH=3;BYMONTHDAY=14;WKST=SU")
        );
    }

    #[test]
    fn the_device_time_goes_back_as_the_server_has_it() {
        let row = map_event(&event(json!({})), "a", "a/c", DEVICE).unwrap().rows.remove(0);
        let current = jscal::current_input(&event(json!({}))).unwrap();
        let (start, secs, zone) = place_of(&row, Some(&current), DEVICE).unwrap();
        assert_eq!((start.as_str(), secs, zone.as_deref()), ("2026-11-20T12:00:00", 3600, Some("Europe/Istanbul")));
        // A floating event stays floating while the phone is in the zone it was placed in.
        let floating = event(json!({"timeZone": null}));
        let row = map_event(&floating, "a", "a/c", DEVICE).unwrap().rows.remove(0);
        let current = jscal::current_input(&floating).unwrap();
        assert_eq!(place_of(&row, Some(&current), DEVICE).unwrap().2, None);
    }

    #[test]
    fn sync_ids_carry_the_account() {
        assert_eq!(parse_sync_id("acct", "acct/e1"), Some(("e1", None)));
        assert_eq!(parse_sync_id("acct", "acct/e1#2026-12-04T12:00:00"), Some(("e1", Some("2026-12-04T12:00:00"))));
        assert_eq!(parse_sync_id("acct", "other/e1"), None);
        assert_eq!(parse_sync_id("acct", "~pending/u"), None);
    }
}
