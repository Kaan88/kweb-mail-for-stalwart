//! What the mock knows about the account beyond mail: the sender identities, the address books
//! and their cards (JSContact), the away message and the quotas. `Identity/get`, `AddressBook/get`,
//! the `ContactCard` methods, the `VacationResponse` methods and `Quota/get`.
//!
//! The mock behaves like Stalwart 1.1.0 where that matters to the core, as found against the
//! real server:
//!
//! - A card is in `addressBookIds`; a create without a known book is `invalidProperties`, and so
//!   is a patch that takes a card out of its last book. The cards are listed in the order they
//!   were made.
//! - `ContactCard/set` with an `ifInState` that is not the state of the cards is `stateMismatch`.
//!   A patch into an entry that is there, or a new key of a map that is there, works; into one
//!   that is not there it is `invalidPatch`. Nothing else is validated (a mail address of
//!   "junk" is stored).
//! - `ContactCard/query` answers `total` when asked, takes `sort` and `position`, and gives no
//!   more ids than its `limit` (which a test can lower: a server may clamp it).
//! - The away message is the one object `singleton`, which cannot be created or destroyed. A date
//!   that is not an RFC 3339 date-time is `invalidProperties`; one with an offset or a fraction
//!   of a second is kept as UTC to the second. An enabled message with no text is accepted.

use std::collections::{BTreeMap, VecDeque};

use chrono::{DateTime, SecondsFormat};
use serde_json::{json, Map, Value};

use super::{
    calendar::apply_patch,
    state::{MethodError, ACCOUNT_ID, USERNAME},
};

pub struct IdentityRec {
    pub id: String,
    pub name: String,
    pub email: String,
    pub signature: String,
}

pub struct Account {
    pub identities: Vec<IdentityRec>,
    /// Cards as the test wrote them, plus `id` and `addressBookIds`.
    contacts: BTreeMap<String, Value>,
    /// Card ids in the order they were made, which is the order of a query.
    order: Vec<String>,
    next_card: u64,
    /// Counts every change to the cards, for the state strings.
    card_version: u64,
    address_books: Vec<Value>,
    /// Changes made right after the next `ContactCard/get`s, one each: another client writing
    /// between this client's read and its write. `(card id, property, new value)`.
    pub interfere: VecDeque<(String, String, Value)>,
    /// The `properties` of every `ContactCard/get` received (`null` when it named none).
    pub card_get_properties: Vec<Value>,
    /// The most ids a `ContactCard/query` gives, whatever its `limit` says.
    pub query_limit: usize,
    /// The `singleton` of `VacationResponse`, and how often it was changed.
    vacation: Value,
    vacation_version: u64,
    /// With false the session names no vacation response capability and the methods do not exist.
    pub vacation_enabled: bool,
    /// With false the session names no contacts capability and the methods do not exist.
    pub contacts_enabled: bool,
    pub quota_enabled: bool,
    /// Bytes used and the limit of the storage quota.
    pub quota: (u64, Option<u64>),
}

fn address_book(id: &str, name: &str, is_default: bool, may_write: bool) -> Value {
    json!({
        "id": id, "name": name, "description": null, "sortOrder": 0, "isDefault": is_default,
        "isSubscribed": true, "shareWith": {},
        "myRights": { "mayRead": true, "mayWrite": may_write, "mayDelete": may_write, "mayShare": false, "maySetMetadata": may_write }
    })
}

fn invalid(description: &str) -> MethodError {
    MethodError { kind: "invalidArguments", description: description.to_string() }
}

fn unknown(name: &str) -> MethodError {
    MethodError { kind: "unknownMethod", description: format!("the server has no {name}") }
}

