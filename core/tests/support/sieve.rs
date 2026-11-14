//! The mock's filter scripts: `SieveScript/get`, `/set` and `/validate`, and the blobs that hold
//! their text.
//!
//! It behaves like Stalwart 1.1.0 where that matters to the core, as found against the real
//! server (`dev/notes/stalwart-sieve.md`):
//!
//! - A script is made from a blob the client uploaded; the server stores its own copy, so the
//!   blob id of a created or updated script is not the uploaded one.
//! - One script is active at a time. Activating one (`isActive`, `onSuccessActivateScript`)
//!   deactivates the other, the away message's script included, and the answer does not say so.
//! - The reserved script `vacation` is there from the start. It is active exactly when the
//!   away message is on, cannot be changed through `SieveScript`, and switching the away
//!   message on deactivates the active script (see `MockServer::set_vacation`).
//! - The state of the scripts moves with every change and with every change of the away message.
//! - Names are unique without regard to case. A script over the size limit is `tooLarge`.
//! - Validation is not a parser here: it refuses what a test told it to refuse
//!   (`refuse_validation`) and a script over the size limit. The error names are Stalwart's
//!   (`invalidScript`) unless a test asks for the RFC's (`invalidSieve`).

use std::collections::BTreeMap;

use serde_json::{json, Map, Value};

use super::{
    account::Account,
    state::{MethodError, ACCOUNT_ID},
};

pub const VACATION_ID: &str = "vac";
const VACATION_BLOB: &str = "vacblob";
const VACATION_TEXT: &str = "require [\"vacation\"];\r\nvacation \"I am away.\";\r\n";

struct Script {
    id: String,
    name: String,
    blob_id: String,
    active: bool,
}

pub struct Sieve {
    /// With false the session names no Sieve capability and the methods do not exist.
    pub enabled: bool,
    pub extensions: Vec<String>,
    pub max_size: u64,
    /// With a text, every validation is refused with it.
    pub refusal: Option<String>,
    /// The RFC's error names instead of Stalwart's.
    pub rfc_names: bool,
    /// With true, once a script is written every read of scripts fails until set false.
    pub break_reads_after_write: bool,
    pub reads_broken: bool,
    scripts: Vec<Script>,
    blobs: BTreeMap<String, String>,
    next: u64,
    version: u64,
}

impl Default for Sieve {
    fn default() -> Self {
        Sieve {
            enabled: true,
            extensions: ["body", "fileinto", "imap4flags", "include", "vacation"].map(str::to_string).to_vec(),
            max_size: 102_400,
            refusal: None,
            rfc_names: false,
            break_reads_after_write: false,
            reads_broken: false,
            scripts: Vec::new(),
            blobs: BTreeMap::new(),
            next: 0,
            version: 0,
        }
    }
}

fn error(kind: &'static str, description: &str) -> MethodError {
    MethodError { kind, description: description.to_string() }
}

/// What the account says about its Sieve support.
impl Sieve {
    pub fn capability(&self) -> Value {
        json!({
            "maxSizeScriptName": 512, "maxSizeScript": self.max_size, "maxNumberScripts": 100,
            "maxNumberRedirects": 1, "sieveExtensions": self.extensions, "notificationMethods": ["mailto"],
            "externalLists": null,
        })
    }

    fn state(&self) -> String {
        format!("sc{}", self.version)
    }

    // ---- what the controls of a test do ------------------------------------------------

    /// `(name, active)` of every script, the away message's included.
    pub fn list(&self, account: &Account) -> Vec<(String, bool)> {
        let mut out = vec![("vacation".to_string(), vacation_on(account))];
        out.extend(self.scripts.iter().map(|s| (s.name.clone(), s.active)));
        out
    }

    pub fn text(&self, name: &str) -> Option<String> {
        let script = self.scripts.iter().find(|s| s.name.eq_ignore_ascii_case(name))?;
        self.blobs.get(&script.blob_id).cloned()
    }

    /// Another client writes a script, making it or replacing the text of the one of that name,
    /// and activates it when it says so. Nothing is validated.
    pub fn put(&mut self, account: &mut Account, name: &str, text: &str, active: bool) -> String {
        let blob_id = self.store(text);
        let id = match self.scripts.iter_mut().find(|s| s.name.eq_ignore_ascii_case(name)) {
            Some(script) => {
                script.blob_id = blob_id;
                script.id.clone()
            }
            None => {
                let id = self.new_id();
                self.scripts.push(Script { id: id.clone(), name: name.to_string(), blob_id, active: false });
                id
            }
        };
        if active {
            self.activate(account, &id);
        }
        self.version += 1;
        id
    }

    /// Another client activates or deactivates a script by name.
    pub fn set_active(&mut self, account: &mut Account, name: &str, active: bool) {
        let id = self.scripts.iter().find(|s| s.name.eq_ignore_ascii_case(name)).map(|s| s.id.clone());
        if let Some(id) = id {
            match active {
                true => self.activate(account, &id),
                false => self.scripts.iter_mut().filter(|s| s.id == id).for_each(|s| s.active = false),
            }
            self.version += 1;
        }
    }

