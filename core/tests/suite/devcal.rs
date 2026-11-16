//! M12 stage 4 against the mock server: the server's calendars mirrored into the phone's calendar
//! provider, through `Core::device_calendar_sync` and a fake provider that behaves like the
//! phone's (rows, a dirty flag the user's edits set, a state the sync keeps).

use std::sync::{Arc, Mutex};

use crate::support::{MockServer, SHARED_ACCOUNT};
use kweb_core::{
    ffi::Core,
    jmap::devcal::{CalendarProvider, DeviceCalendar, DeviceEvent, DeviceSyncOutcome, DeviceSyncReport, ProviderError},
    secrets::MemorySecrets,
};
use serde_json::{json, Value};
use tokio::runtime::Runtime;

const ZONE: &str = "Europe/Istanbul";
const OWNER: &str = "app1";

#[derive(Default)]
struct Inner {
    state: Option<String>,
    calendars: Vec<DeviceCalendar>,
    rows: Vec<DeviceEvent>,
    next: i64,
    /// Every call that wrote, for the tests that say what was not written.
    writes: usize,
}

/// The phone's provider as far as the sync can tell: it keeps rows, the sync's own columns and the dirty flag.
#[derive(Default)]
struct Fake(Mutex<Inner>);

impl Fake {
    fn rows(&self) -> Vec<DeviceEvent> {
        self.0.lock().unwrap().rows.clone()
    }

    fn row(&self, sync_id: &str) -> DeviceEvent {
        self.rows()
            .into_iter()
            .find(|r| r.sync_id.as_deref() == Some(sync_id))
            .unwrap_or_else(|| panic!("no row {sync_id}"))
    }

    fn has(&self, sync_id: &str) -> bool {
        self.rows().iter().any(|r| r.sync_id.as_deref() == Some(sync_id))
    }

    fn titles(&self) -> Vec<String> {
        let mut t: Vec<String> = self.rows().into_iter().map(|r| r.title).collect();
        t.sort();
        t
    }

    fn calendars(&self) -> Vec<DeviceCalendar> {
        self.0.lock().unwrap().calendars.clone()
    }

    /// The user edits a row in the calendar app: the provider marks it dirty.
    fn edit(&self, sync_id: &str, f: impl FnOnce(&mut DeviceEvent)) {
        let mut g = self.0.lock().unwrap();
        let row = g.rows.iter_mut().find(|r| r.sync_id.as_deref() == Some(sync_id)).expect("a row");
        f(row);
        row.dirty = true;
    }

    fn delete(&self, sync_id: &str) {
        let mut g = self.0.lock().unwrap();
        let row = g.rows.iter_mut().find(|r| r.sync_id.as_deref() == Some(sync_id)).expect("a row");
        row.deleted = true;
        row.dirty = true;
    }

    /// The user makes an event in the calendar app.
    fn add(&self, calendar: &str, title: &str, start_ms: i64) {
        let mut g = self.0.lock().unwrap();
        g.next += 1;
        let row = DeviceEvent {
            row_id: Some(g.next),
            sync_id: None,
            calendar_sync_id: format!("{OWNER}/{calendar}"),
            uid: None,
            title: title.into(),
            description: String::new(),
            location: String::new(),
            start_ms,
            end_ms: Some(start_ms + 3_600_000),
            duration: None,
            all_day: false,
            time_zone: ZONE.into(),
            rrule: None,
            exdate: None,
            status: 1,
            availability: 0,
            access_level: 0,
            original_sync_id: None,
            original_instance_ms: None,
            original_all_day: false,
            dirty: true,
            deleted: false,
            shadow: None,
            baseline: None,
        };
        g.rows.push(row);
    }

    fn writes(&self) -> usize {
        self.0.lock().unwrap().writes
    }
}

type R<T> = Result<T, ProviderError>;

