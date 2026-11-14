//! The calendars and events of the mock: `Calendar/get`, `ParticipantIdentity/get` and the
//! `CalendarEvent` methods.
//!
//! Events are JSCalendar objects kept as the test wrote them (plus `id`, `uid`, `@type` and
//! `calendarIds` when missing). The mock behaves like Stalwart 1.1.0 where that matters to the
//! core, as found against the real server:
//!
//! - `CalendarEvent/get` answers `id` always and the other properties only when asked for;
//!   `recurrenceRules` only when `properties` is left out.
//! - `CalendarEvent/query` filters by `inCalendar` (one id) and by `after`/`before`;
//!   `inCalendars` and any other filter is `unsupportedFilter`. `expandRecurrences` needs both
//!   `after` and `before` and does not expand: a series is found where its first start is in the
//!   span (the first start is compared as written, as if it were UTC), and a task with no due
//!   time is never found by a span. Without a span every event of the calendar is found.
//! - `CalendarEvent/set` with `ifInState` that is not the current state is `stateMismatch`.
//! - `CalendarEvent/set` validates only that the calendars exist: a start that is not a date
//!   is stored as it is. A patch into a map the event does not have is `invalidPatch`.

use std::collections::{BTreeMap, VecDeque};

use serde_json::{json, Map, Value};

use super::state::{MethodError, ACCOUNT_ID, USERNAME};

pub struct Calendar {
    events: BTreeMap<String, Value>,
    /// Event ids in the order they were made, which is the order of a query.
    order: Vec<String>,
    next: u64,
    /// Counts every change, for the state strings.
    version: u64,
    calendars: Vec<Value>,
    /// What `ParticipantIdentity/get` says, as `mailto:` addresses.
    addresses: Vec<String>,
    /// The session's `maxExpandedQueryDuration`, `None` when it states none.
    pub max_span: Option<String>,
    /// The events a `CalendarEvent/set` with `sendSchedulingMessages` changed.
    pub scheduled: Vec<String>,
    /// Changes made right after the next `CalendarEvent/get`s, one each: another client writing
    /// between this client's read and its write. `(event id, property, new value)`.
    pub interfere: VecDeque<(String, String, Value)>,
}

impl Default for Calendar {
    fn default() -> Self {
        let mut calendar = Calendar {
            events: BTreeMap::new(),
            order: Vec::new(),
            next: 0,
            version: 0,
            calendars: Vec::new(),
            addresses: vec![format!("mailto:{USERNAME}")],
            max_span: Some("P52W1D".into()),
            scheduled: Vec::new(),
            interfere: VecDeque::new(),
        };
        calendar.add_calendar("cal1", "Personal", true, true);
        calendar
    }
}

fn invalid(description: &str) -> MethodError {
    MethodError { kind: "invalidArguments", description: description.to_string() }
}

fn strings<'a>(args: &'a Value, key: &str) -> Result<Option<Vec<&'a str>>, MethodError> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Array(a)) => Ok(Some(a.iter().filter_map(Value::as_str).collect())),
        Some(_) => Err(invalid(&format!("{key} must be an array of strings"))),
    }
}

impl Calendar {
    /// Stores an event and returns its id (`ev1`, `ev2`, ...).
    pub fn add(&mut self, mut event: Value) -> String {
        self.next += 1;
        self.version += 1;
        let id = format!("ev{}", self.next);
        let fields = event.as_object_mut().expect("an event is a JSON object");
        fields.insert("id".into(), json!(id));
        fields.entry("uid").or_insert_with(|| json!(format!("uid-{id}")));
        fields.entry("@type").or_insert_with(|| json!("Event"));
        fields.entry("calendarIds").or_insert_with(|| json!({ "cal1": true }));
        self.events.insert(id.clone(), event);
        self.order.push(id.clone());
        id
    }

    pub fn remove(&mut self, id: &str) -> bool {
        self.order.retain(|i| i != id);
        self.version += 1;
        self.events.remove(id).is_some()
    }

    pub fn uid_of(&self, id: &str) -> Option<String> {
        self.events.get(id)?["uid"].as_str().map(str::to_string)
    }

    pub fn event(&self, id: &str) -> Option<Value> {
        self.events.get(id).cloned()
    }

    pub fn events(&self) -> Vec<Value> {
        self.order.iter().filter_map(|id| self.events.get(id).cloned()).collect()
    }

