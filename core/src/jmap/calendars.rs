//! Calendars, events and tasks over plain JMAP requests (jmap-client has no calendar support).
//!
//! What Stalwart 1.1.0 does that the drafts do not say, found against the real server; the
//! code follows the server:
//!
//! - `CalendarEvent/query` filters by `inCalendar` (one id); `inCalendars` is `unsupportedFilter`.
//! - It does not expand repeating events, whatever `expandRecurrences` says: a series is found
//!   only when its first start is in the span, and each is one id. So the events of the
//!   visible calendars are fetched whole and the occurrences are worked out in `jscal`.
//! - `recurrenceRules` is in the answer only when `properties` is left out (asked for by name
//!   it is missing; `recurrenceOverrides` came back by name on the local server, but is not
//!   relied on), and `recurrenceOverrides` cannot be asked together with `utcStart`. Events are therefore fetched whole, and times are worked out
//!   here from the local time and the zone.
//! - A patch into a map the event does not have yet (`alerts/x`, `recurrenceOverrides/x`) is
//!   `invalidPatch`; whole maps are set instead.
//! - Nothing is validated on write (a start of "nonsense" or a zone that does not exist is
//!   stored), so `jscal::validate` does it.
//! - A task's alert that is relative to a start the task does not have is dropped silently;
//!   relative to `end` (the due time) it is kept.
//!
//! The longest span an agenda may cover is the session's `maxExpandedQueryDuration`. The typed
//! session of jmap-client drops that value, so the session is read once more as JSON.

use std::collections::BTreeSet;

use chrono_tz::Tz;
use serde_json::{json, Value};

use super::{
    clock::parse_duration,
    errors::{is_state_mismatch, unauthorized},
    is_unauthorized, jscal,
    send::{created_id, destroyed, response, updated, CREATE_ID},
    Connection,
};
use crate::{
    model::{Calendar, EventDetail, EventInput, Occurrence},
    Error, Result,
};

const CALENDARS_CAPABILITY: &str = "urn:ietf:params:jmap:calendars";
/// The span an agenda may cover when the server does not say.
const FALLBACK_SPAN_SECS: u64 = 366 * 86_400;
const NO_SUCH_EVENT: &str = "the event no longer exists on the server";

impl Connection {
    fn calendar_account_id(&self) -> String {
        self.account_for(CALENDARS_CAPABILITY)
    }

    async fn calendar_call(&self, calls: &[(&str, Value)]) -> Result<Vec<(String, Value)>> {
        self.call(&[CALENDARS_CAPABILITY], calls).await
    }

    /// The user's calendars, in the server's order (by `sortOrder`).
    pub async fn calendars(&self) -> Result<Vec<Calendar>> {
        let args = json!({
            "accountId": self.calendar_account_id(),
            "properties": ["name", "color", "sortOrder", "isDefault", "isSubscribed", "isVisible", "myRights"],
        });
        let responses = self.calendar_call(&[("Calendar/get", args)]).await?;
        Ok(calendars_from(response(&responses, "Calendar/get")?))
    }

    /// The addresses that are the user: those of the participant identities, and the sign-in
    /// name. Lower case. A server that has no identities leaves the sign-in name.
    pub async fn my_addresses(&self) -> Result<Vec<String>> {
        let mut out = vec![self.username().to_lowercase()];
        let args = json!({ "accountId": self.calendar_account_id() });
        match self.calendar_call(&[("ParticipantIdentity/get", args)]).await {
            Ok(responses) => out.extend(identity_addresses(response(&responses, "ParticipantIdentity/get")?)),
            Err(e) if is_unauthorized(&e) => return Err(e),
            Err(_) => {}
        }
        Ok(out)
    }