impl CalendarProvider for Fake {
    fn read_state(&self) -> R<Option<String>> {
        Ok(self.0.lock().unwrap().state.clone())
    }
    fn write_state(&self, state: String) -> R<()> {
        self.0.lock().unwrap().state = Some(state);
        Ok(())
    }
    fn list_calendars(&self) -> R<Vec<DeviceCalendar>> {
        Ok(self.0.lock().unwrap().calendars.clone())
    }
    fn upsert_calendar(&self, calendar: DeviceCalendar) -> R<()> {
        let mut g = self.0.lock().unwrap();
        g.writes += 1;
        match g.calendars.iter_mut().find(|c| c.sync_id == calendar.sync_id) {
            Some(c) => *c = calendar,
            None => g.calendars.push(calendar),
        }
        Ok(())
    }
    fn delete_calendar(&self, sync_id: String) -> R<()> {
        let mut g = self.0.lock().unwrap();
        g.calendars.retain(|c| c.sync_id != sync_id);
        g.rows.retain(|r| r.calendar_sync_id != sync_id);
        Ok(())
    }
    fn list_events(&self) -> R<Vec<DeviceEvent>> {
        Ok(self.0.lock().unwrap().rows.clone())
    }
    fn get_event(&self, row_id: i64) -> R<Option<DeviceEvent>> {
        Ok(self.0.lock().unwrap().rows.iter().find(|r| r.row_id == Some(row_id)).cloned())
    }
    fn upsert_event(&self, event: DeviceEvent) -> R<DeviceEvent> {
        let mut g = self.0.lock().unwrap();
        g.writes += 1;
        let at = match event.row_id {
            Some(id) => g.rows.iter().position(|r| r.row_id == Some(id)),
            None => event.sync_id.as_ref().and_then(|s| g.rows.iter().position(|r| r.sync_id.as_ref() == Some(s))),
        };
        let mut event = event;
        match at {
            Some(i) => {
                // The sync's own columns are not touched by an upsert.
                event.row_id = g.rows[i].row_id;
                event.shadow = g.rows[i].shadow.clone();
                event.baseline = g.rows[i].baseline.clone();
                g.rows[i] = event.clone();
            }
            None => {
                g.next += 1;
                event.row_id = Some(g.next);
                g.rows.push(event.clone());
            }
        }
        Ok(event)
    }
    fn set_sync_data(
        &self,
        row_id: i64,
        sync_id: Option<String>,
        shadow: Option<String>,
        baseline: Option<String>,
        dirty: bool,
    ) -> R<()> {
        let mut g = self.0.lock().unwrap();
        let row = g.rows.iter_mut().find(|r| r.row_id == Some(row_id)).expect("a row");
        if sync_id.is_some() {
            row.sync_id = sync_id;
        }
        row.shadow = shadow;
        row.baseline = baseline;
        row.dirty = dirty;
        Ok(())
    }
    fn purge_event(&self, row_id: i64) -> R<()> {
        self.0.lock().unwrap().rows.retain(|r| r.row_id != Some(row_id));
        Ok(())
    }
}

struct Rig {
    // Field order is drop order: the core goes first, the mock and its runtime last.
    core: Arc<Core>,
    fake: Arc<Fake>,
    server: MockServer,
    _rt: Runtime,
    _dir: tempfile::TempDir,
}

impl Rig {
    fn new() -> Rig {
        Rig::with(|_| {})
    }

    fn with(setup: impl FnOnce(&MockServer)) -> Rig {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        let server = rt.block_on(MockServer::start());
        setup(&server);
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("cache.db").to_string_lossy().into_owned();
        let core = Core::new(db_path, Arc::new(MemorySecrets::default())).unwrap();
        let url = core.begin_sign_in_with_redirect(server.url(), "ovh.kweb.mail:/oauth/callback".into()).unwrap();
        core.complete_sign_in(server.authorize(&url).unwrap()).unwrap();
        Rig { core, fake: Arc::new(Fake::default()), server, _rt: rt, _dir: dir }
    }

    fn sync(&self) -> DeviceSyncReport {
        let provider: Arc<dyn CalendarProvider> = self.fake.clone();
        self.core.device_calendar_sync(provider, OWNER.into(), ZONE.into()).expect("a sync run")
    }
}

fn lunch() -> Value {
    json!({"@type": "Event", "title": "Lunch", "start": "2026-11-20T12:00:00", "timeZone": ZONE, "duration": "PT1H",
        "locations": {"l1": {"@type": "Location", "name": "Cafe"}}, "keywords": {"food": true}})
}

