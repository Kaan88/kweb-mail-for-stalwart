//! The mock's management side: `GET /api/account`, API keys as bearer tokens, and the `x:`
//! methods the admin functions use (`x:QueuedMessage`, `x:Account`, `x:Domain`,
//! `x:BlockedIp`, `x:Log`, `x:Action`). It behaves like Stalwart 1.1.0 where that matters to the
//! core (see `dev/notes/stalwart-admin-access.md`):
//!
//! - A key is a bearer token that starts with `API_`. A key that is refused (run out or
//!   deleted) is HTTP 401 everywhere. The session of a key names the key's own account.
//! - `/api/account` gives the permission names and the edition ("community" or "enterprise").
//! - A method the key lacks the permission for is a method error `forbidden` with HTTP 200.
//! - A queue query sorts only by `due`; a log query filters only by `text`, which is a
//!   case-sensitive substring, and has no order but its ids; an account query matches whole
//!   words of name and description only, and sorts by `name` or `id`.
//! - An account takes any name but one with an `@` or none; a name that is taken is
//!   `primaryKeyViolation`; a short password is `invalidProperties` on `secret`; a group with
//!   members is `objectIsLinked`; `quotas/maxDiskQuota` and `credentials/0/secret` are patches
//!   of the object, and `null` in a patch removes the key.
//! - A `set` of a thing that is not there is `notFound` under `notUpdated` or `notDestroyed`.
//!
//! One thing the real server is not asked here: the delivery history and the Enterprise
//! objects (another mock part).

use std::collections::{BTreeMap, HashSet};

use axum::body::Bytes;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use serde_json::{json, Value};

use super::state::{now_secs, rfc3339, ACCOUNT_ID, SESSION_STATE, USERNAME};
use super::{json_response, MockServer, Shared};

const KEY_PREFIX: &str = "API_";
/// The domains of the mock server, by id.
pub const DOMAIN_ID: &str = "d1";
pub const OTHER_DOMAIN_ID: &str = "d2";
/// Another user and a group with that user in it.
pub const BOB_ID: &str = "u2";
pub const GROUP_ID: &str = "g1";
const ACTIONS: [&str; 4] = ["ReloadSettings", "ReloadTlsCertificates", "PauseMtaQueue", "ResumeMtaQueue"];

/// What a key may do. `None` is every permission the mock knows, as the real administrator has.
#[derive(Clone)]
pub struct KeyInfo {
    pub username: String,
    pub permissions: Option<HashSet<String>>,
    pub refused: bool,
    /// Turned away with HTTP 403 (a key limited to other addresses, or an account that may not
    /// sign in), as the real server does.
    pub forbidden: bool,
}

/// The server's password rules, which are settings: a shortest and a longest length and a list of
/// words that make a password "too weak" (the real one scores strength). The texts are the
/// real server's three sentences.
#[derive(Clone, Default)]
pub struct PasswordRules {
    pub min: Option<usize>,
    pub max: Option<usize>,
    pub weak: Vec<String>,
}

impl PasswordRules {
    /// The server's description of why `secret` is not allowed.
    fn refusal(&self, secret: &str) -> Option<String> {
        let len = secret.chars().count();
        if let Some(max) = self.max.filter(|m| len > *m) {
            return Some(format!("Password must be at most {max} characters long."));
        }
        if let Some(min) = self.min.filter(|m| len < *m) {
            return Some(format!("Password must be at least {min} characters long."));
        }
        if self.weak.iter().any(|w| secret.contains(w.as_str())) {
            return Some("Password is too weak. Add another word or two.".into());
        }
        None
    }
}

pub struct Admin {
    keys: BTreeMap<String, KeyInfo>,
    next_key: u32,
    pub edition: String,
    accounts: Vec<Value>,
    queue: Vec<Value>,
    blocked: Vec<Value>,
    log: Vec<Value>,
    next_id: u32,
    /// Every management method that was called, in order ("x:Account/set").
    calls: Vec<String>,
    /// The actions that were started, in order.
    pub actions: Vec<String>,
    pub rules: PasswordRules,
    /// Accounts whose credentials live in an outside directory (LDAP, OIDC).
    external: HashSet<String>,
    /// The licence's number of accounts, when it has a limit.
    account_limit: Option<usize>,
}

