//! M5 against the mock server: calendars, the agenda, events and tasks, invitations, through the
//! `Core` object the way the app uses it.
//!
//! The mock behaves like Stalwart 1.1.0 where it matters (see `support/calendar.rs`): it does not
//! expand repeating events and it validates almost nothing, so the core has to. All the times
//! here are fixed dates in a week of November 2026 and the zone Europe/Istanbul (UTC+3).

mod support;

use std::sync::Arc;

use kweb_core::{
    ffi::Core,
    model::{Agenda, EventInput, Frequency, RepeatRule},
    secrets::MemorySecrets,
    Error,
};
use serde_json::{json, Value};
use support::{MockServer, USERNAME};
use tokio::runtime::Runtime;

const ZONE: &str = "Europe/Istanbul";

struct Rig {
    // Field order is drop order: the core goes first, the mock and its runtime last.
    core: Arc<Core>,
    server: MockServer,
    _rt: Runtime,
    _dir: tempfile::TempDir,
}

impl Rig {
    /// A mock server and a core signed in to it.
    fn new() -> Rig {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        let server = rt.block_on(MockServer::start());
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("cache.db").to_string_lossy().into_owned();
        let core = Core::new(db_path, Arc::new(MemorySecrets::default())).unwrap();
        let url = core.begin_sign_in_with_redirect(server.url(), "ovh.kweb.mail:/oauth/callback".into()).unwrap();
        core.complete_sign_in(server.authorize(&url).unwrap()).unwrap();
        Rig { core, server, _rt: rt, _dir: dir }
    }

    fn agenda(&self, from: &str, to: &str) -> Agenda {
        self.core.agenda(at(from), at(to), ZONE.into()).expect("an agenda")
    }

    /// The agenda of the week of Monday 2026-11-16, as `(event id, local start, recurrence id)`.
    fn week(&self) -> Vec<String> {
        let agenda = self.agenda("2026-11-16T00:00:00", "2026-11-23T00:00:00");
        assert!(!agenda.from_cache);
        agenda.entries.iter().map(|e| format!("{} {}", e.event_id, local(e.starts_at))).collect()
    }
}

/// Unix seconds of a wall-clock time in Istanbul (UTC+3 all year since 2016).
fn at(local: &str) -> i64 {
    let (date, time) = local.split_once('T').unwrap();
    let days = days_from_civil(date);
    let secs: Vec<i64> = time.split(':').map(|p| p.parse().unwrap()).collect();
    days * 86_400 + secs[0] * 3600 + secs[1] * 60 + secs[2] - 3 * 3600
}

/// The wall-clock time in Istanbul, as `MM-DDTHH:MM`.
fn local(unix: i64) -> String {
    let shifted = unix + 3 * 3600;
    let (days, secs) = (shifted.div_euclid(86_400), shifted.rem_euclid(86_400));
    let (month, day) = month_day(days);
    format!("{month:02}-{day:02}T{:02}:{:02}", secs / 3600, secs % 3600 / 60)
}

/// Days since 1970-01-01 of "YYYY-MM-DD" (the algorithm of Howard Hinnant).
fn days_from_civil(date: &str) -> i64 {
    let p: Vec<i64> = date.split('-').map(|p| p.parse().unwrap()).collect();
    let (y, m, d) = (if p[1] <= 2 { p[0] - 1 } else { p[0] }, p[1], p[2]);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn month_day(days: i64) -> (i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    (if mp < 10 { mp + 3 } else { mp - 9 }, d)
}

fn timed() -> Value {
    json!({"@type": "Event", "title": "Timed", "start": "2026-11-16T10:00:00", "timeZone": ZONE, "duration": "PT1H",
        "locations": {"l1": {"@type": "Location", "name": "Room"}}})
}

fn all_day() -> Value {
    json!({"@type": "Event", "title": "AllDay", "start": "2026-11-17T00:00:00", "timeZone": ZONE, "duration": "P1D",
        "showWithoutTime": true})
}

/// A daily series that began a week before the span and never ends.
fn daily() -> Value {
    json!({"@type": "Event", "title": "Daily", "start": "2026-11-09T08:00:00", "timeZone": ZONE, "duration": "PT30M",
        "recurrenceRules": [{"@type": "RecurrenceRule", "frequency": "daily"}]})
}

fn task() -> Value {
    json!({"@type": "Task", "title": "Task", "due": "2026-11-18T17:00:00", "timeZone": ZONE})
}

fn invitation() -> Value {
    json!({"@type": "Event", "title": "Invite", "start": "2026-11-20T10:00:00", "timeZone": ZONE, "duration": "PT1H",
        "replyTo": {"imip": "mailto:boss@example.test"},
        "participants": {
            "m": {"@type": "Participant", "name": "Alice", "participationStatus": "needs-action", "expectReply": true,
                "roles": {"attendee": true}, "sendTo": {"imip": format!("mailto:{USERNAME}")}},
            "o": {"@type": "Participant", "name": "Boss", "participationStatus": "accepted",
                "roles": {"attendee": true, "owner": true}, "sendTo": {"imip": "mailto:boss@example.test"}}}})
}

fn input() -> EventInput {
    EventInput {
        calendar_id: "cal1".into(),
        title: "Dentist".into(),
        location: "Main street".into(),
        description: "Bring the card".into(),
        start: "2026-11-18T14:30:00".into(),
        time_zone: Some(ZONE.into()),
        duration_secs: 3600,
        all_day: false,
        is_task: false,
        repeat: None,
        alert_minutes: vec![10],
    }
}

fn invalid(result: Result<impl std::fmt::Debug, Error>) -> String {
    match result {
        Err(Error::Invalid { msg }) => msg,
        other => panic!("expected Error::Invalid, got {other:?}"),
    }
}

// ---- calendars ---------------------------------------------------------------------------

#[test]
fn the_calendars_are_listed_and_kept_for_when_the_server_is_away() {
    let rig = Rig::new();
    rig.server.add_calendar("work", "Work", false, false);
    let found = rig.core.calendars().unwrap();
    let flags: Vec<_> =
        found.iter().map(|c| (c.id.as_str(), c.name.as_str(), c.is_default, c.is_visible, c.may_write)).collect();
    assert_eq!(flags, [("cal1", "Personal", true, true, true), ("work", "Work", false, false, false)]);
    assert_eq!(found[0].color, None);

    rig.server.set_down(true);
    assert_eq!(rig.core.calendars().unwrap(), found, "from the cache");
}

#[test]
fn calendars_never_fetched_and_a_server_that_is_away_is_an_error_not_an_empty_list() {
    let rig = Rig::new();
    rig.server.set_down(true);
    assert!(matches!(rig.core.calendars(), Err(Error::Temporary { .. })));
}

#[test]
fn nothing_works_signed_out() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cache.db").to_string_lossy().into_owned();
    let core = Core::new(path, Arc::new(MemorySecrets::default())).unwrap();
    assert!(matches!(core.calendars(), Err(Error::SignInNeeded { .. })));
    assert!(matches!(core.agenda(0, 10, ZONE.into()), Err(Error::SignInNeeded { .. })));
    assert!(matches!(core.event("e".into(), None), Err(Error::SignInNeeded { .. })));
    assert!(matches!(core.save_event(None, input()), Err(Error::SignInNeeded { .. })));
    assert!(matches!(core.delete_event("e".into(), None), Err(Error::SignInNeeded { .. })));
    assert!(matches!(core.respond_to_event("e".into(), "accepted".into()), Err(Error::SignInNeeded { .. })));
    assert!(matches!(core.set_task_done("e".into(), true), Err(Error::SignInNeeded { .. })));
}