fn all_day() -> Value {
    json!({"@type": "Event", "title": "Holiday", "start": "2026-11-23T00:00:00", "timeZone": ZONE, "duration": "P2D",
        "showWithoutTime": true})
}

fn weekly() -> Value {
    json!({"@type": "Event", "title": "Standup", "start": "2026-11-16T10:00:00", "timeZone": ZONE, "duration": "PT30M",
        "recurrenceRule": {"@type": "RecurrenceRule", "frequency": "weekly", "byDay": [{"day": "mo"}]},
        "recurrenceOverrides": {
            "2026-11-23T10:00:00": {"excluded": true},
            "2026-11-30T10:00:00": {"title": "Late standup", "start": "2026-11-30T11:00:00"}}})
}

fn sid(id: &str) -> String {
    format!("{OWNER}/{id}")
}

// ---- the mirror ----------------------------------------------------------------------------------

#[test]
fn the_calendar_and_its_events_are_mirrored_and_a_second_run_finds_nothing_to_do() {
    let rig = Rig::new();
    let (lunch_id, holiday_id, series_id) =
        (rig.server.add_event(lunch()), rig.server.add_event(all_day()), rig.server.add_event(weekly()));
    rig.server.add_event(json!({"@type": "Task", "title": "Chore", "due": "2026-11-20T17:00:00", "timeZone": ZONE}));

    let report = rig.sync();
    assert_eq!(report.outcome, DeviceSyncOutcome::Done);
    assert!(report.full);
    let calendars = rig.fake.calendars();
    assert_eq!(calendars.len(), 1);
    assert_eq!(
        (calendars[0].sync_id.as_str(), calendars[0].name.as_str(), calendars[0].access),
        ("app1/cal1", "Personal", 700)
    );
    assert!(!calendars[0].owner.is_empty());

    // The task is not mirrored; the series has its exception row.
    assert_eq!(rig.fake.titles(), ["Holiday", "Late standup", "Lunch", "Standup"]);
    let lunch_row = rig.fake.row(&sid(&lunch_id));
    assert_eq!(
        (lunch_row.location.as_str(), lunch_row.start_ms, lunch_row.end_ms),
        ("Cafe", 1_795_165_200_000, Some(1_795_168_800_000))
    );
    assert!(!lunch_row.dirty && lunch_row.shadow.is_some() && lunch_row.baseline.is_some());
    assert_eq!(lunch_row.calendar_sync_id, "app1/cal1");
    let holiday = rig.fake.row(&sid(&holiday_id));
    assert!(holiday.all_day && holiday.time_zone == "UTC" && holiday.end_ms == Some(holiday.start_ms + 2 * 86_400_000));
    let series = rig.fake.row(&sid(&series_id));
    assert_eq!(series.rrule.as_deref(), Some("FREQ=WEEKLY;BYDAY=MO"));
    assert_eq!(series.exdate.as_deref(), Some("20261123T070000Z"));
    assert!(series.end_ms.is_none() && series.duration.as_deref() == Some("P1800S"));
    let exception = rig.fake.row(&format!("app1/{series_id}#2026-11-30T10:00:00"));
    assert_eq!(exception.original_sync_id.as_deref(), Some(sid(&series_id).as_str()));
    assert!(rig.fake.rows().iter().all(|r| r.uid.is_some() || r.original_sync_id.is_some()));

    let writes = rig.fake.writes();
    let again = rig.sync();
    assert!(!again.full, "the stored state is used");
    assert_eq!((again.written, again.uploaded, again.created, again.destroyed), (0, 0, 0, 0));
    assert_eq!(rig.fake.writes(), writes, "nothing is written when nothing changed");
}