fn all_permissions() -> HashSet<String> {
    let mut all = HashSet::new();
    for object in
        ["QueuedMessage", "Account", "BlockedIp", "Log", "Domain", "Metric", "Trace", "ArchivedItem", "Alert", "Tenant"]
    {
        for op in ["Query", "Get", "Create", "Update", "Destroy"] {
            all.insert(format!("sys{object}{op}"));
        }
    }
    all.insert("sysActionCreate".into());
    for action in ACTIONS {
        all.insert(format!("action{action}"));
    }
    all
}

impl Default for Admin {
    fn default() -> Self {
        let user = |id: &str, name: &str, domain: &str, description: &str, roles: &str| {
            json!({
                "id": id, "@type": "User", "name": name, "domainId": domain, "description": description,
                "emailAddress": format!("{name}@{}", if domain == DOMAIN_ID { "example.test" } else { "other.test" }),
                "roles": { "@type": roles }, "permissions": { "@type": "Inherit" }, "quotas": {},
                "usedDiskQuota": 0, "aliases": {}, "memberGroupIds": {},
                "credentials": { "0": { "@type": "Password", "secret": "originalpass" } },
            })
        };
        let mut alice = user(ACCOUNT_ID, "alice", DOMAIN_ID, "The administrator", "Admin");
        alice["usedDiskQuota"] = json!(2048);
        let mut bob = user(BOB_ID, "bob", DOMAIN_ID, "Bob Builder", "User");
        bob["memberGroupIds"] = json!({ GROUP_ID: true });
        bob["aliases"] =
            json!({ "0": { "name": "bobby", "domainId": OTHER_DOMAIN_ID, "enable": true, "description": null } });
        bob["quotas"] = json!({ "maxDiskQuota": 1_000_000 });
        let group = json!({
            "id": GROUP_ID, "@type": "Group", "name": "staff", "domainId": DOMAIN_ID,
            "description": "All staff", "emailAddress": "staff@example.test", "roles": { "@type": "Default" },
            "permissions": { "@type": "Inherit" }, "quotas": {}, "aliases": {},
        });
        Admin {
            keys: BTreeMap::new(),
            next_key: 1,
            edition: "community".into(),
            accounts: vec![alice, bob, group],
            queue: Vec::new(),
            blocked: Vec::new(),
            log: Vec::new(),
            next_id: 100,
            calls: Vec::new(),
            actions: Vec::new(),
            rules: PasswordRules { min: Some(8), ..PasswordRules::default() },
            external: HashSet::new(),
            account_limit: None,
        }
    }
}

type MethodResult = Result<Value, (&'static str, String)>;

fn forbidden() -> (&'static str, String) {
    ("forbidden", "You are not authorized to perform this action".into())
}

fn forbidden_write() -> (&'static str, String) {
    ("forbidden", "You are not authorized to create, update or destroy objects of this type".into())
}

/// Sets `value` at the slash-separated `path` of `target`; `null` below the top level removes.
fn patch(target: &mut Value, path: &str, value: &Value) {
    let parts: Vec<&str> = path.split('/').collect();
    let mut at = target;
    for part in &parts[..parts.len() - 1] {
        if !at[*part].is_object() {
            at[*part] = json!({});
        }
        at = &mut at[*part];
    }
    let last = parts[parts.len() - 1];
    if value.is_null() && parts.len() > 1 {
        if let Some(map) = at.as_object_mut() {
            map.remove(last);
        }
    } else {
        at[last] = value.clone();
    }
}

fn str_of<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key].as_str().unwrap_or_default()
}

fn ids_of(args: &Value, all: impl Iterator<Item = String>) -> Vec<String> {
    match args["ids"].as_array() {
        Some(ids) => ids.iter().filter_map(|i| i.as_str().map(str::to_string)).collect(),
        None => all.collect(),
    }
}

/// An object with only the properties asked for (and its id).
fn pick(object: &Value, args: &Value) -> Value {
    let Some(props) = args["properties"].as_array() else { return object.clone() };
    let mut out = json!({ "id": object["id"] });
    for p in props.iter().filter_map(Value::as_str) {
        if let Some(v) = object.get(p) {
            out[p] = v.clone();
        }
    }
    out
}