// ---- the agenda --------------------------------------------------------------------------

#[test]
fn the_agenda_of_a_week_has_every_kind_of_entry_in_order() {
    let rig = Rig::new();
    let (timed_id, all_day_id, daily_id, task_id) = (
        rig.server.add_event(timed()),
        rig.server.add_event(all_day()),
        rig.server.add_event(daily()),
        rig.server.add_event(task()),
    );
    // Left out: no time at all, another week, and a calendar that is hidden.
    rig.server.add_event(json!({"@type": "Task", "title": "No due"}));
    rig.server.add_event(json!({"@type": "Event", "title": "Later", "start": "2026-12-01T10:00:00", "timeZone": ZONE}));
    rig.server.add_calendar("hidden", "Hidden", false, true);
    rig.server.add_event(
        json!({"@type": "Event", "title": "Secret", "start": "2026-11-16T12:00:00", "calendarIds": {"hidden": true}}),
    );

    let agenda = rig.agenda("2026-11-16T00:00:00", "2026-11-23T00:00:00");
    assert!(!agenda.from_cache);
    let seen: Vec<String> = agenda
        .entries
        .iter()
        .map(|e| {
            format!(
                "{} {}{}",
                local(e.starts_at),
                e.title,
                e.recurrence_id.as_deref().map_or(String::new(), |_| " *".into())
            )
        })
        .collect();
    assert_eq!(
        seen,
        [
            "11-16T08:00 Daily *",
            "11-16T10:00 Timed",
            "11-17T00:00 AllDay",
            "11-17T08:00 Daily *",
            "11-18T08:00 Daily *",
            "11-18T17:00 Task",
            "11-19T08:00 Daily *",
            "11-20T08:00 Daily *",
            "11-21T08:00 Daily *",
            "11-22T08:00 Daily *",
        ]
    );
    let by_title = |title: &str| agenda.entries.iter().find(|e| e.title == title).unwrap();
    let (timed_entry, all_day_entry, task_entry) = (by_title("Timed"), by_title("AllDay"), by_title("Task"));
    assert_eq!(
        (timed_entry.event_id.as_str(), timed_entry.calendar_id.as_str(), timed_entry.location.as_str()),
        (timed_id.as_str(), "cal1", "Room")
    );
    assert_eq!(timed_entry.ends_at - timed_entry.starts_at, 3600);
    assert!(all_day_entry.all_day && all_day_entry.event_id == all_day_id);
    assert_eq!(all_day_entry.ends_at - all_day_entry.starts_at, 86_400);
    assert!(task_entry.is_task && !task_entry.is_done && task_entry.event_id == task_id);
    assert_eq!(task_entry.ends_at, task_entry.starts_at);
    let series: Vec<_> = agenda.entries.iter().filter(|e| e.event_id == daily_id).collect();
    assert_eq!(series.len(), 7);
    assert!(series.iter().all(|e| e.is_recurring));
    assert_eq!(series[1].recurrence_id.as_deref(), Some("2026-11-17T08:00:00"));
}

#[test]
fn a_series_is_found_in_a_span_long_after_its_first_start() {
    // The server finds a series only by its first start, so the core has to expand it itself.
    let rig = Rig::new();
    rig.server.add_event(daily());
    let agenda = rig.agenda("2026-12-24T00:00:00", "2026-12-26T00:00:00");
    let seen: Vec<String> = agenda.entries.iter().map(|e| local(e.starts_at)).collect();
    assert_eq!(seen, ["12-24T08:00", "12-25T08:00"]);
}

