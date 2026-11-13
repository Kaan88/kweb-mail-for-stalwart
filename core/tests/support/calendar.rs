//! The calendar events of the mock and `CalendarEvent/get`.
//!
//! Events are JSCalendar objects kept as the test wrote them (plus `id`, `uid` and `@type`
//! when missing). The mock answers `properties` like Stalwart does: `id` always, the rest
//! only when asked for.

use std::collections::BTreeMap;

use serde_json::{json, Map, Value};

use super::state::{MethodError, ACCOUNT_ID};

#[derive(Default)]
pub struct Calendar {
    events: BTreeMap<String, Value>,
    next: u64,
}

fn invalid(description: &str) -> MethodError {
    MethodError { kind: "invalidArguments", description: description.to_string() }
}

impl Calendar {
    /// Stores an event and returns its id (`ev1`, `ev2`, ...).
    pub fn add(&mut self, mut event: Value) -> String {
        self.next += 1;
        let id = format!("ev{}", self.next);
        let fields = event.as_object_mut().expect("an event is a JSON object");
        fields.insert("id".into(), json!(id));
        fields.entry("uid").or_insert_with(|| json!(format!("uid-{id}")));
        fields.entry("@type").or_insert_with(|| json!("Event"));
        self.events.insert(id.clone(), event);
        id
    }

    pub fn remove(&mut self, id: &str) -> bool {
        self.events.remove(id).is_some()
    }

    pub fn uid_of(&self, id: &str) -> Option<String> {
        self.events.get(id)?["uid"].as_str().map(str::to_string)
    }

    pub fn get(&self, args: &Value) -> Result<Value, MethodError> {
        let ids: Vec<String> = match args.get("ids") {
            None | Some(Value::Null) => self.events.keys().cloned().collect(),
            Some(Value::Array(a)) => a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect(),
            Some(_) => return Err(invalid("ids must be an array of strings")),
        };
        let props: Option<Vec<&str>> = match args.get("properties") {
            None | Some(Value::Null) => None,
            Some(Value::Array(a)) => Some(a.iter().filter_map(Value::as_str).collect()),
            Some(_) => return Err(invalid("properties must be an array of strings")),
        };
        let mut list = Vec::new();
        let mut not_found = Vec::new();
        for id in ids {
            match self.events.get(&id) {
                Some(event) => list.push(pick(event, props.as_deref())),
                None => not_found.push(id),
            }
        }
        Ok(json!({ "accountId": ACCOUNT_ID, "state": format!("cs{}", self.next), "list": list, "notFound": not_found }))
    }
}

fn pick(event: &Value, props: Option<&[&str]>) -> Value {
    let Some(props) = props else { return event.clone() };
    let mut out = Map::new();
    for (key, value) in event.as_object().into_iter().flatten() {
        if key == "id" || props.contains(&key.as_str()) {
            out.insert(key.clone(), value.clone());
        }
    }
    Value::Object(out)
}