#[test]
fn what_changed_on_the_server_comes_through_the_changes_feed() {
    let rig = Rig::new();
    let lunch_id = rig.server.add_event(lunch());
    let gone = rig.server.add_event(json!({"@type": "Event", "title": "Gone", "start": "2026-11-21T09:00:00", "timeZone": ZONE, "duration": "PT1H"}));
    rig.sync();

    rig.server.patch_event(&lunch_id, json!({"title": "Brunch", "start": "2026-11-20T11:00:00"}));
    rig.server.remove_event(&gone);
    rig.server.add_event(
        json!({"@type": "Event", "title": "New", "start": "2026-11-22T09:00:00", "timeZone": ZONE, "duration": "PT1H"}),
    );

    let report = rig.sync();
    assert!(!report.full);
    assert_eq!(report.written, 2);
    assert_eq!(report.removed_here, 1);
    assert_eq!(rig.fake.titles(), ["Brunch", "New"]);
    assert_eq!(rig.fake.row(&sid(&lunch_id)).start_ms, 1_795_161_600_000);
    assert!(rig.fake.rows().iter().all(|r| !r.dirty));

    // A server that lost its history is read in full.
    rig.server.forget_calendar_history();
    rig.server.add_event(json!({"@type": "Event", "title": "Later", "start": "2026-11-24T09:00:00", "timeZone": ZONE, "duration": "PT1H"}));
    let report = rig.sync();
    assert!(report.full);
    assert_eq!(rig.fake.titles(), ["Brunch", "Later", "New"]);
}

#[test]
fn a_shared_calendar_and_a_calendar_switched_off_never_reach_the_provider() {
    let rig = Rig::with(|server| {
        server.sharing(|s| {
            s.share_calendars(SHARED_ACCOUNT);
            s.calendar(SHARED_ACCOUNT).add_calendar("cal1", "Team calendar", true, true);
            s.calendar(SHARED_ACCOUNT)
                .add(json!({"@type": "Event", "title": "Secret standup", "start": "2026-11-16T10:00:00",
                "timeZone": ZONE, "duration": "PT30M"}));
        });
    });
    rig.server.add_calendar("work", "Work", true, true);
    rig.server.add_event(lunch());
    rig.server.add_event(json!({"@type": "Event", "title": "Report", "start": "2026-11-20T15:00:00", "timeZone": ZONE,
        "duration": "PT1H", "calendarIds": {"work": true}}));

    let names = |rig: &Rig| rig.fake.calendars().into_iter().map(|c| c.name).collect::<Vec<_>>();
    rig.sync();
    assert_eq!(names(&rig), ["Personal", "Work"]);
    assert_eq!(rig.fake.titles(), ["Lunch", "Report"]);
    assert!(!format!("{:?}", rig.fake.rows()).contains("Secret"));

    let choices = rig.core.device_calendars().unwrap();
    assert_eq!(choices.iter().map(|c| (c.id.as_str(), c.on)).collect::<Vec<_>>(), [("cal1", true), ("work", true)]);
    rig.core.device_calendar_set("work".into(), false).unwrap();
    let report = rig.sync();
    assert_eq!(report.outcome, DeviceSyncOutcome::Done);
    assert_eq!(names(&rig), ["Personal"]);
    assert_eq!(rig.fake.titles(), ["Lunch"], "the switched off calendar goes with its events");

    rig.core.device_calendar_set("work".into(), true).unwrap();
    rig.sync();
    assert_eq!(rig.fake.titles(), ["Lunch", "Report"]);
}

// ---- the phone's edits -----------------------------------------------------------------------------

#[test]
fn an_edit_on_the_phone_goes_up_as_a_patch_of_what_changed_only() {
    let rig = Rig::new();
    let id = rig.server.add_event(lunch());
    rig.sync();

    rig.fake.edit(&sid(&id), |r| {
        r.title = "Long lunch".into();
        r.end_ms = Some(r.start_ms + 2 * 3_600_000);
    });
    let report = rig.sync();
    assert_eq!((report.uploaded, report.conflicts), (1, 0));
    let server = rig.server.event(&id).unwrap();
    assert_eq!(server["title"], "Long lunch");
    assert_eq!(server["duration"], "PT2H");
    assert_eq!(server["start"], "2026-11-20T12:00:00");
    // What the phone does not show stays as it was.
    assert_eq!(server["keywords"], json!({"food": true}));
    assert_eq!(server["locations"]["l1"]["name"], "Cafe");
    let row = rig.fake.row(&sid(&id));
    assert!(!row.dirty);
    assert_eq!(row.title, "Long lunch");

    // A row marked dirty with no real change sends nothing.
    rig.fake.edit(&sid(&id), |_| {});
    let report = rig.sync();
    assert_eq!(report.uploaded, 0);
    assert!(!rig.fake.row(&sid(&id)).dirty);
}

