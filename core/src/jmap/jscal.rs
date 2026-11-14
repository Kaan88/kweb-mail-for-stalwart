//! JSCalendar (RFC 8984) objects as JMAP Calendars carries them, and the app's types.
//!
//! Pure functions: no network, no cache. What comes in is the JSON object the server gave for
//! an event or a task; what goes out is an agenda entry, the detail of one event, or the JSON
//! to create or change one. Times stay wall-clock times of a zone until an instant is needed.
//!
//! Where the real server (Stalwart 1.1) differs from the drafts, the code follows the server;
//! each such place says so.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{Duration, NaiveDateTime};
use chrono_tz::Tz;
use serde_json::{json, Map, Value};

use super::{
    calendar::first_location,
    clock::{
        format_duration, format_local, local_date, local_naive, parse_duration, parse_duration_split, parse_local,
        parse_strict, unix, zone,
    },
    recur,
};
use crate::{
    model::{Calendar, EventDetail, EventInput, Frequency, Occurrence, Participant, RepeatRule},
    Error, Result,
};

const WEEKDAYS: [&str; 7] = ["mo", "tu", "we", "th", "fr", "sa", "su"];
const ANSWERS: [&str; 3] = ["accepted", "tentative", "declined"];

fn text<'a>(object: &'a Value, key: &str) -> Option<&'a str> {
    object.get(key)?.as_str().filter(|s| !s.is_empty())
}

fn is_task(event: &Value) -> bool {
    event["@type"] == "Task"
}

/// The ids of the calendars an event is in.
fn calendar_ids(event: &Value) -> Vec<&str> {
    let ids = event["calendarIds"].as_object().into_iter().flatten();
    ids.filter(|(_, member)| **member == true).map(|(id, _)| id.as_str()).collect()
}

/// When an event starts, or a task is due (a task's start when it has no due time).
fn start_text(event: &Value, task: bool) -> Option<&str> {
    let due = if task { text(event, "due") } else { None };
    due.or_else(|| text(event, "start"))
}

/// A date without a time, or `showWithoutTime`.
fn is_all_day(event: &Value, start: &str) -> bool {
    event["showWithoutTime"] == true || !start.contains('T')
}

// ---- reading ----

/// Where an agenda is read.
pub struct View<'a> {
    /// The zone of the phone: all-day entries and floating times are placed in it.
    pub zone: Tz,
    /// The calendars whose events are shown.
    pub visible: &'a BTreeSet<String>,
    /// The user's own addresses, lower case.
    pub me: &'a [String],
}

/// An event or one of its occurrences.
struct Instance<'a> {
    /// The recurrence id: the original start as a local date-time. `None` for an event that does not repeat.
    id: Option<String>,
    start: NaiveDateTime,
    /// What this occurrence changes (an override), if anything.
    patch: Option<&'a Value>,
}

/// What an event's times are read against.
struct Shape {
    task: bool,
    all_day: bool,
    /// The zone its wall-clock times are in: its own, else the reader's.
    zone: Tz,
    repeats: bool,
    /// The series' first start, which recurrence ids are counted from: a task's start when it
    /// has one, else its due time.
    first: NaiveDateTime,
    /// How long after `first` a task is due; zero unless it has both a start and a due time.
    offset: Duration,
}

/// `None` for an event without a time that can be read. Floating times and all-day entries
/// have no zone of their own: they are read in `reader`'s.
fn shape(event: &Value, reader: Tz) -> Option<Shape> {
    let task = is_task(event);
    let begin = start_text(event, task)?;
    let time = |key: &str| text(event, key).and_then(parse_local);
    let first = if task { time("start").or_else(|| time("due"))? } else { time("start")? };
    let offset = match (task, time("start"), time("due")) {
        (true, Some(start), Some(due)) => due - start,
        _ => Duration::zero(),
    };
    let all_day = is_all_day(event, begin);
    let zone = if all_day { None } else { text(event, "timeZone").and_then(zone) }.unwrap_or(reader);
    let repeats = event["recurrenceRules"].as_array().is_some_and(|rules| !rules.is_empty());
    Some(Shape { task, all_day, zone, repeats, first, offset })
}

/// Where an occurrence is: its start (a task: its due time) as a local date-time. An override
/// may move it; for a task a moved `start` carries the due time along.
fn placed(shape: &Shape, patch: Option<&Value>, at: NaiveDateTime) -> NaiveDateTime {
    let own = |key: &str| patch.and_then(|p| text(p, key)).and_then(parse_local);
    let later = |t: NaiveDateTime| t.checked_add_signed(shape.offset).unwrap_or(t);
    if shape.task {
        own("due").or_else(|| own("start").map(later)).unwrap_or_else(|| later(at))
    } else {
        own("start").unwrap_or(at)
    }
}

/// The instant an occurrence ends. Days are added to the wall clock and the hours after the
/// conversion, so that a stay of days across a clock change still ends at the same time of day.
/// An end that cannot be computed (a duration of billions of years) leaves it without a length.
fn end_instant(shape: &Shape, at: NaiveDateTime, duration: &str, starts_at: i64) -> i64 {
    if shape.task {
        return starts_at;
    }
    let (date, time) = parse_duration_split(duration).unwrap_or((0, 0));
    let secs = |n: u64| i64::try_from(n).ok().and_then(Duration::try_seconds);
    let end = if shape.all_day {
        let local = date.checked_add(time).and_then(secs).and_then(|d| at.checked_add_signed(d));
        local.map(|local| unix(local, shape.zone))
    } else {
        let local = secs(date).and_then(|d| at.checked_add_signed(d)).map(|local| unix(local, shape.zone));
        local.zip(i64::try_from(time).ok()).and_then(|(base, time)| base.checked_add(time))
    };
    end.unwrap_or(starts_at)
}

/// The agenda entries of one event that fall in `[from, to)`: one for an event that does not
/// repeat, one per occurrence for one that does. Nothing for an event that is cancelled, in a
/// calendar that is not shown, or without a time; an occurrence that is excluded or cancelled
/// is left out.
///
/// Stalwart 1.1 does not expand repeating events when asked (`expandRecurrences` finds the
/// series only by its first start), so the occurrences are worked out here from the rules.
pub fn occurrences(event: &Value, from: i64, to: i64, view: &View) -> Vec<Occurrence> {
    let Some(id) = text(event, "id") else { return Vec::new() };
    let Some(shape) = shape(event, view.zone) else { return Vec::new() };
    let Some(calendar_id) = calendar_ids(event).into_iter().find(|c| view.visible.contains(*c)) else {
        return Vec::new();
    };
    if event["status"] == "cancelled" {
        return Vec::new();
    }
    // Local times are at most a day away from the instant, whatever the zone.
    let limit = local_naive(to, shape.zone) + Duration::days(1);

    let mut out = Vec::new();
    for instance in instances(event, &shape, limit) {
        let patch = instance.patch.filter(|p| p.is_object());
        if patch.is_some_and(|p| p["excluded"] == true) {
            continue;
        }
        let field = |key: &str| patch.and_then(|p| p.get(key)).or_else(|| event.get(key));
        if field("status").and_then(Value::as_str) == Some("cancelled") {
            continue;
        }
        let at = placed(&shape, patch, instance.start);
        let starts_at = unix(at, shape.zone);
        let ends_at = end_instant(&shape, at, field("duration").and_then(Value::as_str).unwrap_or_default(), starts_at);
        // An entry without a length is a point in time.
        if !(starts_at < to && ends_at.max(starts_at.saturating_add(1)) > from) {
            continue;
        }
        out.push(Occurrence {
            event_id: id.to_string(),
            recurrence_id: instance.id,
            calendar_id: calendar_id.to_string(),
            title: field("title").and_then(Value::as_str).unwrap_or_default().to_string(),
            location: first_location(event, patch),
            starts_at,
            ends_at,
            all_day: shape.all_day,
            is_task: shape.task,
            is_done: field("progress").and_then(Value::as_str) == Some("completed"),
            is_recurring: shape.repeats,
            my_status: my_status_in(event, patch, view.me),
        });
    }
    out
}

/// The occurrences of an event that can start before `limit`: the rules' own, and every one an
/// override names (it may move into the span from a start far outside it; the caller keeps what
/// falls in the span). A rule that is not understood leaves just the first one.
fn instances<'a>(event: &'a Value, shape: &Shape, limit: NaiveDateTime) -> Vec<Instance<'a>> {
    if !shape.repeats {
        return vec![Instance { id: None, start: shape.first, patch: None }];
    }
    let starts = recur::expand(&event["recurrenceRules"], shape.first, limit).unwrap_or_else(|| vec![shape.first]);
    let overrides = event["recurrenceOverrides"].as_object();
    let mut out: Vec<Instance> = starts
        .into_iter()
        .map(|start| {
            let id = format_local(start);
            let patch = overrides.and_then(|o| o.get(&id));
            Instance { id: Some(id), start, patch }
        })
        .collect();
    let known: BTreeSet<String> = out.iter().filter_map(|i| i.id.clone()).collect();
    for (key, patch) in overrides.into_iter().flatten() {
        if let (false, Some(start)) = (known.contains(key), parse_local(key)) {
            out.push(Instance { id: Some(key.clone()), start, patch: Some(patch) });
        }
    }
    out
}

/// One occurrence by its recurrence id: one the rules produce or an override adds.
fn find_instance<'a>(event: &'a Value, shape: &Shape, recurrence_id: &str) -> Option<Instance<'a>> {
    let overrides = event["recurrenceOverrides"].as_object();
    if let Some((key, patch)) = overrides.and_then(|o| o.get_key_value(recurrence_id)) {
        return Some(Instance { id: Some(key.clone()), start: parse_local(key)?, patch: Some(patch) });
    }
    let at = parse_local(recurrence_id)?;
    let starts = recur::expand(&event["recurrenceRules"], shape.first, at).unwrap_or_else(|| vec![shape.first]);
    let id = format_local(at);
    starts.contains(&at).then(|| Instance { patch: overrides.and_then(|o| o.get(&id)), id: Some(id), start: at })
}

/// Entries by start time, all-day entries first within a day (the day in `zone`).
pub fn sort(entries: &mut [Occurrence], zone: Tz) {
    entries.sort_by_cached_key(|e| {
        (
            local_date(e.starts_at, zone),
            !e.all_day,
            e.starts_at,
            e.ends_at,
            e.title.clone(),
            e.event_id.clone(),
            e.recurrence_id.clone(),
        )
    });
}

/// `mailto:` taken off an address.
pub(super) fn strip_mailto(address: &str) -> &str {
    let address = address.trim();
    if address.len() >= 7 && address.as_bytes()[..7].eq_ignore_ascii_case(b"mailto:") {
        &address[7..]
    } else {
        address
    }
}

/// A participant's address: `calendarAddress` (newer drafts), else where scheduling messages
/// go (`sendTo`, which is what Stalwart 1.1 writes).
fn address_of(participant: &Value) -> Option<&str> {
    let send_to = participant["sendTo"].as_object().into_iter().flat_map(Map::values);
    let found = text(participant, "calendarAddress")
        .or_else(|| participant["sendTo"].get("imip").and_then(Value::as_str))
        .or_else(|| send_to.filter_map(Value::as_str).next());
    found.map(strip_mailto).filter(|a| !a.is_empty())
}

fn participants(event: &Value, me: &[String]) -> Vec<Participant> {
    let mut out: Vec<Participant> = Vec::new();
    for participant in event["participants"].as_object().into_iter().flat_map(Map::values) {
        let (name, email) =
            (text(participant, "name").unwrap_or_default(), address_of(participant).unwrap_or_default());
        if name.is_empty() && email.is_empty() {
            continue;
        }
        out.push(Participant {
            name: name.to_string(),
            email: email.to_string(),
            status: text(participant, "participationStatus").unwrap_or("needs-action").to_string(),
            is_organizer: participant["roles"]["owner"] == true,
            is_me: me.contains(&email.to_lowercase()),
        });
    }
    out.sort_by_cached_key(|p| (!p.is_organizer, p.name.to_lowercase(), p.email.to_lowercase()));
    out
}