#[test]
fn the_agenda_follows_the_calendars_that_are_shown_and_reads_the_users_answer() {
    let rig = Rig::new();
    rig.server.add_calendar("work", "Work", true, true);
    rig.server.add_calendar("hidden", "Hidden", false, true);
    rig.server.add_event(timed());
    let mut work = invitation();
    work["calendarIds"] = json!({"work": true});
    rig.server.add_event(work);
    let mut hidden = timed();
    hidden["calendarIds"] = json!({"hidden": true});
    rig.server.add_event(hidden);
    let agenda = rig.agenda("2026-11-16T00:00:00", "2026-11-23T00:00:00");
    let seen: Vec<(&str, &str, Option<&str>)> =
        agenda.entries.iter().map(|e| (e.title.as_str(), e.calendar_id.as_str(), e.my_status.as_deref())).collect();
    assert_eq!(seen, [("Timed", "cal1", None), ("Invite", "work", Some("needs-action"))]);
    // The user is also known by the other addresses of the participant identities.
    rig.server.set_calendar_addresses(&["someone@else.test", "alice@alias.test"]);
    let me = rig.agenda("2026-11-16T00:00:00", "2026-11-23T00:00:00").entries;
    assert_eq!(me[1].my_status, Some("needs-action".into()), "the sign-in name counts too");
    let mut alias = invitation();
    alias["participants"]["m"]["sendTo"] = json!({"imip": "mailto:Alice@Alias.test"});
    rig.server.add_event(alias);
    let found = rig.agenda("2026-11-16T00:00:00", "2026-11-23T00:00:00").entries;
    assert_eq!(found.iter().filter(|e| e.my_status.is_some()).count(), 2);
}

#[test]
fn a_big_calendar_is_read_page_by_page() {
    let rig = Rig::new();
    for n in 0..1200 {
        let mut event = timed();
        event["title"] = json!(format!("Event {n}"));
        event["start"] = json!(format!("2026-11-{:02}T{:02}:00:00", 16 + n % 7, 6 + n % 14));
        rig.server.add_event(event);
    }
    rig.server.reset_request_counts();
    let agenda = rig.agenda("2026-11-16T00:00:00", "2026-11-23T00:00:00");
    assert_eq!(agenda.entries.len(), 1200);
    assert_eq!(rig.server.request_count("CalendarEvent/query"), 3, "500 ids a page");
    assert!(agenda.entries.windows(2).all(|w| w[0].starts_at <= w[1].starts_at));
}

#[test]
fn the_agenda_comes_from_the_cache_when_the_server_is_away() {
    let rig = Rig::new();
    rig.server.add_event(timed());
    rig.server.add_event(all_day());
    rig.server.add_event(task());
    let live = rig.agenda("2026-11-16T00:00:00", "2026-11-23T00:00:00");
    assert!(!live.from_cache && live.entries.len() == 3);

    rig.server.set_down(true);
    let cached = rig.agenda("2026-11-16T00:00:00", "2026-11-23T00:00:00");
    assert!(cached.from_cache);
    assert_eq!(cached.entries, live.entries, "in the same order");
    // A part of the span is answered from the same cache, and a span never seen is empty.
    let part = rig.agenda("2026-11-17T00:00:00", "2026-11-18T00:00:00");
    assert_eq!(part.entries.iter().map(|e| e.title.as_str()).collect::<Vec<_>>(), ["AllDay"]);
    let unseen = rig.agenda("2027-03-01T00:00:00", "2027-03-08T00:00:00");
    assert!(unseen.from_cache && unseen.entries.is_empty());

    rig.server.set_down(false);
    assert!(!rig.agenda("2026-11-16T00:00:00", "2026-11-23T00:00:00").from_cache);
}

#[test]
fn what_the_server_no_longer_has_goes_from_the_cache_at_the_next_look() {
    let rig = Rig::new();
    let gone = rig.server.add_event(timed());
    rig.server.add_event(all_day());
    assert_eq!(rig.week().len(), 2);
    rig.server.remove_event(&gone);
    assert_eq!(rig.week().len(), 1);
    rig.server.set_down(true);
    let cached = rig.agenda("2026-11-16T00:00:00", "2026-11-23T00:00:00");
    assert_eq!(cached.entries.iter().map(|e| e.title.as_str()).collect::<Vec<_>>(), ["AllDay"]);
}

#[test]
fn a_span_the_server_would_not_expand_is_refused() {
    let rig = Rig::new();
    rig.server.add_event(timed());
    let from = at("2026-01-01T00:00:00");
    // Stalwart says P52W1D: 365 days. 365 is fine, one more is not.
    assert!(rig.core.agenda(from, from + 365 * 86_400, ZONE.into()).is_ok());
    let msg = invalid(rig.core.agenda(from, from + 366 * 86_400, ZONE.into()));
    assert!(msg.contains("365 days"), "{msg}");
    // A server that does not say gets a year and a day.
    let rig = Rig::new();
    rig.server.set_calendar_max_span(None);
    assert!(rig.core.agenda(from, from + 366 * 86_400, ZONE.into()).is_ok());
    assert!(matches!(rig.core.agenda(from, from + 366 * 86_400 + 1, ZONE.into()), Err(Error::Invalid { .. })));
    // A shorter limit is the server's own.
    let rig = Rig::new();
    rig.server.set_calendar_max_span(Some("P4W"));
    assert!(rig.core.agenda(from, from + 28 * 86_400, ZONE.into()).is_ok());
    assert!(invalid(rig.core.agenda(from, from + 29 * 86_400, ZONE.into())).contains("28 days"));
}

#[test]
fn a_span_that_ends_before_it_starts_and_a_zone_that_is_no_zone_are_refused_without_asking_the_server() {
    let rig = Rig::new();
    rig.server.reset_request_counts();
    for (from, to, zone) in
        [(10, 10, ZONE), (10, 5, ZONE), (0, 10, ""), (0, 10, "Europe/ Istanbul"), (0, 10, "Mars/Base")]
    {
        assert!(matches!(rig.core.agenda(from, to, zone.into()), Err(Error::Invalid { .. })), "{from} {to} {zone:?}");
    }
    assert_eq!(rig.server.request_count("/jmap"), 0);
}

