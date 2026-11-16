//! One run of the sync: calendars, then what changed on the server, then what changed on the
//! phone, then the clean-up. The server's copy of an event (the "shadow") and what the provider
//! held after our last write (the "baseline") are kept in the row's own sync columns, so both go
//! with the Android account.
//!
//! Merging is done per unit (title, description, place, timing). The device's change to a unit is
//! what differs from the baseline; the server's is what differs between the old and the new
//! shadow, both read the same way, so nothing the provider rewrites on its own counts as an edit.

use std::{
    cell::Cell,
    collections::{BTreeMap, BTreeSet},
};

use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{
    map::{self, Mapped, Proj, Unit, PENDING, UNITS},
    net::{Page, ServerCalendar},
    CalendarProvider, DeviceCalendar, DeviceEvent, DeviceSyncOutcome, DeviceSyncReport, ProviderError,
};
use crate::{
    jmap::{clock::zone, jscal, Connection},
    Error, Result,
};

/// More deletions than this, and more than [`GUARD_SHARE`] of the events, are not done.
const GUARD_COUNT: usize = 50;
const GUARD_SHARE: usize = 5;
/// A server that answers with no events while the phone holds more than this many is not believed.
const EMPTY_GUARD: usize = 10;
const PAGE: usize = 256;

pub struct Options<'a> {
    /// The app account id: the prefix of every sync id.
    pub owner: &'a str,
    /// Calendar ids the user switched off.
    pub off: &'a BTreeSet<String>,
    /// The phone's time zone (a name), for floating times.
    pub device_zone: &'a str,
}

#[derive(Serialize, Deserialize, Default)]
struct State {
    owner: String,
    events: Option<String>,
    calendars: Option<String>,
    zone: String,
    /// The calendars that were mirrored: when they change (one switched on, a new one) the events are read in full.
    #[serde(default)]
    wanted: Vec<String>,
}

#[derive(Default, Clone)]
struct Group {
    master: Option<DeviceEvent>,
    exceptions: Vec<DeviceEvent>,
}

impl Group {
    fn rows(&self) -> impl Iterator<Item = &DeviceEvent> {
        self.master.iter().chain(self.exceptions.iter())
    }

    /// Something was changed on the phone: the event itself or a row of its occurrences.
    fn touched(&self) -> bool {
        self.rows().any(|r| r.dirty || r.deleted || r.sync_id.is_none())
    }
}

struct Rows {
    groups: BTreeMap<String, Group>,
    pending: Vec<DeviceEvent>,
    fresh: Vec<DeviceEvent>,
}

fn p<T>(r: std::result::Result<T, ProviderError>) -> Result<T> {
    r.map_err(|e| match e {
        ProviderError::PermissionDenied => Error::invalid("the calendar permission is missing"),
        ProviderError::Failed { msg } => Error::temporary(format!("the phone's calendar: {msg}")),
    })
}

enum Take {
    /// Neither side changed it, or both ended up the same.
    Server,
    /// Only the phone changed it: it stays and goes up.
    Device,
    /// Both changed it differently: the server's.
    Conflict,
}

/// Who owns a unit. `old` is the server's earlier copy read as the new one is; `base` what the
/// provider held after our last write.
fn decide(unit: Unit, dev: &Proj, base: &Proj, old: Option<&Proj>, new: &Proj) -> Take {
    if dev.same(base, unit) {
        return Take::Server;
    }
    let server_changed = old.is_none_or(|o| !new.same(o, unit));
    match (server_changed, dev.same(new, unit)) {
        (false, _) => Take::Device,
        (true, true) => Take::Server,
        (true, false) => Take::Conflict,
    }
}

struct Run<'a> {
    conn: &'a Connection,
    provider: &'a dyn CalendarProvider,
    owner: &'a str,
    device: Tz,
    cals: Vec<ServerCalendar>,
    wanted: BTreeSet<String>,
    report: DeviceSyncReport,
}