    /// The agenda of `[from, to)`: the calendars as the server has them now, and the entries of
    /// the visible ones. A span longer than the server expands is `Error::Invalid`.
    pub async fn agenda(&self, from: i64, to: i64, zone: Tz) -> Result<(Vec<Calendar>, Vec<Occurrence>)> {
        let span = self.calendar_span.get_or_try_init(|| self.read_span()).await.copied().unwrap_or(FALLBACK_SPAN_SECS);
        if to - from > span as i64 {
            return Err(Error::invalid(format!("the server shows at most {} days at a time", span / 86_400)));
        }
        let calendars = self.calendars().await?;
        let visible: BTreeSet<String> = calendars.iter().filter(|c| c.is_visible).map(|c| c.id.clone()).collect();
        let me = self.my_addresses().await?;
        let events = self.events_in(&visible).await?;
        let view = jscal::View { zone, visible: &visible, me: &me };
        let mut entries: Vec<Occurrence> = events.iter().flat_map(|e| jscal::occurrences(e, from, to, &view)).collect();
        jscal::sort(&mut entries, zone);
        Ok((calendars, entries))
    }

    /// Every event and task of these calendars, whole. Each page is one request: the ids and,
    /// by reference, the events.
    async fn events_in(&self, calendars: &BTreeSet<String>) -> Result<Vec<Value>> {
        let (account, limit) = (self.calendar_account_id(), self.get_batch());
        let (mut out, mut seen) = (Vec::new(), BTreeSet::new());
        for calendar in calendars {
            let mut position = 0;
            loop {
                let query = json!({
                    "accountId": account, "filter": { "inCalendar": calendar }, "position": position, "limit": limit,
                });
                let get = json!({
                    "accountId": account,
                    "#ids": { "resultOf": "c0", "name": "CalendarEvent/query", "path": "/ids" },
                });
                let responses =
                    self.calendar_call(&[("CalendarEvent/query", query), ("CalendarEvent/get", get)]).await?;
                let found = response(&responses, "CalendarEvent/query")?["ids"].as_array().map_or(0, Vec::len);
                for event in response(&responses, "CalendarEvent/get")?["list"].as_array().into_iter().flatten() {
                    // An event in two of the calendars is one event.
                    if event["id"].as_str().is_some_and(|id| seen.insert(id.to_string())) {
                        out.push(event.clone());
                    }
                }
                if found < limit {
                    break;
                }
                position += found;
            }
        }
        Ok(out)
    }

    /// One event or task, whole, with the state of the calendar events it was read at. One the
    /// server does not have is `Error::Invalid`.
    async fn event_get(&self, id: &str) -> Result<(Value, Option<String>)> {
        let args = json!({ "accountId": self.calendar_account_id(), "ids": [id] });
        let responses = self.calendar_call(&[("CalendarEvent/get", args)]).await?;
        let answer = response(&responses, "CalendarEvent/get")?;
        let event = answer["list"].get(0).cloned().ok_or_else(|| Error::invalid(NO_SUCH_EVENT))?;
        Ok((event, answer["state"].as_str().map(str::to_string)))
    }

    /// An event or task in full, for its own screen; with `recurrence_id` one occurrence of it.
    pub async fn event_detail(&self, id: &str, recurrence_id: Option<&str>) -> Result<EventDetail> {
        let (event, _) = self.event_get(id).await?;
        let (calendars, me) = (self.calendars().await?, self.my_addresses().await?);
        jscal::detail_at(&event, &calendars, &me, recurrence_id)
    }

    /// Creates an event or task (`id` is `None`) or changes one. Returns its id. Only what the
    /// app owns is written; an update that changes nothing sends nothing.
    pub async fn save_event(&self, id: Option<&str>, input: &EventInput) -> Result<String> {
        let calendars = self.calendars().await?;
        let Some(id) = id else {
            jscal::validate(input, &calendars, None)?;
            let args = json!({
                "accountId": self.calendar_account_id(),
                "create": { CREATE_ID: jscal::create_object(input) },
            });
            let responses = self.calendar_call(&[("CalendarEvent/set", args)]).await?;
            return created_id(response(&responses, "CalendarEvent/set")?);
        };
        self.change(id, false, |event| {
            jscal::validate(input, &calendars, jscal::current_input(event).as_ref())?;
            jscal::update_patch(event, input)
        })
        .await?;
        Ok(id.to_string())
    }