/// The user's own answer, when they are a participant.
fn my_status(event: &Value, me: &[String]) -> Option<String> {
    participants(event, me).into_iter().find(|p| p.is_me).map(|p| p.status)
}

/// The user's answer to one occurrence: an override that patches their `participationStatus`
/// (as a path or inside a whole `participants` map) wins over the series'.
fn my_status_in(event: &Value, patch: Option<&Value>, me: &[String]) -> Option<String> {
    let overridden = patch.and_then(|patch| {
        let id = my_participant_id(event, me);
        let by_path = id.and_then(|id| {
            let whole = patch.get(format!("participants/{id}")).and_then(|p| text(p, "participationStatus"));
            whole.or_else(|| text(patch, &format!("participants/{id}/participationStatus")))
        });
        let mine = |p: &&Value| address_of(p).is_some_and(|a| me.contains(&a.to_lowercase()));
        let in_map = patch["participants"].as_object().into_iter().flat_map(Map::values).find(mine);
        by_path.or_else(|| in_map.and_then(|p| text(p, "participationStatus"))).map(str::to_string)
    });
    overridden.or_else(|| my_status(event, me))
}

/// The id of the participant that is the user.
fn my_participant_id(event: &Value, me: &[String]) -> Option<String> {
    let mut found = event["participants"].as_object().into_iter().flatten();
    found.find(|(_, p)| address_of(p).is_some_and(|a| me.contains(&a.to_lowercase()))).map(|(id, _)| id.clone())
}

/// What the app can show and edit of an event, with whether the rest is left alone.
pub fn detail(event: &Value, calendars: &[Calendar], me: &[String]) -> Option<EventDetail> {
    let event_id = text(event, "id")?.to_string();
    let task = is_task(event);
    let begin = start_text(event, task).unwrap_or_default();
    let all_day = !begin.is_empty() && is_all_day(event, begin);
    let (repeat, has_complex_repeat) = repeat_of(event);
    let calendar_id = calendar_ids(event).first().map(|c| c.to_string()).unwrap_or_default();
    let duration = event.get("duration").and_then(Value::as_str).and_then(parse_duration);
    let input = EventInput {
        calendar_id: calendar_id.clone(),
        title: text(event, "title").unwrap_or_default().to_string(),
        location: first_location(event, None),
        description: text(event, "description").unwrap_or_default().to_string(),
        start: parse_local(begin).map(format_local).unwrap_or_default(),
        time_zone: text(event, "timeZone").map(str::to_string),
        duration_secs: if task { 0 } else { duration.unwrap_or(0) },
        all_day,
        is_task: task,
        repeat,
        alert_minutes: own_alerts(event).into_values().collect::<BTreeSet<_>>().into_iter().collect(),
    };
    Some(EventDetail {
        event_id,
        input,
        is_done: text(event, "progress") == Some("completed"),
        participants: participants(event, me),
        my_status: my_status(event, me),
        has_complex_repeat,
        may_write: calendars.iter().any(|c| c.id == calendar_id && c.may_write),
    })
}

/// The detail of one occurrence of a repeating event, built the way the agenda builds its
/// entries: the override's changes to the time, length, title, place, description and the
/// user's answer are applied. `input.repeat` and the rest stay the series'. An occurrence the
/// series does not have, or that is excluded, is `Error::Invalid`.
pub fn detail_at(
    event: &Value,
    calendars: &[Calendar],
    me: &[String],
    recurrence_id: Option<&str>,
) -> Result<EventDetail> {
    let mut found =
        detail(event, calendars, me).ok_or_else(|| Error::temporary("the server's answer is not an event"))?;
    let Some(recurrence_id) = recurrence_id else { return Ok(found) };
    let gone = || Error::invalid("This occurrence does not exist.");
    let shape = shape(event, Tz::UTC).filter(|s| s.repeats).ok_or_else(gone)?;
    let instance = find_instance(event, &shape, recurrence_id).ok_or_else(gone)?;
    let patch = instance.patch.filter(|p| p.is_object());
    if patch.is_some_and(|p| p["excluded"] == true) {
        return Err(gone());
    }
    let input = &mut found.input;
    input.start = format_local(placed(&shape, patch, instance.start));
    let own = |key: &str| patch.and_then(|p| p.get(key)).and_then(Value::as_str);
    if !shape.task {
        input.duration_secs = own("duration").and_then(parse_duration).unwrap_or(input.duration_secs);
    }
    if let Some(title) = own("title") {
        input.title = title.to_string();
    }
    if let Some(description) = own("description") {
        input.description = description.to_string();
    }
    input.location = first_location(event, patch);
    found.is_done = own("progress").map_or(found.is_done, |p| p == "completed");
    found.my_status = my_status_in(event, patch, me);
    for participant in found.participants.iter_mut().filter(|p| p.is_me) {
        participant.status = found.my_status.clone().unwrap_or_else(|| participant.status.clone());
    }
    Ok(found)
}

/// What the app can edit of an event as it is now, in the form [`detail`] reports it.
pub fn current_input(event: &Value) -> Option<EventInput> {
    detail(event, &[], &[]).map(|d| d.input)
}

/// The repeat rule as the app has it, and whether the event has a rule the app cannot hold
/// (then the first is `None`): several rules, a rule with more parts than the app edits, a rule
/// that excludes, or an occurrence that was changed. An occurrence that was only excluded is fine.
fn repeat_of(event: &Value) -> (Option<RepeatRule>, bool) {
    let Some(rules) = event["recurrenceRules"].as_array().filter(|rules| !rules.is_empty()) else {
        return (None, false);
    };
    let only_exclusions = event["recurrenceOverrides"].as_object().is_none_or(|all| {
        all.values()
            .all(|patch| patch.as_object().is_some_and(|p| p.len() == 1 && p.get("excluded") == Some(&json!(true))))
    });
    let excluding_rules = event["excludedRecurrenceRules"].as_array().is_some_and(|r| !r.is_empty());
    match rules.as_slice() {
        [rule] if only_exclusions && !excluding_rules => match simple_rule(rule) {
            Some(rule) => (Some(rule), false),
            None => (None, true),
        },
        _ => (None, true),
    }
}

/// A rule with only a frequency, an interval, weekdays (without ordinals, in a weekly rule) and an end.
fn simple_rule(rule: &Value) -> Option<RepeatRule> {
    const KNOWN: [&str; 9] =
        ["@type", "frequency", "interval", "byDay", "until", "count", "rscale", "skip", "firstDayOfWeek"];
    let fields = rule.as_object()?;
    let default_only = |key: &str, only: &str| fields.get(key).is_none_or(|v| v == only);
    if fields.keys().any(|k| !KNOWN.contains(&k.as_str()))
        || !(default_only("rscale", "gregorian")
            && default_only("skip", "omit")
            && default_only("firstDayOfWeek", "mo"))
    {
        return None;
    }
    let frequency = match fields.get("frequency")?.as_str()? {
        "daily" => Frequency::Daily,
        "weekly" => Frequency::Weekly,
        "monthly" => Frequency::Monthly,
        "yearly" => Frequency::Yearly,
        _ => return None,
    };
    let interval = u32::try_from(fields.get("interval").map_or(Some(1), Value::as_u64)?).ok().filter(|n| *n >= 1)?;
    let mut by_day = Vec::new();
    for entry in fields.get("byDay").map_or(&[][..], |d| d.as_array().map_or(&[][..], Vec::as_slice)) {
        let day = text(entry, "day").filter(|d| WEEKDAYS.contains(d) && entry.get("nthOfPeriod").is_none())?;
        by_day.push(day.to_string());
    }
    if !by_day.is_empty() && frequency != Frequency::Weekly {
        return None;
    }
    let until = match fields.get("until") {
        None => None,
        Some(until) => Some(parse_local(until.as_str()?)?.date().to_string()),
    };
    let count = match fields.get("count") {
        None => None,
        Some(count) => Some(u32::try_from(count.as_u64()?).ok()?),
    };
    Some(RepeatRule { frequency, interval, by_day, until, count })
}

/// What an alert is measured from: the start; for a task, the time the app shows (its due
/// time), which is its end, unless it has a start and no due time.
fn alert_anchor(event: &Value) -> &'static str {
    if is_task(event) && (text(event, "due").is_some() || text(event, "start").is_none()) {
        "end"
    } else {
        "start"
    }
}

/// How many minutes before the anchor an offset such as `-PT15M` is; `PT0S` is at the anchor.
/// `None` for an offset after it and for one that is not a whole number of minutes.
fn minutes_before(offset: &str) -> Option<u32> {
    let secs = match offset.strip_prefix('-') {
        Some(rest) => parse_duration(rest)?,
        None => parse_duration(offset).filter(|secs| *secs == 0)?,
    };
    u32::try_from(secs / 60).ok().filter(|_| secs % 60 == 0)
}

/// The alerts the app owns, as `id -> minutes before`: shown (not mailed) alerts that fire a
/// whole number of minutes before the anchor. Others (after the start, at an absolute time,
/// relative to the other end) stay as the server has them.
fn own_alerts(event: &Value) -> BTreeMap<String, u32> {
    let anchor = alert_anchor(event);
    let mut out = BTreeMap::new();
    for (id, alert) in event["alerts"].as_object().into_iter().flatten() {
        let trigger = &alert["trigger"];
        let shown = alert.get("action").is_none_or(|a| a == "display");
        let measured_from = trigger.get("relativeTo").and_then(Value::as_str).unwrap_or("start");
        let offset = trigger["offset"].as_str().filter(|_| trigger["@type"] == "OffsetTrigger");
        if let (true, true, Some(minutes)) = (shown, measured_from == anchor, offset.and_then(minutes_before)) {
            out.insert(id.clone(), minutes);
        }
    }
    out
}

// ---- writing ----

/// The span of an agenda: a zone name the host knows and an end after the start.
pub fn check_span(from: i64, to: i64, time_zone: &str) -> Result<Tz> {
    if to <= from {
        return Err(Error::invalid("the span must end after it starts"));
    }
    if time_zone.is_empty() || time_zone.contains(' ') {
        return Err(Error::invalid("the time zone must be an IANA name such as Europe/Istanbul"));
    }
    zone(time_zone).ok_or_else(|| Error::invalid(format!("unknown time zone '{time_zone}'")))
}

/// What the user typed, checked so that the message can be shown as it is. `calendars` are the
/// user's calendars: the one chosen has to be among them and writable. `current` is what the
/// event being changed has now: what the user left as it was is not checked again (a zone or an
/// all-day shape the app would not take for a new event must not make the event unchangeable).
pub fn validate(input: &EventInput, calendars: &[Calendar], current: Option<&EventInput>) -> Result<()> {
    let bad = |message: &str| Err(Error::invalid(message));
    let kept = |same: fn(&EventInput, &EventInput) -> bool| current.is_some_and(|c| same(c, input));
    if input.title.trim().is_empty() && !kept(|c, i| c.title == i.title.trim()) {
        return bad("Give it a title.");
    }
    let undated_task = input.is_task && input.start.is_empty();
    let start = parse_strict(&input.start);
    if start.is_none() && !undated_task && !kept(|c, i| c.start == i.start) {
        return bad("The start must be a date and time such as 2026-11-12T10:00:00.");
    }
    if let Some(name) = &input.time_zone {
        if zone(name).is_none() && !kept(|c, i| c.time_zone == i.time_zone) {
            return Err(Error::invalid(format!("Unknown time zone '{name}'.")));
        }
    }
    if input.all_day && !kept(|c, i| (c.all_day, &c.start, c.duration_secs) == (i.all_day, &i.start, i.duration_secs)) {
        let midnight = start.is_none_or(|at| at.time() == chrono::NaiveTime::MIN);
        let whole_days = input.duration_secs.is_multiple_of(86_400) && (input.is_task || input.duration_secs >= 86_400);
        if !midnight || !whole_days {
            return bad("An all-day entry starts at midnight (T00:00:00) and lasts whole days.");
        }
    }
    if undated_task && !input.alert_minutes.is_empty() && !kept(|c, i| c.alert_minutes == i.alert_minutes) {
        return bad("A task needs a due date to have reminders.");
    }
    if let Some(repeat) = &input.repeat {
        if !kept(|c, i| c.repeat == i.repeat) {
            validate_repeat(repeat)?;
        }
    }
    match calendars.iter().find(|c| c.id == input.calendar_id) {
        None => bad("Choose a calendar."),
        Some(c) if !c.may_write => Err(Error::invalid(format!("You cannot change the calendar '{}'.", c.name))),
        Some(_) => Ok(()),
    }
}