#[test]
fn the_same_unit_changed_on_both_sides_goes_to_the_server_and_different_units_both_stay() {
    let rig = Rig::new();
    let id = rig.server.add_event(lunch());
    rig.sync();

    rig.fake.edit(&sid(&id), |r| {
        r.title = "Phone title".into();
        r.location = "Phone place".into();
    });
    rig.server.patch_event(&id, json!({"title": "Server title", "description": "Server notes"}));
    let report = rig.sync();
    assert_eq!(report.conflicts, 1, "the title");
    let server = rig.server.event(&id).unwrap();
    assert_eq!(server["title"], "Server title");
    assert_eq!(server["description"], "Server notes");
    assert_eq!(server["locations"]["l1"]["name"], "Phone place", "the unit only the phone changed went up");
    let row = rig.fake.row(&sid(&id));
    assert_eq!(
        (row.title.as_str(), row.description.as_str(), row.location.as_str()),
        ("Server title", "Server notes", "Phone place")
    );
    assert!(!row.dirty);
}

#[test]
fn a_delete_on_the_server_beats_an_edit_on_the_phone_and_a_delete_on_the_phone_beats_an_edit_on_the_server() {
    let rig = Rig::new();
    let (edited, deleted) = (rig.server.add_event(lunch()), rig.server.add_event(all_day()));
    rig.sync();

    rig.fake.edit(&sid(&edited), |r| r.title = "Edited here".into());
    rig.server.remove_event(&edited);
    rig.fake.delete(&sid(&deleted));
    rig.server.patch_event(&deleted, json!({"title": "Edited there"}));

    let report = rig.sync();
    assert_eq!((report.removed_here, report.destroyed), (1, 1));
    assert!(rig.fake.rows().is_empty(), "both are gone from the phone");
    assert!(rig.server.events().is_empty(), "and the phone's delete reached the server");
}

#[test]
fn an_event_made_on_the_phone_is_made_once_on_the_server() {
    let rig = Rig::new();
    rig.sync();
    rig.fake.add("cal1", "Dentist", 1_795_165_200_000);
    let report = rig.sync();
    assert_eq!(report.created, 1);
    let events = rig.server.events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["title"], "Dentist");
    assert_eq!(events[0]["start"], "2026-11-20T12:00:00");
    assert_eq!(events[0]["timeZone"], ZONE);
    let id = events[0]["id"].as_str().unwrap().to_string();
    let row = rig.fake.row(&sid(&id));
    assert!(!row.dirty && row.uid == events[0]["uid"].as_str().map(str::to_string));

    // Again, and after the server's own echo of it: still one.
    rig.sync();
    rig.sync();
    assert_eq!(rig.server.events().len(), 1);
    assert_eq!(rig.fake.rows().len(), 1);
}

#[test]
fn an_upload_that_got_no_answer_is_adopted_by_its_uid_and_never_made_twice() {
    let rig = Rig::new();
    rig.sync();
    // The event reached the server, but the phone never learned its id: its row still says pending.
    let id = rig
        .server
        .add_event(json!({"@type": "Event", "uid": "u-lost", "title": "Dentist", "start": "2026-11-20T12:00:00",
        "timeZone": ZONE, "duration": "PT1H"}));
    rig.fake.add("cal1", "Dentist", 1_795_165_200_000);
    rig.fake.edit_pending("u-lost");
    let report = rig.sync();
    assert_eq!(report.created, 0);
    assert_eq!(rig.server.events().len(), 1);
    let row = rig.fake.row(&sid(&id));
    assert!(!row.dirty);
    assert_eq!(rig.fake.rows().len(), 1);
}

impl Fake {
    /// Makes the one new row what an interrupted upload leaves: claimed with a pending id and the uid.
    fn edit_pending(&self, uid: &str) {
        let mut g = self.0.lock().unwrap();
        let row = g.rows.iter_mut().find(|r| r.sync_id.is_none()).expect("a new row");
        row.uid = Some(uid.into());
        row.sync_id = Some(format!("~pending/{uid}"));
    }
}