    /// Deletes an event or task, or one occurrence of a repeating event.
    pub async fn delete_event(&self, id: &str, recurrence_id: Option<&str>) -> Result<()> {
        if let Some(recurrence_id) = recurrence_id {
            return self.change(id, false, |event| jscal::exclude_patch(event, recurrence_id)).await;
        }
        let args = json!({ "accountId": self.calendar_account_id(), "destroy": [id] });
        let responses = self.calendar_call(&[("CalendarEvent/set", args)]).await?;
        destroyed(response(&responses, "CalendarEvent/set")?, id)
    }

    /// Answers an invitation; the server tells the organizer.
    pub async fn respond_to_event(&self, id: &str, status: &str) -> Result<()> {
        let me = self.my_addresses().await?;
        self.change(id, true, |event| jscal::respond_patch(event, &me, status)).await
    }

    pub async fn set_task_done(&self, id: &str, done: bool) -> Result<()> {
        self.change(id, false, |event| jscal::progress_patch(event, done)).await
    }

    /// Reads the event, builds a patch from it and sends it as of the state it was read at. The
    /// patches set whole maps, so an event someone else changed in between must not be written
    /// over: then it is read again and the patch rebuilt, once. An empty patch sends nothing.
    /// With `notify` the server also sends the scheduling messages the change calls for.
    async fn change(&self, id: &str, notify: bool, build: impl Fn(&Value) -> Result<Value>) -> Result<()> {
        for _ in 0..2 {
            let (event, state) = self.event_get(id).await?;
            let patch = build(&event)?;
            if patch.as_object().is_some_and(|fields| fields.is_empty()) {
                return Ok(());
            }
            let mut args = json!({ "accountId": self.calendar_account_id(), "update": { id: patch } });
            if let Some(state) = state {
                args["ifInState"] = json!(state);
            }
            if notify {
                args["sendSchedulingMessages"] = json!(true);
            }
            match self.calendar_call(&[("CalendarEvent/set", args)]).await {
                Err(e) if is_state_mismatch(&e) => continue,
                Err(e) => return Err(e),
                Ok(responses) => return updated(response(&responses, "CalendarEvent/set")?, id, NO_SUCH_EVENT),
            }
        }
        Err(Error::temporary("the event was changed elsewhere while it was being saved; try again"))
    }

    /// The longest span in seconds the server expands, from the session.
    async fn read_span(&self) -> Result<u64> {
        let session = self.session_json().await?;
        let capability =
            |scope: &Value| scope[CALENDARS_CAPABILITY]["maxExpandedQueryDuration"].as_str().and_then(parse_duration);
        let own = &session["accounts"][self.calendar_account_id()]["accountCapabilities"];
        Ok(capability(own).or_else(|| capability(&session["capabilities"])).unwrap_or(FALLBACK_SPAN_SECS))
    }

    /// The session as JSON. Stalwart answers the well-known URL with a redirect, which is
    /// followed only within the same server, so the token goes nowhere else.
    pub(super) async fn session_json(&self) -> Result<Value> {
        let failed = |e: &dyn std::fmt::Display| Error::temporary(format!("session: {e}"));
        let mut url = url::Url::parse(self.client.session_url()).map_err(|e| failed(&e))?;
        for _ in 0..4 {
            let answer = self
                .http
                .get(url.clone())
                .headers(self.client.headers().clone())
                .send()
                .await
                .map_err(|e| failed(&e))?;
            let status = answer.status();
            if status.is_redirection() {
                let location = answer.headers().get(reqwest::header::LOCATION).and_then(|l| l.to_str().ok());
                let next = location.and_then(|l| url.join(l).ok());
                let place = |u: &url::Url| {
                    (u.scheme().to_string(), u.host_str().map(str::to_string), u.port_or_known_default())
                };
                match next {
                    Some(next) if place(&next) == place(&url) => url = next,
                    _ => return Err(failed(&"the session moved to another server")),
                }
                continue;
            }
            if status.as_u16() == 401 {
                return Err(unauthorized("session: HTTP 401"));
            }
            if !status.is_success() {
                return Err(failed(&format!("HTTP {status}")));
            }
            let bytes = answer.bytes().await.map_err(|e| failed(&e))?;
            return serde_json::from_slice(&bytes).map_err(|e| failed(&format!("the answer is not JSON: {e}")));
        }
        Err(failed(&"too many redirects"))
    }
}