pub async fn run(conn: &Connection, provider: &dyn CalendarProvider, opts: &Options<'_>) -> Result<DeviceSyncReport> {
    let device = zone(opts.device_zone).unwrap_or(Tz::UTC);
    let (cals, cal_state) = conn.devcal_calendars().await?;
    let wanted: BTreeSet<String> =
        cals.iter().filter(|c| c.readable && !opts.off.contains(&c.id)).map(|c| c.id.clone()).collect();
    let report = DeviceSyncReport {
        outcome: DeviceSyncOutcome::Done,
        full: false,
        calendars: u32::try_from(wanted.len()).unwrap_or(u32::MAX),
        written: 0,
        removed_here: 0,
        uploaded: 0,
        created: 0,
        destroyed: 0,
        conflicts: 0,
        reverted: 0,
        held_back: 0,
        not_sent: 0,
    };
    let mut me = Run { conn, provider, owner: opts.owner, device, cals, wanted, report };
    me.mirror_calendars(&conn.devcal_login())?;

    let stored: Option<State> = p(provider.read_state())?.and_then(|s| serde_json::from_str(&s).ok());
    let kept = stored.filter(|s| s.owner == opts.owner && s.events.is_some());
    let force = kept.as_ref().is_some_and(|s| s.zone != opts.device_zone);
    let rows = me.rows()?;

    let wanted_now: Vec<String> = me.wanted.iter().cloned().collect();
    let loaded = me.load(kept.as_ref().filter(|s| !force && s.wanted == wanted_now)).await?;
    me.report.full = loaded.full;

    // What is to go from the phone, and what is to be written.
    let mut writes: Vec<(&Value, String)> = Vec::new();
    let mut present = BTreeSet::new();
    let mut elsewhere = BTreeSet::new();
    for event in &loaded.events {
        let Some(id) = map::event_id(event) else { continue };
        match me.target(event) {
            Some(cal) if !jscal::is_task(event) => {
                present.insert(id.to_string());
                writes.push((event, cal));
            }
            _ => {
                elsewhere.insert(id.to_string());
            }
        }
    }
    let candidates: BTreeSet<String> = if loaded.full {
        rows.groups.keys().filter(|id| !present.contains(*id)).cloned().collect()
    } else {
        loaded.destroyed.iter().chain(elsewhere.iter()).filter(|id| rows.groups.contains_key(*id)).cloned().collect()
    };
    let (guarded, exempt): (Vec<&String>, Vec<&String>) =
        candidates.iter().partition(|id| me.in_wanted(&rows.groups[*id]));
    let in_wanted = rows.groups.values().filter(|g| me.in_wanted(g)).count();
    if loaded.full && present.is_empty() && in_wanted > EMPTY_GUARD {
        me.report.outcome = DeviceSyncOutcome::StoppedServerEmpty;
        me.report.held_back = u32::try_from(in_wanted).unwrap_or(u32::MAX);
        return Ok(me.report);
    }
    if me.too_many(guarded.len(), rows.groups.len()) {
        me.report.outcome = DeviceSyncOutcome::StoppedTooManyDeletions;
        me.report.held_back = u32::try_from(guarded.len()).unwrap_or(u32::MAX);
        return Ok(me.report);
    }
    for id in guarded.into_iter().chain(exempt) {
        for row in rows.groups[id].rows() {
            if let Some(row_id) = row.row_id {
                p(provider.purge_event(row_id))?;
            }
        }
        me.report.removed_here += 1;
    }
    let mut by_uid: BTreeMap<String, DeviceEvent> = BTreeMap::new();
    for row in &rows.pending {
        if let Some(uid) = &row.uid {
            by_uid.insert(uid.clone(), row.clone());
        }
    }
    for (event, cal) in writes {
        let id = map::event_id(event).unwrap_or_default();
        let group = rows.groups.get(id);
        let adopt = if group.is_none() { event["uid"].as_str().and_then(|u| by_uid.remove(u)) } else { None };
        let adopted =
            adopt.map(|row| Group { master: Some(DeviceEvent { dirty: false, ..row }), exceptions: Vec::new() });
        me.write_event(event, &cal, group.or(adopted.as_ref()), force)?;
    }

    me.upload().await?;
    if me.report.outcome != DeviceSyncOutcome::Done {
        return Ok(me.report);
    }
    me.drop_unwanted_calendars()?;
    let state = State {
        owner: opts.owner.to_string(),
        events: loaded.new_state,
        calendars: cal_state,
        zone: opts.device_zone.to_string(),
        wanted: wanted_now,
    };
    p(provider.write_state(serde_json::to_string(&state).unwrap_or_default()))?;
    Ok(me.report)
}