#[test]
fn a_span_error_is_not_answered_from_the_cache() {
    // Invalid is the caller's mistake: even with the server away it is not a cache hit.
    let rig = Rig::new();
    rig.server.set_down(true);
    assert!(matches!(rig.core.agenda(10, 5, ZONE.into()), Err(Error::Invalid { .. })));
}

// ---- events: create, read, change, delete -----------------------------------------------

#[test]
fn an_event_is_created_read_back_changed_and_deleted() {
    let rig = Rig::new();
    let id = rig.core.save_event(None, input()).unwrap();

    // What the server has is what the app owns, and the app reads it back as it wrote it.
    let stored = rig.server.event(&id).unwrap();
    assert_eq!(stored["@type"], "Event");
    assert_eq!(
        (stored["title"].as_str(), stored["start"].as_str(), stored["duration"].as_str()),
        (Some("Dentist"), Some("2026-11-18T14:30:00"), Some("PT1H"))
    );
    assert_eq!(stored["calendarIds"], json!({"cal1": true}));
    let detail = rig.core.event(id.clone(), None).unwrap();
    assert_eq!(detail.input, input());
    assert!(detail.may_write && !detail.has_complex_repeat && detail.participants.is_empty());
    assert_eq!(rig.week(), [format!("{id} 11-18T14:30")]);

    // A change sends only what changed; what the app does not own stays.
    rig.server.add_event(invitation());
    let mut changed = input();
    changed.title = "Dentist (moved)".into();
    changed.start = "2026-11-19T15:00:00".into();
    changed.location = String::new();
    changed.alert_minutes = vec![10, 60];
    assert_eq!(rig.core.save_event(Some(id.clone()), changed.clone()).unwrap(), id);
    let stored = rig.server.event(&id).unwrap();
    assert_eq!(
        (stored["title"].as_str(), stored["start"].as_str()),
        (Some("Dentist (moved)"), Some("2026-11-19T15:00:00"))
    );
    assert!(stored.get("locations").is_none());
    assert_eq!(stored["uid"], format!("uid-{id}"));
    assert_eq!(rig.core.event(id.clone(), None).unwrap().input, changed);
    assert_eq!(rig.week().first().unwrap(), &format!("{id} 11-19T15:00"));

    rig.core.delete_event(id.clone(), None).unwrap();
    assert!(rig.server.event(&id).is_none());
    assert_eq!(invalid(rig.core.event(id, None)), "the event no longer exists on the server");
}

#[test]
fn a_change_leaves_everything_the_app_does_not_own_on_the_server() {
    let rig = Rig::new();
    let mut event = invitation();
    event["x-custom"] = json!({"kept": true});
    event["recurrenceRules"] = json!([{"frequency": "monthly", "byMonthDay": [1, 15]}]);
    event["alerts"] = json!({"abs": {"trigger": {"@type": "AbsoluteTrigger", "when": "2026-11-20T06:00:00Z"}}});
    let id = rig.server.add_event(event.clone());
    let detail = rig.core.event(id.clone(), None).unwrap();
    assert!(detail.has_complex_repeat);
    assert_eq!(detail.input.repeat, None);
    assert_eq!((detail.participants.len(), detail.my_status.as_deref()), (2, Some("needs-action")));

    let mut input = detail.input.clone();
    input.title = "Renamed".into();
    input.repeat =
        Some(RepeatRule { frequency: Frequency::Daily, interval: 1, by_day: vec![], until: None, count: None });
    input.alert_minutes = vec![5];
    rig.core.save_event(Some(id.clone()), input).unwrap();
    let stored = rig.server.event(&id).unwrap();
    assert_eq!(stored["title"], "Renamed");
    assert_eq!(stored["x-custom"], event["x-custom"]);
    assert_eq!(stored["participants"], event["participants"]);
    assert_eq!(stored["recurrenceRules"], event["recurrenceRules"], "a rule the app cannot hold is not touched");
    assert_eq!(stored["alerts"]["abs"], event["alerts"]["abs"]);
    assert_eq!(stored["alerts"].as_object().unwrap().len(), 2);
}

#[test]
fn a_change_that_changes_nothing_sends_nothing() {
    let rig = Rig::new();
    let id = rig.server.add_event(timed());
    let detail = rig.core.event(id.clone(), None).unwrap();
    rig.server.reset_request_counts();
    rig.core.save_event(Some(id), detail.input).unwrap();
    assert_eq!(rig.server.request_count("CalendarEvent/set"), 0);
}

