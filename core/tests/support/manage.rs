//! The mock's side of what a user manages in their own account: `GET /api/account` and the
//! `x:` methods for masked addresses, the password (with two-step codes), app passwords, API
//! keys, passkeys and OAuth grants. Written from `dev/notes/stalwart-admin-access.md` and
//! `dev/notes/stalwart-enterprise-observed.md`, which are what Stalwart 1.1.0 answered:
//!
//! - Three kinds of server: one with the licence, one that has the Enterprise build without a
//!   licence (it refuses `x:MaskedEmail` with a `forbidden` that says so), and one that knows
//!   none of this (no `/api/account`, every `x:` method is `unknownMethod`).
//! - A sign-in without the settings and security scopes gets `forbidden` ("You are not
//!   authorized to perform this action") from every `x:` method.
//! - `x:AccountPassword` is a singleton. Every change needs `currentSecret`, and with two-step
//!   codes on also `otpAuth.otpCode`. Its `otpAuth` reads `{}` or `{"otpUrl": "****"}`.
//! - Secrets are shown once, in the answer to the create; a get shows `****`.
//! - Five app passwords and five API keys at most (`overQuota`).
//! - A sign-in with fewer rights than the account may not make a key with `Inherit` ("not
//!   authorized to grant permissions"), but may with the rights it has (`Replace`).

use serde_json::{json, Map, Value};

use super::state::{now_secs, rfc3339, MethodError, ACCOUNT_ID};

/// What kind of server this is.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Edition {
    /// Enterprise with a licence: masked addresses work.
    Licensed,
    /// An Enterprise build without a licence ("community" in `/api/account`).
    Unlicensed,
    /// Knows nothing of the management API.
    Unaware,
}

pub const PASSWORD: &str = "current-password";
/// The code the mock takes as the current two-step code.
pub const OTP_CODE: &str = "123456";
const ENTERPRISE: &str = "This feature is only available in the Enterprise edition. Obtain your trial license at https://license.stalw.art/trial.";
const NOT_AUTHORIZED: &str = "You are not authorized to perform this action";
const QUOTA: usize = 5;

#[derive(Clone)]
struct Masked {
    id: String,
    email: String,
    description: Option<String>,
    for_domain: Option<String>,
    enable: bool,
    created_at: i64,
}

#[derive(Clone)]
struct Credential {
    id: String,
    description: String,
    secret: String,
    created_at: i64,
    expires_at: Option<String>,
    permissions: Value,
}

#[derive(Clone)]
struct Passkey {
    id: String,
    description: String,
    created_at: i64,
}

#[derive(Clone)]
struct Grant {
    id: String,
    client_id: String,
    client_name: Option<String>,
    scopes: Vec<String>,
    created_at: i64,
    pending: bool,
}

pub struct Manage {
    pub edition: Edition,
    /// The metadata lists the settings and security scopes.
    pub offered: bool,
    /// Sign-ins are granted them, and the `x:` methods answer.
    pub granted: bool,
    /// `x:Email/get` (where the limit is) answers, as it does for an administrator.
    pub limit_readable: bool,
    pub max_masked: u32,
    /// The sign-in has fewer rights than the account.
    pub inherit_refused: bool,
    /// `/api/account` lists the permissions for masked addresses.
    pub masked_permissions: bool,
    /// Every `x:` method answers with this method error kind (a failing server, not a refusal).
    pub method_failure: Option<&'static str>,
    /// `/api/account` answers with this HTTP status.
    pub account_status: Option<u16>,
    /// The metadata address answers 503.
    pub metadata_down: bool,
    masked: Vec<Masked>,
    password: String,
    otp_url: Option<String>,
    app_passwords: Vec<Credential>,
    api_keys: Vec<Credential>,
    passkeys: Vec<Passkey>,
    grants: Vec<Grant>,
    next: u64,
    /// What each create asked for as permissions: `(method, permissions)`.
    pub create_permissions: Vec<(String, Value)>,
}