    pub fn add_calendar(&mut self, id: &str, name: &str, visible: bool, writable: bool) {
        self.calendars.push(json!({
            "id": id, "name": name, "description": null, "color": null, "sortOrder": self.calendars.len(),
            "isSubscribed": true, "isVisible": visible, "isDefault": self.calendars.is_empty(), "timeZone": null,
            "myRights": {
                "mayReadFreeBusy": true, "mayReadItems": true, "mayWriteAll": writable, "mayWriteOwn": writable,
                "mayUpdatePrivate": writable, "mayRSVP": true, "mayShare": false, "mayDelete": writable,
                "maySetMetadata": writable,
            },
        }));
    }

    pub fn set_addresses(&mut self, addresses: &[&str]) {
        self.addresses = addresses.iter().map(|a| format!("mailto:{a}")).collect();
    }

    /// Answers one of the calendar methods.
    pub fn call(&mut self, name: &str, args: &Value) -> Result<Value, MethodError> {
        match name {
            "Calendar/get" => self.calendars_get(args),
            "ParticipantIdentity/get" => Ok(self.identities()),
            "CalendarEvent/get" => {
                let answer = self.get(args);
                if let Some((id, property, value)) = self.interfere.pop_front() {
                    if let Some(event) = self.events.get_mut(&id).and_then(Value::as_object_mut) {
                        event.insert(property, value);
                        self.version += 1;
                    }
                }
                answer
            }
            "CalendarEvent/query" => self.query(args),
            "CalendarEvent/set" => self.set(args),
            other => Err(MethodError { kind: "unknownMethod", description: other.to_string() }),
        }
    }

    fn state(&self) -> String {
        format!("cs{}", self.version)
    }

    fn calendars_get(&self, args: &Value) -> Result<Value, MethodError> {
        let props = strings(args, "properties")?;
        let list: Vec<Value> = self.calendars.iter().map(|c| pick(c, props.as_deref())).collect();
        Ok(json!({ "accountId": ACCOUNT_ID, "state": self.state(), "list": list, "notFound": [] }))
    }

    fn identities(&self) -> Value {
        let list: Vec<Value> = self
            .addresses
            .iter()
            .enumerate()
            .map(|(i, a)| json!({ "id": format!("pi{i}"), "name": "Alice", "calendarAddress": a, "isDefault": i == 0 }))
            .collect();
        json!({ "accountId": ACCOUNT_ID, "state": "n", "list": list, "notFound": [] })
    }

    pub fn get(&self, args: &Value) -> Result<Value, MethodError> {
        let ids: Vec<String> = match args.get("ids") {
            None | Some(Value::Null) => self.order.clone(),
            Some(Value::Array(a)) => a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect(),
            Some(_) => return Err(invalid("ids must be an array of strings")),
        };
        let props = strings(args, "properties")?;
        let mut list = Vec::new();
        let mut not_found = Vec::new();
        for id in ids {
            match self.events.get(&id) {
                Some(event) => list.push(pick(event, props.as_deref())),
                None => not_found.push(id),
            }
        }
        Ok(json!({ "accountId": ACCOUNT_ID, "state": self.state(), "list": list, "notFound": not_found }))
    }

    fn query(&self, args: &Value) -> Result<Value, MethodError> {
        let (mut calendar, mut after, mut before) = (None, None, None);
        for (key, value) in args.get("filter").and_then(Value::as_object).into_iter().flatten() {
            match key.as_str() {
                "inCalendar" => calendar = value.as_str(),
                "after" => after = value.as_str(),
                "before" => before = value.as_str(),
                other => return Err(MethodError { kind: "unsupportedFilter", description: other.to_string() }),
            }
        }
        if args["expandRecurrences"] == true && (after.is_none() || before.is_none()) {
            return Err(invalid("Both 'after' and 'before' filters are required when expanding recurrences"));
        }
        let wanted = |event: &Value| {
            let in_calendar = calendar.is_none_or(|c| event["calendarIds"].get(c).is_some_and(|m| *m == true));
            // The first start, compared as written; a `Z` on the filter is only the way it is spelled.
            let first = event["due"].as_str().filter(|_| event["@type"] == "Task").or_else(|| event["start"].as_str());
            let in_span = match (after, before) {
                (None, None) => true,
                (a, b) => first.is_some_and(|f| {
                    a.is_none_or(|a| f >= a.trim_end_matches('Z')) && b.is_none_or(|b| f < b.trim_end_matches('Z'))
                }),
            };
            in_calendar && in_span
        };
        let found: Vec<&String> = self.order.iter().filter(|id| wanted(&self.events[*id])).collect();
        let position = args["position"].as_u64().unwrap_or(0) as usize;
        let limit = args["limit"].as_u64().map_or(usize::MAX, |l| l as usize);
        let ids: Vec<&String> = found.iter().skip(position).take(limit).copied().collect();
        let mut answer = json!({
            "accountId": ACCOUNT_ID, "queryState": self.state(), "canCalculateChanges": false,
            "position": position, "ids": ids,
        });
        if args["calculateTotal"] == true {
            answer["total"] = json!(found.len());
        }
        Ok(answer)
    }