#[test]
fn events_with_a_repeat_rule_an_all_day_entry_and_a_task_are_created_as_the_app_means_them() {
    let rig = Rig::new();
    let weekly = RepeatRule {
        frequency: Frequency::Weekly,
        interval: 2,
        by_day: vec!["mo".into(), "we".into()],
        until: Some("2026-12-31".into()),
        count: None,
    };
    let series = rig
        .core
        .save_event(None, EventInput { repeat: Some(weekly.clone()), start: "2026-11-16T09:00:00".into(), ..input() })
        .unwrap();
    assert_eq!(rig.core.event(series.clone(), None).unwrap().input.repeat, Some(weekly));
    // Every second week, Monday and Wednesday: the week of the 16th has both, the week of the 23rd none.
    let agenda = rig.agenda("2026-11-16T00:00:00", "2026-12-01T00:00:00");
    let seen: Vec<String> = agenda.entries.iter().map(|e| local(e.starts_at)).collect();
    assert_eq!(seen, ["11-16T09:00", "11-18T09:00", "11-30T09:00"]);

    let trip = EventInput {
        all_day: true,
        start: "2026-11-17T00:00:00".into(),
        duration_secs: 2 * 86_400,
        time_zone: None,
        alert_minutes: vec![],
        ..input()
    };
    let trip_id = rig.core.save_event(None, trip.clone()).unwrap();
    let stored = rig.server.event(&trip_id).unwrap();
    assert_eq!(
        (stored["showWithoutTime"].clone(), stored["duration"].as_str(), stored.get("timeZone")),
        (json!(true), Some("P2D"), None)
    );
    assert_eq!(rig.core.event(trip_id, None).unwrap().input, trip);

    let todo = EventInput {
        is_task: true,
        duration_secs: 0,
        start: "2026-11-18T17:00:00".into(),
        alert_minutes: vec![30],
        ..input()
    };
    let todo_id = rig.core.save_event(None, todo.clone()).unwrap();
    let stored = rig.server.event(&todo_id).unwrap();
    assert_eq!(
        (stored["@type"].as_str(), stored["due"].as_str(), stored.get("start")),
        (Some("Task"), Some("2026-11-18T17:00:00"), None)
    );
    assert_eq!(stored["alerts"]["a30"]["trigger"]["relativeTo"], "end", "a task has no start to measure from");
    assert_eq!(rig.core.event(todo_id, None).unwrap().input, todo);
}

#[test]
fn an_edit_of_a_series_changes_the_whole_series() {
    let rig = Rig::new();
    let id = rig.server.add_event(daily());
    let mut edited = rig.core.event(id.clone(), None).unwrap().input;
    edited.title = "Daily (renamed)".into();
    edited.repeat =
        Some(RepeatRule { frequency: Frequency::Daily, interval: 2, by_day: vec![], until: None, count: Some(3) });
    rig.core.save_event(Some(id.clone()), edited).unwrap();
    let seen: Vec<String> =
        rig.agenda("2026-11-09T00:00:00", "2026-12-09T00:00:00").entries.iter().map(|e| local(e.starts_at)).collect();
    assert_eq!(seen, ["11-09T08:00", "11-11T08:00", "11-13T08:00"]);
    // Taking the repeat off leaves one event.
    let mut single = rig.core.event(id.clone(), None).unwrap().input;
    single.repeat = None;
    rig.core.save_event(Some(id.clone()), single).unwrap();
    assert!(rig.server.event(&id).unwrap().get("recurrenceRules").is_none());
    assert_eq!(rig.agenda("2026-11-09T00:00:00", "2026-12-09T00:00:00").entries.len(), 1);
}

#[test]
fn one_deleted_occurrence_leaves_the_others() {
    let rig = Rig::new();
    let id = rig.server.add_event(daily());
    assert_eq!(rig.week().len(), 7);
    rig.core.delete_event(id.clone(), Some("2026-11-18T08:00:00".into())).unwrap();
    // The entries of the event were dropped from the cache, so none is brought back while the server is away.
    rig.server.set_down(true);
    let cached = rig.agenda("2026-11-16T00:00:00", "2026-11-23T00:00:00");
    assert!(cached.from_cache && cached.entries.is_empty());
    rig.server.set_down(false);

    let seen = rig.week();
    assert_eq!(seen.len(), 6);
    assert!(!seen.iter().any(|e| e.ends_with("11-18T08:00")));
    assert_eq!(
        rig.server.event(&id).unwrap()["recurrenceOverrides"],
        json!({"2026-11-18T08:00:00": {"excluded": true}})
    );
    // A second one adds to the first, and the series still reads as a simple repeat.
    rig.core.delete_event(id.clone(), Some("2026-11-19T08:00:00".into())).unwrap();
    assert_eq!(rig.week().len(), 5);
    let detail = rig.core.event(id.clone(), None).unwrap();
    assert!(detail.input.repeat.is_some() && !detail.has_complex_repeat);
    // Everything goes with the series.
    rig.core.delete_event(id, None).unwrap();
    assert!(rig.week().is_empty());
    // An occurrence of an event that does not repeat, or one named wrongly, is refused.
    let single = rig.server.add_event(timed());
    assert!(invalid(rig.core.delete_event(single, Some("2026-11-16T10:00:00".into()))).contains("repeat"));
    let again = rig.server.add_event(daily());
    assert!(matches!(rig.core.delete_event(again, Some("Monday".into())), Err(Error::Invalid { .. })));
}

#[test]
fn a_cached_entry_is_not_shown_after_the_user_changed_the_event() {
    let rig = Rig::new();
    let id = rig.server.add_event(timed());
    let before = rig.agenda("2026-11-16T00:00:00", "2026-11-23T00:00:00");
    assert_eq!(before.entries[0].title, "Timed");
    let mut edited = rig.core.event(id.clone(), None).unwrap().input;
    edited.title = "Retitled".into();
    rig.core.save_event(Some(id), edited).unwrap();
    rig.server.set_down(true);
    let cached = rig.agenda("2026-11-16T00:00:00", "2026-11-23T00:00:00");
    assert!(cached.from_cache);
    assert!(cached.entries.is_empty(), "the stale copy is gone: {:?}", cached.entries);
}

#[test]
fn deleting_an_event_that_is_already_gone_is_done_and_changing_it_is_not() {
    let rig = Rig::new();
    let id = rig.server.add_event(timed());
    rig.server.remove_event(&id);
    rig.core.delete_event(id.clone(), None).unwrap();
    assert!(matches!(rig.core.save_event(Some(id.clone()), input()), Err(Error::Invalid { .. })));
    assert!(matches!(rig.core.respond_to_event(id.clone(), "accepted".into()), Err(Error::Invalid { .. })));
    assert!(matches!(rig.core.set_task_done(id, true), Err(Error::Invalid { .. })));
}

// ---- invitations and tasks ---------------------------------------------------------------