impl Default for Manage {
    fn default() -> Self {
        Manage {
            edition: Edition::Licensed,
            offered: false,
            granted: false,
            limit_readable: true,
            max_masked: 500,
            inherit_refused: false,
            masked_permissions: true,
            method_failure: None,
            account_status: None,
            metadata_down: false,
            masked: Vec::new(),
            password: PASSWORD.into(),
            otp_url: None,
            app_passwords: Vec::new(),
            api_keys: Vec::new(),
            passkeys: Vec::new(),
            grants: Vec::new(),
            next: 0,
            create_permissions: Vec::new(),
        }
    }
}

fn refuse(kind: &'static str, description: &str) -> MethodError {
    MethodError { kind, description: description.to_string() }
}

fn ids_of(args: &Value) -> Option<Vec<String>> {
    args["ids"].as_array().map(|a| a.iter().filter_map(|i| i.as_str().map(str::to_string)).collect())
}

impl Manage {
    fn id(&mut self, prefix: &str) -> String {
        self.next += 1;
        format!("{prefix}{}", self.next)
    }

    // ---- what the tests control ----

    pub fn add_masked(
        &mut self,
        email: &str,
        description: &str,
        for_domain: &str,
        enable: bool,
        created_at: i64,
    ) -> String {
        let id = self.id("m");
        self.masked.push(Masked {
            id: id.clone(),
            email: email.into(),
            description: (!description.is_empty()).then(|| description.to_string()),
            for_domain: (!for_domain.is_empty()).then(|| for_domain.to_string()),
            enable,
            created_at,
        });
        id
    }

    pub fn masked_state(&self) -> Vec<(String, String, bool, Option<String>)> {
        self.masked.iter().map(|m| (m.id.clone(), m.email.clone(), m.enable, m.description.clone())).collect()
    }

    pub fn masked_domain(&self, id: &str) -> Option<String> {
        self.masked.iter().find(|m| m.id == id).and_then(|m| m.for_domain.clone())
    }

    pub fn add_passkey(&mut self, name: &str, created_at: i64) -> String {
        let id = self.id("p");
        self.passkeys.push(Passkey { id: id.clone(), description: name.into(), created_at });
        id
    }

    pub fn passkeys(&self) -> Vec<(String, String)> {
        self.passkeys.iter().map(|p| (p.id.clone(), p.description.clone())).collect()
    }

    pub fn add_grant(&mut self, client_id: &str, name: Option<&str>, scopes: &[&str], pending: bool) -> String {
        let id = self.id("g");
        self.grants.push(Grant {
            id: id.clone(),
            client_id: client_id.into(),
            client_name: name.map(str::to_string),
            scopes: scopes.iter().map(|s| s.to_string()).collect(),
            created_at: now_secs(),
            pending,
        });
        id
    }

    pub fn grant_ids(&self) -> Vec<String> {
        self.grants.iter().map(|g| g.id.clone()).collect()
    }

    /// The sign-in the app just made.
    pub fn app_signed_in(&mut self, client_id: &str) {
        if !self.grants.iter().any(|g| g.client_id == client_id) {
            self.add_grant(
                client_id,
                Some("kweb mail"),
                &["mail", "calendars", "contacts", "settings", "security"],
                // As Stalwart has it once the token was refreshed: the flag stays set.
                true,
            );
        }
    }

    pub fn add_credential(&mut self, api_key: bool, description: &str, expires_at: Option<&str>) -> String {
        let id = self.id(if api_key { "k" } else { "a" });
        let secret = if api_key { format!("API_{id}") } else { format!("app_{id}") };
        let one = Credential {
            id: id.clone(),
            description: description.into(),
            secret,
            created_at: now_secs(),
            expires_at: expires_at.map(str::to_string),
            permissions: json!({ "@type": "Inherit" }),
        };
        if api_key {
            self.api_keys.push(one)
        } else {
            self.app_passwords.push(one)
        }
        id
    }

    pub fn credential_ids(&self, api_key: bool) -> Vec<String> {
        let list = if api_key { &self.api_keys } else { &self.app_passwords };
        list.iter().map(|c| c.id.clone()).collect()
    }