impl Admin {
    fn id(&mut self, prefix: &str) -> String {
        self.next_id += 1;
        format!("{prefix}{}", self.next_id)
    }

    fn domain_name(id: &str) -> Option<&'static str> {
        match id {
            DOMAIN_ID => Some("example.test"),
            OTHER_DOMAIN_ID => Some("other.test"),
            _ => None,
        }
    }

    /// The permissions a method needs.
    fn needed(name: &str, args: &Value) -> Vec<String> {
        let Some((object, method)) = name.strip_prefix("x:").and_then(|n| n.split_once('/')) else { return Vec::new() };
        let sys = |op: &str| format!("sys{object}{op}");
        match method {
            "query" => vec![sys("Query")],
            "get" => vec![sys("Get")],
            _ if object == "Action" => {
                let mut need = vec![sys("Create")];
                for new in args["create"].as_object().into_iter().flat_map(|c| c.values()) {
                    need.push(format!("action{}", str_of(new, "@type")));
                }
                need
            }
            _ => {
                let mut need = Vec::new();
                for (key, op) in [("create", "Create"), ("update", "Update"), ("destroy", "Destroy")] {
                    if !args[key].is_null() {
                        need.push(sys(op));
                    }
                }
                need
            }
        }
    }

    fn handle(&mut self, key: &KeyInfo, name: &str, args: &Value) -> MethodResult {
        self.calls.push(name.to_string());
        let need = Self::needed(name, args);
        if let Some(have) = &key.permissions {
            if need.iter().any(|n| !have.contains(n)) {
                return Err(if name.ends_with("/set") { forbidden_write() } else { forbidden() });
            }
        }
        let account = if key.username == USERNAME { ACCOUNT_ID } else { "other" };
        let mut answer = match name {
            "x:QueuedMessage/query" => self.queue_query(args)?,
            "x:QueuedMessage/get" => self.queue_get(args),
            "x:QueuedMessage/set" => self.queue_set(args),
            "x:Account/query" => self.account_query(args)?,
            "x:Account/get" => self.account_get(args),
            "x:Account/set" => self.account_set(args),
            "x:Domain/get" => self.domain_get(args),
            "x:BlockedIp/query" => self.simple_query(&self.blocked.clone(), args),
            "x:BlockedIp/get" => Self::simple_get(&self.blocked, args),
            "x:BlockedIp/set" => Self::simple_destroy(&mut self.blocked, args),
            "x:Log/query" => self.log_query(args)?,
            "x:Log/get" => Self::simple_get(&self.log, args),
            "x:Action/set" => self.action_set(args),
            _ => return Err(("unknownMethod", name.to_string())),
        };
        answer["accountId"] = json!(account);
        Ok(answer)
    }

    // ---- the queue ----

    fn queue_query(&self, args: &Value) -> MethodResult {
        if let Some(sort) = args["sort"].as_array() {
            if sort.iter().any(|s| s["property"] != "due") {
                return Err(("unsupportedSort", "Only sorting by 'due' is supported for queued messages".into()));
            }
        }
        let mut items: Vec<&Value> = self.queue.iter().collect();
        items.sort_by_key(|m| m["nextRetry"].as_str().map(str::to_string).unwrap_or_else(|| "9999".into()));
        Ok(window(items.into_iter().map(|m| str_of(m, "id").to_string()).collect(), args))
    }

    fn queue_get(&self, args: &Value) -> Value {
        Self::simple_get(&self.queue, args)
    }

    fn queue_set(&mut self, args: &Value) -> Value {
        let mut out = json!({});
        for (id, change) in args["update"].as_object().into_iter().flatten() {
            match self.queue.iter_mut().find(|m| str_of(m, "id") == id) {
                Some(m) => {
                    for (k, v) in change.as_object().into_iter().flatten() {
                        patch(m, k, v);
                    }
                    out["updated"][id] = Value::Null;
                }
                None => out["notUpdated"][id] = json!({ "type": "notFound" }),
            }
        }
        for id in args["destroy"].as_array().into_iter().flatten().filter_map(Value::as_str) {
            match self.queue.iter().position(|m| str_of(m, "id") == id) {
                Some(at) => {
                    self.queue.remove(at);
                    out["destroyed"] = json!([id]);
                }
                None => out["notDestroyed"][id] = json!({ "type": "notFound" }),
            }
        }
        out
    }

    // ---- accounts ----

    fn account_query(&self, args: &Value) -> MethodResult {
        let filter = &args["filter"];
        if let Some(f) = filter.as_object() {
            if let Some(bad) = f.keys().find(|k| *k != "text" && *k != "domainId") {
                return Err(("unsupportedFilter", bad.clone()));
            }
        }
        let mut items: Vec<&Value> = self.accounts.iter().collect();
        if let Some(text) = filter["text"].as_str() {
            // Whole words of the name and the description, without regard to case.
            let words = |v: &Value| {
                format!("{} {}", str_of(v, "name"), str_of(v, "description"))
                    .to_lowercase()
                    .split_whitespace()
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            };
            let want = text.to_lowercase();
            items.retain(|a| words(a).contains(&want));
        }
        match args["sort"][0]["property"].as_str() {
            Some("name") => items.sort_by_key(|a| str_of(a, "name").to_string()),
            Some("id") | None => {}
            Some(other) => return Err(("unsupportedSort", format!("Property {other} is not supported for sorting"))),
        }
        Ok(window(items.into_iter().map(|a| str_of(a, "id").to_string()).collect(), args))
    }

    fn account_get(&self, args: &Value) -> Value {
        let mut out = Self::simple_get(&self.accounts, args);
        for item in out["list"].as_array_mut().into_iter().flatten() {
            // Secrets are never given back; a permission list always has both lists.
            for credential in item["credentials"].as_object_mut().into_iter().flat_map(|c| c.values_mut()) {
                credential["secret"] = json!("****");
            }
            if matches!(item["permissions"]["@type"].as_str(), Some("Merge") | Some("Replace")) {
                for list in ["enabledPermissions", "disabledPermissions"] {
                    if item["permissions"][list].is_null() {
                        item["permissions"][list] = json!({});
                    }
                }
            }
        }
        out
    }

    fn account_set(&mut self, args: &Value) -> Value {
        let mut out = json!({});
        for (client_id, new) in args["create"].as_object().into_iter().flatten() {
            match self.account_create(new) {
                Ok(id) => out["created"][client_id] = json!({ "id": id }),
                Err(e) => out["notCreated"][client_id] = e,
            }
        }
        for (id, change) in args["update"].as_object().into_iter().flatten() {
            let Some(at) = self.accounts.iter().position(|a| str_of(a, "id") == id) else {
                out["notUpdated"][id] = json!({ "type": "notFound" });
                continue;
            };
            let mut next = self.accounts[at].clone();
            for (k, v) in change.as_object().into_iter().flatten() {
                patch(&mut next, k, v);
            }
            let touches_secret = change.as_object().is_some_and(|c| c.keys().any(|k| k.ends_with("/secret")));
            if touches_secret && self.external.contains(id) {
                out["notUpdated"][id] = json!({
                    "type": "forbidden",
                    "description": "Cannot change credentials for accounts in an external directory.",
                });
                continue;
            }
            if let Some(description) = touches_secret
                .then(|| next["credentials"]["0"]["secret"].as_str())
                .flatten()
                .and_then(|s| self.rules.refusal(s))
            {
                out["notUpdated"][id] =
                    json!({ "type": "invalidProperties", "description": description, "properties": ["secret"] });
                continue;
            }
            self.accounts[at] = next;
            out["updated"][id] = Value::Null;
        }
        for id in args["destroy"].as_array().into_iter().flatten().filter_map(Value::as_str) {
            let Some(at) = self.accounts.iter().position(|a| str_of(a, "id") == id) else {
                out["notDestroyed"][id] = json!({ "type": "notFound" });
                continue;
            };
            let members: Vec<Value> = self
                .accounts
                .iter()
                .filter(|a| a["memberGroupIds"].get(id).is_some())
                .map(|a| json!({ "object": "Account", "id": a["id"] }))
                .collect();
            if !members.is_empty() {
                out["notDestroyed"][id] = json!({
                    "type": "objectIsLinked", "objectId": { "object": "Account", "id": id }, "linkedObjects": members,
                });
                continue;
            }
            self.accounts.remove(at);
            out["destroyed"] = json!([id]);
        }
        out
    }

    fn account_create(&mut self, new: &Value) -> Result<String, Value> {
        let name = str_of(new, "name");
        let domain = str_of(new, "domainId");
        let secret = new["credentials"]["0"]["secret"].as_str().unwrap_or_default();
        if name.is_empty() || name.contains('@') {
            return Err(
                json!({ "type": "invalidPatch", "description": "Invalid email local part", "properties": ["name"] }),
            );
        }
        let Some(domain_name) = Self::domain_name(domain) else {
            return Err(json!({ "type": "invalidForeignKey", "objectId": { "object": "Domain", "id": domain } }));
        };
        if let Some(limit) = self.account_limit.filter(|l| self.accounts.len() >= *l) {
            return Err(json!({
                "type": "forbidden", "description": format!("Enterprise licensed account limit reached: {limit} accounts licensed."),
            }));
        }
        if let Some(description) = self.rules.refusal(secret) {
            return Err(json!({ "type": "invalidProperties", "description": description, "properties": ["secret"] }));
        }
        let email = format!("{name}@{domain_name}");
        if let Some(taken) = self.accounts.iter().find(|a| str_of(a, "emailAddress") == email) {
            return Err(json!({
                "type": "primaryKeyViolation", "properties": ["email"],
                "objectId": { "object": "Account", "id": taken["id"] },
            }));
        }
        let id = self.id("u");
        let mut account = json!({
            "id": id, "@type": "User", "name": name, "domainId": domain, "emailAddress": email,
            "description": new["description"], "roles": new["roles"], "permissions": { "@type": "Inherit" },
            "quotas": {}, "usedDiskQuota": 0, "aliases": {}, "memberGroupIds": {},
            "credentials": new["credentials"],
        });
        account["createdAt"] = json!(rfc3339(now_secs()));
        self.accounts.push(account);
        Ok(id)
    }

    fn domain_get(&self, args: &Value) -> Value {
        let domains = [(DOMAIN_ID, "example.test"), (OTHER_DOMAIN_ID, "other.test")];
        let wanted = ids_of(args, domains.iter().map(|(id, _)| id.to_string()));
        json!({ "list": domains.iter().filter(|(id, _)| wanted.iter().any(|w| w == id)).map(|(id, name)| json!({ "id": id, "name": name })).collect::<Vec<_>>(), "notFound": [] })
    }

    // ---- logs and the rest ----

    fn log_query(&self, args: &Value) -> MethodResult {
        if let Some(f) = args["filter"].as_object() {
            if let Some(bad) = f.keys().find(|k| *k != "text") {
                return Err(("unsupportedFilter", bad.clone()));
            }
        }
        if args["sort"][0]["property"].as_str().is_some_and(|p| p != "id") {
            return Err(("unsupportedSort", "Only sorting by 'id' is supported for logs".into()));
        }
        let text = args["filter"]["text"].as_str();
        let ids = self
            .log
            .iter()
            .filter(|l| text.is_none_or(|t| format!("{} {}", str_of(l, "event"), str_of(l, "details")).contains(t)))
            .map(|l| str_of(l, "id").to_string())
            .collect();
        let mut out = window(ids, args);
        // The real server does not page by position; it gives no total either.
        out.as_object_mut().map(|o| o.remove("total"));
        Ok(out)
    }

    fn simple_query(&self, items: &[Value], args: &Value) -> Value {
        window(items.iter().map(|i| str_of(i, "id").to_string()).collect(), args)
    }

    fn simple_get(items: &[Value], args: &Value) -> Value {
        let wanted = ids_of(args, items.iter().map(|i| str_of(i, "id").to_string()));
        let list: Vec<Value> = wanted
            .iter()
            .filter_map(|id| items.iter().find(|i| str_of(i, "id") == id))
            .map(|i| pick(i, args))
            .collect();
        let missing: Vec<&String> =
            wanted.iter().filter(|id| !items.iter().any(|i| str_of(i, "id") == id.as_str())).collect();
        json!({ "list": list, "notFound": missing })
    }

    fn simple_destroy(items: &mut Vec<Value>, args: &Value) -> Value {
        let mut out = json!({});
        for id in args["destroy"].as_array().into_iter().flatten().filter_map(Value::as_str) {
            match items.iter().position(|i| str_of(i, "id") == id) {
                Some(at) => {
                    items.remove(at);
                    out["destroyed"] = json!([id]);
                }
                None => out["notDestroyed"][id] = json!({ "type": "notFound" }),
            }
        }
        out
    }

    fn action_set(&mut self, args: &Value) -> Value {
        let mut out = json!({});
        for (client_id, new) in args["create"].as_object().into_iter().flatten() {
            let kind = str_of(new, "@type");
            if ACTIONS.contains(&kind) {
                self.actions.push(kind.to_string());
                let id = self.id("act");
                out["created"][client_id] = json!({ "id": id });
            } else {
                out["notCreated"][client_id] = json!({ "type": "invalidProperties", "description": "Invalid action" });
            }
        }
        out
    }
}

