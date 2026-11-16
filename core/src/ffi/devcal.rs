//! The calendars on the phone's own calendar provider ("Device calendar"): the sync and the
//! choice of which of the user's calendars it mirrors. The engine is `jmap::devcal`.

use std::sync::Arc;

use super::Core;
use crate::{
    jmap::devcal::{self, CalendarProvider, DeviceCalendarChoice, DeviceSyncReport, Options},
    Result,
};

#[uniffi::export]
impl Core {
    /// The user's own calendars and whether the mirror shows each. A new calendar is on until the user
    /// switches it off. Shared calendars are never in the list.
    pub fn device_calendars(&self) -> Result<Vec<DeviceCalendarChoice>> {
        let off = devcal::parse_off(self.store.kv_get(&self.account_id()?, devcal::OFF_KEY)?);
        Ok(self
            .calendars()?
            .into_iter()
            .map(|c| DeviceCalendarChoice { on: !off.contains(&c.id), id: c.id, name: c.name, color: c.color })
            .collect())
    }

    /// Switches one calendar on or off for the mirror. The next sync follows it.
    pub fn device_calendar_set(&self, calendar_id: String, on: bool) -> Result<()> {
        let account_id = self.account_id()?;
        let mut off = devcal::parse_off(self.store.kv_get(&account_id, devcal::OFF_KEY)?);
        if on {
            off.remove(&calendar_id);
        } else {
            off.insert(calendar_id);
        }
        let list: Vec<&String> = off.iter().collect();
        self.store.kv_set(&account_id, devcal::OFF_KEY, &serde_json::to_string(&list).unwrap_or_default())
    }

    /// One run of the sync for the phone's calendar provider: what changed on the server comes to the phone, what
    /// the user changed there goes up. `owner` is the app account id (the prefix of every sync id), `device_zone`
    /// the phone's time zone. Blocks until the run is done; the host calls it from its sync adapter. What was done,
    /// or why it stopped, is in the report; the server being out of reach is an error.
    pub fn device_calendar_sync(
        &self,
        provider: Arc<dyn CalendarProvider>,
        owner: String,
        device_zone: String,
    ) -> Result<DeviceSyncReport> {
        let off = devcal::parse_off(self.store.kv_get(&self.account_id()?, devcal::OFF_KEY)?);
        self.with_connection(|conn| {
            let (provider, owner, off, zone) = (provider.clone(), owner.clone(), off.clone(), device_zone.clone());
            async move {
                devcal::run(&conn, &*provider, &Options { owner: &owner, off: &off, device_zone: &zone }).await
            }
        })
    }
}