#[test]
fn an_invitation_is_answered_and_the_organizer_is_told() {
    let rig = Rig::new();
    let id = rig.server.add_event(invitation());
    assert_eq!(rig.core.event(id.clone(), None).unwrap().my_status.as_deref(), Some("needs-action"));
    rig.core.respond_to_event(id.clone(), "accepted".into()).unwrap();
    let stored = rig.server.event(&id).unwrap();
    assert_eq!(stored["participants"]["m"]["participationStatus"], "accepted");
    assert_eq!(stored["participants"]["o"]["participationStatus"], "accepted", "nobody else's answer changes");
    assert_eq!(rig.server.scheduled_events(), std::slice::from_ref(&id), "scheduling messages were asked for");
    let detail = rig.core.event(id.clone(), None).unwrap();
    assert_eq!(detail.my_status.as_deref(), Some("accepted"));
    assert!(detail.participants.iter().any(|p| p.is_me && p.status == "accepted"));
    assert!(detail.participants.iter().any(|p| p.is_organizer && p.name == "Boss" && p.email == "boss@example.test"));
    assert_eq!(
        rig.agenda("2026-11-16T00:00:00", "2026-11-23T00:00:00").entries[0].my_status.as_deref(),
        Some("accepted")
    );
    rig.core.respond_to_event(id.clone(), "declined".into()).unwrap();
    assert_eq!(rig.server.event(&id).unwrap()["participants"]["m"]["participationStatus"], "declined");
}

#[test]
fn an_answer_to_something_that_is_no_invitation_or_is_no_answer_is_refused() {
    let rig = Rig::new();
    let invited = rig.server.add_event(invitation());
    let own = rig.server.add_event(timed());
    assert!(invalid(rig.core.respond_to_event(own, "accepted".into())).contains("not invited"));
    for answer in ["needs-action", "yes", ""] {
        assert!(invalid(rig.core.respond_to_event(invited.clone(), answer.into())).contains("accepted"), "{answer}");
    }
    assert!(rig.server.scheduled_events().is_empty());
    // Not the user's invitation: the participant is someone else.
    let mut others = invitation();
    others["participants"]["m"]["sendTo"] = json!({"imip": "mailto:other@example.test"});
    let others = rig.server.add_event(others);
    assert!(invalid(rig.core.respond_to_event(others, "accepted".into())).contains("not invited"));
}

#[test]
fn a_task_is_done_and_undone() {
    let rig = Rig::new();
    let id = rig.server.add_event(task());
    rig.core.set_task_done(id.clone(), true).unwrap();
    assert_eq!(rig.server.event(&id).unwrap()["progress"], "completed");
    assert!(rig.core.event(id.clone(), None).unwrap().is_done);
    let entries = rig.agenda("2026-11-16T00:00:00", "2026-11-23T00:00:00").entries;
    assert!(entries[0].is_task && entries[0].is_done);
    rig.core.set_task_done(id.clone(), false).unwrap();
    assert_eq!(rig.server.event(&id).unwrap()["progress"], "needs-action");
    assert!(!rig.agenda("2026-11-16T00:00:00", "2026-11-23T00:00:00").entries[0].is_done);
    // An event cannot be done.
    let event = rig.server.add_event(timed());
    assert!(invalid(rig.core.set_task_done(event, true)).contains("task"));
}

// ---- what is refused before the server is asked ------------------------------------------

#[test]
fn what_cannot_be_saved_is_refused_with_a_reason_and_nothing_is_sent() {
    let rig = Rig::new();
    rig.server.add_calendar("readonly", "Holidays", true, false);
    let repeat = |interval: u32, by_day: &[&str], until: Option<&str>| {
        Some(RepeatRule {
            frequency: Frequency::Weekly,
            interval,
            by_day: by_day.iter().map(|d| d.to_string()).collect(),
            until: until.map(str::to_string),
            count: None,
        })
    };
    let cases: Vec<(EventInput, &str)> = vec![
        (EventInput { title: "   ".into(), ..input() }, "title"),
        (EventInput { start: "2026-11-18".into(), ..input() }, "start"),
        (EventInput { start: "18.11.2026 14:30".into(), ..input() }, "start"),
        (EventInput { start: String::new(), ..input() }, "start"),
        (EventInput { time_zone: Some("Mars/Base".into()), ..input() }, "Mars/Base"),
        (EventInput { calendar_id: "nowhere".into(), ..input() }, "calendar"),
        (EventInput { calendar_id: "readonly".into(), ..input() }, "Holidays"),
        (EventInput { repeat: repeat(0, &[], None), ..input() }, "every 1"),
        (EventInput { repeat: repeat(1, &["monday"], None), ..input() }, "'monday'"),
        (EventInput { repeat: repeat(1, &[], Some("next year")), ..input() }, "date"),
        (
            EventInput { all_day: true, start: "2026-11-18T09:00:00".into(), duration_secs: 86_400, ..input() },
            "all-day",
        ),
        (EventInput { all_day: true, start: "2026-11-18T00:00:00".into(), duration_secs: 3600, ..input() }, "all-day"),
    ];
    rig.server.reset_request_counts();
    for (bad, word) in cases {
        let msg = invalid(rig.core.save_event(None, bad.clone()));
        assert!(msg.contains(word), "{word}: {msg}");
        // The same on an event that exists.
        let id = rig.server.add_event(timed());
        assert!(matches!(rig.core.save_event(Some(id), bad), Err(Error::Invalid { .. })));
    }
    assert_eq!(rig.server.request_count("CalendarEvent/set"), 0);
    assert_eq!(rig.server.events().len(), 12, "only the ones the test made itself");
}

