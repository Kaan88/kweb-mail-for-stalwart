//! What the mock knows about the account beyond mail: the sender identities, the address book
//! (JSContact cards) and the quotas. `Identity/get`, `ContactCard/query`, `ContactCard/get`
//! and `Quota/get`.

use serde_json::{json, Map, Value};

use super::state::{MethodError, ACCOUNT_ID, USERNAME};

pub struct IdentityRec {
    pub id: String,
    pub name: String,
    pub email: String,
    pub signature: String,
}

pub struct Account {
    pub identities: Vec<IdentityRec>,
    /// Cards as the test wrote them, plus `id`.
    contacts: Vec<Value>,
    /// With false the session names no contacts capability and the methods do not exist.
    pub contacts_enabled: bool,
    pub quota_enabled: bool,
    /// Bytes used and the limit of the storage quota.
    pub quota: (u64, Option<u64>),
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
            contacts: Vec::new(),
            contacts_enabled: true,
            quota_enabled: true,
            quota: (1_500, Some(10_000)),
        }
    }
}

impl Account {
    /// Adds a card and returns its id (`c1`, `c2`, ...).
    pub fn add_contact(&mut self, mut card: Value) -> String {
        let id = format!("c{}", self.contacts.len() + 1);
        card.as_object_mut().expect("a card is a JSON object").insert("id".into(), json!(id));
        self.contacts.push(card);
        id
    }

    pub fn call(&self, name: &str, args: &Value) -> Result<Value, MethodError> {
        match name {
            "Identity/get" => self.identity_get(args),
            "ContactCard/query" if self.contacts_enabled => Ok(self.contact_query(args)),
            "ContactCard/get" if self.contacts_enabled => self.contact_get(args),
            "Quota/get" if self.quota_enabled => Ok(self.quota_get()),
            other => Err(unknown(other)),
        }
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

    fn contact_query(&self, args: &Value) -> Value {
        let limit = args["limit"].as_u64().map_or(usize::MAX, |l| l as usize);
        let ids: Vec<&str> = self.contacts.iter().filter_map(|c| c["id"].as_str()).take(limit).collect();
        json!({ "accountId": ACCOUNT_ID, "queryState": "cq1", "canCalculateChanges": false, "position": 0, "ids": ids })
    }

    fn contact_get(&self, args: &Value) -> Result<Value, MethodError> {
        let Some(ids) = args["ids"].as_array() else { return Err(invalid("ids is required")) };
        let props = props(args)?;
        let mut list = Vec::new();
        let mut not_found = Vec::new();
        for id in ids.iter().filter_map(Value::as_str) {
            match self.contacts.iter().find(|c| c["id"] == id) {
                Some(card) => list.push(pick(card, props.as_deref())),
                None => not_found.push(id),
            }
        }
        Ok(json!({ "accountId": ACCOUNT_ID, "state": "cs1", "list": list, "notFound": not_found }))
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
