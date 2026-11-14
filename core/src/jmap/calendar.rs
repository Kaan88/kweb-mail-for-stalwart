//! Calendar and task details for a reminder.
//!
//! Stalwart fires the alarm itself and pushes a `CalendarAlert` that only names the event.
//! The title, time and place come from one `CalendarEvent/get`. jmap-client has no calendar
//! methods, so this is a plain JMAP request over the same URL and headers.
//!
//! Nothing here converts time zones or formats dates: the host knows the user's zone and
//! locale. The reminder carries the occurrence's local start and its zone as the server has
//! them.

use std::collections::BTreeMap;

use serde_json::{json, Value};

use super::{errors::unauthorized, Connection};
use crate::{model::Reminder, Error, Result};

const CORE_CAPABILITY: &str = "urn:ietf:params:jmap:core";
const CALENDARS_CAPABILITY: &str = "urn:ietf:params:jmap:calendars";

impl Connection {
    /// The details of the event or task an alert fired for. `recurrence_id` is the alert's own:
    /// for a recurring event it says which occurrence the alarm is about.
    ///
    /// An event the server no longer has is `Error::Invalid`; a 401 stays recognisable with
    /// `is_unauthorized`; everything else is `Error::Temporary`.
    pub async fn reminder(&self, event_id: &str, alert_id: &str, recurrence_id: Option<&str>) -> Result<Reminder> {
        let event = self.calendar_event(event_id).await?;
        Ok(reminder_from_event(&event, event_id, alert_id, recurrence_id))
    }

    async fn calendar_event(&self, event_id: &str) -> Result<Value> {
        let body = get_request(&self.calendar_account(), event_id);
        let response = self
            .http
            .post(self.client.session().api_url())
            .headers(self.client.headers().clone())
            .body(body.to_string())
            .send()
            .await
            .map_err(|e| Error::temporary(format!("CalendarEvent/get: {e}")))?;
        let status = response.status().as_u16();
        let bytes = response.bytes().await.map_err(|e| Error::temporary(format!("CalendarEvent/get: {e}")))?;
        event_from_response(status, &bytes)
    }

    /// The account that holds the calendars; the mail account when the session names none.
    fn calendar_account(&self) -> String {
        let session = self.client.session();
        let calendars = session.primary_accounts().find(|(c, _)| c.as_str() == CALENDARS_CAPABILITY);
        calendars.map_or_else(|| self.account_id.clone(), |(_, account)| account.clone())
    }
}

fn get_request(account_id: &str, event_id: &str) -> Value {
    json!({
        "using": [CORE_CAPABILITY, CALENDARS_CAPABILITY],
        "methodCalls": [["CalendarEvent/get", {
            "accountId": account_id,
            // No `properties`: Stalwart 1.1 leaves some (`recurrenceRules`) out when properties are
            // named, and a reminder for a moved occurrence needs the overrides to be there.
            "ids": [event_id],
        }, "a"]]
    })
}

/// The one event out of a `CalendarEvent/get` answer.
fn event_from_response(status: u16, body: &[u8]) -> Result<Value> {
    if status == 401 {
        return Err(unauthorized(format!("CalendarEvent/get: HTTP {status}")));
    }
    if !(200..300).contains(&status) {
        return Err(Error::temporary(format!("CalendarEvent/get: HTTP {status}")));
    }
    let value: Value = serde_json::from_slice(body)
        .map_err(|e| Error::temporary(format!("CalendarEvent/get: the answer is not JSON: {e}")))?;
    let call = &value["methodResponses"][0];
    if call[0] != "CalendarEvent/get" {
        // A method-level error, such as unknownMethod when the server has no calendars.
        return Err(Error::temporary(format!("CalendarEvent/get failed: {}", call[1])));
    }
    match call[1]["list"].get(0) {
        Some(event) => Ok(event.clone()),
        None => Err(Error::invalid("the event no longer exists on the server")),
    }
}

/// Builds the reminder from a JSCalendar Event or Task. Missing or odd fields give empty
/// ones: a reminder with some details is better than none.
fn reminder_from_event(event: &Value, event_id: &str, alert_id: &str, recurrence_id: Option<&str>) -> Reminder {
    let is_task = event["@type"] == "Task";
    // The override for this occurrence, if it has one. Without one the occurrence has the
    // series' own values, at the time the alert names.
    let patch = recurrence_id.map(|rid| &event["recurrenceOverrides"][rid]).filter(|p| p.is_object());
    let field = |key: &str| patch.and_then(|p| p.get(key)).or_else(|| event.get(key));
    let text = |key: &str| field(key).and_then(Value::as_str).filter(|s| !s.is_empty());

    let at = occurrence_start(is_task, patch, event, recurrence_id);
    let all_day = field("showWithoutTime").and_then(Value::as_bool).unwrap_or(false) || at.is_some_and(is_date);
    let (local_start, time_zone) = match at {
        None => (None, None),
        Some(at) if all_day => (Some(midnight(at)), None),
        Some(at) => split_utc(at, text("timeZone")),
    };
    Reminder {
        event_id: event_id.to_string(),
        alert_id: alert_id.to_string(),
        recurrence_id: recurrence_id.map(str::to_string),
        is_task,
        title: text("title").unwrap_or_default().to_string(),
        location: first_location(event, patch),
        local_start,
        time_zone,
        all_day,
    }
}