fn validate_repeat(repeat: &RepeatRule) -> Result<()> {
    let bad = |message: &str| Err(Error::invalid(message));
    if repeat.interval < 1 {
        return bad("A repeat happens every 1 or more days, weeks, months or years.");
    }
    if let Some(day) = repeat.by_day.iter().find(|d| !WEEKDAYS.contains(&d.as_str())) {
        return Err(Error::invalid(format!("'{day}' is not a weekday (use mo, tu, we, th, fr, sa, su).")));
    }
    if !repeat.by_day.is_empty() && repeat.frequency != Frequency::Weekly {
        return bad("Weekdays can only be chosen for a weekly repeat.");
    }
    if let Some(until) = &repeat.until {
        if until.len() != 10 || parse_strict(&format!("{until}T00:00:00")).is_none() {
            return bad("The end of a repeat must be a date such as 2026-12-31.");
        }
    }
    if repeat.count == Some(0) {
        return bad("A repeat happens at least once.");
    }
    if repeat.until.is_some() && repeat.count.is_some() {
        return bad("A repeat ends on a date or after a number of times, not both.");
    }
    Ok(())
}

fn frequency_name(frequency: Frequency) -> &'static str {
    match frequency {
        Frequency::Daily => "daily",
        Frequency::Weekly => "weekly",
        Frequency::Monthly => "monthly",
        Frequency::Yearly => "yearly",
    }
}

/// The rule as JSCalendar. The end date is the whole of that day: the rule's `until` is a
/// local date-time, so it is the last second of it.
fn rule_json(rule: &RepeatRule) -> Value {
    let mut out = json!({ "@type": "RecurrenceRule", "frequency": frequency_name(rule.frequency) });
    if rule.interval > 1 {
        out["interval"] = json!(rule.interval);
    }
    if !rule.by_day.is_empty() {
        out["byDay"] = rule.by_day.iter().map(|day| json!({ "@type": "NDay", "day": day })).collect();
    }
    if let Some(until) = &rule.until {
        out["until"] = json!(format!("{until}T23:59:59"));
    }
    if let Some(count) = rule.count {
        out["count"] = json!(count);
    }
    out
}

fn alert_json(minutes: u32, anchor: &str) -> Value {
    let offset =
        if minutes == 0 { format_duration(0) } else { format!("-{}", format_duration(u64::from(minutes) * 60)) };
    json!({ "@type": "Alert", "trigger": { "@type": "OffsetTrigger", "offset": offset, "relativeTo": anchor } })
}

fn location_json(name: &str) -> Value {
    json!({ "@type": "Location", "name": name })
}

/// A new event or task. Only what the app owns is written; the server adds the rest.
pub fn create_object(input: &EventInput) -> Value {
    let mut out = Map::new();
    out.insert("@type".into(), json!(if input.is_task { "Task" } else { "Event" }));
    out.insert("calendarIds".into(), json!({ &input.calendar_id: true }));
    out.insert("title".into(), json!(input.title.trim()));
    if input.is_task {
        if !input.start.is_empty() {
            out.insert("due".into(), json!(input.start));
        }
    } else {
        out.insert("start".into(), json!(input.start));
        out.insert("duration".into(), json!(format_duration(input.duration_secs)));
    }
    if let Some(name) = &input.time_zone {
        out.insert("timeZone".into(), json!(name));
    }
    if input.all_day {
        out.insert("showWithoutTime".into(), json!(true));
    }
    if !input.description.is_empty() {
        out.insert("description".into(), json!(input.description));
    }
    if !input.location.trim().is_empty() {
        out.insert("locations".into(), json!({ "l1": location_json(input.location.trim()) }));
    }
    if let Some(repeat) = &input.repeat {
        out.insert("recurrenceRules".into(), json!([rule_json(repeat)]));
    }
    let anchor = if input.is_task { "end" } else { "start" };
    let minutes: BTreeSet<u32> = input.alert_minutes.iter().copied().collect();
    if !minutes.is_empty() {
        let alerts = minutes.iter().map(|m| (format!("a{m}"), alert_json(*m, anchor))).collect();
        out.insert("alerts".into(), Value::Object(alerts));
    }
    Value::Object(out)
}

/// The property a task's time is written to, as [`detail`] reports it: its due time, else its
/// start (a task that has only a start is not given a due time by a save).
fn task_time_key(event: &Value) -> &'static str {
    if text(event, "due").is_none() && text(event, "start").is_some() {
        "start"
    } else {
        "due"
    }
}

/// The property recurrence ids are counted from: the start, a task's due time when it has no start.
fn series_key(event: &Value) -> &'static str {
    if is_task(event) && text(event, "start").is_none() {
        "due"
    } else {
        "start"
    }
}

/// The overrides with every key, and every `start` or `due` inside, moved by `delta`: when the
/// series starts at another time its occurrences keep their place in it.
fn shifted_overrides(overrides: &Map<String, Value>, delta: Duration) -> Value {
    let shift = |text: &str| parse_local(text).and_then(|at| at.checked_add_signed(delta)).map(format_local);
    let mut out = Map::new();
    for (key, patch) in overrides {
        let mut patch = patch.clone();
        for field in ["start", "due"] {
            let moved = patch.get(field).and_then(Value::as_str).and_then(shift);
            if let (Some(moved), Some(fields)) = (moved, patch.as_object_mut()) {
                fields.insert(field.into(), json!(moved));
            }
        }
        out.insert(shift(key).unwrap_or_else(|| key.clone()), patch);
    }
    Value::Object(out)
}

/// The patch that turns `existing` into what the user edited, or an empty one when nothing
/// changed. Only the properties the app owns are in it, and only those that differ from what
/// [`detail`] reported (so what the app cannot show is never rewritten); everything else stays
/// on the server.
///
/// Maps (`locations`, `alerts`, `calendarIds`, `recurrenceOverrides`) are always set whole:
/// Stalwart 1.1 refuses a patch into a map the event does not have yet (`invalidPatch`), and a
/// whole map is as good where it does.
pub fn update_patch(existing: &Value, input: &EventInput) -> Result<Value> {
    if is_task(existing) != input.is_task {
        return Err(Error::invalid("An event cannot become a task, or a task an event."));
    }
    let current = current_input(existing).ok_or_else(|| Error::temporary("the server's answer is not an event"))?;
    let mut patch = Map::new();
    let set = |patch: &mut Map<String, Value>, key: &str, new: Option<Value>| {
        patch.insert(key.to_string(), new.unwrap_or(Value::Null));
    };
    if input.title.trim() != current.title {
        set(&mut patch, "title", Some(json!(input.title.trim())));
    }
    if input.start != current.start {
        let key = if input.is_task { task_time_key(existing) } else { "start" };
        set(&mut patch, key, (!input.start.is_empty()).then(|| json!(input.start)));
        // The overrides are named by the original starts of the series: they move with it.
        let (old, new) = (parse_local(&current.start), parse_local(&input.start));
        let overrides = existing["recurrenceOverrides"].as_object().filter(|o| !o.is_empty());
        if let (true, Some(old), Some(new), Some(overrides)) = (key == series_key(existing), old, new, overrides) {
            if new != old {
                patch.insert("recurrenceOverrides".into(), shifted_overrides(overrides, new - old));
            }
        }
    }
    if !input.is_task && current.duration_secs != input.duration_secs {
        patch.insert("duration".into(), json!(format_duration(input.duration_secs)));
    }
    if input.time_zone != current.time_zone {
        set(&mut patch, "timeZone", input.time_zone.as_ref().map(|name| json!(name)));
    }
    if current.all_day != input.all_day {
        patch.insert("showWithoutTime".into(), if input.all_day { json!(true) } else { Value::Null });
    }
    if input.description != current.description {
        set(&mut patch, "description", (!input.description.is_empty()).then(|| json!(input.description)));
    }
    if let Some(locations) = locations_patch(existing, input.location.trim()) {
        patch.insert("locations".into(), locations);
    }
    if let Some(alerts) = alerts_patch(existing, &input.alert_minutes) {
        patch.insert("alerts".into(), alerts);
        // The server's own default alerts would otherwise still apply beside (or instead of) these.
        if existing["useDefaultAlerts"] == true {
            patch.insert("useDefaultAlerts".into(), json!(false));
        }
    }
    let (repeat, complex) = repeat_of(existing);
    if !complex && repeat != input.repeat {
        match &input.repeat {
            Some(rule) => drop(patch.insert("recurrenceRules".into(), json!([rule_json(rule)]))),
            None => {
                patch.insert("recurrenceRules".into(), Value::Null);
                // What excluded single occurrences of the series goes with it.
                if existing.get("recurrenceOverrides").is_some_and(|o| !o.is_null()) {
                    patch.insert("recurrenceOverrides".into(), Value::Null);
                }
            }
        }
    }
    if input.calendar_id != current.calendar_id {
        // Only the calendar the app shows is replaced: the event stays in the others.
        let mut ids = existing["calendarIds"].as_object().cloned().unwrap_or_default();
        if let Some(first) = calendar_ids(existing).first() {
            ids.remove(*first);
        }
        ids.insert(input.calendar_id.clone(), json!(true));
        patch.insert("calendarIds".into(), Value::Object(ids));
    }
    Ok(Value::Object(patch))
}

/// The whole `locations` map after the first named place is set to `wanted` (removed when
/// empty); `None` when it stays as it is. Null when the map ends up empty.
fn locations_patch(existing: &Value, wanted: &str) -> Option<Value> {
    let mut places: BTreeMap<String, Value> = match &existing["locations"] {
        Value::Object(map) => map.iter().map(|(id, place)| (id.clone(), place.clone())).collect(),
        _ => BTreeMap::new(),
    };
    let first = places.iter().find(|(_, place)| text(place, "name").is_some()).map(|(id, _)| id.clone());
    match (first, wanted.is_empty()) {
        (None, true) => return None,
        (Some(id), true) => drop(places.remove(&id)),
        (Some(id), false) => {
            if text(&places[&id], "name") == Some(wanted) {
                return None;
            }
            // Whatever else the place says (coordinates, a link) stays.
            places.get_mut(&id)?["name"] = json!(wanted);
        }
        (None, false) => {
            let id = (1..).map(|n| format!("l{n}")).find(|id| !places.contains_key(id)).unwrap_or_default();
            places.insert(id, location_json(wanted));
        }
    }
    Some(if places.is_empty() { Value::Null } else { Value::Object(places.into_iter().collect()) })
}

/// The whole `alerts` map after the alerts the app owns are replaced by `minutes` (a reminder
/// that is already there stays as it is); `None` when nothing changes. Null when it ends up empty.
fn alerts_patch(existing: &Value, minutes: &[u32]) -> Option<Value> {
    let wanted: BTreeSet<u32> = minutes.iter().copied().collect();
    let current = existing["alerts"].as_object().cloned().unwrap_or_default();
    let owned = own_alerts(existing);
    let (mut next, mut kept) = (Map::new(), BTreeSet::new());
    for (id, alert) in &current {
        // Another kind of alert stays; one of ours stays when it is wanted (once).
        if owned.get(id).is_none_or(|m| wanted.contains(m) && kept.insert(*m)) {
            next.insert(id.clone(), alert.clone());
        }
    }
    let anchor = alert_anchor(existing);
    for m in wanted.difference(&kept) {
        let id =
            (0..).map(|n| if n == 0 { format!("a{m}") } else { format!("a{m}-{n}") }).find(|id| !next.contains_key(id));
        next.insert(id.unwrap_or_default(), alert_json(*m, anchor));
    }
    if next == current {
        None
    } else if next.is_empty() {
        Some(Value::Null)
    } else {
        Some(Value::Object(next))
    }
}