#[test]
fn edits_of_a_series_and_of_a_read_only_calendar_are_put_back() {
    let rig = Rig::new();
    rig.server.add_calendar("shared-ro", "Read only", true, false);
    let series = rig.server.add_event(weekly());
    let fixed = rig.server.add_event(json!({"@type": "Event", "title": "Fixed", "start": "2026-11-20T09:00:00",
        "timeZone": ZONE, "duration": "PT1H", "calendarIds": {"shared-ro": true}}));
    rig.sync();
    assert_eq!(rig.fake.calendars().iter().find(|c| c.sync_id == "app1/shared-ro").unwrap().access, 200);

    rig.fake.edit(&sid(&series), |r| r.title = "Hacked".into());
    rig.fake.edit(&sid(&fixed), |r| r.title = "Hacked too".into());
    let report = rig.sync();
    assert_eq!(report.reverted, 2);
    assert_eq!(rig.fake.row(&sid(&series)).title, "Standup");
    assert_eq!(rig.fake.row(&sid(&fixed)).title, "Fixed");
    assert!(rig.fake.rows().iter().all(|r| !r.dirty));
    assert_eq!(rig.server.event(&series).unwrap()["title"], "Standup");
    assert_eq!(rig.server.event(&fixed).unwrap()["title"], "Fixed");

    // A delete of the series comes back; a new event in the read-only calendar goes.
    rig.fake.delete(&sid(&series));
    rig.fake.add("shared-ro", "Mine", 1_795_165_200_000);
    rig.sync();
    assert!(rig.fake.has(&sid(&series)) && !rig.fake.row(&sid(&series)).deleted);
    assert!(!rig.fake.titles().contains(&"Mine".to_string()));
    assert_eq!(rig.server.events().len(), 2);
}

// ---- the guard -------------------------------------------------------------------------------------

#[test]
fn too_many_deletions_stop_the_run_and_nothing_is_deleted() {
    let rig = Rig::new();
    let ids: Vec<String> = (0..60)
        .map(|n| {
            rig.server.add_event(json!({"@type": "Event", "title": format!("E{n}"), "start": "2026-11-20T09:00:00",
                "timeZone": ZONE, "duration": "PT1H"}))
        })
        .collect();
    rig.sync();
    assert_eq!(rig.fake.rows().len(), 60);

    for id in &ids[..55] {
        rig.server.remove_event(id);
    }
    let report = rig.sync();
    assert_eq!(report.outcome, DeviceSyncOutcome::StoppedTooManyDeletions);
    assert_eq!(report.held_back, 55);
    assert_eq!(rig.fake.rows().len(), 60, "the phone's copy is untouched");

    // The same on the way up: the phone deleting most of its events does not reach the server.
    let rig = Rig::new();
    for n in 0..60 {
        rig.server.add_event(json!({"@type": "Event", "title": format!("E{n}"), "start": "2026-11-20T09:00:00",
            "timeZone": ZONE, "duration": "PT1H"}));
    }
    rig.sync();
    for row in rig.fake.rows().iter().take(55) {
        rig.fake.delete(row.sync_id.as_deref().unwrap());
    }
    let report = rig.sync();
    assert_eq!(report.outcome, DeviceSyncOutcome::StoppedTooManyDeletions);
    assert_eq!(rig.server.events().len(), 60);
}

#[test]
fn a_server_that_answers_with_nothing_is_not_believed() {
    let rig = Rig::new();
    let ids: Vec<String> = (0..12)
        .map(|n| {
            rig.server.add_event(json!({"@type": "Event", "title": format!("E{n}"), "start": "2026-11-20T09:00:00",
                "timeZone": ZONE, "duration": "PT1H"}))
        })
        .collect();
    rig.sync();
    for id in &ids {
        rig.server.remove_event(id);
    }
    rig.server.forget_calendar_history();
    let report = rig.sync();
    assert_eq!(report.outcome, DeviceSyncOutcome::StoppedServerEmpty);
    assert_eq!(rig.fake.rows().len(), 12);
}