/// The answer of a query: the ids from `position` on, at most `limit`, and the total when asked.
fn window(ids: Vec<String>, args: &Value) -> Value {
    let total = ids.len();
    let position = args["position"].as_u64().unwrap_or(0) as usize;
    let limit = args["limit"].as_u64().map_or(5000, |l| l as usize).min(5000);
    let page: Vec<String> = ids.into_iter().skip(position).take(limit).collect();
    let mut out = json!({ "queryState": "n", "position": position, "ids": page });
    if args["calculateTotal"] == true {
        out["total"] = json!(total);
    }
    out
}

// ---- HTTP ----

fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers.get(header::AUTHORIZATION)?.to_str().ok()?.strip_prefix("Bearer ")
}

/// What the session of a key is, or the refusal. `None` when the bearer is not an API key.
fn key_of(s: &Shared, headers: &HeaderMap) -> Option<Result<KeyInfo, Response>> {
    let token = bearer(headers).filter(|t| t.starts_with(KEY_PREFIX))?;
    // The Enterprise mock has a key of its own with the same look.
    if super::enterprise::key_accepted(s, token) {
        return None;
    }
    let admin = s.admin.lock().unwrap();
    Some(match admin.keys.get(token) {
        Some(key) if key.forbidden => Err(super::json_response(
            StatusCode::FORBIDDEN,
            json!({ "type": "about:blank", "status": 403, "title": "Forbidden",
                "detail": "You do not have enough permissions to access this resource." }),
        )),
        Some(key) if !key.refused => Ok(key.clone()),
        _ => Err(super::unauthorized()),
    })
}