    /// The away message was changed (by a client or by `VacationResponse/set`): the state of the
    /// scripts moves, and switching it on switches the active script off.
    pub fn vacation_changed(&mut self, account: &Account) {
        if vacation_on(account) {
            self.scripts.iter_mut().for_each(|s| s.active = false);
        }
        self.version += 1;
    }

    /// The type and bytes of a script's text, by blob id.
    pub fn blob(&self, blob_id: &str) -> Option<(String, Vec<u8>)> {
        let text = match blob_id {
            VACATION_BLOB => VACATION_TEXT,
            _ => self.blobs.get(blob_id)?,
        };
        Some(("application/sieve".to_string(), text.as_bytes().to_vec()))
    }

    // ---- the methods ---------------------------------------------------------------------

    pub fn call(
        &mut self,
        name: &str,
        args: &Value,
        account: &mut Account,
        uploaded: &dyn Fn(&str) -> Option<Vec<u8>>,
    ) -> Result<Value, MethodError> {
        if !self.enabled {
            return Err(error("unknownMethod", &format!("the server has no {name}")));
        }
        match name {
            "SieveScript/get" if self.reads_broken => Err(error("serverFail", "the mock cannot read scripts now")),
            "SieveScript/get" => Ok(self.get(args, account)),
            "SieveScript/validate" => self.validate(args, uploaded),
            "SieveScript/set" => {
                let result = self.set(args, account, uploaded);
                self.reads_broken |= result.is_ok() && self.break_reads_after_write;
                result
            }
            other => Err(error("unknownMethod", &format!("the server has no {other}"))),
        }
    }

    fn json(&self, script: &Script) -> Value {
        json!({ "id": script.id, "name": script.name, "blobId": script.blob_id, "isActive": script.active })
    }

    fn vacation_json(account: &Account) -> Value {
        json!({ "id": VACATION_ID, "name": "vacation", "blobId": VACATION_BLOB, "isActive": vacation_on(account) })
    }

    fn get(&self, args: &Value, account: &Account) -> Value {
        let all: Vec<Value> =
            std::iter::once(Self::vacation_json(account)).chain(self.scripts.iter().map(|s| self.json(s))).collect();
        let (mut list, mut not_found) = (Vec::new(), Vec::new());
        match args["ids"].as_array() {
            None => list = all,
            Some(ids) => {
                for id in ids.iter().filter_map(Value::as_str) {
                    match all.iter().find(|s| s["id"] == id) {
                        Some(found) => list.push(found.clone()),
                        None => not_found.push(id.to_string()),
                    }
                }
            }
        }
        json!({ "accountId": ACCOUNT_ID, "state": self.state(), "list": list, "notFound": not_found })
    }