    pub fn credential(&self, api_key: bool, id: &str) -> Option<(String, Option<String>, Value)> {
        let list = if api_key { &self.api_keys } else { &self.app_passwords };
        list.iter().find(|c| c.id == id).map(|c| (c.description.clone(), c.expires_at.clone(), c.permissions.clone()))
    }

    pub fn two_step_url(&self) -> Option<String> {
        self.otp_url.clone()
    }

    pub fn set_two_step_url(&mut self, url: Option<&str>) {
        self.otp_url = url.map(str::to_string);
    }

    pub fn password(&self) -> String {
        self.password.clone()
    }

    // ---- GET /api/account ----

    /// The answer, or `None` for a server that has no such address.
    pub fn account(&self) -> Option<Value> {
        let edition = match self.edition {
            Edition::Licensed => "enterprise",
            Edition::Unlicensed => "community",
            Edition::Unaware => return None,
        };
        let mut permissions: Vec<&str> = vec!["authenticate", "emailReceive", "emailSend"];
        if self.granted {
            if self.masked_permissions {
                permissions.extend([
                    "sysMaskedEmailGet",
                    "sysMaskedEmailQuery",
                    "sysMaskedEmailCreate",
                    "sysMaskedEmailUpdate",
                    "sysMaskedEmailDestroy",
                ]);
            }
            permissions.extend([
                "sysAccountPasswordGet",
                "sysAccountPasswordUpdate",
                "sysAppPasswordGet",
                "sysApiKeyGet",
                "sysPasskeyGet",
                "sysOAuthGrantGet",
            ]);
        }
        Some(json!({ "permissions": permissions, "edition": edition, "locale": "en-US" }))
    }

    // ---- the x: methods ----

    pub fn call(&mut self, name: &str, args: &Value) -> Result<Value, MethodError> {
        if self.edition == Edition::Unaware {
            return Err(refuse("unknownMethod", name));
        }
        if let Some(kind) = self.method_failure {
            return Err(refuse(kind, "the server could not do that now"));
        }
        if !self.granted {
            return Err(refuse("forbidden", NOT_AUTHORIZED));
        }
        let (object, method) =
            name.strip_prefix("x:").and_then(|n| n.split_once('/')).ok_or_else(|| refuse("unknownMethod", name))?;
        match (object, method) {
            ("MaskedEmail", _) => {
                if self.edition == Edition::Unlicensed {
                    return Err(refuse("forbidden", ENTERPRISE));
                }
                self.masked_call(method, args)
            }
            ("Email", "get") => {
                if !self.limit_readable {
                    return Err(refuse("forbidden", NOT_AUTHORIZED));
                }
                Ok(
                    json!({ "accountId": ACCOUNT_ID, "list": [{ "id": "singleton", "maxMaskedAddresses": self.max_masked }], "notFound": [] }),
                )
            }
            ("AccountPassword", "get") => Ok(json!({
                "accountId": ACCOUNT_ID,
                "list": [{ "secret": "****", "otpAuth": if self.otp_url.is_some() { json!({ "otpUrl": "****" }) } else { json!({}) }, "id": "singleton" }],
                "notFound": [],
            })),
            ("AccountPassword", "set") => self.password_set(args),
            ("AppPassword", _) => self.credential_call(false, method, args),
            ("ApiKey", _) => self.credential_call(true, method, args),
            ("Passkey", _) => self.passkey_call(method, args),
            ("OAuthGrant", _) => self.grant_call(method, args),
            _ => Err(refuse("unknownMethod", name)),
        }
    }

    fn masked_json(m: &Masked) -> Value {
        json!({
            "id": m.id, "accountId": ACCOUNT_ID, "email": m.email, "enable": m.enable,
            "description": m.description, "forDomain": m.for_domain, "url": null, "createdBy": null,
            "createdAt": rfc3339(m.created_at), "expiresAt": null,
        })
    }