/// The session for a key. `None` for any other bearer.
pub(super) fn session(s: &Shared, headers: &HeaderMap) -> Option<Response> {
    let key = match key_of(s, headers)? {
        Ok(key) => key,
        Err(refusal) => return Some(refusal),
    };
    let account = if key.username == USERNAME { ACCOUNT_ID } else { "other" };
    let b = &s.base;
    let mail = "urn:ietf:params:jmap:mail";
    let core = "urn:ietf:params:jmap:core";
    Some(json_response(
        StatusCode::OK,
        json!({
            "capabilities": {
                core: {
                    "maxSizeUpload": 50_000_000, "maxConcurrentUpload": 4, "maxSizeRequest": 10_000_000,
                    "maxConcurrentRequests": 4, "maxCallsInRequest": 16, "maxObjectsInGet": 500,
                    "maxObjectsInSet": 500, "collationAlgorithms": ["i;ascii-casemap"],
                },
                mail: {},
                "urn:stalwart:jmap": {},
            },
            "accounts": { account: {
                "name": key.username, "isPersonal": true, "isReadOnly": false,
                "accountCapabilities": { mail: { "maxMailboxesPerEmail": null, "maxMailboxDepth": 10, "maxSizeMailboxName": 255,
                    "maxSizeAttachmentsPerEmail": 50_000_000, "emailQuerySortOptions": ["receivedAt"], "mayCreateTopLevelMailbox": true } },
            } },
            "primaryAccounts": { mail: account },
            "username": key.username,
            "apiUrl": format!("{b}/jmap"),
            "downloadUrl": format!("{b}/jmap/download/{{accountId}}/{{blobId}}/{{name}}?accept={{type}}"),
            "uploadUrl": format!("{b}/jmap/upload/{{accountId}}"),
            "eventSourceUrl": format!("{b}/eventsource?types={{types}}&closeafter={{closeafter}}&ping={{ping}}"),
            "state": SESSION_STATE,
        }),
    ))
}

