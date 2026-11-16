//! The requests the device calendar sync makes. All of them name the user's own account (the
//! account of the calendars capability), never another.

use std::collections::BTreeSet;

use serde_json::{json, Value};

use crate::{
    jmap::{
        calendars::CALENDARS_CAPABILITY,
        send::{created_id, destroyed, response, CREATE_ID},
        Connection,
    },
    Error, Result,
};

/// An own calendar as the server gives it.
#[derive(Debug, Clone)]
pub(super) struct ServerCalendar {
    pub id: String,
    pub name: String,
    pub color: Option<String>,
    pub time_zone: Option<String>,
    pub readable: bool,
    pub write_all: bool,
    pub write_own: bool,
}

pub(super) enum Page {
    Changes {
        created: Vec<String>,
        updated: Vec<String>,
        destroyed: Vec<String>,
        new_state: String,
        more: bool,
    },
    /// The server cannot say what changed since that state: read everything.
    Reset,
}

impl Connection {
    pub(super) fn devcal_account(&self) -> String {
        self.calendar_account_id()
    }

    pub(super) async fn devcal_calendars(&self) -> Result<(Vec<ServerCalendar>, Option<String>)> {
        let args = json!({
            "accountId": self.devcal_account(),
            "properties": ["name", "color", "sortOrder", "isSubscribed", "timeZone", "myRights"],
        });
        let responses = self.calendar_call(&[("Calendar/get", args)]).await?;
        let get = response(&responses, "Calendar/get")?;
        let mut out = Vec::new();
        for c in get["list"].as_array().into_iter().flatten() {
            let Some(id) = c["id"].as_str() else { continue };
            let rights = &c["myRights"];
            out.push(ServerCalendar {
                id: id.to_string(),
                name: c["name"].as_str().unwrap_or_default().to_string(),
                color: c["color"].as_str().filter(|s| !s.is_empty()).map(str::to_string),
                time_zone: c["timeZone"].as_str().filter(|s| !s.is_empty()).map(str::to_string),
                readable: rights["mayReadItems"] != false && c["isSubscribed"] != false,
                write_all: rights["mayWriteAll"] == true,
                write_own: rights["mayWriteOwn"] == true,
            });
        }
        Ok((out, get["state"].as_str().map(str::to_string)))
    }

    /// The current state of the user's calendar events, without reading any.
    pub(super) async fn devcal_events_state(&self) -> Result<Option<String>> {
        let args = json!({ "accountId": self.devcal_account(), "ids": [] });
        let responses = self.calendar_call(&[("CalendarEvent/get", args)]).await?;
        Ok(response(&responses, "CalendarEvent/get")?["state"].as_str().map(str::to_string))
    }

    pub(super) async fn devcal_changes(&self, since: &str, max: usize) -> Result<Page> {
        let args = json!({ "accountId": self.devcal_account(), "sinceState": since, "maxChanges": max });
        let responses = self.call_keeping_errors(&[CALENDARS_CAPABILITY], &[("CalendarEvent/changes", args)]).await?;
        if let Some(error) = responses.iter().find(|(n, _)| n == "error").map(|(_, a)| a) {
            // Stalwart answers a state it does not know with invalidArguments, the drafts with cannotCalculateChanges.
            return match error["type"].as_str() {
                Some("cannotCalculateChanges" | "invalidArguments") => Ok(Page::Reset),
                Some("forbidden") => {
                    Err(Error::invalid("the server does not let this account read its calendar changes"))
                }
                _ => Err(Error::temporary("CalendarEvent/changes failed")),
            };
        }
        let answer = response(&responses, "CalendarEvent/changes")?;
        let ids = |key: &str| -> Vec<String> {
            answer[key].as_array().into_iter().flatten().filter_map(|i| i.as_str().map(str::to_string)).collect()
        };
        let new_state = answer["newState"]
            .as_str()
            .ok_or_else(|| Error::temporary("the server's answer has no new state"))?
            .to_string();
        Ok(Page::Changes {
            created: ids("created"),
            updated: ids("updated"),
            destroyed: ids("destroyed"),
            new_state,
            more: answer["hasMoreChanges"] == true,
        })
    }

    /// These events, whole; the second list is the ids the server does not have.
    pub(super) async fn devcal_events(&self, ids: &[String]) -> Result<(Vec<Value>, Vec<String>)> {
        let (mut found, mut gone) = (Vec::new(), Vec::new());
        for chunk in ids.chunks(self.get_batch().max(1)) {
            let args = json!({ "accountId": self.devcal_account(), "ids": chunk });
            let responses = self.calendar_call(&[("CalendarEvent/get", args)]).await?;
            let answer = response(&responses, "CalendarEvent/get")?;
            found.extend(answer["list"].as_array().into_iter().flatten().cloned());
            gone.extend(
                answer["notFound"].as_array().into_iter().flatten().filter_map(|i| i.as_str().map(str::to_string)),
            );
        }
        Ok((found, gone))
    }

    /// Every event of these calendars, whole.
    pub(super) async fn devcal_all(&self, calendars: &BTreeSet<String>) -> Result<Vec<Value>> {
        self.events_in(&self.devcal_account(), calendars).await
    }

    /// One event, read now, with the state it was read at; `None` when the server does not have it.
    pub(super) async fn devcal_event(&self, id: &str) -> Result<Option<Value>> {
        let args = json!({ "accountId": self.devcal_account(), "ids": [id] });
        let responses = self.calendar_call(&[("CalendarEvent/get", args)]).await?;
        Ok(response(&responses, "CalendarEvent/get")?["list"].get(0).cloned())
    }

    /// Reads the event, builds a patch from it and sends it as of the state it was read at (see
    /// `Connection::change`). `Ok(false)`: the event is gone from the server.
    pub(super) async fn devcal_update(&self, id: &str, build: impl Fn(&Value) -> Result<Value>) -> Result<bool> {
        let account = self.devcal_account();
        match self.change(&account, id, false, build).await {
            Ok(()) => Ok(true),
            Err(Error::Invalid { msg }) if msg.contains("no longer exists") => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// Makes an event; returns its id.
    pub(super) async fn devcal_create(&self, object: Value) -> Result<String> {
        let args = json!({ "accountId": self.devcal_account(), "create": { CREATE_ID: object } });
        let responses = self.calendar_call(&[("CalendarEvent/set", args)]).await?;
        created_id(response(&responses, "CalendarEvent/set")?)
    }

    /// Destroys an event; one the server no longer has counts as destroyed.
    pub(super) async fn devcal_destroy(&self, id: &str) -> Result<()> {
        let args = json!({ "accountId": self.devcal_account(), "destroy": [id] });
        let responses = self.calendar_call(&[("CalendarEvent/set", args)]).await?;
        destroyed(response(&responses, "CalendarEvent/set")?, id)
    }

    pub(super) fn devcal_login(&self) -> String {
        self.username()
    }
}