/// When this occurrence starts, or the task is due.
///
/// For a recurring event the alert's recurrenceId is the occurrence's original start, so it
/// is the start unless an override moved it. A task shows its due time.
fn occurrence_start<'a>(
    is_task: bool,
    patch: Option<&'a Value>,
    event: &'a Value,
    recurrence_id: Option<&'a str>,
) -> Option<&'a str> {
    let keys: &[&str] = if is_task { &["due", "start"] } else { &["start"] };
    let own = |v: &'a Value| keys.iter().find_map(|k| v[*k].as_str());
    patch.and_then(own).or(recurrence_id).or_else(|| own(event)).filter(|s| !s.is_empty())
}

/// A date without a time.
fn is_date(at: &str) -> bool {
    !at.contains('T')
}

/// The host parses `local_start` as a date-time, so an all-day date gets a midnight.
fn midnight(at: &str) -> String {
    if is_date(at) {
        format!("{at}T00:00:00")
    } else {
        at.to_string()
    }
}

/// A trailing `Z` is UTC written as an instant. The host expects a local time and a zone, so
/// it becomes the same wall-clock time in `UTC`. No zone at all is a floating time.
fn split_utc(at: &str, zone: Option<&str>) -> (Option<String>, Option<String>) {
    match at.strip_suffix('Z') {
        Some(local) => (Some(local.to_string()), Some("UTC".to_string())),
        None => (Some(at.to_string()), zone.map(str::to_string)),
    }
}

/// The name of the first location that has one. An override can replace the whole `locations`
/// map or patch single entries with JSON-pointer keys (`locations/l1/name`).
pub(super) fn first_location(event: &Value, patch: Option<&Value>) -> String {
    let own = patch.and_then(|p| p.get("locations")).or(event.get("locations"));
    let mut places: BTreeMap<String, Value> = match own {
        Some(Value::Object(map)) => map.iter().map(|(id, l)| (id.clone(), l.clone())).collect(),
        _ => BTreeMap::new(),
    };
    if let Some(Value::Object(patch)) = patch {
        for (key, value) in patch {
            apply_location_patch(&mut places, key, value);
        }
    }
    let name = |l: &Value| l["name"].as_str().filter(|n| !n.is_empty()).map(str::to_string);
    places.values().find_map(name).unwrap_or_default()
}