/// A JMAP request made with a key. `None` for any other bearer.
pub(super) fn jmap(s: &Shared, headers: &HeaderMap, body: &Bytes) -> Option<Response> {
    let key = match key_of(s, headers)? {
        Ok(key) => key,
        Err(refusal) => return Some(refusal),
    };
    let Ok(req) = serde_json::from_slice::<Value>(body) else {
        return Some((StatusCode::BAD_REQUEST, "not JSON").into_response());
    };
    let mut responses = Vec::new();
    let mut admin = s.admin.lock().unwrap();
    for call in req["methodCalls"].as_array().into_iter().flatten() {
        let (name, args, id) = (str_of_call(&call[0]), &call[1], str_of_call(&call[2]));
        match admin.handle(&key, name, args) {
            Ok(answer) => responses.push(json!([name, answer, id])),
            Err((kind, description)) => {
                responses.push(json!(["error", { "type": kind, "description": description }, id]))
            }
        }
    }
    Some(json_response(StatusCode::OK, json!({ "methodResponses": responses, "sessionState": SESSION_STATE })))
}

fn str_of_call(v: &Value) -> &str {
    v.as_str().unwrap_or_default()
}

/// `GET /api/account`: the permissions and the edition. A key is looked up; an OAuth access
/// token is a plain user with no admin permission.
pub(super) async fn account_route(
    axum::extract::State(s): axum::extract::State<std::sync::Arc<Shared>>,
    headers: HeaderMap,
) -> Response {
    let (permissions, edition) = match key_of(&s, &headers) {
        Some(Ok(key)) => {
            let mut names: Vec<String> = key.permissions.unwrap_or_else(all_permissions).into_iter().collect();
            names.push("authenticate".into());
            names.sort();
            (names, s.admin.lock().unwrap().edition.clone())
        }
        Some(Err(refusal)) => return refusal,
        None => {
            if let Err(r) = super::check_bearer(&s, &headers) {
                return r;
            }
            (vec!["authenticate".into(), "emailReceive".into()], s.admin.lock().unwrap().edition.clone())
        }
    };
    json_response(StatusCode::OK, json!({ "permissions": permissions, "edition": edition, "locale": "en-US" }))
}