impl Default for Account {
    fn default() -> Self {
        let identity = |id: &str, name: &str, email: &str, signature: &str| IdentityRec {
            id: id.into(),
            name: name.into(),
            email: email.into(),
            signature: signature.into(),
        };
        Account {
            identities: vec![
                identity("i1", "Alice Test", USERNAME, "Alice Test\nTest Corp"),
                identity("i2", "Alice (alias)", "alias@example.test", ""),
            ],
            contacts: BTreeMap::new(),
            order: Vec::new(),
            next_card: 0,
            card_version: 0,
            address_books: vec![address_book("b", "Stalwart Address Book (tester@kweb.test)", true, true)],
            interfere: VecDeque::new(),
            card_get_properties: Vec::new(),
            query_limit: usize::MAX,
            vacation: json!({
                "id": "singleton", "isEnabled": false, "fromDate": null, "toDate": null,
                "subject": null, "textBody": null, "htmlBody": null
            }),
            vacation_version: 0,
            vacation_enabled: true,
            contacts_enabled: true,
            quota_enabled: true,
            quota: (1_500, Some(10_000)),
        }
    }
}

impl Account {
    /// Adds a card and returns its id (`c1`, `c2`, ...). One that is in no address book is in
    /// the default one.
    pub fn add_contact(&mut self, mut card: Value) -> String {
        self.next_card += 1;
        self.card_version += 1;
        let id = format!("c{}", self.next_card);
        let fields = card.as_object_mut().expect("a card is a JSON object");
        fields.insert("id".into(), json!(id));
        fields.entry("@type").or_insert_with(|| json!("Card"));
        fields.entry("version").or_insert_with(|| json!("1.0"));
        fields.entry("addressBookIds").or_insert_with(|| json!({ "b": true }));
        self.contacts.insert(id.clone(), card);
        self.order.push(id.clone());
        id
    }

    /// Adds an address book (the default one is `b`).
    pub fn add_address_book(&mut self, id: &str, name: &str, may_write: bool) {
        self.address_books.push(address_book(id, name, false, may_write));
    }

    /// Whether the address book may be written.
    pub fn book_writable(&self, id: &str) -> bool {
        self.address_books.iter().any(|b| b["id"] == id && b["myRights"]["mayWrite"] == true)
    }

    /// Drops the address books, for a shared account that starts with only what the test adds.
    pub fn clear_address_books(&mut self) {
        self.address_books.clear();
    }

    /// Sets a property of a card, as another client would.
    pub fn set_card_property(&mut self, id: &str, property: &str, value: Value) {
        if let Some(card) = self.contacts.get_mut(id).and_then(Value::as_object_mut) {
            card.insert(property.into(), value);
            self.card_version += 1;
        }
    }

    /// Deletes a card, as another client would.
    pub fn remove_card(&mut self, id: &str) {
        self.contacts.remove(id);
        self.order.retain(|i| i != id);
        self.card_version += 1;
    }

    /// The cards in the order they were made.
    pub fn cards(&self) -> Vec<Value> {
        self.order.iter().filter_map(|id| self.contacts.get(id).cloned()).collect()
    }

    pub fn card(&self, id: &str) -> Option<Value> {
        self.contacts.get(id).cloned()
    }

    /// The away message as the server has it.
    pub fn vacation(&self) -> Value {
        self.vacation.clone()
    }

    /// Replaces the properties of the away message, as another client would.
    pub fn set_vacation(&mut self, properties: Value) {
        for (key, value) in properties.as_object().expect("properties are an object") {
            self.vacation[key] = value.clone();
        }
        self.vacation_version += 1;
    }

    pub fn call(&mut self, name: &str, args: &Value) -> Result<Value, MethodError> {
        match name {
            "Identity/get" => self.identity_get(args),
            "AddressBook/get" if self.contacts_enabled => Ok(self.address_book_get()),
            "ContactCard/query" if self.contacts_enabled => Ok(self.contact_query(args)),
            "ContactCard/get" if self.contacts_enabled => {
                self.card_get_properties.push(args["properties"].clone());
                let answer = self.contact_get(args);
                if let Some((id, property, value)) = self.interfere.pop_front() {
                    if let Some(card) = self.contacts.get_mut(&id).and_then(Value::as_object_mut) {
                        card.insert(property, value);
                        self.card_version += 1;
                    }
                }
                answer
            }
            "ContactCard/set" if self.contacts_enabled => self.contact_set(args),
            "VacationResponse/get" if self.vacation_enabled => Ok(self.vacation_get()),
            "VacationResponse/set" if self.vacation_enabled => self.vacation_set(args),
            "Quota/get" if self.quota_enabled => Ok(self.quota_get()),
            other => Err(unknown(other)),
        }
    }