/// The patch that leaves one occurrence of a repeating event out. The whole overrides map is
/// set: Stalwart 1.1 refuses a patch into one the event does not have yet.
pub fn exclude_patch(existing: &Value, recurrence_id: &str) -> Result<Value> {
    if parse_strict(recurrence_id).is_none() {
        return Err(Error::invalid("An occurrence is named by its start, such as 2026-11-12T10:00:00."));
    }
    if existing["recurrenceRules"].as_array().is_none_or(|rules| rules.is_empty()) {
        return Err(Error::invalid("This event does not repeat."));
    }
    let mut overrides = existing["recurrenceOverrides"].as_object().cloned().unwrap_or_default();
    overrides.insert(recurrence_id.to_string(), json!({ "excluded": true }));
    Ok(json!({ "recurrenceOverrides": overrides }))
}

/// The patch that sets the user's answer to an invitation.
pub fn respond_patch(existing: &Value, me: &[String], status: &str) -> Result<Value> {
    if !ANSWERS.contains(&status) {
        return Err(Error::invalid("The answer is accepted, tentative or declined."));
    }
    let id = my_participant_id(existing, me).ok_or_else(|| Error::invalid("You are not invited to this event."))?;
    if existing["participants"][&id]["roles"]["owner"] == true {
        return Err(Error::invalid("You organise this event: there is nobody to answer."));
    }
    Ok(json!({ format!("participants/{id}/participationStatus"): status }))
}

/// The patch that marks a task done or not done.
pub fn progress_patch(existing: &Value, done: bool) -> Result<Value> {
    if !is_task(existing) {
        return Err(Error::invalid("Only a task can be marked done."));
    }
    Ok(json!({ "progress": if done { "completed" } else { "needs-action" } }))
}

#[cfg(test)]
mod tests {
    use super::*;

    // The events below are what Stalwart 1.1.0 answered to `CalendarEvent/get`, less the
    // properties the app does not read.
    fn timed() -> Value {
        json!({"@type": "Event", "id": "b", "uid": "u-b", "calendarIds": {"b": true}, "title": "Timed",
            "start": "2026-11-16T10:00:00", "timeZone": "Europe/Istanbul", "duration": "PT1H", "description": "d",
            "locations": {"l1": {"@type": "Location", "name": "Room"}},
            "alerts": {"a1": {"@type": "Alert", "action": "display",
                "trigger": {"@type": "OffsetTrigger", "offset": "-PT15M", "relativeTo": "start"}}},
            "isOrigin": true, "useDefaultAlerts": false, "mayInviteOthers": false})
    }

    fn all_day() -> Value {
        json!({"@type": "Event", "id": "c", "calendarIds": {"b": true}, "title": "AllDay",
            "start": "2026-11-17T00:00:00", "timeZone": "Europe/Istanbul", "duration": "P1D", "showWithoutTime": true})
    }

    fn daily() -> Value {
        json!({"@type": "Event", "id": "d", "calendarIds": {"b": true}, "title": "Daily",
            "start": "2026-11-16T08:00:00", "timeZone": "Europe/Istanbul", "duration": "PT30M",
            "recurrenceRules": [{"@type": "RecurrenceRule", "count": 5, "frequency": "daily"}]})
    }

    fn task() -> Value {
        json!({"@type": "Task", "id": "e", "calendarIds": {"b": true}, "title": "Task",
            "due": "2026-11-18T17:00:00", "timeZone": "Europe/Istanbul"})
    }

    fn floating() -> Value {
        json!({"@type": "Event", "id": "f", "calendarIds": {"b": true}, "title": "Floating",
            "start": "2026-11-19T07:00:00", "duration": "PT1H"})
    }

    fn invitation() -> Value {
        json!({"@type": "Event", "id": "m", "calendarIds": {"b": true}, "title": "Invite",
            "start": "2026-11-20T10:00:00", "timeZone": "Europe/Istanbul", "duration": "PT1H",
            "replyTo": {"imip": "mailto:admin@kweb.test"},
            "participants": {
                "m": {"@type": "Participant", "expectReply": true, "name": "Test user", "participationStatus": "needs-action",
                    "roles": {"attendee": true}, "sendTo": {"imip": "mailto:tester@kweb.test"}},
                "o": {"@type": "Participant", "expectReply": false, "name": "Org", "participationStatus": "accepted",
                    "roles": {"attendee": true, "owner": true}, "sendTo": {"imip": "mailto:admin@kweb.test"}}}})
    }

    fn calendar(id: &str, may_write: bool) -> Calendar {
        Calendar {
            id: id.into(),
            name: format!("Calendar {id}"),
            color: None,
            is_default: true,
            is_visible: true,
            may_write,
        }
    }

    fn at(zone_name: &str, local: &str) -> i64 {
        unix(parse_strict(local).unwrap(), zone(zone_name).unwrap())
    }

    /// The entries of `events` between two wall-clock times in Istanbul, the phone's zone unless said.
    fn agenda_in(phone: &str, events: &[Value], from: &str, to: &str) -> Vec<Occurrence> {
        let visible: BTreeSet<String> = ["b".to_string()].into();
        let me = vec!["tester@kweb.test".to_string()];
        let view = View { zone: zone(phone).unwrap(), visible: &visible, me: &me };
        let (from, to) = (at("Europe/Istanbul", from), at("Europe/Istanbul", to));
        let mut out: Vec<Occurrence> = events.iter().flat_map(|e| occurrences(e, from, to, &view)).collect();
        sort(&mut out, view.zone);
        out
    }

    fn week(events: &[Value]) -> Vec<Occurrence> {
        agenda_in("Europe/Istanbul", events, "2026-11-16T00:00:00", "2026-11-23T00:00:00")
    }

    fn one(event: Value) -> Occurrence {
        let mut found = week(&[event]);
        assert_eq!(found.len(), 1, "{found:?}");
        found.remove(0)
    }

    #[test]
    fn a_timed_event() {
        let e = one(timed());
        assert_eq!((e.event_id.as_str(), e.calendar_id.as_str(), e.title.as_str()), ("b", "b", "Timed"));
        assert_eq!(e.starts_at, at("UTC", "2026-11-16T07:00:00"));
        assert_eq!(e.ends_at, at("UTC", "2026-11-16T08:00:00"));
        assert_eq!(e.location, "Room");
        assert_eq!((e.all_day, e.is_task, e.is_done, e.is_recurring), (false, false, false, false));
        assert_eq!((e.recurrence_id, e.my_status), (None, None));
    }

    #[test]
    fn an_all_day_event_is_placed_in_the_phones_zone() {
        let e = one(all_day());
        assert!(e.all_day);
        assert_eq!(e.starts_at, at("Europe/Istanbul", "2026-11-17T00:00:00"));
        assert_eq!(e.ends_at, at("Europe/Istanbul", "2026-11-18T00:00:00"));
        // Read in New York, the same date is midnight there, not midnight in the event's zone.
        let found = agenda_in("America/New_York", &[all_day()], "2026-11-16T00:00:00", "2026-11-23T00:00:00");
        assert_eq!(found[0].starts_at, at("America/New_York", "2026-11-17T00:00:00"));
        assert_eq!(found[0].ends_at, at("America/New_York", "2026-11-18T00:00:00"));
        // A bare date is all-day too, and so is a three-day event.
        let e = one(
            json!({"@type": "Event", "id": "x", "calendarIds": {"b": true}, "start": "2026-11-18", "duration": "P3D"}),
        );
        assert!(e.all_day);
        assert_eq!(e.ends_at - e.starts_at, 3 * 86_400);
    }

    #[test]
    fn a_floating_time_is_the_same_wall_clock_in_the_phones_zone() {
        let e = one(floating());
        assert_eq!(e.starts_at, at("Europe/Istanbul", "2026-11-19T07:00:00"));
        let found = agenda_in("America/New_York", &[floating()], "2026-11-19T00:00:00", "2026-11-21T00:00:00");
        assert_eq!(found[0].starts_at, at("America/New_York", "2026-11-19T07:00:00"));
    }

    #[test]
    fn a_task_is_at_its_due_time() {
        let t = one(task());
        assert_eq!((t.is_task, t.is_done, t.all_day, t.is_recurring), (true, false, false, false));
        assert_eq!(t.starts_at, at("Europe/Istanbul", "2026-11-18T17:00:00"));
        assert_eq!(t.ends_at, t.starts_at);
        let mut done = task();
        done["progress"] = json!("completed");
        assert!(one(done).is_done);
        // One with only a start is there; one with no time at all has no place in the agenda.
        let started = one(
            json!({"@type": "Task", "id": "s", "calendarIds": {"b": true}, "title": "t", "start": "2026-11-17T09:00:00"}),
        );
        assert_eq!(started.starts_at, at("Europe/Istanbul", "2026-11-17T09:00:00"));
        assert!(week(&[json!({"@type": "Task", "id": "g", "calendarIds": {"b": true}, "title": "NoDue"})]).is_empty());
        // Its due time is a point: just before the span it is out, at the start of the span in.
        let before = agenda_in("Europe/Istanbul", &[task()], "2026-11-18T17:00:01", "2026-11-20T00:00:00");
        let from = agenda_in("Europe/Istanbul", &[task()], "2026-11-18T17:00:00", "2026-11-20T00:00:00");
        let until = agenda_in("Europe/Istanbul", &[task()], "2026-11-17T00:00:00", "2026-11-18T17:00:00");
        assert_eq!((before.len(), from.len(), until.len()), (0, 1, 0));
    }

    #[test]
    fn an_event_is_in_a_span_when_it_overlaps_it() {
        let found = |from: &str, to: &str| agenda_in("Europe/Istanbul", &[timed()], from, to).len();
        assert_eq!(found("2026-11-16T10:30:00", "2026-11-16T12:00:00"), 1, "it began before the span");
        assert_eq!(found("2026-11-16T11:00:00", "2026-11-16T12:00:00"), 0, "it ended as the span began");
        assert_eq!(found("2026-11-16T08:00:00", "2026-11-16T10:00:00"), 0, "it begins as the span ends");
    }

    #[test]
    fn the_occurrences_of_a_repeating_event() {
        let all = week(&[daily()]);
        let ids: Vec<_> = all.iter().map(|e| e.recurrence_id.clone().unwrap()).collect();
        assert_eq!(
            ids,
            [
                "2026-11-16T08:00:00",
                "2026-11-17T08:00:00",
                "2026-11-18T08:00:00",
                "2026-11-19T08:00:00",
                "2026-11-20T08:00:00"
            ]
        );
        assert!(all.iter().all(|e| e.is_recurring && e.event_id == "d" && e.title == "Daily"));
        assert_eq!(all[2].starts_at, at("Europe/Istanbul", "2026-11-18T08:00:00"));
        assert_eq!(all[2].ends_at - all[2].starts_at, 1800);
        // Only the ones in the span, even though the series began before it.
        let middle = agenda_in("Europe/Istanbul", &[daily()], "2026-11-18T00:00:00", "2026-11-19T00:00:00");
        assert_eq!(middle.len(), 1);
        assert_eq!(middle[0].recurrence_id.as_deref(), Some("2026-11-18T08:00:00"));
        assert!(agenda_in("Europe/Istanbul", &[daily()], "2026-11-21T00:00:00", "2026-12-21T00:00:00").is_empty());
    }

    #[test]
    fn a_series_in_a_zone_with_summer_time_keeps_its_wall_clock_time() {
        let weekly = json!({"@type": "Event", "id": "w", "calendarIds": {"b": true}, "title": "Weekly",
            "start": "2026-10-26T09:00:00", "timeZone": "America/New_York", "duration": "PT1H",
            "recurrenceRules": [{"frequency": "weekly"}]});
        let found = agenda_in("America/New_York", &[weekly], "2026-10-26T00:00:00", "2026-11-10T00:00:00");
        let starts: Vec<i64> = found.iter().map(|e| e.starts_at).collect();
        // The clocks went back on 2026-11-01: 09:00 is 13:00 UTC before and 14:00 UTC after.
        assert_eq!(
            starts,
            [at("UTC", "2026-10-26T13:00:00"), at("UTC", "2026-11-02T14:00:00"), at("UTC", "2026-11-09T14:00:00")]
        );
    }