    fn invalid_name(&self) -> &'static str {
        if self.rfc_names {
            "invalidSieve"
        } else {
            "invalidScript"
        }
    }

    /// Why the server refuses this text, if it does.
    fn refuse(&self, text: &str) -> Option<(&'static str, String)> {
        if text.len() as u64 > self.max_size {
            return Some((self.invalid_name(), "Sieve script is too large at line 0, column 0.".to_string()));
        }
        self.refusal.as_ref().map(|why| (self.invalid_name(), why.clone()))
    }

    fn validate(&self, args: &Value, uploaded: &dyn Fn(&str) -> Option<Vec<u8>>) -> Result<Value, MethodError> {
        let blob_id = args["blobId"].as_str().unwrap_or_default();
        let error = match uploaded(blob_id) {
            None => json!({ "type": "blobNotFound" }),
            Some(bytes) => match self.refuse(&String::from_utf8_lossy(&bytes)) {
                Some((kind, description)) => json!({ "type": kind, "description": description }),
                None => Value::Null,
            },
        };
        Ok(json!({ "accountId": ACCOUNT_ID, "error": error }))
    }

    fn store(&mut self, text: &str) -> String {
        self.next += 1;
        let id = format!("svb{}", self.next);
        self.blobs.insert(id.clone(), text.to_string());
        id
    }

    fn new_id(&mut self) -> String {
        self.next += 1;
        format!("sc{}", self.next)
    }

    fn activate(&mut self, account: &mut Account, id: &str) {
        self.scripts.iter_mut().for_each(|s| s.active = s.id == id);
        if vacation_on(account) {
            account.set_vacation(json!({ "isEnabled": false }));
        }
    }

    /// A blob the client uploaded and the text of it, or the set error that says why not.
    fn take(&self, blob_id: &str, uploaded: &dyn Fn(&str) -> Option<Vec<u8>>) -> Result<String, Value> {
        let Some(bytes) = uploaded(blob_id).or_else(|| self.blobs.get(blob_id).map(|t| t.as_bytes().to_vec())) else {
            return Err(json!({ "type": "blobNotFound", "description": "Blob not found." }));
        };
        let text = String::from_utf8_lossy(&bytes).into_owned();
        if text.len() as u64 > self.max_size {
            return Err(json!({ "type": "tooLarge", "description": "Sieve script is too large at line 0, column 0." }));
        }
        match self.refuse(&text) {
            Some((kind, description)) => Err(json!({ "type": kind, "description": description })),
            None => Ok(text),
        }
    }

    fn set(
        &mut self,
        args: &Value,
        account: &mut Account,
        uploaded: &dyn Fn(&str) -> Option<Vec<u8>>,
    ) -> Result<Value, MethodError> {
        let old_state = self.state();
        if args["ifInState"].as_str().is_some_and(|wanted| wanted != old_state) {
            return Err(error(
                "stateMismatch",
                "An \"ifInState\" argument was supplied, but it does not match the current state.",
            ));
        }
        let (mut created, mut not_created) = (Map::new(), Map::new());
        let (mut updated, mut not_updated) = (Map::new(), Map::new());
        let mut ids: BTreeMap<String, String> = BTreeMap::new();
        for (cid, object) in args["create"].as_object().into_iter().flatten() {
            let name = object["name"].as_str().unwrap_or_default();
            let blob = object["blobId"].as_str().unwrap_or_default();
            if name.eq_ignore_ascii_case("vacation") {
                let reserved = "The 'vacation' name is reserved, please use a different name.";
                not_created.insert(cid.clone(), json!({ "type": "forbidden", "description": reserved }));
                continue;
            }
            if let Some(same) = self.scripts.iter().find(|s| s.name.eq_ignore_ascii_case(name)) {
                let description = format!("A sieve script with name '{name}' already exists.");
                let e = json!({ "type": "alreadyExists", "description": description, "existingId": same.id });
                not_created.insert(cid.clone(), e);
                continue;
            }
            match self.take(blob, uploaded) {
                Err(e) => {
                    not_created.insert(cid.clone(), e);
                }
                Ok(text) => {
                    let (id, blob_id) = (self.new_id(), self.store(&text));
                    let name = if name.is_empty() { id.clone() } else { name.to_string() };
                    let active = object["isActive"] == true;
                    self.scripts.push(Script { id: id.clone(), name, blob_id: blob_id.clone(), active: false });
                    if active {
                        self.activate(account, &id);
                    }
                    created.insert(cid.clone(), json!({ "id": id, "blobId": blob_id }));
                    ids.insert(cid.clone(), id);
                }
            }
        }
        for (id, patch) in args["update"].as_object().into_iter().flatten() {
            if id == VACATION_ID {
                let why = "The 'vacation' script cannot be modified, use VacationResponse/set instead.";
                not_updated.insert(id.clone(), json!({ "type": "forbidden", "description": why }));
                continue;
            }
            let Some(at) = self.scripts.iter().position(|s| s.id == *id) else {
                not_updated.insert(id.clone(), json!({ "type": "notFound" }));
                continue;
            };
            let mut answer = Value::Null;
            if let Some(blob) = patch["blobId"].as_str() {
                match self.take(blob, uploaded) {
                    Err(e) => {
                        not_updated.insert(id.clone(), e);
                        continue;
                    }
                    Ok(text) => {
                        let blob_id = self.store(&text);
                        self.scripts[at].blob_id = blob_id.clone();
                        answer = json!({ "blobId": blob_id });
                    }
                }
            }
            if let Some(name) = patch["name"].as_str() {
                self.scripts[at].name = name.to_string();
            }
            match patch["isActive"].as_bool() {
                Some(true) => self.activate(account, id),
                Some(false) => self.scripts[at].active = false,
                None => {}
            }
            updated.insert(id.clone(), answer);
        }
        // The id may be a creation id of this request ("#s1"); one that is nothing is ignored.
        let target = args["onSuccessActivateScript"].as_str().map(|t| match t.strip_prefix('#') {
            Some(cid) => ids.get(cid).cloned().unwrap_or_default(),
            None => t.to_string(),
        });
        if let Some(id) = target.filter(|id| self.scripts.iter().any(|s| s.id == *id)) {
            self.activate(account, &id);
        }
        if args["onSuccessDeactivateScript"] == true {
            self.scripts.iter_mut().for_each(|s| s.active = false);
            if vacation_on(account) {
                account.set_vacation(json!({ "isEnabled": false }));
            }
        }
        self.version += 1;
        let mut answer = json!({ "accountId": ACCOUNT_ID, "oldState": old_state, "newState": self.state() });
        for (key, map) in
            [("created", created), ("notCreated", not_created), ("updated", updated), ("notUpdated", not_updated)]
        {
            if !map.is_empty() {
                answer[key] = Value::Object(map);
            }
        }
        Ok(answer)
    }
}

fn vacation_on(account: &Account) -> bool {
    account.vacation()["isEnabled"] == true
}