/// The calendars of a `Calendar/get` answer. A calendar the user is not subscribed to or has
/// hidden is not visible; one with no rights stated is read-only.
fn calendars_from(get: &Value) -> Vec<Calendar> {
    let mut out: Vec<(i64, Calendar)> = Vec::new();
    for c in get["list"].as_array().into_iter().flatten() {
        let Some(id) = c["id"].as_str() else { continue };
        let rights = &c["myRights"];
        out.push((
            c["sortOrder"].as_i64().unwrap_or(0),
            Calendar {
                id: id.to_string(),
                name: c["name"].as_str().unwrap_or_default().to_string(),
                color: c["color"].as_str().filter(|s| !s.is_empty()).map(str::to_string),
                is_default: c["isDefault"] == true,
                is_visible: c["isSubscribed"] != false && c["isVisible"] != false,
                may_write: rights["mayWriteAll"] == true || rights["mayWriteOwn"] == true,
            },
        ));
    }
    out.sort_by_key(|(order, _)| *order);
    out.into_iter().map(|(_, calendar)| calendar).collect()
}

/// The `calendarAddress` of each participant identity, without `mailto:`, in lower case.
fn identity_addresses(get: &Value) -> Vec<String> {
    let addresses = get["list"].as_array().into_iter().flatten().filter_map(|i| i["calendarAddress"].as_str());
    addresses.map(|a| jscal::strip_mailto(a).to_lowercase()).filter(|a| !a.is_empty()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calendars_as_stalwart_gives_them() {
        let get = json!({"list": [
            {"id": "b", "name": "Stalwart Calendar", "color": null, "sortOrder": 0, "isDefault": true,
             "isSubscribed": true, "isVisible": true,
             "myRights": {"mayWriteAll": true, "mayWriteOwn": true, "mayReadItems": true}},
            {"id": "w", "name": "Work", "color": "#3a7bd5", "sortOrder": -1, "isVisible": false,
             "myRights": {"mayWriteAll": false, "mayWriteOwn": true}},
            {"id": "s", "name": "Shared", "color": "", "isSubscribed": false, "myRights": {"mayReadItems": true}},
            {"name": "no id"}
        ]});
        let found = calendars_from(&get);
        let flags = |c: &Calendar| (c.id.clone(), c.color.clone(), c.is_default, c.is_visible, c.may_write);
        assert_eq!(
            found.iter().map(flags).collect::<Vec<_>>(),
            [
                ("w".into(), Some("#3a7bd5".into()), false, false, true),
                ("b".into(), None, true, true, true),
                ("s".into(), None, false, false, false),
            ]
        );
        assert_eq!(found[1].name, "Stalwart Calendar");
    }

    #[test]
    fn identity_addresses_lose_mailto_and_case() {
        let get = json!({"list": [
            {"id": "a", "calendarAddress": "mailto:Tester@KWeb.test"},
            {"id": "b", "calendarAddress": "alias@kweb.test"},
            {"id": "c", "calendarAddress": null}
        ]});
        assert_eq!(identity_addresses(&get), ["tester@kweb.test", "alias@kweb.test"]);
    }
}