fn apply_location_patch(places: &mut BTreeMap<String, Value>, key: &str, value: &Value) {
    let Some(path) = key.strip_prefix("locations/") else { return };
    match path.split_once('/') {
        Some((id, "name")) => {
            places.entry(id.to_string()).or_insert_with(|| json!({}))["name"] = value.clone();
        }
        None if value.is_null() => {
            places.remove(path);
        }
        None => {
            places.insert(path.to_string(), value.clone());
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make(event: Value, rid: Option<&str>) -> Reminder {
        reminder_from_event(&event, "ev1", "al1", rid)
    }

    #[test]
    fn plain_event() {
        let r = make(
            json!({"@type": "Event", "title": "Standup", "start": "2026-11-11T09:30:00",
                "timeZone": "Europe/Istanbul", "locations": {"l1": {"name": "Room 2"}}}),
            None,
        );
        assert_eq!((r.event_id.as_str(), r.alert_id.as_str()), ("ev1", "al1"));
        assert_eq!((r.title.as_str(), r.location.as_str(), r.is_task, r.all_day), ("Standup", "Room 2", false, false));
        assert_eq!(r.local_start.as_deref(), Some("2026-11-11T09:30:00"));
        assert_eq!(r.time_zone.as_deref(), Some("Europe/Istanbul"));
    }

    #[test]
    fn task_uses_its_due_time() {
        let r = make(
            json!({"@type": "Task", "title": "Tax", "start": "2026-11-01T08:00:00", "due": "2026-11-20T17:00:00",
                "timeZone": "America/New_York"}),
            None,
        );
        assert!(r.is_task);
        assert_eq!(r.local_start.as_deref(), Some("2026-11-20T17:00:00"));
        assert_eq!(r.time_zone.as_deref(), Some("America/New_York"));
        // A task with only a start falls back to it.
        let r = make(json!({"@type": "Task", "title": "t", "start": "2026-11-01T08:00:00"}), None);
        assert_eq!(r.local_start.as_deref(), Some("2026-11-01T08:00:00"));
    }

    #[test]
    fn all_day_has_no_zone() {
        let r = make(
            json!({"@type": "Event", "title": "Holiday", "start": "2026-11-11T00:00:00", "showWithoutTime": true,
                "timeZone": "Europe/Istanbul"}),
            None,
        );
        assert!(r.all_day);
        assert_eq!(r.local_start.as_deref(), Some("2026-11-11T00:00:00"));
        assert_eq!(r.time_zone, None);
        // A bare date is all-day too, and gets a midnight.
        let r = make(json!({"@type": "Event", "title": "d", "start": "2026-11-12"}), None);
        assert!(r.all_day);
        assert_eq!(r.local_start.as_deref(), Some("2026-11-12T00:00:00"));
    }

    #[test]
    fn floating_time_has_no_zone() {
        let r = make(json!({"@type": "Event", "title": "Gym", "start": "2026-11-11T07:00:00"}), None);
        assert_eq!(r.local_start.as_deref(), Some("2026-11-11T07:00:00"));
        assert_eq!(r.time_zone, None);
        assert!(!r.all_day);
    }

    #[test]
    fn utc_instant_becomes_local_time_in_utc() {
        let r = make(json!({"@type": "Event", "title": "u", "start": "2026-11-11T07:00:00Z"}), None);
        assert_eq!(r.local_start.as_deref(), Some("2026-11-11T07:00:00"));
        assert_eq!(r.time_zone.as_deref(), Some("UTC"));
    }

    #[test]
    fn recurrence_id_selects_the_occurrence() {
        let event = json!({"@type": "Event", "title": "Weekly", "start": "2026-11-04T10:00:00",
            "timeZone": "Europe/Istanbul", "recurrenceRules": [{"frequency": "weekly"}]});
        let r = make(event, Some("2026-11-18T10:00:00"));
        assert_eq!(r.local_start.as_deref(), Some("2026-11-18T10:00:00"));
        assert_eq!(r.time_zone.as_deref(), Some("Europe/Istanbul"));
        assert_eq!(r.title, "Weekly");
    }

    #[test]
    fn override_changes_title_start_and_location() {
        let event = json!({"@type": "Event", "title": "Weekly", "start": "2026-11-04T10:00:00",
        "timeZone": "Europe/Istanbul", "locations": {"l1": {"name": "Room 1"}},
        "recurrenceOverrides": {
            "2026-11-11T10:00:00": {"title": "Weekly (moved)", "start": "2026-11-11T11:15:00",
                "locations": {"x": {"name": "Cafe"}}},
            "2026-11-18T10:00:00": {"locations/l1/name": "Room 9"},
            "2026-11-25T10:00:00": {"locations/l1": null}
        }});
        let moved = make(event.clone(), Some("2026-11-11T10:00:00"));
        assert_eq!(moved.title, "Weekly (moved)");
        assert_eq!(moved.local_start.as_deref(), Some("2026-11-11T11:15:00"));
        assert_eq!(moved.location, "Cafe");
        let renamed_room = make(event.clone(), Some("2026-11-18T10:00:00"));
        assert_eq!(renamed_room.location, "Room 9");
        assert_eq!(renamed_room.local_start.as_deref(), Some("2026-11-18T10:00:00"));
        assert_eq!(make(event.clone(), Some("2026-11-25T10:00:00")).location, "");
        // An occurrence without an override keeps the series' own values.
        assert_eq!(make(event, Some("2026-12-02T10:00:00")).location, "Room 1");
    }

    #[test]
    fn missing_fields_give_empty_details() {
        let r = make(json!({}), None);
        assert_eq!((r.title.as_str(), r.location.as_str(), r.is_task, r.all_day), ("", "", false, false));
        assert_eq!((r.local_start, r.time_zone), (None, None));
        let r = make(json!({"title": "", "locations": {"l": {"@type": "Location"}}}), None);
        assert_eq!((r.title.as_str(), r.location.as_str()), ("", ""));
    }

    #[test]
    fn response_with_an_event() {
        let body = json!({"methodResponses": [["CalendarEvent/get", {"list": [{"id": "e", "title": "T"}]}, "a"]]});
        let event = event_from_response(200, body.to_string().as_bytes()).unwrap();
        assert_eq!(event["title"], "T");
    }

    #[test]
    fn a_gone_event_is_an_error() {
        let body = json!({"methodResponses": [["CalendarEvent/get", {"list": [], "notFound": ["e"]}, "a"]]});
        let e = event_from_response(200, body.to_string().as_bytes()).unwrap_err();
        assert!(matches!(e, Error::Invalid { .. }));
    }

    #[test]
    fn failures_keep_their_kind() {
        use super::super::is_unauthorized;
        assert!(is_unauthorized(&event_from_response(401, b"").unwrap_err()));
        let e = event_from_response(503, b"").unwrap_err();
        assert!(matches!(e, Error::Temporary { .. }) && !is_unauthorized(&e));
        let method_error = json!({"methodResponses": [["error", {"type": "unknownMethod"}, "a"]]});
        assert!(matches!(event_from_response(200, method_error.to_string().as_bytes()), Err(Error::Temporary { .. })));
        assert!(matches!(event_from_response(200, b"<html>"), Err(Error::Temporary { .. })));
    }

    #[test]
    fn request_asks_for_the_whole_event_in_the_right_account() {
        let body = get_request("acc", "ev9");
        let call = &body["methodCalls"][0];
        assert_eq!(call[0], "CalendarEvent/get");
        assert_eq!(call[1]["accountId"], "acc");
        assert_eq!(call[1]["ids"], json!(["ev9"]));
        assert!(call[1].get("properties").is_none(), "the real server leaves the overrides out when asked by name");
    }
}