struct Loaded {
    events: Vec<Value>,
    destroyed: BTreeSet<String>,
    new_state: Option<String>,
    full: bool,
}

impl Run<'_> {
    fn cal_sync(&self, id: &str) -> String {
        format!("{}/{id}", self.owner)
    }

    fn cal_id<'s>(&self, sync: &'s str) -> Option<&'s str> {
        sync.strip_prefix(self.owner)?.strip_prefix('/')
    }

    fn writable(&self, cal_sync: &str) -> bool {
        let id = self.cal_id(cal_sync);
        self.cals.iter().any(|c| Some(c.id.as_str()) == id && (c.write_all || c.write_own))
    }

    fn in_wanted(&self, group: &Group) -> bool {
        group.rows().next().and_then(|r| self.cal_id(&r.calendar_sync_id)).is_some_and(|id| self.wanted.contains(id))
    }

    fn too_many(&self, deletions: usize, total: usize) -> bool {
        deletions > GUARD_COUNT && deletions * GUARD_SHARE > total
    }

    /// The first calendar of the event that is mirrored, as the sync id.
    fn target(&self, event: &Value) -> Option<String> {
        jscal::calendar_ids(event).into_iter().find(|c| self.wanted.contains(*c)).map(|c| self.cal_sync(c))
    }

    fn mirror_calendars(&self, login: &str) -> Result<()> {
        let have = p(self.provider.list_calendars())?;
        for (n, c) in self.cals.iter().filter(|c| self.wanted.contains(&c.id)).enumerate() {
            let want = DeviceCalendar {
                sync_id: self.cal_sync(&c.id),
                name: c.name.clone(),
                color: color_of(c.color.as_deref(), n),
                access: if c.write_all {
                    700
                } else if c.write_own {
                    500
                } else {
                    200
                },
                owner: login.to_string(),
                time_zone: c.time_zone.clone().unwrap_or_else(|| self.device.name().to_string()),
            };
            if !have.contains(&want) {
                p(self.provider.upsert_calendar(want))?;
            }
        }
        Ok(())
    }

    fn drop_unwanted_calendars(&self) -> Result<()> {
        for c in p(self.provider.list_calendars())? {
            if self.cal_id(&c.sync_id).is_some_and(|id| !self.wanted.contains(id)) {
                p(self.provider.delete_calendar(c.sync_id))?;
            }
        }
        Ok(())
    }

    fn rows(&self) -> Result<Rows> {
        let mut out = Rows { groups: BTreeMap::new(), pending: Vec::new(), fresh: Vec::new() };
        for row in p(self.provider.list_events())? {
            match row.sync_id.as_deref() {
                Some(sync) if sync.starts_with(PENDING) => out.pending.push(row),
                Some(sync) => {
                    let Some((id, key)) = map::parse_sync_id(self.owner, sync) else { continue };
                    let group = out.groups.entry(id.to_string()).or_default();
                    if key.is_some() {
                        group.exceptions.push(row);
                    } else {
                        group.master = Some(row);
                    }
                }
                None => match row.original_sync_id.as_deref().and_then(|o| map::parse_sync_id(self.owner, o)) {
                    Some((id, _)) => out.groups.entry(id.to_string()).or_default().exceptions.push(row),
                    None => out.fresh.push(row),
                },
            }
        }
        Ok(out)
    }

    async fn load(&self, state: Option<&State>) -> Result<Loaded> {
        if let Some(mut since) = state.and_then(|s| s.events.clone()) {
            let (mut ids, mut destroyed) = (BTreeSet::new(), BTreeSet::new());
            loop {
                match self.conn.devcal_changes(&since, PAGE).await? {
                    Page::Reset => return self.load_all().await,
                    Page::Changes { created, updated, destroyed: gone, new_state, more } => {
                        ids.extend(created.into_iter().chain(updated));
                        destroyed.extend(gone);
                        since = new_state;
                        if !more {
                            break;
                        }
                    }
                }
            }
            ids.retain(|id| !destroyed.contains(id));
            let ids: Vec<String> = ids.into_iter().collect();
            let (events, missing) = self.conn.devcal_events(&ids).await?;
            destroyed.extend(missing);
            return Ok(Loaded { events, destroyed, new_state: Some(since), full: false });
        }
        self.load_all().await
    }

    /// Everything, from a state read before the events so that a change in between is read again next time.
    async fn load_all(&self) -> Result<Loaded> {
        let new_state = self.conn.devcal_events_state().await?;
        let events = self.conn.devcal_all(&self.wanted).await?;
        Ok(Loaded { events, destroyed: BTreeSet::new(), new_state, full: true })
    }

    /// Writes one event of the server to the phone: the rows, clean, with the server's copy as the shadow. What the phone
    /// changed meanwhile is merged first, or put back for what cannot be sent.
    fn write_event(&mut self, event: &Value, cal_sync: &str, group: Option<&Group>, force: bool) -> Result<()> {
        let Some(mapped) = map::map_event(event, self.owner, cal_sync, self.device) else { return Ok(()) };
        let shadow = event.to_string();
        let read_only = mapped.read_only || !self.writable(cal_sync);
        let mut master = mapped.rows[0].clone();
        let (mut kept, mut base) = (Vec::new(), None);
        let existing = group.and_then(|g| g.master.clone());
        if let (Some(row), Some(g)) = (&existing, group) {
            if g.touched() {
                if read_only {
                    self.report.reverted += 1;
                } else if row.deleted {
                    // The phone's delete wins: the upload says so to the server.
                    return Ok(());
                } else if row.dirty {
                    base = row.baseline.as_deref().and_then(Proj::parse);
                    let new = Proj::of(&master);
                    let old = row.shadow.as_deref().and_then(|s| serde_json::from_str::<Value>(s).ok()).and_then(|o| {
                        map::map_event(&o, self.owner, &row.calendar_sync_id, self.device).map(|m| Proj::of(&m.rows[0]))
                    });
                    let (dev, base_proj) = (Proj::of(row), base.clone().unwrap_or_else(|| new.clone()));
                    for unit in UNITS {
                        match decide(unit, &dev, &base_proj, old.as_ref(), &new) {
                            Take::Device => kept.push(unit),
                            Take::Conflict => self.report.conflicts += 1,
                            Take::Server => {}
                        }
                    }
                    for unit in &kept {
                        map::copy_unit(&mut master, row, *unit);
                    }
                }
            } else if !force
                && row.shadow.as_deref() == Some(shadow.as_str())
                && row.calendar_sync_id == cal_sync
                && g.exceptions.len() == mapped.rows.len() - 1
            {
                return Ok(());
            }
        }
        master.row_id = existing.as_ref().and_then(|r| r.row_id);
        master.dirty = !kept.is_empty();
        let stored = p(self.provider.upsert_event(master))?;
        let Some(row_id) = stored.row_id else { return Err(Error::temporary("the phone's calendar gave no row")) };
        let mut baseline = Proj::of(&stored);
        if let Some(base) = &base {
            for unit in &kept {
                baseline.replace(base, *unit);
            }
        }
        p(self.provider.set_sync_data(row_id, None, Some(shadow), Some(baseline.text()), !kept.is_empty()))?;
        self.write_exceptions(&mapped, group)?;
        self.report.written += 1;
        Ok(())
    }

    fn write_exceptions(&self, mapped: &Mapped, group: Option<&Group>) -> Result<()> {
        let old: Vec<&DeviceEvent> = group.map(|g| g.exceptions.iter().collect()).unwrap_or_default();
        let mut keep = BTreeSet::new();
        for row in &mapped.rows[1..] {
            let found = old.iter().find(|o| o.sync_id == row.sync_id && o.sync_id.is_some());
            let mut row = row.clone();
            row.row_id = found.and_then(|o| o.row_id);
            if let Some(id) = row.row_id {
                keep.insert(id);
            }
            let stored = p(self.provider.upsert_event(row))?;
            if let Some(id) = stored.row_id {
                keep.insert(id);
                p(self.provider.set_sync_data(id, None, None, None, false))?;
            }
        }
        for row in old {
            if let Some(id) = row.row_id.filter(|id| !keep.contains(id)) {
                p(self.provider.purge_event(id))?;
            }
        }
        Ok(())
    }

    /// What the phone changed: deletions, edits and new events go to the server; edits the server cannot take are put back.
    async fn upload(&mut self) -> Result<()> {
        let rows = self.rows()?;
        // Rows with no sync id that were deleted before they were ever sent: nothing to tell the server.
        for row in rows.fresh.iter().chain(rows.pending.iter()).filter(|r| r.deleted) {
            if let Some(id) = row.row_id {
                p(self.provider.purge_event(id))?;
            }
        }
        let mut destroys = Vec::new();
        let mut edits = Vec::new();
        let mut reverts = Vec::new();
        for (id, group) in &rows.groups {
            if !group.touched() {
                continue;
            }
            let Some(master) = &group.master else {
                // Rows of occurrences whose event is not here: nothing to put them back from.
                for row in group.rows() {
                    if let Some(row_id) = row.row_id {
                        p(self.provider.purge_event(row_id))?;
                    }
                }
                continue;
            };
            let sheet: Option<Value> = master.shadow.as_deref().and_then(|s| serde_json::from_str(s).ok());
            let read_only = !self.writable(&master.calendar_sync_id)
                || master.rrule.is_some()
                || !group.exceptions.is_empty()
                || sheet
                    .as_ref()
                    .and_then(|s| map::map_event(s, self.owner, &master.calendar_sync_id, self.device))
                    .is_none_or(|m| m.read_only);
            if read_only {
                reverts.push((id.clone(), group.clone(), sheet));
            } else if master.deleted {
                destroys.push((id.clone(), group.clone()));
            } else if master.dirty {
                edits.push((id.clone(), group.clone()));
            }
        }
        for (_, group, sheet) in reverts {
            self.revert(&group, sheet.as_ref())?;
        }
        if self.too_many(destroys.len(), rows.groups.len()) {
            self.report.outcome = DeviceSyncOutcome::StoppedTooManyDeletions;
            self.report.held_back = u32::try_from(destroys.len()).unwrap_or(u32::MAX);
        } else {
            for (id, group) in destroys {
                self.conn.devcal_destroy(&id).await?;
                for row in group.rows() {
                    if let Some(row_id) = row.row_id {
                        p(self.provider.purge_event(row_id))?;
                    }
                }
                self.report.destroyed += 1;
            }
        }
        for (id, group) in edits {
            self.update(&id, group).await?;
        }
        for row in rows.pending.iter().chain(rows.fresh.iter()).filter(|r| !r.deleted) {
            self.create(row).await?;
        }
        Ok(())
    }

    /// Puts an event back as the server has it (the phone may not change it): its deleted rows return and
    /// the occurrences the phone made go.
    fn revert(&mut self, group: &Group, sheet: Option<&Value>) -> Result<()> {
        let Some(master) = &group.master else { return Ok(()) };
        let calendar = self.cal_id(&master.calendar_sync_id).map(str::to_string);
        let present = calendar.as_ref().is_some_and(|c| self.cals.iter().any(|k| k.id == *c));
        match (sheet, present) {
            (Some(sheet), true) => {
                let clean = Group {
                    master: Some(DeviceEvent { dirty: false, deleted: false, ..master.clone() }),
                    exceptions: group.exceptions.clone(),
                };
                let cal = master.calendar_sync_id.clone();
                self.report.reverted += 1;
                let before = self.report.reverted;
                self.write_event(sheet, &cal, Some(&clean), true)?;
                self.report.reverted = before;
            }
            _ => {
                for row in group.rows() {
                    if let Some(id) = row.row_id {
                        p(self.provider.purge_event(id))?;
                    }
                }
            }
        }
        Ok(())
    }

    /// Sends the changes of one event the phone edited, as a patch of the units that changed, and takes the
    /// server's copy back as the row.
    async fn update(&mut self, id: &str, group: Group) -> Result<()> {
        let Some(row) = group.master.clone() else { return Ok(()) };
        let (owner, device) = (self.owner, self.device);
        let cal_sync = row.calendar_sync_id.clone();
        let dev = Proj::of(&row);
        let sheet: Option<Value> = row.shadow.as_deref().and_then(|s| serde_json::from_str(s).ok());
        let old = sheet.and_then(|s| map::map_event(&s, owner, &cal_sync, device)).map(|m| Proj::of(&m.rows[0]));
        let base = row.baseline.as_deref().and_then(Proj::parse);
        let (lost, sent) = (Cell::new(0u32), Cell::new(false));
        let found = self
            .conn
            .devcal_update(id, |fresh| {
                let current = jscal::current_input(fresh)
                    .ok_or_else(|| Error::temporary("the server's answer is not an event"))?;
                let Some(new) = map::map_event(fresh, owner, &cal_sync, device).map(|m| Proj::of(&m.rows[0])) else {
                    return Ok(serde_json::json!({}));
                };
                let base = base.clone().unwrap_or_else(|| new.clone());
                let (mut input, mut changed, mut lost_now) = (current.clone(), false, 0);
                for unit in UNITS {
                    match decide(unit, &dev, &base, old.as_ref(), &new) {
                        Take::Device => {
                            changed = true;
                            match unit {
                                Unit::Title => input.title = row.title.trim().to_string(),
                                Unit::Description => input.description = row.description.clone(),
                                Unit::Location => input.location = row.location.trim().to_string(),
                                Unit::Timing => {
                                    if let Some((start, length, zone_name)) =
                                        map::place_of(&row, Some(&current), device)
                                    {
                                        input.start = start;
                                        input.duration_secs = length;
                                        input.time_zone = zone_name;
                                        input.all_day = row.all_day;
                                    }
                                }
                            }
                        }
                        Take::Conflict => lost_now += 1,
                        Take::Server => {}
                    }
                }
                lost.set(lost_now);
                let patch = if changed { jscal::update_patch(fresh, &input)? } else { serde_json::json!({}) };
                sent.set(patch.as_object().is_some_and(|f| !f.is_empty()));
                Ok(patch)
            })
            .await?;
        if !found {
            // Deleted on the server while the phone edited it: the server's delete wins.
            for r in group.rows() {
                if let Some(row_id) = r.row_id {
                    p(self.provider.purge_event(row_id))?;
                }
            }
            self.report.removed_here += 1;
            return Ok(());
        }
        self.report.conflicts += lost.get();
        if sent.get() {
            self.report.uploaded += 1;
        }
        self.settle(id, &row, &cal_sync).await
    }

    /// Reads the event back after an upload and makes it the row, clean, unless the phone changed the row meanwhile:
    /// then the row stays dirty and the next run sends that.
    async fn settle(&mut self, id: &str, sent: &DeviceEvent, cal_sync: &str) -> Result<()> {
        let Some(event) = self.conn.devcal_event(id).await? else { return Ok(()) };
        let Some(row_id) = sent.row_id else { return Ok(()) };
        let now = p(self.provider.get_event(row_id))?;
        let unchanged = now.as_ref().is_some_and(|n| Proj::of(n) == Proj::of(sent) && !n.deleted);
        let clean = DeviceEvent { dirty: false, deleted: false, ..sent.clone() };
        if unchanged {
            return self.write_event(
                &event,
                cal_sync,
                Some(&Group { master: Some(clean), exceptions: Vec::new() }),
                true,
            );
        }
        if now.is_some() {
            let sync = map::sync_id(self.owner, id);
            p(self.provider.set_sync_data(
                row_id,
                Some(sync),
                Some(event.to_string()),
                Some(Proj::of(sent).text()),
                true,
            ))?;
        }
        Ok(())
    }

    /// An event made on the phone goes to the server. It is claimed first with a pending id and its uid, so that
    /// an upload that gets no answer is found again by that uid and never made twice.
    async fn create(&mut self, row: &DeviceEvent) -> Result<()> {
        let Some(calendar) = self.cal_id(&row.calendar_sync_id).map(str::to_string) else { return Ok(()) };
        if !self.writable(&row.calendar_sync_id) {
            // A read-only calendar takes no new events: the row goes.
            if let Some(id) = row.row_id {
                p(self.provider.purge_event(id))?;
            }
            self.report.reverted += 1;
            return Ok(());
        }
        let Some(input) = map::input_of_new(row, &calendar, self.device) else {
            self.report.not_sent += 1;
            return Ok(());
        };
        let uid = match row.uid.clone().filter(|u| !u.is_empty()) {
            Some(uid) => uid,
            None => crate::auth::random_url_safe(16)?,
        };
        let mut claimed = row.clone();
        claimed.uid = Some(uid.clone());
        claimed.sync_id = Some(format!("{PENDING}{uid}"));
        claimed.dirty = true;
        let claimed = p(self.provider.upsert_event(claimed))?;
        let mut object = jscal::create_object(&input);
        object["uid"] = serde_json::json!(uid);
        let id = self.conn.devcal_create(object).await?;
        self.report.created += 1;
        let cal_sync = row.calendar_sync_id.clone();
        self.settle(&id, &claimed, &cal_sync).await
    }
}