#[test]
fn an_event_cannot_become_a_task() {
    let rig = Rig::new();
    let id = rig.server.add_event(timed());
    let mut as_task = rig.core.event(id.clone(), None).unwrap().input;
    as_task.is_task = true;
    as_task.duration_secs = 0;
    assert!(invalid(rig.core.save_event(Some(id.clone()), as_task)).contains("task"));
    assert_eq!(rig.server.event(&id).unwrap()["@type"], "Event");
}

#[test]
fn an_event_is_moved_to_another_calendar() {
    let rig = Rig::new();
    let id = rig.server.add_event(timed());
    rig.server.add_calendar("work", "Work", true, true);
    let mut input = rig.core.event(id.clone(), None).unwrap().input;
    input.calendar_id = "work".into();
    rig.core.save_event(Some(id.clone()), input).unwrap();
    assert_eq!(rig.server.event(&id).unwrap()["calendarIds"], json!({"work": true}));
    assert_eq!(rig.core.event(id, None).unwrap().input.calendar_id, "work");
}

#[test]
fn a_write_while_the_server_is_away_is_a_temporary_error_and_changes_nothing() {
    let rig = Rig::new();
    let id = rig.server.add_event(timed());
    rig.server.set_down(true);
    // The calendars were never fetched, so even the check of the calendar has nothing to go on.
    assert!(matches!(rig.core.save_event(None, input()), Err(Error::Temporary { .. })));
    assert!(matches!(rig.core.delete_event(id.clone(), None), Err(Error::Temporary { .. })));
    assert!(matches!(rig.core.respond_to_event(id.clone(), "accepted".into()), Err(Error::Temporary { .. })));
    assert!(matches!(rig.core.set_task_done(id.clone(), true), Err(Error::Temporary { .. })));
    assert!(matches!(rig.core.event(id.clone(), None), Err(Error::Temporary { .. })));
    rig.server.set_down(false);
    assert_eq!(rig.server.events().len(), 1);
    assert_eq!(rig.server.event(&id).unwrap()["title"], "Timed");
}

// ---- what a strict review found ----------------------------------------------------------

#[test]
fn a_write_after_someone_else_changed_the_event_is_made_from_a_fresh_read() {
    let rig = Rig::new();
    let id = rig.server.add_event(timed());
    let mut edited = rig.core.event(id.clone(), None).unwrap().input;
    edited.title = "Mine".into();
    // Another client adds something between our read and our write: the first write is refused.
    rig.server.change_event_after_next_get(&id, "x-theirs", json!("kept"));
    rig.server.reset_request_counts();
    rig.core.save_event(Some(id.clone()), edited).unwrap();
    let stored = rig.server.event(&id).unwrap();
    assert_eq!((stored["title"].as_str(), stored["x-theirs"].as_str()), (Some("Mine"), Some("kept")));
    assert_eq!((rig.server.request_count("CalendarEvent/get"), rig.server.request_count("CalendarEvent/set")), (2, 2));

    // A map that is set whole must not wipe out what another client put in it: here an occurrence they deleted.
    let series = rig.server.add_event(daily());
    rig.server.change_event_after_next_get(
        &series,
        "recurrenceOverrides",
        json!({"2026-11-18T08:00:00": {"excluded": true}}),
    );
    rig.core.delete_event(series.clone(), Some("2026-11-19T08:00:00".into())).unwrap();
    assert_eq!(
        rig.server.event(&series).unwrap()["recurrenceOverrides"],
        json!({"2026-11-18T08:00:00": {"excluded": true}, "2026-11-19T08:00:00": {"excluded": true}})
    );
}

#[test]
fn a_second_clash_is_a_temporary_error_and_writes_nothing() {
    let rig = Rig::new();
    let id = rig.server.add_event(timed());
    let mut edited = rig.core.event(id.clone(), None).unwrap().input;
    edited.title = "Mine".into();
    rig.server.change_event_after_next_get(&id, "x-theirs", json!("One"));
    rig.server.change_event_after_next_get(&id, "x-theirs", json!("Two"));
    assert!(matches!(rig.core.save_event(Some(id.clone()), edited.clone()), Err(Error::Temporary { .. })));
    let stored = rig.server.event(&id).unwrap();
    assert_eq!((stored["title"].as_str(), stored["x-theirs"].as_str()), (Some("Timed"), Some("Two")));
    // Once the other client is quiet the same save goes through.
    rig.core.save_event(Some(id.clone()), edited).unwrap();
    assert_eq!(rig.server.event(&id).unwrap()["title"], "Mine");
    // The same for the other writes.
    let task = rig.server.add_event(task());
    rig.server.change_event_after_next_get(&task, "title", json!("a"));
    rig.server.change_event_after_next_get(&task, "title", json!("b"));
    assert!(matches!(rig.core.set_task_done(task, true), Err(Error::Temporary { .. })));
}

#[test]
fn an_event_in_two_calendars_stays_in_both_unless_its_calendar_is_changed() {
    let rig = Rig::new();
    rig.server.add_calendar("work", "Work", true, true);
    rig.server.add_calendar("other", "Other", true, true);
    let mut shared = timed();
    shared["calendarIds"] = json!({"cal1": true, "work": true});
    let id = rig.server.add_event(shared);
    let detail = rig.core.event(id.clone(), None).unwrap();
    rig.server.reset_request_counts();
    rig.core.save_event(Some(id.clone()), detail.input.clone()).unwrap();
    assert_eq!(rig.server.request_count("CalendarEvent/set"), 0);
    let mut renamed = detail.input.clone();
    renamed.title = "Renamed".into();
    rig.core.save_event(Some(id.clone()), renamed).unwrap();
    assert_eq!(rig.server.event(&id).unwrap()["calendarIds"], json!({"cal1": true, "work": true}));
    // Moved: the calendar the app shows (the first) is replaced, the other one stays.
    let moved = EventInput { calendar_id: "other".into(), ..detail.input };
    rig.core.save_event(Some(id.clone()), moved).unwrap();
    assert_eq!(rig.server.event(&id).unwrap()["calendarIds"], json!({"other": true, "work": true}));
}