/// Controls for the tests: what the server holds and who has a key.
impl MockServer {
    fn with_admin<T>(&self, f: impl FnOnce(&mut Admin) -> T) -> T {
        f(&mut self.shared.admin.lock().unwrap())
    }

    /// A new API key of `username`, with every permission (`None`) or only these.
    pub fn admin_key(&self, username: &str, permissions: Option<&[&str]>) -> String {
        self.with_admin(|a| {
            let key = format!("{KEY_PREFIX}{:08}key", a.next_key);
            a.next_key += 1;
            let permissions = permissions.map(|p| p.iter().map(|s| s.to_string()).collect());
            a.keys.insert(
                key.clone(),
                KeyInfo { username: username.to_string(), permissions, refused: false, forbidden: false },
            );
            key
        })
    }

    /// A key that is turned down from now on (run out, or deleted).
    pub fn refuse_admin_key(&self, key: &str) {
        self.with_admin(|a| {
            if let Some(k) = a.keys.get_mut(key) {
                k.refused = true;
            }
        });
    }

    /// A key the server answers with HTTP 403 from now on (limited to other addresses, or its
    /// account may not sign in).
    pub fn forbid_admin_key(&self, key: &str) {
        self.with_admin(|a| {
            if let Some(k) = a.keys.get_mut(key) {
                k.forbidden = true;
            }
        });
    }

    /// The server's password settings: shortest, longest, and words that make a password weak.
    pub fn set_password_rules(&self, min: Option<usize>, max: Option<usize>, weak: &[&str]) {
        self.with_admin(|a| a.rules = PasswordRules { min, max, weak: weak.iter().map(|w| w.to_string()).collect() });
    }