    fn masked_call(&mut self, method: &str, args: &Value) -> Result<Value, MethodError> {
        match method {
            "query" => {
                let limit = args["limit"].as_u64().map_or(usize::MAX, |l| l as usize);
                let ids: Vec<&str> = self.masked.iter().rev().take(limit).map(|m| m.id.as_str()).collect();
                Ok(
                    json!({ "accountId": ACCOUNT_ID, "queryState": "n", "canCalculateChanges": true, "position": 0, "ids": ids }),
                )
            }
            "get" => {
                let (mut list, mut not_found) = (Vec::new(), Vec::new());
                match ids_of(args) {
                    None => list.extend(self.masked.iter().rev().map(Self::masked_json)),
                    Some(ids) => {
                        for id in ids {
                            match self.masked.iter().find(|m| m.id == id) {
                                Some(m) => list.push(Self::masked_json(m)),
                                None => not_found.push(id),
                            }
                        }
                    }
                }
                Ok(json!({ "accountId": ACCOUNT_ID, "list": list, "notFound": not_found }))
            }
            "set" => Ok(self.masked_set(args)),
            _ => Err(refuse("unknownMethod", &format!("x:MaskedEmail/{method}"))),
        }
    }

    fn masked_set(&mut self, args: &Value) -> Value {
        let (mut created, mut not_created) = (Map::new(), Map::new());
        for (key, new) in args["create"].as_object().into_iter().flatten() {
            let prefix = new["emailPrefix"].as_str().unwrap_or_default().to_string();
            if !prefix.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_') || prefix.len() > 64 {
                not_created.insert(key.clone(), json!({ "type": "invalidProperties", "description": "Invalid value for property.", "properties": ["emailPrefix"] }));
            } else if self.masked.len() as u32 >= self.max_masked {
                let text = format!("You have exceeded your quota of {} masked addresses.", self.max_masked);
                not_created.insert(key.clone(), json!({ "type": "overQuota", "description": text }));
            } else {
                let id = self.id("m");
                let email = format!("{prefix}x7q{}@mask.example.test", self.next);
                self.masked.push(Masked {
                    id: id.clone(),
                    email: email.clone(),
                    description: new["description"].as_str().map(str::to_string),
                    for_domain: new["forDomain"].as_str().map(str::to_string),
                    enable: true,
                    created_at: now_secs(),
                });
                created.insert(key.clone(), json!({ "id": id, "email": email }));
            }
        }
        let (mut updated, mut not_updated) = (Map::new(), Map::new());
        for (id, patch) in args["update"].as_object().into_iter().flatten() {
            match self.masked.iter_mut().find(|m| m.id == *id) {
                None => {
                    not_updated.insert(id.clone(), json!({ "type": "notFound" }));
                }
                Some(m) => {
                    if let Some(on) = patch["enable"].as_bool() {
                        m.enable = on;
                    }
                    if patch.get("description").is_some() {
                        m.description = patch["description"].as_str().map(str::to_string);
                    }
                    updated.insert(id.clone(), Value::Null);
                }
            }
        }
        let (mut destroyed, mut not_destroyed) = (Vec::new(), Map::new());
        for id in args["destroy"].as_array().into_iter().flatten().filter_map(Value::as_str) {
            match self.masked.iter().position(|m| m.id == id) {
                Some(at) => {
                    self.masked.remove(at);
                    destroyed.push(id.to_string());
                }
                None => {
                    not_destroyed.insert(id.to_string(), json!({ "type": "notFound" }));
                }
            }
        }
        json!({ "accountId": ACCOUNT_ID, "created": created, "notCreated": not_created, "updated": updated,
            "notUpdated": not_updated, "destroyed": destroyed, "notDestroyed": not_destroyed })
    }

    fn password_set(&mut self, args: &Value) -> Result<Value, MethodError> {
        let mut not_updated = Map::new();
        if let Some(patch) = args["update"]["singleton"].as_object() {
            let failure = |kind: &str, text: &str| json!({ "type": kind, "description": text });
            let current = patch.get("currentSecret").and_then(Value::as_str);
            let code = patch.get("otpAuth").and_then(|o| o["otpCode"].as_str());
            let error = if current.is_none() {
                Some(failure("forbidden", "Current secret must be provided to change the password or OTP auth."))
            } else if current != Some(self.password.as_str()) {
                Some(failure("forbidden", "Current secret is incorrect."))
            } else if self.otp_url.is_some() && code.is_none() {
                Some(failure("forbidden", "Current OTP code is required to change the password or OTP auth."))
            } else if self.otp_url.is_some() && code != Some(OTP_CODE) {
                Some(failure("forbidden", "Current secret is incorrect."))
            } else if patch.get("secret").and_then(Value::as_str).is_some_and(|s| s.chars().count() < 8) {
                Some(
                    json!({ "type": "invalidProperties", "description": "Password must be at least 8 characters long.", "properties": ["secret"] }),
                )
            } else {
                None
            };
            match error {
                Some(e) => {
                    not_updated.insert("singleton".into(), e);
                }
                None => {
                    if let Some(new) = patch.get("secret").and_then(Value::as_str) {
                        self.password = new.to_string();
                    }
                    if let Some(otp) = patch.get("otpAuth").and_then(|o| o.get("otpUrl")) {
                        self.otp_url = otp.as_str().map(str::to_string);
                    }
                    return Ok(json!({ "accountId": ACCOUNT_ID, "updated": { "singleton": null } }));
                }
            }
        }
        Ok(json!({ "accountId": ACCOUNT_ID, "notUpdated": not_updated }))
    }

    fn credential_call(&mut self, api_key: bool, method: &str, args: &Value) -> Result<Value, MethodError> {
        match method {
            "get" => {
                let list = if api_key { &self.api_keys } else { &self.app_passwords };
                let shown: Vec<Value> = list
                    .iter()
                    .filter(|c| ids_of(args).is_none_or(|ids| ids.contains(&c.id)))
                    .map(|c| {
                        json!({ "id": c.id, "description": c.description, "secret": "****", "createdAt": rfc3339(c.created_at),
                            "expiresAt": c.expires_at, "permissions": c.permissions, "allowedIps": {} })
                    })
                    .collect();
                Ok(json!({ "accountId": ACCOUNT_ID, "list": shown, "notFound": [] }))
            }
            "set" => Ok(self.credential_set(api_key, args)),
            _ => Err(refuse("unknownMethod", &format!("x:Credential/{method}"))),
        }
    }

    fn credential_set(&mut self, api_key: bool, args: &Value) -> Value {
        let method = if api_key { "x:ApiKey/set" } else { "x:AppPassword/set" };
        let (mut created, mut not_created) = (Map::new(), Map::new());
        for (key, new) in args["create"].as_object().into_iter().flatten() {
            self.create_permissions.push((method.to_string(), new["permissions"].clone()));
            let count = if api_key { self.api_keys.len() } else { self.app_passwords.len() };
            let description = new["description"].as_str().unwrap_or_default();
            let error = if description.is_empty() {
                Some(
                    json!({ "type": "invalidPatch", "description": "Invalid value for property.", "properties": ["description"] }),
                )
            } else if self.inherit_refused && new["permissions"]["@type"] == "Inherit" {
                Some(json!({ "type": "forbidden",
                    "description": "You are not authorized to grant permissions: interactAi, sysArchivedItemQuery, sysArchivedItemDestroy" }))
            } else if count >= QUOTA {
                let what = if api_key { "API keys" } else { "app passwords" };
                Some(
                    json!({ "type": "overQuota", "description": format!("You have exceeded your quota of {QUOTA} {what}.") }),
                )
            } else {
                None
            };
            match error {
                Some(e) => {
                    not_created.insert(key.clone(), e);
                }
                None => {
                    let id = self.add_credential(api_key, description, new["expiresAt"].as_str());
                    let list = if api_key { &mut self.api_keys } else { &mut self.app_passwords };
                    if let Some(c) = list.iter_mut().find(|c| c.id == id) {
                        c.permissions = new["permissions"].clone();
                    }
                    let secret = if api_key { format!("API_{id}") } else { format!("app_{id}") };
                    created.insert(key.clone(), json!({ "id": id, "secret": secret }));
                }
            }
        }
        let (mut destroyed, mut not_destroyed) = (Vec::new(), Map::new());
        for id in args["destroy"].as_array().into_iter().flatten().filter_map(Value::as_str) {
            let list = if api_key { &mut self.api_keys } else { &mut self.app_passwords };
            match list.iter().position(|c| c.id == id) {
                Some(at) => {
                    list.remove(at);
                    destroyed.push(id.to_string());
                }
                None => {
                    not_destroyed.insert(id.to_string(), json!({ "type": "notFound" }));
                }
            }
        }
        json!({ "accountId": ACCOUNT_ID, "created": created, "notCreated": not_created, "destroyed": destroyed, "notDestroyed": not_destroyed })
    }

    fn passkey_call(&mut self, method: &str, args: &Value) -> Result<Value, MethodError> {
        match method {
            "get" => {
                let list: Vec<Value> = self
                    .passkeys
                    .iter()
                    .filter(|p| ids_of(args).is_none_or(|ids| ids.contains(&p.id)))
                    .map(|p| json!({ "id": p.id, "description": p.description, "createdAt": rfc3339(p.created_at) }))
                    .collect();
                Ok(json!({ "accountId": ACCOUNT_ID, "list": list, "notFound": [] }))
            }
            "set" => {
                let (mut updated, mut not_updated) = (Map::new(), Map::new());
                for (id, patch) in args["update"].as_object().into_iter().flatten() {
                    match self.passkeys.iter_mut().find(|p| p.id == *id) {
                        Some(p) => {
                            if let Some(name) = patch["description"].as_str() {
                                p.description = name.to_string();
                            }
                            updated.insert(id.clone(), Value::Null);
                        }
                        None => {
                            not_updated.insert(id.clone(), json!({ "type": "notFound" }));
                        }
                    }
                }
                let (mut destroyed, mut not_destroyed) = (Vec::new(), Map::new());
                for id in args["destroy"].as_array().into_iter().flatten().filter_map(Value::as_str) {
                    match self.passkeys.iter().position(|p| p.id == id) {
                        Some(at) => {
                            self.passkeys.remove(at);
                            destroyed.push(id.to_string());
                        }
                        None => {
                            not_destroyed.insert(id.to_string(), json!({ "type": "notFound" }));
                        }
                    }
                }
                Ok(json!({ "accountId": ACCOUNT_ID, "updated": updated, "notUpdated": not_updated,
                    "destroyed": destroyed, "notDestroyed": not_destroyed }))
            }
            _ => Err(refuse("unknownMethod", &format!("x:Passkey/{method}"))),
        }
    }

    fn grant_call(&mut self, method: &str, args: &Value) -> Result<Value, MethodError> {
        match method {
            "get" => {
                let (mut list, mut not_found) = (Vec::new(), Vec::new());
                let wanted = ids_of(args);
                for g in self.grants.iter().filter(|g| wanted.as_ref().is_none_or(|ids| ids.contains(&g.id))) {
                    let scopes: Map<String, Value> = g.scopes.iter().map(|s| (s.clone(), json!(true))).collect();
                    list.push(json!({ "id": g.id, "clientId": g.client_id, "clientName": g.client_name, "clientKind": "dynamic",
                        "scopes": scopes, "createdAt": rfc3339(g.created_at), "lastUsedAt": rfc3339(g.created_at),
                        "generation": 0, "pending": g.pending }));
                }
                for id in wanted.into_iter().flatten().filter(|id| !self.grants.iter().any(|g| g.id == *id)) {
                    not_found.push(id);
                }
                Ok(json!({ "accountId": ACCOUNT_ID, "list": list, "notFound": not_found }))
            }
            "set" => {
                let (mut destroyed, mut not_destroyed) = (Vec::new(), Map::new());
                for id in args["destroy"].as_array().into_iter().flatten().filter_map(Value::as_str) {
                    match self.grants.iter().position(|g| g.id == id) {
                        Some(at) => {
                            self.grants.remove(at);
                            destroyed.push(id.to_string());
                        }
                        None => {
                            not_destroyed.insert(id.to_string(), json!({ "type": "notFound" }));
                        }
                    }
                }
                Ok(json!({ "accountId": ACCOUNT_ID, "destroyed": destroyed, "notDestroyed": not_destroyed }))
            }
            _ => Err(refuse("unknownMethod", &format!("x:OAuthGrant/{method}"))),
        }
    }
}