    #[test]
    fn overrides_exclude_move_and_add_occurrences() {
        let mut series = daily();
        series["recurrenceOverrides"] = json!({
            "2026-11-17T08:00:00": {"excluded": true},
            "2026-11-18T08:00:00": {"title": "Daily (moved)", "start": "2026-11-18T11:15:00", "locations": {"x": {"name": "Cafe"}}},
            "2026-11-19T08:00:00": {"status": "cancelled"},
            "2026-11-21T18:00:00": {"title": "Extra"},
        });
        let found = week(&[series]);
        let shown: Vec<(&str, &str)> =
            found.iter().map(|e| (e.recurrence_id.as_deref().unwrap(), e.title.as_str())).collect();
        assert_eq!(
            shown,
            [
                ("2026-11-16T08:00:00", "Daily"),
                ("2026-11-18T08:00:00", "Daily (moved)"),
                ("2026-11-20T08:00:00", "Daily"),
                ("2026-11-21T18:00:00", "Extra"),
            ]
        );
        assert_eq!(found[1].starts_at, at("Europe/Istanbul", "2026-11-18T11:15:00"));
        assert_eq!(found[1].location, "Cafe");
        assert_eq!(found[0].location, "");
    }

    #[test]
    fn a_rule_that_is_not_understood_leaves_the_first_occurrence() {
        let mut series = daily();
        series["recurrenceRules"] = json!([{"frequency": "daily", "bySetPosition": [1]}]);
        let found = week(&[series]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].recurrence_id.as_deref(), Some("2026-11-16T08:00:00"));
        assert!(found[0].is_recurring);
    }

    #[test]
    fn an_invitation_carries_the_users_answer() {
        assert_eq!(one(invitation()).my_status.as_deref(), Some("needs-action"));
        let mut accepted = invitation();
        accepted["participants"]["m"]["participationStatus"] = json!("accepted");
        assert_eq!(one(accepted).my_status.as_deref(), Some("accepted"));
        // The address is matched however it is written.
        let mut loud = invitation();
        loud["participants"]["m"]["sendTo"] = json!({"imip": "MAILTO:Tester@KWEB.test"});
        assert_eq!(one(loud).my_status.as_deref(), Some("needs-action"));
        let mut other = invitation();
        other["participants"]["m"]["sendTo"] = json!({"imip": "mailto:someone@else.test"});
        assert_eq!(one(other).my_status, None);
    }

    #[test]
    fn what_is_cancelled_hidden_or_without_an_id_is_not_listed() {
        let mut cancelled = timed();
        cancelled["status"] = json!("cancelled");
        assert!(week(&[cancelled]).is_empty());
        let mut elsewhere = timed();
        elsewhere["calendarIds"] = json!({"other": true});
        assert!(week(&[elsewhere]).is_empty());
        let mut nameless = timed();
        nameless.as_object_mut().unwrap().remove("id");
        assert!(week(&[nameless]).is_empty());
        let mut garbled = timed();
        garbled["start"] = json!("nonsense");
        assert!(week(&[garbled]).is_empty());
        // An event in a shown and a hidden calendar is shown, as the shown one.
        let mut both = timed();
        both["calendarIds"] = json!({"a": true, "b": true});
        assert_eq!(one(both).calendar_id, "b");
    }

    #[test]
    fn an_event_with_an_unknown_zone_is_read_in_the_phones() {
        let mut odd = timed();
        odd["timeZone"] = json!("Custom/Place");
        assert_eq!(one(odd).starts_at, at("Europe/Istanbul", "2026-11-16T10:00:00"));
    }

    #[test]
    fn the_agenda_is_ordered_by_start_with_all_day_entries_first_in_a_day() {
        let found = week(&[daily(), floating(), timed(), task(), all_day(), invitation()]);
        let order: Vec<String> = found
            .iter()
            .map(|e| {
                format!(
                    "{}{}",
                    e.event_id,
                    e.recurrence_id.as_deref().map_or(String::new(), |r| format!(" {}", &r[8..10]))
                )
            })
            .collect();
        assert_eq!(order, ["d 16", "b", "c", "d 17", "d 18", "e", "f", "d 19", "d 20", "m"]);
        // c is the all-day entry of the 17th: it is before d of the 17th, which starts at 08:00.
        assert!(found[2].all_day && found[3].starts_at > found[2].starts_at);
    }

    // ---- the detail of one event ----

    fn me() -> Vec<String> {
        vec!["tester@kweb.test".to_string()]
    }

    fn detail_of(event: &Value) -> EventDetail {
        detail(event, &[calendar("b", true)], &me()).expect("an event")
    }

    #[test]
    fn the_detail_of_a_timed_event() {
        let d = detail_of(&timed());
        assert_eq!(d.event_id, "b");
        assert_eq!(
            d.input,
            EventInput {
                calendar_id: "b".into(),
                title: "Timed".into(),
                location: "Room".into(),
                description: "d".into(),
                start: "2026-11-16T10:00:00".into(),
                time_zone: Some("Europe/Istanbul".into()),
                duration_secs: 3600,
                all_day: false,
                is_task: false,
                repeat: None,
                alert_minutes: vec![15],
            }
        );
        assert_eq!((d.is_done, d.my_status, d.has_complex_repeat, d.may_write), (false, None, false, true));
        assert!(d.participants.is_empty());
        // The calendar's rights decide whether it can be changed.
        assert!(!detail(&timed(), &[calendar("b", false)], &me()).unwrap().may_write);
        assert!(!detail(&timed(), &[], &me()).unwrap().may_write);
        assert_eq!(detail(&json!({"title": "no id"}), &[], &me()), None);
    }

    #[test]
    fn the_detail_of_all_day_floating_and_task_entries() {
        let d = detail_of(&all_day());
        assert_eq!(
            (d.input.all_day, d.input.duration_secs, d.input.start.as_str()),
            (true, 86_400, "2026-11-17T00:00:00")
        );
        let d = detail_of(&floating());
        assert_eq!((d.input.time_zone, d.input.alert_minutes), (None, vec![]));
        let mut done = task();
        done["progress"] = json!("completed");
        let d = detail_of(&done);
        assert_eq!(
            (d.input.is_task, d.is_done, d.input.start.as_str(), d.input.duration_secs),
            (true, true, "2026-11-18T17:00:00", 0)
        );
        // No due time: an empty start.
        let d = detail_of(&json!({"@type": "Task", "id": "g", "calendarIds": {"b": true}, "title": "NoDue"}));
        assert_eq!((d.input.is_task, d.input.start.as_str()), (true, ""));
    }

    #[test]
    fn alerts_the_app_owns_are_the_shown_offsets_before_the_start() {
        let mut event = timed();
        event["alerts"] = json!({
            "a": {"trigger": {"@type": "OffsetTrigger", "offset": "-PT1H"}},
            "b": {"trigger": {"@type": "OffsetTrigger", "offset": "-P1D", "relativeTo": "start"}},
            "c": {"trigger": {"@type": "OffsetTrigger", "offset": "PT0S"}},
            "after": {"trigger": {"@type": "OffsetTrigger", "offset": "PT10M"}},
            "end": {"trigger": {"@type": "OffsetTrigger", "offset": "-PT5M", "relativeTo": "end"}},
            "absolute": {"trigger": {"@type": "AbsoluteTrigger", "when": "2026-11-16T06:00:00Z"}},
            "mail": {"action": "email", "trigger": {"@type": "OffsetTrigger", "offset": "-PT20M"}},
            "seconds": {"trigger": {"@type": "OffsetTrigger", "offset": "-PT90S"}},
        });
        assert_eq!(detail_of(&event).input.alert_minutes, [0, 60, 1440]);
        // A task with only a due time measures from its end.
        let mut t = task();
        t["alerts"] = json!({"x": {"trigger": {"@type": "OffsetTrigger", "offset": "-PT30M", "relativeTo": "end"}}});
        assert_eq!(detail_of(&t).input.alert_minutes, [30]);
    }

    #[test]
    fn the_participants_are_listed_with_the_organizer_first_and_the_user_marked() {
        let d = detail_of(&invitation());
        assert_eq!(
            d.participants,
            [
                Participant {
                    name: "Org".into(),
                    email: "admin@kweb.test".into(),
                    status: "accepted".into(),
                    is_organizer: true,
                    is_me: false
                },
                Participant {
                    name: "Test user".into(),
                    email: "tester@kweb.test".into(),
                    status: "needs-action".into(),
                    is_organizer: false,
                    is_me: true
                },
            ]
        );
        assert_eq!(d.my_status.as_deref(), Some("needs-action"));
        // The newer drafts say calendarAddress; a participant with no address at all still shows by name.
        let mut event = invitation();
        event["participants"]["m"] = json!({"name": "Test user", "calendarAddress": "mailto:tester@kweb.test"});
        event["participants"]["n"] = json!({"name": "Nameless"});
        event["participants"]["z"] = json!({});
        let d = detail_of(&event);
        assert_eq!(d.participants.len(), 3);
        assert_eq!((d.participants[1].name.as_str(), d.participants[1].email.as_str()), ("Nameless", ""));
        assert_eq!((d.participants[2].email.as_str(), d.participants[2].is_me), ("tester@kweb.test", true));
    }

    fn repeats(rules: Value) -> Value {
        let mut event = daily();
        event["recurrenceRules"] = rules;
        event
    }

    #[test]
    fn a_simple_repeat_rule_is_the_apps_own() {
        let found = |rules: Value| {
            let d = detail_of(&repeats(rules));
            (d.input.repeat, d.has_complex_repeat)
        };
        let rule = |frequency, interval, by_day: &[&str], until: Option<&str>, count| RepeatRule {
            frequency,
            interval,
            by_day: by_day.iter().map(|d| d.to_string()).collect(),
            until: until.map(str::to_string),
            count,
        };
        assert_eq!(
            found(json!([{"@type": "RecurrenceRule", "frequency": "daily", "count": 5}])),
            (Some(rule(Frequency::Daily, 1, &[], None, Some(5))), false)
        );
        let weekly = json!([{"frequency": "weekly", "interval": 2, "until": "2026-12-31T23:59:59",
            "byDay": [{"@type": "NDay", "day": "mo"}, {"day": "we"}]}]);
        assert_eq!(found(weekly), (Some(rule(Frequency::Weekly, 2, &["mo", "we"], Some("2026-12-31"), None)), false));
        assert_eq!(
            found(json!([{"frequency": "monthly", "rscale": "gregorian", "skip": "omit"}])),
            (Some(rule(Frequency::Monthly, 1, &[], None, None)), false)
        );
        assert_eq!(found(json!([{"frequency": "yearly"}])).0.unwrap().frequency, Frequency::Yearly);
        // No rule at all: nothing, and nothing complex.
        let mut plain = timed();
        assert_eq!((detail_of(&plain).input.repeat, detail_of(&plain).has_complex_repeat), (None, false));
        plain["recurrenceRules"] = json!([]);
        assert!(!detail_of(&plain).has_complex_repeat);
    }

    #[test]
    fn a_repeat_rule_the_app_cannot_hold_is_reported_and_not_shown() {
        for rules in [
            json!([{"frequency": "daily"}, {"frequency": "weekly"}]),
            json!([{"frequency": "monthly", "byMonthDay": [1, 15]}]),
            json!([{"frequency": "monthly", "byDay": [{"day": "tu", "nthOfPeriod": 2}]}]),
            json!([{"frequency": "weekly", "byDay": [{"day": "tu", "nthOfPeriod": 2}]}]),
            json!([{"frequency": "daily", "byDay": [{"day": "tu"}]}]),
            json!([{"frequency": "hourly"}]),
            json!([{"frequency": "daily", "bySetPosition": [1]}]),
            json!([{"frequency": "daily", "skip": "forward"}]),
            json!([{"frequency": "daily", "interval": 0}]),
            json!([{"frequency": "daily", "until": "later"}]),
        ] {
            let d = detail_of(&repeats(rules.clone()));
            assert_eq!((d.input.repeat, d.has_complex_repeat), (None, true), "{rules}");
        }
        let mut event = daily();
        event["excludedRecurrenceRules"] = json!([{"frequency": "weekly"}]);
        assert!(detail_of(&event).has_complex_repeat);
    }

    #[test]
    fn excluded_occurrences_are_fine_and_changed_ones_are_not() {
        let mut event = daily();
        event["recurrenceOverrides"] = json!({"2026-11-17T08:00:00": {"excluded": true}});
        let d = detail_of(&event);
        assert_eq!((d.input.repeat.is_some(), d.has_complex_repeat), (true, false));
        event["recurrenceOverrides"]["2026-11-18T08:00:00"] = json!({"title": "moved"});
        let d = detail_of(&event);
        assert_eq!((d.input.repeat, d.has_complex_repeat), (None, true));
    }

    // ---- writing ----

    fn input() -> EventInput {
        detail_of(&timed()).input
    }

    fn patch(existing: &Value, input: &EventInput) -> Value {
        update_patch(existing, input).unwrap()
    }

    fn invalid(result: Result<impl std::fmt::Debug>) -> String {
        match result {
            Err(Error::Invalid { msg }) => msg,
            other => panic!("expected Error::Invalid, got {other:?}"),
        }
    }

    #[test]
    fn a_new_event_is_written_with_what_the_app_owns() {
        assert_eq!(
            create_object(&input()),
            json!({"@type": "Event", "calendarIds": {"b": true}, "title": "Timed", "start": "2026-11-16T10:00:00",
                "duration": "PT1H", "timeZone": "Europe/Istanbul", "description": "d",
                "locations": {"l1": {"@type": "Location", "name": "Room"}},
                "alerts": {"a15": {"@type": "Alert",
                    "trigger": {"@type": "OffsetTrigger", "offset": "-PT15M", "relativeTo": "start"}}}})
        );
        // Floating, all-day, nothing optional: only what is needed.
        let bare = EventInput {
            time_zone: None,
            all_day: true,
            start: "2026-11-17T00:00:00".into(),
            duration_secs: 2 * 86_400,
            location: " ".into(),
            description: String::new(),
            alert_minutes: vec![],
            title: "  Trip ".into(),
            ..input()
        };
        assert_eq!(
            create_object(&bare),
            json!({"@type": "Event", "calendarIds": {"b": true}, "title": "Trip", "start": "2026-11-17T00:00:00",
                "duration": "P2D", "showWithoutTime": true})
        );
    }

    #[test]
    fn a_new_task_has_a_due_time_and_alerts_measured_from_it() {
        let task = EventInput {
            is_task: true,
            duration_secs: 0,
            location: String::new(),
            description: String::new(),
            start: "2026-11-18T17:00:00".into(),
            alert_minutes: vec![30, 30, 1440],
            ..input()
        };
        let end = |offset: &str| json!({"@type": "Alert", "trigger": {"@type": "OffsetTrigger", "offset": offset, "relativeTo": "end"}});
        assert_eq!(
            create_object(&task),
            json!({"@type": "Task", "calendarIds": {"b": true}, "title": "Timed", "due": "2026-11-18T17:00:00",
                "timeZone": "Europe/Istanbul", "alerts": {"a30": end("-PT30M"), "a1440": end("-P1D")}})
        );
        let undated = EventInput { start: String::new(), alert_minutes: vec![], time_zone: None, ..task };
        assert_eq!(create_object(&undated), json!({"@type": "Task", "calendarIds": {"b": true}, "title": "Timed"}));
    }

    #[test]
    fn a_repeat_rule_is_written_with_its_end_as_the_last_second_of_the_day() {
        let weekly = RepeatRule {
            frequency: Frequency::Weekly,
            interval: 2,
            by_day: vec!["mo".into()],
            until: Some("2026-12-31".into()),
            count: None,
        };
        assert_eq!(
            rule_json(&weekly),
            json!({"@type": "RecurrenceRule", "frequency": "weekly", "interval": 2, "byDay": [{"@type": "NDay", "day": "mo"}],
                "until": "2026-12-31T23:59:59"})
        );
        let daily =
            RepeatRule { frequency: Frequency::Daily, interval: 1, by_day: vec![], until: None, count: Some(5) };
        assert_eq!(rule_json(&daily), json!({"@type": "RecurrenceRule", "frequency": "daily", "count": 5}));
        // What is written reads back as what was meant.
        let mut event = timed();
        event["recurrenceRules"] = json!([rule_json(&weekly)]);
        assert_eq!(detail_of(&event).input.repeat, Some(weekly));
    }

    #[test]
    fn an_unchanged_event_gives_an_empty_patch() {
        let mut series = daily();
        series["recurrenceOverrides"] = json!({"2026-11-17T08:00:00": {"excluded": true}});
        for event in [timed(), all_day(), daily(), series, task(), floating(), invitation()] {
            assert_eq!(patch(&event, &detail_of(&event).input), json!({}), "{event}");
        }
    }

    #[test]
    fn a_patch_has_only_what_changed() {
        let mut changed = input();
        changed.title = " Renamed ".into();
        changed.start = "2026-11-16T11:00:00".into();
        changed.duration_secs = 7200;
        assert_eq!(
            patch(&timed(), &changed),
            json!({"title": "Renamed", "start": "2026-11-16T11:00:00", "duration": "PT2H"})
        );
        let mut zone_gone = input();
        zone_gone.time_zone = None;
        zone_gone.description = String::new();
        assert_eq!(patch(&timed(), &zone_gone), json!({"timeZone": null, "description": null}));
        let mut now_all_day =
            EventInput { all_day: true, start: "2026-11-16T00:00:00".into(), duration_secs: 86_400, ..input() };
        assert_eq!(
            patch(&timed(), &now_all_day),
            json!({"start": "2026-11-16T00:00:00", "duration": "P1D", "showWithoutTime": true})
        );
        now_all_day.all_day = false;
        assert_eq!(patch(&all_day(), &now_all_day)["showWithoutTime"], Value::Null);
        let moved = EventInput { calendar_id: "other".into(), ..input() };
        assert_eq!(patch(&timed(), &moved), json!({"calendarIds": {"other": true}}));
        // Properties the app does not know are not in the patch, even on an event that has them.
        assert!(patch(&invitation(), &detail_of(&invitation()).input).as_object().unwrap().is_empty());
    }

    #[test]
    fn the_first_location_is_set_changed_or_removed() {
        let place = |name: &str| EventInput { location: name.into(), ..input() };
        assert_eq!(
            patch(&timed(), &place("Cafe")),
            json!({"locations": {"l1": {"@type": "Location", "name": "Cafe"}}})
        );
        assert_eq!(patch(&timed(), &place("")), json!({"locations": null}));
        // None yet: the whole map is set (a patch into a missing map is refused by the server).
        let mut none = timed();
        none.as_object_mut().unwrap().remove("locations");
        assert_eq!(patch(&none, &place("Cafe")), json!({"locations": {"l1": {"@type": "Location", "name": "Cafe"}}}));
        assert_eq!(patch(&none, &place("")), json!({}));
        // Other places, and what a place says besides its name, stay.
        let mut several = timed();
        several["locations"] =
            json!({"l1": {"name": "Room", "coordinates": "geo:1,2"}, "l2": {"name": "Annex"}, "l3": {}});
        assert_eq!(
            patch(&several, &place("Cafe"))["locations"],
            json!({"l1": {"name": "Cafe", "coordinates": "geo:1,2"}, "l2": {"name": "Annex"}, "l3": {}})
        );
        assert_eq!(patch(&several, &place("")), json!({"locations": {"l2": {"name": "Annex"}, "l3": {}}}));
        // A place without a name is not "the location": a new one is added beside it.
        let mut unnamed = timed();
        unnamed["locations"] = json!({"l1": {"coordinates": "geo:1,2"}});
        assert_eq!(
            patch(&unnamed, &place("Cafe"))["locations"],
            json!({"l1": {"coordinates": "geo:1,2"}, "l2": {"@type": "Location", "name": "Cafe"}})
        );
    }

    #[test]
    fn the_alerts_the_app_owns_are_replaced_and_others_stay() {
        let alerts = |minutes: &[u32]| EventInput { alert_minutes: minutes.to_vec(), ..input() };
        assert_eq!(patch(&timed(), &alerts(&[])), json!({"alerts": null}));
        let added = patch(&timed(), &alerts(&[15, 60, 60]))["alerts"].clone();
        // The one that is there stays as the server has it; a new one is added.
        assert_eq!(added["a1"], timed()["alerts"]["a1"]);
        assert_eq!(added["a60"], alert_json(60, "start"));
        assert_eq!(added.as_object().unwrap().len(), 2);
        assert_eq!(patch(&timed(), &alerts(&[15])), json!({}));
        // Alerts of another kind stay, and an owned one that appears twice is kept once.
        let mut event = timed();
        event["alerts"]["abs"] = json!({"trigger": {"@type": "AbsoluteTrigger", "when": "2026-11-16T06:00:00Z"}});
        event["alerts"]["dup"] = json!({"trigger": {"@type": "OffsetTrigger", "offset": "-PT15M"}});
        let kept = patch(&event, &alerts(&[]))["alerts"].clone();
        assert_eq!(kept, json!({"abs": event["alerts"]["abs"]}));
        let once = patch(&event, &alerts(&[15]))["alerts"].clone();
        assert!(once.get("a1").is_some() && once.get("dup").is_none() && once.get("abs").is_some());
        // A task with a due time only keeps its reminders against the end.
        let mut t = EventInput { alert_minutes: vec![30], ..detail_of(&task()).input };
        assert_eq!(patch(&task(), &t)["alerts"]["a30"], alert_json(30, "end"));
        t.alert_minutes = vec![];
        assert_eq!(patch(&task(), &t), json!({}));
    }

    #[test]
    fn a_repeat_rule_is_set_changed_or_removed() {
        let every_day =
            RepeatRule { frequency: Frequency::Daily, interval: 1, by_day: vec![], until: None, count: None };
        let with = |rule: Option<RepeatRule>| EventInput { repeat: rule, ..input() };
        assert_eq!(
            patch(&timed(), &with(Some(every_day.clone()))),
            json!({"recurrenceRules": [rule_json(&every_day)]})
        );
        let counted = RepeatRule { count: Some(5), ..every_day.clone() };
        assert_eq!(patch(&daily(), &EventInput { repeat: Some(counted), ..detail_of(&daily()).input }), json!({}));
        let forever = EventInput { repeat: Some(every_day.clone()), ..detail_of(&daily()).input };
        assert_eq!(patch(&daily(), &forever), json!({"recurrenceRules": [rule_json(&every_day)]}));
        // Removed: the exclusions of single occurrences go with it.
        let none = EventInput { repeat: None, ..detail_of(&daily()).input };
        assert_eq!(patch(&daily(), &none), json!({"recurrenceRules": null}));
        let mut excluded = daily();
        excluded["recurrenceOverrides"] = json!({"2026-11-17T08:00:00": {"excluded": true}});
        assert_eq!(patch(&excluded, &none), json!({"recurrenceRules": null, "recurrenceOverrides": null}));
    }

    #[test]
    fn a_complex_repeat_rule_is_left_alone_whatever_the_input_says() {
        let mut complex = daily();
        complex["recurrenceRules"] = json!([{"frequency": "monthly", "byMonthDay": [1, 15]}]);
        let input = detail_of(&complex).input;
        assert_eq!(input.repeat, None);
        assert_eq!(patch(&complex, &input), json!({}));
        let wants_a_rule = EventInput {
            repeat: Some(RepeatRule {
                frequency: Frequency::Daily,
                interval: 1,
                by_day: vec![],
                until: None,
                count: None,
            }),
            ..input
        };
        assert_eq!(patch(&complex, &wants_a_rule), json!({}));
        let renamed = EventInput { title: "New".into(), ..wants_a_rule };
        assert_eq!(patch(&complex, &renamed), json!({"title": "New"}));
    }

    #[test]
    fn a_task_changes_its_due_time_and_an_event_cannot_become_one() {
        let later = EventInput { start: "2026-11-19T09:00:00".into(), ..detail_of(&task()).input };
        assert_eq!(patch(&task(), &later), json!({"due": "2026-11-19T09:00:00"}));
        let undated = EventInput { start: String::new(), ..detail_of(&task()).input };
        assert_eq!(patch(&task(), &undated), json!({"due": null}));
        assert!(invalid(update_patch(&timed(), &EventInput { is_task: true, ..input() })).contains("task"));
        assert!(invalid(update_patch(&task(), &input())).contains("task"));
    }

    #[test]
    fn one_occurrence_is_excluded_by_setting_the_whole_overrides_map() {
        assert_eq!(
            exclude_patch(&daily(), "2026-11-17T08:00:00").unwrap(),
            json!({"recurrenceOverrides": {"2026-11-17T08:00:00": {"excluded": true}}})
        );
        let mut event = daily();
        event["recurrenceOverrides"] =
            json!({"2026-11-17T08:00:00": {"excluded": true}, "2026-11-18T08:00:00": {"title": "moved"}});
        assert_eq!(
            exclude_patch(&event, "2026-11-18T08:00:00").unwrap(),
            json!({"recurrenceOverrides": {"2026-11-17T08:00:00": {"excluded": true}, "2026-11-18T08:00:00": {"excluded": true}}})
        );
        assert!(invalid(exclude_patch(&timed(), "2026-11-16T10:00:00")).contains("repeat"));
        assert!(invalid(exclude_patch(&daily(), "tomorrow")).contains("occurrence"));
    }

    #[test]
    fn an_invitation_is_answered_in_the_users_participant() {
        assert_eq!(
            respond_patch(&invitation(), &me(), "accepted").unwrap(),
            json!({"participants/m/participationStatus": "accepted"})
        );
        for answer in ["tentative", "declined"] {
            assert!(respond_patch(&invitation(), &me(), answer).is_ok());
        }
        assert!(invalid(respond_patch(&invitation(), &me(), "needs-action")).contains("accepted"));
        assert!(invalid(respond_patch(&invitation(), &me(), "")).contains("accepted"));
        assert!(invalid(respond_patch(&timed(), &me(), "accepted")).contains("not invited"));
        assert!(invalid(respond_patch(&invitation(), &["x@y.test".to_string()], "accepted")).contains("not invited"));
    }

    #[test]
    fn a_task_is_done_by_its_progress() {
        assert_eq!(progress_patch(&task(), true).unwrap(), json!({"progress": "completed"}));
        assert_eq!(progress_patch(&task(), false).unwrap(), json!({"progress": "needs-action"}));
        assert!(invalid(progress_patch(&timed(), true)).contains("task"));
    }

    #[test]
    fn what_cannot_be_saved_is_refused_with_a_reason() {
        let calendars = [calendar("b", true), calendar("ro", false)];
        assert!(validate(&input(), &calendars, None).is_ok());
        let case = |input: EventInput| invalid(validate(&input, &calendars, None));
        assert!(case(EventInput { title: " \t".into(), ..input() }).contains("title"));
        for start in ["", "tomorrow", "2026-11-16", "2026-11-16T10:00", "2026-11-16T10:00:00Z", "2026-13-16T10:00:00"] {
            assert!(case(EventInput { start: start.into(), ..input() }).contains("start"), "{start}");
        }
        assert!(case(EventInput { time_zone: Some("Mars/Base".into()), ..input() }).contains("Mars/Base"));
        assert!(case(EventInput { time_zone: Some(String::new()), ..input() }).contains("time zone"));
        assert!(case(EventInput { calendar_id: "nope".into(), ..input() }).contains("calendar"));
        assert!(case(EventInput { calendar_id: "ro".into(), ..input() }).contains("Calendar ro"));
    }

    #[test]
    fn an_all_day_entry_starts_at_midnight_and_lasts_whole_days() {
        let calendars = [calendar("b", true)];
        let day =
            |start: &str, secs: u64| EventInput { all_day: true, start: start.into(), duration_secs: secs, ..input() };
        assert!(validate(&day("2026-11-17T00:00:00", 86_400), &calendars, None).is_ok());
        assert!(validate(&day("2026-11-17T00:00:00", 3 * 86_400), &calendars, None).is_ok());
        for (start, secs) in [
            ("2026-11-17T09:00:00", 86_400),
            ("2026-11-17T00:00:00", 3600),
            ("2026-11-17T00:00:00", 90_000),
            ("2026-11-17T00:00:00", 0),
        ] {
            assert!(invalid(validate(&day(start, secs), &calendars, None)).contains("all-day"), "{start} {secs}");
        }
        // A task is all-day with no length at all.
        let task = EventInput { is_task: true, duration_secs: 0, ..day("2026-11-17T00:00:00", 0) };
        assert!(validate(&task, &calendars, None).is_ok());
    }

    #[test]
    fn a_task_may_have_no_due_date_but_then_no_reminders() {
        let calendars = [calendar("b", true)];
        let undated =
            EventInput { is_task: true, start: String::new(), duration_secs: 0, alert_minutes: vec![], ..input() };
        assert!(validate(&undated, &calendars, None).is_ok());
        let with_alert = EventInput { alert_minutes: vec![10], ..undated };
        assert!(invalid(validate(&with_alert, &calendars, None)).contains("reminders"));
    }

    #[test]
    fn a_repeat_rule_that_makes_no_sense_is_refused() {
        let calendars = [calendar("b", true)];
        let rule = |interval: u32, by_day: &[&str], until: Option<&str>, count: Option<u32>, frequency| {
            let by_day = by_day.iter().map(|d| d.to_string()).collect();
            EventInput {
                repeat: Some(RepeatRule { frequency, interval, by_day, until: until.map(str::to_string), count }),
                ..input()
            }
        };
        let ok = |input: EventInput| validate(&input, &calendars, None).is_ok();
        assert!(ok(rule(1, &[], None, None, Frequency::Daily)));
        assert!(ok(rule(3, &["mo", "su"], Some("2026-12-31"), None, Frequency::Weekly)));
        assert!(ok(rule(1, &[], None, Some(10), Frequency::Yearly)));
        let bad = |input: EventInput| invalid(validate(&input, &calendars, None));
        assert!(bad(rule(0, &[], None, None, Frequency::Daily)).contains("every 1"));
        assert!(bad(rule(1, &["xx"], None, None, Frequency::Weekly)).contains("'xx'"));
        assert!(bad(rule(1, &["MO"], None, None, Frequency::Weekly)).contains("'MO'"));
        assert!(bad(rule(1, &["mo"], None, None, Frequency::Monthly)).contains("weekly"));
        for until in ["2026-12-31T00:00:00", "31.12.2026", "2026-13-01", "soon", ""] {
            assert!(bad(rule(1, &[], Some(until), None, Frequency::Daily)).contains("date"), "{until}");
        }
        assert!(bad(rule(1, &[], None, Some(0), Frequency::Daily)).contains("once"));
        assert!(bad(rule(1, &[], Some("2026-12-31"), Some(3), Frequency::Daily)).contains("not both"));
    }

    #[test]
    fn a_span_needs_an_end_after_its_start_and_a_zone() {
        assert_eq!(check_span(0, 1, "Europe/Istanbul").unwrap(), zone("Europe/Istanbul").unwrap());
        assert!(invalid(check_span(5, 5, "UTC")).contains("end"));
        assert!(invalid(check_span(6, 5, "UTC")).contains("end"));
        for name in ["", "Europe/ Istanbul", " UTC", "Mars/Base", "europe/istanbul"] {
            assert!(matches!(check_span(0, 1, name), Err(Error::Invalid { .. })), "{name:?}");
        }
    }

    // ---- what a strict review found ----

    fn series_with_overrides() -> Value {
        let mut series = daily();
        series["recurrenceOverrides"] = json!({
            "2026-11-17T08:00:00": {"excluded": true},
            "2026-11-18T08:00:00": {"title": "moved", "start": "2026-11-18T11:15:00"},
        });
        series
    }

    #[test]
    fn moving_the_start_of_a_series_moves_its_overrides_with_it() {
        let series = series_with_overrides();
        let later = EventInput { start: "2026-11-16T09:30:00".into(), ..detail_of(&series).input };
        assert_eq!(
            patch(&series, &later),
            json!({"start": "2026-11-16T09:30:00", "recurrenceOverrides": {
                "2026-11-17T09:30:00": {"excluded": true},
                "2026-11-18T09:30:00": {"title": "moved", "start": "2026-11-18T12:45:00"}}})
        );
        // Another date moves them by days; another zone alone leaves the wall-clock times as they are.
        let next_day = EventInput { start: "2026-11-17T08:00:00".into(), ..detail_of(&series).input };
        let keys = patch(&series, &next_day)["recurrenceOverrides"].clone();
        assert_eq!(
            keys.as_object().unwrap().keys().collect::<Vec<_>>(),
            ["2026-11-18T08:00:00", "2026-11-19T08:00:00"]
        );
        let zone_only = EventInput { time_zone: Some("UTC".into()), ..detail_of(&series).input };
        assert_eq!(patch(&series, &zone_only), json!({"timeZone": "UTC"}));
        // Nothing to move without overrides; a task that is due later than it starts keeps its series.
        let plain = EventInput { start: "2026-11-16T09:30:00".into(), ..detail_of(&daily()).input };
        assert_eq!(patch(&daily(), &plain), json!({"start": "2026-11-16T09:30:00"}));
        let mut both = task();
        both["start"] = json!("2026-11-17T09:00:00");
        both["recurrenceRules"] = json!([{"frequency": "daily"}]);
        both["recurrenceOverrides"] = json!({"2026-11-18T09:00:00": {"excluded": true}});
        let due_later = EventInput { start: "2026-11-18T19:00:00".into(), ..detail_of(&both).input };
        assert_eq!(patch(&both, &due_later), json!({"due": "2026-11-18T19:00:00"}));
        // A repeat that is taken off takes the overrides with it, not a shifted copy.
        let none = EventInput { start: "2026-11-16T09:30:00".into(), repeat: None, ..detail_of(&daily()).input };
        assert_eq!(patch(&series_with_excluded(), &none)["recurrenceOverrides"], Value::Null);
    }

    fn series_with_excluded() -> Value {
        let mut series = daily();
        series["recurrenceOverrides"] = json!({"2026-11-17T08:00:00": {"excluded": true}});
        series
    }

    #[test]
    fn a_save_changes_only_the_calendar_the_app_shows() {
        let mut shared = timed();
        shared["calendarIds"] = json!({"b": true, "z": true});
        let unchanged = detail_of(&shared).input;
        assert_eq!(patch(&shared, &unchanged), json!({}));
        let moved = EventInput { calendar_id: "c".into(), ..unchanged };
        assert_eq!(patch(&shared, &moved), json!({"calendarIds": {"c": true, "z": true}}));
    }

    #[test]
    fn a_task_with_a_start_and_no_due_time_keeps_its_shape() {
        let mut started = json!({"@type": "Task", "id": "s", "calendarIds": {"b": true}, "title": "t",
            "start": "2026-11-17T09:00:00", "alerts": {"x": {"trigger": {"@type": "OffsetTrigger",
                "offset": "-PT30M", "relativeTo": "start"}}}});
        let input = detail_of(&started).input;
        assert_eq!((input.start.as_str(), &input.alert_minutes[..]), ("2026-11-17T09:00:00", &[30][..]));
        assert_eq!(patch(&started, &input), json!({}), "no due time is written");
        let later = EventInput { start: "2026-11-17T10:00:00".into(), ..input.clone() };
        assert_eq!(patch(&started, &later), json!({"start": "2026-11-17T10:00:00"}));
        let more = EventInput { alert_minutes: vec![30, 60], ..input };
        assert_eq!(patch(&started, &more)["alerts"]["a60"], alert_json(60, "start"));
        // With both, the time shown is the due time and the reminders are measured from it.
        started["due"] = json!("2026-11-17T17:00:00");
        started["alerts"]["x"]["trigger"]["relativeTo"] = json!("end");
        let input = detail_of(&started).input;
        assert_eq!((input.start.as_str(), &input.alert_minutes[..]), ("2026-11-17T17:00:00", &[30][..]));
        assert_eq!(patch(&started, &input), json!({}));
        let later = EventInput { start: "2026-11-17T18:00:00".into(), ..input };
        assert_eq!(patch(&started, &later), json!({"due": "2026-11-17T18:00:00"}));
    }

    #[test]
    fn what_the_user_did_not_change_is_not_checked_again() {
        let calendars = [calendar("b", true)];
        let mut odd = timed();
        odd["timeZone"] = json!("Custom/Place");
        let current = detail_of(&odd).input;
        assert!(validate(&current, &calendars, None).is_err());
        assert!(validate(&current, &calendars, Some(&current)).is_ok());
        let renamed = EventInput { title: "New".into(), ..current.clone() };
        assert!(validate(&renamed, &calendars, Some(&current)).is_ok());
        let other = EventInput { time_zone: Some("Mars/Base".into()), ..current.clone() };
        assert!(invalid(validate(&other, &calendars, Some(&current))).contains("Mars/Base"));
        // An all-day entry that starts mid-day (as another client may write it) can be renamed, not reshaped.
        let mut shaped = all_day();
        shaped["start"] = json!("2026-11-17T09:00:00");
        shaped["duration"] = json!("PT1H");
        let current = detail_of(&shaped).input;
        assert!(current.all_day);
        let renamed = EventInput { title: "New".into(), ..current.clone() };
        assert!(validate(&renamed, &calendars, Some(&current)).is_ok());
        let longer = EventInput { duration_secs: 7200, ..current.clone() };
        assert!(invalid(validate(&longer, &calendars, Some(&current))).contains("all-day"));
        // A calendar that cannot be written is still refused.
        assert!(validate(&renamed, &[calendar("b", false)], Some(&current)).is_err());
    }

    #[test]
    fn changing_the_alerts_turns_the_default_alerts_off() {
        let mut event = timed();
        event["useDefaultAlerts"] = json!(true);
        let alerts = |minutes: &[u32]| EventInput { alert_minutes: minutes.to_vec(), ..input() };
        assert_eq!(patch(&event, &alerts(&[15])), json!({}));
        assert_eq!(patch(&event, &alerts(&[])), json!({"alerts": null, "useDefaultAlerts": false}));
        let more = patch(&event, &alerts(&[15, 60]));
        assert_eq!((&more["useDefaultAlerts"], more["alerts"].as_object().unwrap().len()), (&json!(false), 2));
        assert!(patch(&timed(), &alerts(&[])).get("useDefaultAlerts").is_none());
    }

    #[test]
    fn the_organizer_has_nobody_to_answer() {
        let organizer = vec!["admin@kweb.test".to_string()];
        assert!(invalid(respond_patch(&invitation(), &organizer, "accepted")).contains("organise"));
    }

    fn weekly_from(start: &str) -> Value {
        json!({"@type": "Event", "id": "w", "calendarIds": {"b": true}, "title": "Weekly", "start": start,
            "timeZone": "Europe/Istanbul", "duration": "PT1H", "recurrenceRules": [{"frequency": "weekly"}]})
    }

    #[test]
    fn an_occurrence_moved_into_the_span_from_far_outside_it_is_found() {
        let mut series = weekly_from("2026-11-02T08:00:00");
        series["recurrenceOverrides"] = json!({
            "2026-12-21T08:00:00": {"start": "2026-11-19T12:00:00"},
            "2026-11-16T08:00:00": {"start": "2026-11-30T12:00:00"},
        });
        let found = week(&[series]);
        let shown: Vec<(&str, i64)> =
            found.iter().map(|e| (e.recurrence_id.as_deref().unwrap(), e.starts_at)).collect();
        assert_eq!(shown, [("2026-12-21T08:00:00", at("Europe/Istanbul", "2026-11-19T12:00:00"))]);
    }

    #[test]
    fn a_series_by_set_position_is_followed() {
        let mut last_friday = weekly_from("2026-11-27T18:00:00");
        last_friday["recurrenceRules"] =
            json!([{"frequency": "monthly", "byDay": [{"day": "fr"}], "bySetPosition": [-1]}]);
        let found = agenda_in("Europe/Istanbul", &[last_friday], "2026-11-01T00:00:00", "2027-01-31T00:00:00");
        let ids: Vec<_> = found.iter().map(|e| e.recurrence_id.clone().unwrap()).collect();
        assert_eq!(ids, ["2026-11-27T18:00:00", "2026-12-25T18:00:00", "2027-01-29T18:00:00"]);
    }

    #[test]
    fn a_huge_duration_does_not_panic_and_gives_an_entry_without_a_length() {
        for duration in ["P99999999999W", "P999999999W", "P9223372036854775807D", "PT18446744073709551615S"] {
            for mut event in [timed(), all_day(), daily()] {
                event["duration"] = json!(duration);
                let found = week(&[event]);
                assert!(!found.is_empty(), "{duration}");
                assert!(found.iter().all(|e| e.ends_at >= e.starts_at), "{duration}");
            }
        }
        let mut event = timed();
        event["duration"] = json!("P99999999999W");
        let e = one(event);
        assert_eq!(e.ends_at, e.starts_at);
    }

    #[test]
    fn days_are_calendar_days_and_hours_are_exact_across_a_clock_change() {
        // New York put the clocks back on 2026-11-01 (02:00 became 01:00).
        let stay = |duration: &str| {
            let mut event = timed();
            event["start"] = json!("2026-10-31T10:00:00");
            event["timeZone"] = json!("America/New_York");
            event["duration"] = json!(duration);
            let found = agenda_in("America/New_York", &[event], "2026-10-30T00:00:00", "2026-11-10T00:00:00");
            found[0].ends_at
        };
        assert_eq!(stay("P3D"), at("America/New_York", "2026-11-03T10:00:00"));
        assert_eq!(stay("P1W"), at("America/New_York", "2026-11-07T10:00:00"));
        // The days are added to the wall clock, then the hours to the instant.
        assert_eq!(stay("P2DT2H"), at("America/New_York", "2026-11-02T10:00:00") + 2 * 3600);
        assert_eq!(stay("PT48H"), at("America/New_York", "2026-10-31T10:00:00") + 48 * 3600);
    }

    #[test]
    fn the_users_answer_is_read_per_occurrence() {
        let mut series = invitation();
        series["recurrenceRules"] = json!([{"frequency": "daily", "count": 4}]);
        series["recurrenceOverrides"] = json!({
            "2026-11-21T10:00:00": {"participants/m/participationStatus": "declined"},
            "2026-11-22T10:00:00": {"participants/m": {"participationStatus": "tentative"}},
            "2026-11-23T10:00:00": {"participants": {"m": {"sendTo": {"imip": "mailto:tester@kweb.test"},
                "participationStatus": "accepted"}}},
        });
        let found = agenda_in("Europe/Istanbul", &[series.clone()], "2026-11-20T00:00:00", "2026-11-25T00:00:00");
        let answers: Vec<_> = found.iter().map(|e| e.my_status.as_deref().unwrap()).collect();
        assert_eq!(answers, ["needs-action", "declined", "tentative", "accepted"]);
        let at_occurrence = |rid: &str| detail_at(&series, &[], &me(), Some(rid)).unwrap();
        assert_eq!(at_occurrence("2026-11-21T10:00:00").my_status.as_deref(), Some("declined"));
        assert_eq!(at_occurrence("2026-11-20T10:00:00").my_status.as_deref(), Some("needs-action"));
        assert!(at_occurrence("2026-11-21T10:00:00").participants.iter().any(|p| p.is_me && p.status == "declined"));
    }

    #[test]
    fn a_repeating_task_is_expanded_from_its_start_and_due_after_it() {
        let mut series = json!({"@type": "Task", "id": "r", "calendarIds": {"b": true}, "title": "Report",
            "start": "2026-11-16T09:00:00", "due": "2026-11-16T17:00:00", "timeZone": "Europe/Istanbul",
            "recurrenceRules": [{"frequency": "daily", "count": 4}],
            "recurrenceOverrides": {
                "2026-11-17T09:00:00": {"due": "2026-11-17T20:00:00"},
                "2026-11-18T09:00:00": {"start": "2026-11-18T10:00:00"},
                "2026-11-19T09:00:00": {"excluded": true}}});
        let shown = |event: &Value| -> Vec<(String, i64)> {
            week(std::slice::from_ref(event)).iter().map(|e| (e.recurrence_id.clone().unwrap(), e.starts_at)).collect()
        };
        let day = |d: &str| at("Europe/Istanbul", d);
        assert_eq!(
            shown(&series),
            [
                ("2026-11-16T09:00:00".to_string(), day("2026-11-16T17:00:00")),
                ("2026-11-17T09:00:00".to_string(), day("2026-11-17T20:00:00")),
                ("2026-11-18T09:00:00".to_string(), day("2026-11-18T18:00:00")),
            ]
        );
        // A task that has only a due time is expanded from it.
        series.as_object_mut().unwrap().remove("start");
        series["recurrenceOverrides"] = json!({});
        let ids: Vec<_> = shown(&series).into_iter().map(|(id, _)| id).collect();
        assert_eq!(ids, ["2026-11-16T17:00:00", "2026-11-17T17:00:00", "2026-11-18T17:00:00", "2026-11-19T17:00:00"]);
    }

    #[test]
    fn the_detail_of_one_occurrence_has_what_its_override_changes() {
        let mut series = daily();
        series["recurrenceOverrides"] = json!({
            "2026-11-17T08:00:00": {"title": "Retitled", "start": "2026-11-17T11:15:00", "duration": "PT2H",
                "description": "Bring cake", "locations": {"x": {"name": "Cafe"}}},
            "2026-11-19T08:00:00": {"excluded": true},
            "2026-11-25T18:00:00": {"title": "Extra"},
        });
        let calendars = [calendar("b", true)];
        let occurrence = |rid: &str| detail_at(&series, &calendars, &me(), Some(rid));
        let moved = occurrence("2026-11-17T08:00:00").unwrap();
        assert_eq!(
            (moved.input.start.as_str(), moved.input.title.as_str(), moved.input.duration_secs),
            ("2026-11-17T11:15:00", "Retitled", 7200)
        );
        assert_eq!((moved.input.location.as_str(), moved.input.description.as_str()), ("Cafe", "Bring cake"));
        // The rest is the series': its rule is not an occurrence's own, and the answer to a changed series is shown.
        assert_eq!(moved.input.repeat, None);
        assert!(moved.has_complex_repeat);
        // One that is not changed has the series' values at its own start.
        let plain = occurrence("2026-11-18T08:00:00").unwrap();
        assert_eq!(
            (
                plain.input.start.as_str(),
                plain.input.title.as_str(),
                plain.input.duration_secs,
                plain.input.description.as_str()
            ),
            ("2026-11-18T08:00:00", "Daily", 1800, "")
        );
        assert_eq!(occurrence("2026-11-25T18:00:00").unwrap().input.title, "Extra");
        // Without a recurrence id it is the series.
        assert_eq!(detail_at(&series, &calendars, &me(), None).unwrap(), detail_of(&series));
        // Excluded, outside the rule, past its count, not a start, or an event that does not repeat.
        for rid in ["2026-11-19T08:00:00", "2026-11-18T09:00:00", "2026-12-30T08:00:00", "tomorrow", ""] {
            assert!(invalid(occurrence(rid)).contains("occurrence"), "{rid:?}");
        }
        let single = detail_at(&timed(), &calendars, &me(), Some("2026-11-16T10:00:00"));
        assert!(invalid(single).contains("occurrence"));
    }

    #[test]
    fn the_detail_of_an_occurrence_of_a_repeating_task_is_its_due_time() {
        let series = json!({"@type": "Task", "id": "r", "calendarIds": {"b": true}, "title": "Report",
            "start": "2026-11-16T09:00:00", "due": "2026-11-16T17:00:00",
            "recurrenceRules": [{"frequency": "daily"}],
            "recurrenceOverrides": {"2026-11-18T09:00:00": {"due": "2026-11-18T20:00:00", "progress": "completed"}}});
        let calendars = [calendar("b", true)];
        let found = |rid: &str| detail_at(&series, &calendars, &me(), Some(rid)).unwrap();
        assert_eq!(found("2026-11-17T09:00:00").input.start, "2026-11-17T17:00:00");
        let done = found("2026-11-18T09:00:00");
        assert_eq!(
            (done.input.start.as_str(), done.is_done, done.input.duration_secs),
            ("2026-11-18T20:00:00", true, 0)
        );
    }
}