    fn card_state(&self) -> String {
        format!("cs{}", self.card_version)
    }

    fn identity_get(&self, args: &Value) -> Result<Value, MethodError> {
        let props = props(args)?;
        let list: Vec<Value> = self
            .identities
            .iter()
            .map(|i| {
                let all = [
                    ("name", json!(i.name)),
                    ("email", json!(i.email)),
                    ("textSignature", json!(i.signature)),
                    ("htmlSignature", json!("")),
                    ("mayDelete", json!(false)),
                ];
                let mut out = Map::new();
                out.insert("id".into(), json!(i.id));
                for (k, v) in all {
                    if props.as_ref().is_none_or(|p| p.iter().any(|x| x == k)) {
                        out.insert(k.into(), v);
                    }
                }
                Value::Object(out)
            })
            .collect();
        Ok(json!({ "accountId": ACCOUNT_ID, "state": "is1", "list": list, "notFound": [] }))
    }

    fn address_book_get(&self) -> Value {
        json!({ "accountId": ACCOUNT_ID, "state": "sae", "list": self.address_books, "notFound": [] })
    }

    fn contact_query(&self, args: &Value) -> Value {
        let position = args["position"].as_u64().unwrap_or(0) as usize;
        let limit = args["limit"].as_u64().map_or(usize::MAX, |l| l as usize).min(self.query_limit);
        let ids: Vec<&String> = self.order.iter().skip(position).take(limit).collect();
        let mut answer = json!({
            "accountId": ACCOUNT_ID, "queryState": self.card_state(), "canCalculateChanges": true,
            "position": position, "ids": ids
        });
        if args["calculateTotal"] == true {
            answer["total"] = json!(self.order.len());
        }
        answer
    }

    fn contact_get(&self, args: &Value) -> Result<Value, MethodError> {
        let Some(ids) = args["ids"].as_array() else { return Err(invalid("ids is required")) };
        let props = props(args)?;
        let mut list = Vec::new();
        let mut not_found = Vec::new();
        for id in ids.iter().filter_map(Value::as_str) {
            match self.contacts.get(id) {
                Some(card) => list.push(pick(card, props.as_deref())),
                None => not_found.push(id),
            }
        }
        Ok(json!({ "accountId": ACCOUNT_ID, "state": self.card_state(), "list": list, "notFound": not_found }))
    }

    /// Whether every address book of the card exists and the card is in at least one.
    fn books_ok(&self, card: &Value) -> Result<(), String> {
        let books: Vec<&String> = card["addressBookIds"].as_object().into_iter().flatten().map(|(id, _)| id).collect();
        if books.is_empty() {
            return Err("Contact has to belong to at least one address book.".into());
        }
        match books.iter().find(|id| !self.address_books.iter().any(|b| b["id"] == ***id)) {
            Some(id) => Err(format!("addressBookId {id} does not exist.")),
            None => Ok(()),
        }
    }

