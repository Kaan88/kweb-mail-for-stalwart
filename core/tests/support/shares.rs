//! What the mock knows about the user sharing their own things with others (see
//! `dev/notes/stalwart-sharing.md`): the `shareWith` of the user's own folders, calendars,
//! address books and file folders, and the directory of users.
//!
//! It behaves like Stalwart 1.1.0 where that matters to the core:
//!
//! - `shareWith` is a map from a user's id to the rights, every right named, true or false. A
//!   patch `shareWith/<id>` sets or (with null) removes one entry; `shareWith/<id>/<right>`
//!   one right. An entry with no right left is removed. An unknown user is `invalidProperties`
//!   ("Account id zz is invalid."), an unknown right too ("Invalid permission ...").
//! - A mail folder gives `shareWith` only when it is asked for by name.
//! - `Principal/query` is `forbidden` ("The administrator has disabled directory queries")
//!   unless the test switches the directory on; `Principal/get` then gives only the user and
//!   those the test lists as having shared something with them.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{json, Map, Value};

use super::state::{MethodError, ACCOUNT_ID, USERNAME};

const MAILBOX: [&str; 11] = [
    "mayReadItems",
    "mayAddItems",
    "mayRemoveItems",
    "maySetSeen",
    "maySetKeywords",
    "mayCreateChild",
    "mayRename",
    "maySubmit",
    "mayDelete",
    "mayShare",
    "maySetMetadata",
];
const CALENDAR: [&str; 9] = [
    "mayReadFreeBusy",
    "mayReadItems",
    "mayWriteAll",
    "mayWriteOwn",
    "mayUpdatePrivate",
    "mayRSVP",
    "mayShare",
    "mayDelete",
    "maySetMetadata",
];
const ADDRESS_BOOK: [&str; 5] = ["mayRead", "mayWrite", "mayDelete", "mayShare", "maySetMetadata"];
const FILE_NODE: [&str; 7] =
    ["mayRead", "mayAddChildren", "mayRename", "mayDelete", "mayModifyContent", "mayShare", "maySetMetadata"];

/// The JMAP types that can be shared, and the rights each has.
fn rights_of(object: &str) -> &'static [&'static str] {
    match object {
        "Mailbox" => &MAILBOX,
        "Calendar" => &CALENDAR,
        "AddressBook" => &ADDRESS_BOOK,
        _ => &FILE_NODE,
    }
}

pub struct OwnShares {
    /// Users of the server: `(id, name, email)`. The user is one of them.
    people: Vec<(String, String, String)>,
    /// With true `Principal/query` answers; with false it is forbidden.
    directory: bool,
    /// Ids `Principal/get` gives while the directory is off: those that shared with the user.
    sharers: BTreeSet<String>,
    /// `(type, object id)` to person to the rights that are on.
    entries: BTreeMap<(String, String), BTreeMap<String, BTreeSet<String>>>,
    /// Every patch of a `shareWith` that was sent: `(type, object id, patch)`.
    pub patches: Vec<(String, String, Value)>,
}

impl Default for OwnShares {
    fn default() -> Self {
        OwnShares {
            people: vec![(ACCOUNT_ID.into(), "Alice".into(), USERNAME.into())],
            directory: true,
            sharers: BTreeSet::new(),
            entries: BTreeMap::new(),
            patches: Vec::new(),
        }
    }
}

fn merr(kind: &'static str, description: impl Into<String>) -> MethodError {
    MethodError { kind, description: description.into() }
}

impl OwnShares {
    pub fn add_person(&mut self, id: &str, name: &str, email: &str) {
        self.people.push((id.into(), name.into(), email.into()));
    }

    pub fn set_directory(&mut self, on: bool) {
        self.directory = on;
    }

    /// The user `id` has shared something with the user: `Principal/get` gives them even with
    /// the directory off.
    pub fn add_sharer(&mut self, id: &str) {
        self.sharers.insert(id.into());
    }

    /// Gives `person` the rights of `object` `id` that are named, as another client would.
    pub fn grant(&mut self, object: &str, id: &str, person: &str, rights: &[&str]) {
        let entry = self.entries.entry((object.into(), id.into())).or_default();
        entry.insert(person.into(), rights.iter().map(|r| r.to_string()).collect());
    }

    /// The rights that are on for `person` in `object` `id`; empty when there is no entry.
    pub fn rights(&self, object: &str, id: &str, person: &str) -> Vec<String> {
        let entry = self.entries.get(&(object.to_string(), id.to_string()));
        entry.and_then(|e| e.get(person)).map(|r| r.iter().cloned().collect()).unwrap_or_default()
    }