#[test]
fn moving_a_series_keeps_its_deleted_and_changed_occurrences() {
    let rig = Rig::new();
    let mut series = daily();
    series["recurrenceOverrides"] = json!({
        "2026-11-17T08:00:00": {"excluded": true},
        "2026-11-18T08:00:00": {"title": "Daily (moved)", "start": "2026-11-18T11:15:00"},
    });
    let id = rig.server.add_event(series);
    let mut later = rig.core.event(id.clone(), None).unwrap().input;
    later.start = "2026-11-09T09:30:00".into();
    rig.core.save_event(Some(id.clone()), later).unwrap();
    assert_eq!(
        rig.server.event(&id).unwrap()["recurrenceOverrides"],
        json!({
            "2026-11-17T09:30:00": {"excluded": true},
            "2026-11-18T09:30:00": {"title": "Daily (moved)", "start": "2026-11-18T12:45:00"},
        })
    );
    let found: Vec<(String, String)> = rig
        .agenda("2026-11-16T00:00:00", "2026-11-20T00:00:00")
        .entries
        .iter()
        .map(|e| (local(e.starts_at), e.title.clone()))
        .collect();
    assert_eq!(
        found,
        [
            ("11-16T09:30".to_string(), "Daily".to_string()),
            ("11-18T12:45".to_string(), "Daily (moved)".to_string()),
            ("11-19T09:30".to_string(), "Daily".to_string()),
        ]
    );
}

#[test]
fn the_detail_of_one_occurrence_is_what_the_agenda_shows_for_it() {
    let rig = Rig::new();
    let mut series = daily();
    series["recurrenceOverrides"] = json!({
        "2026-11-17T08:00:00": {"title": "Retitled", "start": "2026-11-17T11:15:00", "duration": "PT2H"},
        "2026-11-19T08:00:00": {"excluded": true},
    });
    let id = rig.server.add_event(series);
    let agenda = rig.agenda("2026-11-16T00:00:00", "2026-11-21T00:00:00");
    for entry in &agenda.entries {
        let detail = rig.core.event(id.clone(), entry.recurrence_id.clone()).unwrap();
        assert_eq!(
            (detail.input.title.as_str(), local(entry.starts_at)),
            (entry.title.as_str(), {
                // The detail's start is local to the event's zone, which is the zone of this test.
                let (date, time) = detail.input.start.split_once('T').unwrap();
                format!("{}T{}", &date[5..], &time[..5])
            })
        );
    }
    let moved = rig.core.event(id.clone(), Some("2026-11-17T08:00:00".into())).unwrap();
    assert_eq!((moved.input.start.as_str(), moved.input.duration_secs), ("2026-11-17T11:15:00", 7200));
    assert_eq!(rig.core.event(id.clone(), None).unwrap().input.start, "2026-11-09T08:00:00");
    for rid in ["2026-11-19T08:00:00", "2026-11-17T09:00:00", "nonsense"] {
        assert!(matches!(rig.core.event(id.clone(), Some(rid.into())), Err(Error::Invalid { .. })), "{rid}");
    }
    let single = rig.server.add_event(timed());
    assert!(matches!(rig.core.event(single, Some("2026-11-16T10:00:00".into())), Err(Error::Invalid { .. })));
}

#[test]
fn an_event_stored_in_a_shape_the_app_would_not_write_can_still_be_edited() {
    let rig = Rig::new();
    let mut odd = timed();
    odd["timeZone"] = json!("Custom/Place");
    odd["useDefaultAlerts"] = json!(true);
    let id = rig.server.add_event(odd);
    let mut input = rig.core.event(id.clone(), None).unwrap().input;
    input.title = "Renamed".into();
    rig.core.save_event(Some(id.clone()), input.clone()).unwrap();
    assert_eq!(rig.server.event(&id).unwrap()["title"], "Renamed");
    // Taking the reminders off an event that has the server's default ones turns those off too.
    input.alert_minutes = vec![5];
    rig.core.save_event(Some(id.clone()), input.clone()).unwrap();
    assert_eq!(rig.server.event(&id).unwrap()["useDefaultAlerts"], false);
    // A zone that is changed is checked.
    input.time_zone = Some("Mars/Base".into());
    assert!(invalid(rig.core.save_event(Some(id), input)).contains("Mars/Base"));
}

#[test]
fn a_task_with_only_a_start_is_saved_without_gaining_a_due_time() {
    let rig = Rig::new();
    let id = rig.server.add_event(json!({"@type": "Task", "title": "Started", "start": "2026-11-17T09:00:00"}));
    let mut input = rig.core.event(id.clone(), None).unwrap().input;
    input.title = "Renamed".into();
    rig.core.save_event(Some(id.clone()), input).unwrap();
    let stored = rig.server.event(&id).unwrap();
    assert_eq!((stored["title"].as_str(), stored.get("due")), (Some("Renamed"), None));
}

#[test]
fn the_organizer_cannot_answer_their_own_invitation() {
    let rig = Rig::new();
    let mut mine = invitation();
    mine["participants"]["m"]["roles"] = json!({"attendee": true, "owner": true});
    mine["participants"]["o"]["roles"] = json!({"attendee": true});
    let id = rig.server.add_event(mine);
    assert!(invalid(rig.core.respond_to_event(id.clone(), "accepted".into())).contains("organise"));
    assert!(rig.server.scheduled_events().is_empty());
    assert_eq!(rig.server.event(&id).unwrap()["participants"]["m"]["participationStatus"], "needs-action");
}