    fn set(&mut self, args: &Value) -> Result<Value, MethodError> {
        if args["ifInState"].as_str().is_some_and(|state| state != self.state()) {
            return Err(MethodError { kind: "stateMismatch", description: "the state has changed".into() });
        }
        let known = |event: &Value| -> Result<(), String> {
            let ids = event["calendarIds"].as_object().into_iter().flatten().map(|(id, _)| id);
            for id in ids {
                if !self.calendars.iter().any(|c| c["id"] == *id) {
                    return Err(format!("calendarId {id} does not exist."));
                }
            }
            Ok(())
        };
        let (mut created, mut not_created) = (Map::new(), Map::new());
        let mut to_add = Vec::new();
        for (cid, event) in args["create"].as_object().into_iter().flatten() {
            match known(event) {
                Ok(()) => to_add.push((cid.clone(), event.clone())),
                Err(why) => {
                    let error =
                        json!({ "type": "invalidProperties", "description": why, "properties": ["calendarIds"] });
                    not_created.insert(cid.clone(), error);
                }
            }
        }
        for (cid, event) in to_add {
            let id = self.add(event);
            created.insert(cid, json!({ "id": id, "uid": self.uid_of(&id), "isOrigin": true, "version": "2.0" }));
        }

        let (mut updated, mut not_updated) = (Map::new(), Map::new());
        for (id, patch) in args["update"].as_object().into_iter().flatten() {
            let Some(event) = self.events.get(id) else {
                not_updated.insert(id.clone(), json!({ "type": "notFound" }));
                continue;
            };
            match apply_patch(event, patch) {
                Ok(next) => {
                    self.events.insert(id.clone(), next);
                    self.version += 1;
                    updated.insert(id.clone(), Value::Null);
                    if args["sendSchedulingMessages"] == true {
                        self.scheduled.push(id.clone());
                    }
                }
                Err(path) => {
                    let error = json!({ "type": "invalidPatch", "description": "Patch operation failed.", "properties": [path] });
                    not_updated.insert(id.clone(), error);
                }
            }
        }

        let (mut destroyed, mut not_destroyed) = (Vec::new(), Map::new());
        for id in strings(args, "destroy")?.unwrap_or_default() {
            if self.remove(id) {
                destroyed.push(id.to_string());
            } else {
                not_destroyed.insert(id.to_string(), json!({ "type": "notFound" }));
            }
        }

        let mut answer = json!({ "accountId": ACCOUNT_ID, "oldState": "cs", "newState": self.state() });
        for (key, map) in [
            ("created", created),
            ("notCreated", not_created),
            ("updated", updated),
            ("notUpdated", not_updated),
            ("notDestroyed", not_destroyed),
        ] {
            if !map.is_empty() {
                answer[key] = Value::Object(map);
            }
        }
        if !destroyed.is_empty() {
            answer["destroyed"] = json!(destroyed);
        }
        Ok(answer)
    }
}

fn pick(object: &Value, props: Option<&[&str]>) -> Value {
    let Some(props) = props else { return object.clone() };
    let mut out = Map::new();
    for (key, value) in object.as_object().into_iter().flatten() {
        // Stalwart 1.1 leaves the rules out when properties are named, even when they are asked for.
        let withheld = key == "recurrenceRules";
        if key == "id" || (props.contains(&key.as_str()) && !withheld) {
            out.insert(key.clone(), value.clone());
        }
    }
    Value::Object(out)
}

/// A PatchObject applied to a copy of the event: every path is a JSON pointer without the
/// leading slash, a null removes. A path into an object that is not there fails, and then the
/// event is unchanged. The error is the path that failed.
fn apply_patch(event: &Value, patch: &Value) -> Result<Value, String> {
    let mut next = event.clone();
    for (path, value) in patch.as_object().into_iter().flatten() {
        let parts: Vec<String> = path.split('/').map(|p| p.replace("~1", "/").replace("~0", "~")).collect();
        let (last, parents) = parts.split_last().ok_or_else(|| path.clone())?;
        let mut at = &mut next;
        for part in parents {
            at = at.get_mut(part).filter(|v| v.is_object()).ok_or_else(|| path.clone())?;
        }
        let object = at.as_object_mut().ok_or_else(|| path.clone())?;
        if value.is_null() {
            object.remove(last);
        } else {
            object.insert(last.clone(), value.clone());
        }
    }
    Ok(next)
}