/// The calendar's colour as ARGB: its CSS colour, else one of a few, by position.
fn color_of(css: Option<&str>, n: usize) -> i32 {
    const PALETTE: [u32; 6] = [0xFF3A7BD5, 0xFFD5503A, 0xFF3AA66B, 0xFF9B59B6, 0xFFE6A23C, 0xFF16A0A0];
    let parsed = css.and_then(|c| c.strip_prefix('#')).and_then(|hex| match hex.len() {
        6 => u32::from_str_radix(hex, 16).ok(),
        3 => u32::from_str_radix(hex, 16).ok().map(|v| {
            let (r, g, b) = ((v >> 8) & 0xF, (v >> 4) & 0xF, v & 0xF);
            ((r * 17) << 16) | ((g * 17) << 8) | (b * 17)
        }),
        _ => None,
    });
    (0xFF00_0000 | parsed.unwrap_or(PALETTE[n % PALETTE.len()] & 0x00FF_FFFF)) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_come_from_css_or_the_palette() {
        assert_eq!(color_of(Some("#3a7bd5"), 0) as u32, 0xFF3A7BD5);
        assert_eq!(color_of(Some("#fff"), 0) as u32, 0xFFFFFFFF);
        assert_eq!(color_of(Some("teal"), 1) as u32, 0xFFD5503A);
        assert_eq!(color_of(None, 6) as u32, 0xFF3A7BD5);
    }

    #[test]
    fn the_guard_needs_both_many_and_a_share() {
        let run = |n, total| n > GUARD_COUNT && n * GUARD_SHARE > total;
        assert!(run(51, 200));
        assert!(!run(51, 400));
        assert!(!run(50, 60));
    }
}
