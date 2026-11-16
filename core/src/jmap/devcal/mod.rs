//! The server's own calendars in the phone's calendar provider ("Device calendar").
//!
//! The design (one Android account per app account, a changes feed with stored state, units merged
//! three ways, deletion guard) follows Bulwark Mobile's device sync (github.com/bulwarkmail/native,
//! AGPL-3.0 with an app store permission), as written down in `dev/notes/device-sync-research.md`.
//! Only the design was taken: no code, tests or text of Bulwark's are in this module.
//!
//! The engine ([`engine`]) lives here in the core and talks to the server. The provider is a port
//! the host implements ([`CalendarProvider`], over `CalendarContract` on Android): the core never
//! sees an Android type, and the host never talks to the server.
//!
//! What the mirror holds, and what it leaves alone:
//!
//! - Only the user's own calendars of the account, never a shared account or a shared calendar
//!   (any app with the calendar permission could read what is written to the provider).
//! - Events that do not repeat go both ways. A device edit is uploaded as a JMAP patch of the
//!   units the user changed (title, description, place, timing), made on the freshly read event
//!   and sent with `ifInState`. Same unit changed on both sides: the server wins. Changed on the
//!   device and deleted on the server: the server's delete wins. Deleted on the device and changed
//!   on the server: the device's delete wins.
//! - A repeating series is written read-only with its exceptions (RRULE, EXDATE and one row per
//!   changed occurrence). A device edit of a series is put back from the server's copy.
//! - Reminders are not written: the app keeps its own, so nothing reminds twice.
//! - Tasks are not mirrored.
//! - Every row's `_SYNC_ID` is `<app account id>/<event id>` (an exception adds `#<recurrence id>`;
//!   an event made on the device and not yet uploaded is `~pending/<uid>`), so ids that repeat
//!   between accounts never meet.
//! - Titles, descriptions and places are never written to a log or an error.

mod engine;
mod map;
mod net;

pub use engine::{run, Options};

use std::collections::BTreeSet;

/// The calendars the user switched off for the mirror, and nothing else: a calendar that is new
/// on the server is on until the user says otherwise.
pub(crate) const OFF_KEY: &str = "devcal_off";

pub(crate) fn parse_off(text: Option<String>) -> BTreeSet<String> {
    text.and_then(|t| serde_json::from_str::<Vec<String>>(&t).ok()).unwrap_or_default().into_iter().collect()
}

/// A calendar as the provider keeps it. `sync_id` is `<app account id>/<calendar id>`.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct DeviceCalendar {
    pub sync_id: String,
    pub name: String,
    /// ARGB.
    pub color: i32,
    /// `CalendarContract` access level: 700 owner, 500 contributor, 200 read.
    pub access: i32,
    /// Never empty (some calendar apps crash on a calendar with no owner).
    pub owner: String,
    pub time_zone: String,
}

/// One row of the provider's events table, with what the sync keeps beside it.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct DeviceEvent {
    /// The provider's own row id; `None` for a row that is to be made.
    pub row_id: Option<i64>,
    pub sync_id: Option<String>,
    pub calendar_sync_id: String,
    pub uid: Option<String>,
    pub title: String,
    pub description: String,
    pub location: String,
    pub start_ms: i64,
    pub end_ms: Option<i64>,
    /// An RFC 5545 duration, for a row with a repeat rule (which has no end).
    pub duration: Option<String>,
    pub all_day: bool,
    pub time_zone: String,
    pub rrule: Option<String>,
    pub exdate: Option<String>,
    /// `CalendarContract` status: 0 tentative, 1 confirmed, 2 canceled.
    pub status: i32,
    /// 0 busy, 1 free.
    pub availability: i32,
    pub access_level: i32,
    pub original_sync_id: Option<String>,
    pub original_instance_ms: Option<i64>,
    pub original_all_day: bool,
    /// Changed on the device since the last sync.
    pub dirty: bool,
    /// Deleted on the device, waiting for the sync to say so to the server.
    pub deleted: bool,
    /// The server's copy as the sync last saw it.
    pub shadow: Option<String>,
    /// What the row held after the sync's last write, to tell the user's edits from the provider's own rewriting.
    pub baseline: Option<String>,
}

#[derive(Debug, Clone, PartialEq, thiserror::Error, uniffi::Error)]
pub enum ProviderError {
    #[error("{msg}")]
    Failed { msg: String },
    /// The calendar permission is not (or no longer) granted.
    #[error("the calendar permission is missing")]
    PermissionDenied,
}

impl From<uniffi::UnexpectedUniFFICallbackError> for ProviderError {
    fn from(e: uniffi::UnexpectedUniFFICallbackError) -> Self {
        ProviderError::Failed { msg: e.reason }
    }
}

/// The port to the phone's calendar provider. Implemented by the host; every call blocks until done
/// and must not call back into the core.
#[uniffi::export(with_foreign)]
pub trait CalendarProvider: Send + Sync {
    /// The sync's own state (an opaque string), kept where it disappears with the Android account.
    fn read_state(&self) -> Result<Option<String>, ProviderError>;
    fn write_state(&self, state: String) -> Result<(), ProviderError>;
    fn list_calendars(&self) -> Result<Vec<DeviceCalendar>, ProviderError>;
    /// Makes or updates the calendar with this `sync_id`.
    fn upsert_calendar(&self, calendar: DeviceCalendar) -> Result<(), ProviderError>;
    /// Removes the calendar and, with it, its events.
    fn delete_calendar(&self, sync_id: String) -> Result<(), ProviderError>;
    /// Every row of the account's events, deleted ones included.
    fn list_events(&self) -> Result<Vec<DeviceEvent>, ProviderError>;
    fn get_event(&self, row_id: i64) -> Result<Option<DeviceEvent>, ProviderError>;
    /// Writes the row (found by `row_id`, else by `sync_id`, else made) as the caller's sync, which does not
    /// make it dirty unless `event.dirty` says so; `deleted` false takes a deletion mark back. Returns the row as
    /// the provider now holds it (it rewrites some values).
    fn upsert_event(&self, event: DeviceEvent) -> Result<DeviceEvent, ProviderError>;
    /// Sets the sync's own columns of a row and its dirty flag, and `sync_id` when given.
    fn set_sync_data(
        &self,
        row_id: i64,
        sync_id: Option<String>,
        shadow: Option<String>,
        baseline: Option<String>,
        dirty: bool,
    ) -> Result<(), ProviderError>;
    /// Removes the row for good.
    fn purge_event(&self, row_id: i64) -> Result<(), ProviderError>;
}

/// How a run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum DeviceSyncOutcome {
    Done,
    /// More than 50 events, and more than a fifth of them, were to be deleted: nothing of that was done.
    StoppedTooManyDeletions,
    /// The server answered with no events at all while the phone holds more than ten: nothing was deleted.
    StoppedServerEmpty,
}

/// What one run did. Counts only; nothing of an event.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct DeviceSyncReport {
    pub outcome: DeviceSyncOutcome,
    pub full: bool,
    pub calendars: u32,
    pub written: u32,
    pub removed_here: u32,
    pub uploaded: u32,
    pub created: u32,
    pub destroyed: u32,
    pub conflicts: u32,
    /// Device edits of a series (or in a read-only calendar) that were put back.
    pub reverted: u32,
    /// How many deletions the guard held back.
    pub held_back: u32,
    /// Events made on the phone that the server cannot take in this version (a series).
    pub not_sent: u32,
}

/// An own calendar and whether the mirror shows it.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct DeviceCalendarChoice {
    pub id: String,
    pub name: String,
    pub color: Option<String>,
    pub on: bool,
}