    fn contact_set(&mut self, args: &Value) -> Result<Value, MethodError> {
        if args["ifInState"].as_str().is_some_and(|state| state != self.card_state()) {
            return Err(MethodError { kind: "stateMismatch", description: "the state has changed".into() });
        }
        let old_state = self.card_state();
        let invalid_books =
            |why: String| json!({ "type": "invalidProperties", "description": why, "properties": ["addressBookIds"] });
        let (mut created, mut not_created) = (Map::new(), Map::new());
        for (cid, card) in args["create"].as_object().into_iter().flatten() {
            match self.books_ok(card) {
                Ok(()) => {
                    let id = self.add_contact(card.clone());
                    created.insert(cid.clone(), json!({ "id": id }));
                }
                Err(why) => {
                    not_created.insert(cid.clone(), invalid_books(why));
                }
            }
        }
        let (mut updated, mut not_updated) = (Map::new(), Map::new());
        for (id, patch) in args["update"].as_object().into_iter().flatten() {
            let Some(card) = self.contacts.get(id) else {
                not_updated.insert(id.clone(), json!({ "type": "notFound" }));
                continue;
            };
            match apply_patch(card, patch) {
                Err(path) => {
                    let error = json!({ "type": "invalidPatch", "description": "Patch operation failed.", "properties": [path] });
                    not_updated.insert(id.clone(), error);
                }
                Ok(next) => match self.books_ok(&next) {
                    Err(why) => {
                        not_updated.insert(id.clone(), invalid_books(why));
                    }
                    Ok(()) => {
                        self.contacts.insert(id.clone(), next);
                        self.card_version += 1;
                        updated.insert(id.clone(), Value::Null);
                    }
                },
            }
        }
        let (mut destroyed, mut not_destroyed) = (Vec::new(), Map::new());
        for id in args["destroy"].as_array().into_iter().flatten().filter_map(Value::as_str) {
            if self.contacts.remove(id).is_some() {
                self.order.retain(|i| i != id);
                self.card_version += 1;
                destroyed.push(id.to_string());
            } else {
                not_destroyed.insert(id.to_string(), json!({ "type": "notFound" }));
            }
        }
        let mut answer = json!({ "accountId": ACCOUNT_ID, "oldState": old_state, "newState": self.card_state() });
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

    fn vacation_state(&self) -> String {
        format!("vs{}", self.vacation_version)
    }

    fn vacation_get(&self) -> Value {
        json!({ "accountId": ACCOUNT_ID, "state": self.vacation_state(), "list": [self.vacation], "notFound": [] })
    }

    fn vacation_set(&mut self, args: &Value) -> Result<Value, MethodError> {
        let old_state = self.vacation_state();
        let mut answer = json!({ "accountId": ACCOUNT_ID, "oldState": old_state });
        if let Some(create) = args["create"].as_object() {
            let refused: Map<String, Value> = create
                .keys()
                .map(|k| {
                    (
                        k.clone(),
                        json!({ "type": "singleton", "description": "Singletons cannot be created or destroyed." }),
                    )
                })
                .collect();
            answer["notCreated"] = Value::Object(refused);
        }
        for (id, patch) in args["update"].as_object().into_iter().flatten() {
            let date = |v: &Value| v.is_null() || v.as_str().is_some_and(|s| DateTime::parse_from_rfc3339(s).is_ok());
            let bad: Option<&str> =
                ["fromDate", "toDate"].into_iter().find(|key| patch.get(*key).is_some_and(|v| !date(v)));
            if id != "singleton" {
                answer["notUpdated"] = json!({ id: { "type": "notFound" } });
            } else if let Some(key) = bad {
                let error = json!({ "type": "invalidProperties", "description": "Field could not be set.", "properties": [key] });
                answer["notUpdated"] = json!({ id: error });
            } else {
                let mut patch = patch.clone();
                for key in ["fromDate", "toDate"] {
                    if let Some(at) = patch[key].as_str().and_then(|s| DateTime::parse_from_rfc3339(s).ok()) {
                        patch[key] = json!(at.to_utc().to_rfc3339_opts(SecondsFormat::Secs, true));
                    }
                }
                self.set_vacation(patch);
                answer["updated"] = json!({ id: null });
            }
        }
        answer["newState"] = json!(self.vacation_state());
        Ok(answer)
    }

    fn quota_get(&self) -> Value {
        let (used, limit) = self.quota;
        let mut storage = json!({
            "id": "q1", "name": "Storage", "resourceType": "octets", "scope": "account", "used": used,
            "types": ["Mail"],
        });
        if let Some(limit) = limit {
            storage["hardLimit"] = json!(limit);
        }
        let count = json!({
            "id": "q2", "name": "Messages", "resourceType": "count", "scope": "account", "used": 7,
            "types": ["Mail"],
        });
        json!({ "accountId": ACCOUNT_ID, "state": "qs1", "list": [storage, count], "notFound": [] })
    }
}

fn props(args: &Value) -> Result<Option<Vec<String>>, MethodError> {
    match args.get("properties") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Array(a)) => Ok(Some(a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())),
        Some(_) => Err(invalid("properties must be an array of strings")),
    }
}

fn pick(card: &Value, props: Option<&[String]>) -> Value {
    let Some(props) = props else { return card.clone() };
    let mut out = Map::new();
    for (key, value) in card.as_object().into_iter().flatten() {
        if key == "id" || props.contains(key) {
            out.insert(key.clone(), value.clone());
        }
    }
    Value::Object(out)
}