    /// An account whose password lives in an outside directory.
    pub fn set_account_external(&self, id: &str) {
        self.with_admin(|a| {
            a.external.insert(id.to_string());
        });
    }

    /// The licence allows only `limit` accounts.
    pub fn set_account_limit(&self, limit: usize) {
        self.with_admin(|a| a.account_limit = Some(limit));
    }

    /// "enterprise" is a licensed server; "community" the default.
    pub fn set_edition(&self, edition: &str) {
        self.with_admin(|a| a.edition = edition.to_string());
    }

    /// Puts a message in the queue and gives its id. `message` has what the server has:
    /// `returnPath`, `size`, `createdAt`, `nextRetry`, `recipients`.
    pub fn queue_add(&self, mut message: Value) -> String {
        self.with_admin(|a| {
            let id = a.id("q");
            message["id"] = json!(id);
            a.queue.push(message);
            id
        })
    }

    pub fn queue_ids(&self) -> Vec<String> {
        self.with_admin(|a| a.queue.iter().map(|m| str_of(m, "id").to_string()).collect())
    }

    pub fn queue_message(&self, id: &str) -> Option<Value> {
        self.with_admin(|a| a.queue.iter().find(|m| str_of(m, "id") == id).cloned())
    }

    pub fn block_add(&self, address: &str, reason: &str, created: i64, expires: Option<i64>) -> String {
        self.with_admin(|a| {
            let id = a.id("b");
            a.blocked.push(json!({
                "id": id, "address": address, "reason": reason, "createdAt": rfc3339(created),
                "expiresAt": expires.map(rfc3339),
            }));
            id
        })
    }

    pub fn blocked_ids(&self) -> Vec<String> {
        self.with_admin(|a| a.blocked.iter().map(|b| str_of(b, "id").to_string()).collect())
    }

    /// Adds a log line; lines are in the order given, which is the server's id order.
    pub fn log_add(&self, at: i64, level: &str, event: &str, details: &str) {
        self.with_admin(|a| {
            let id = a.id("l");
            a.log.push(
                json!({ "id": id, "timestamp": rfc3339(at), "level": level, "event": event, "details": details }),
            );
        });
    }

    /// The account the server holds, as the server has it.
    pub fn server_account(&self, id: &str) -> Option<Value> {
        self.with_admin(|a| a.accounts.iter().find(|x| str_of(x, "id") == id).cloned())
    }

    /// Sets a property of an account as it is held, for a test that needs an account made so.
    pub fn set_server_account_field(&self, id: &str, field: &str, value: Value) {
        self.with_admin(|a| {
            if let Some(x) = a.accounts.iter_mut().find(|x| str_of(x, "id") == id) {
                x[field] = value;
            }
        });
    }

    pub fn server_account_by_name(&self, name: &str) -> Option<Value> {
        self.with_admin(|a| a.accounts.iter().find(|x| str_of(x, "name") == name).cloned())
    }

    pub fn server_account_count(&self) -> usize {
        self.with_admin(|a| a.accounts.len())
    }

    /// Makes `count` more users named `user<n>`, for a list that needs paging.
    pub fn add_users(&self, count: usize) {
        self.with_admin(|a| {
            for n in 0..count {
                let id = a.id("u");
                let name = format!("user{n:03}");
                a.accounts.push(json!({
                    "id": id, "@type": "User", "name": name, "domainId": DOMAIN_ID,
                    "emailAddress": format!("{name}@example.test"), "description": "Bulk user",
                    "roles": { "@type": "User" }, "permissions": { "@type": "Inherit" }, "quotas": {},
                    "usedDiskQuota": 0, "aliases": {}, "memberGroupIds": {},
                    "credentials": { "0": { "@type": "Password", "secret": "originalpass" } },
                }));
            }
        });
    }

    /// The management methods called so far, in order ("x:Account/set").
    pub fn admin_calls(&self) -> Vec<String> {
        self.with_admin(|a| a.calls.clone())
    }

    /// The actions started so far, in order ("PauseMtaQueue").
    pub fn admin_actions(&self) -> Vec<String> {
        self.with_admin(|a| a.actions.clone())
    }
}