    /// Everybody `object` `id` is shared with.
    pub fn people_of(&self, object: &str, id: &str) -> Vec<String> {
        let entry = self.entries.get(&(object.to_string(), id.to_string()));
        entry.map(|e| e.keys().cloned().collect()).unwrap_or_default()
    }

    /// The `shareWith` of `object` `id`, as the owner sees it: every right, true or false.
    pub fn share_with(&self, object: &str, id: &str) -> Value {
        let mut out = Map::new();
        let entry = self.entries.get(&(object.to_string(), id.to_string()));
        for (person, on) in entry.into_iter().flatten() {
            let all: Map<String, Value> =
                rights_of(object).iter().map(|r| (r.to_string(), json!(on.contains(*r)))).collect();
            out.insert(person.clone(), Value::Object(all));
        }
        Value::Object(out)
    }

    /// `Principal/query` and `Principal/get`.
    pub fn principal(&self, name: &str, args: &Value) -> Result<Value, MethodError> {
        if name == "Principal/query" {
            if !self.directory {
                return Err(merr("forbidden", "The administrator has disabled directory queries."));
            }
            let text = args["filter"]["text"].as_str().unwrap_or_default().to_lowercase();
            let limit = args["limit"].as_u64().map_or(usize::MAX, |l| l as usize);
            let found = self.people.iter().filter(|(_, n, e)| {
                // Like the real server: whole words only.
                let word = |s: &str| s.to_lowercase().split(|c: char| !c.is_alphanumeric()).any(|w| w == text);
                text.is_empty() || word(n) || word(e)
            });
            let ids: Vec<&String> = found.map(|(id, _, _)| id).take(limit).collect();
            return Ok(json!({ "accountId": ACCOUNT_ID, "queryState": "n", "position": 0, "ids": ids }));
        }
        let wanted: Vec<&str> = match args["ids"].as_array() {
            Some(ids) => ids.iter().filter_map(Value::as_str).collect(),
            None if self.directory => self.people.iter().map(|(id, _, _)| id.as_str()).collect(),
            None => vec![ACCOUNT_ID],
        };
        let (mut list, mut not_found) = (Vec::new(), Vec::new());
        for id in wanted {
            let known = self.people.iter().find(|(p, _, _)| p == id);
            let visible = self.directory || id == ACCOUNT_ID || self.sharers.contains(id);
            match known.filter(|_| visible) {
                Some((id, name, email)) => {
                    list.push(json!({ "id": id, "type": "individual", "name": name, "email": email }))
                }
                None => not_found.push(id.to_string()),
            }
        }
        Ok(json!({ "accountId": ACCOUNT_ID, "state": "n", "list": list, "notFound": not_found }))
    }

    /// Applies the `shareWith` keys of one object's patch. A set error is the answer for it.
    pub fn patch(&mut self, object: &str, id: &str, patch: &Map<String, Value>) -> Result<(), Value> {
        let bad = |why: String| json!({ "type": "invalidProperties", "description": why, "properties": ["shareWith"] });
        let rights = rights_of(object);
        let mut next = self.entries.get(&(object.to_string(), id.to_string())).cloned().unwrap_or_default();
        for (key, value) in patch {
            let parts: Vec<String> = key.split('/').map(|p| p.replace("~1", "/").replace("~0", "~")).collect();
            let person = parts.get(1).cloned().unwrap_or_default();
            if !self.people.iter().any(|(p, _, _)| *p == person) {
                return Err(
                    json!({ "type": "invalidProperties", "description": format!("Account id {person} is invalid.") }),
                );
            }
            let valid = |r: &str| rights.contains(&r);
            match (parts.len(), value) {
                (2, Value::Null) => {
                    next.remove(&person);
                }
                (2, Value::Object(map)) => {
                    let mut on = BTreeSet::new();
                    for (r, v) in map {
                        if !valid(r) {
                            return Err(bad(format!("Invalid permission \"{r}\".")));
                        }
                        if v.as_bool().is_none() {
                            return Err(bad(format!("Permission \"{r}\" must be a boolean.")));
                        }
                        if *v == true {
                            on.insert(r.clone());
                        }
                    }
                    next.insert(person, on);
                }
                (3, v) if v.is_boolean() && valid(&parts[2]) => {
                    let on = next.entry(person).or_default();
                    if *v == true {
                        on.insert(parts[2].clone());
                    } else {
                        on.remove(&parts[2]);
                    }
                }
                _ => return Err(bad(format!("Invalid patch \"{key}\"."))),
            }
        }
        next.retain(|_, on| !on.is_empty());
        self.patches.push((object.into(), id.into(), Value::Object(patch.clone())));
        self.entries.insert((object.into(), id.into()), next);
        Ok(())
    }
}
