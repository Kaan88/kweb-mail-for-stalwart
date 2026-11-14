//! What the mock knows about file storage: `FileNode/get`, `/query` and `/set`, and the blobs
//! behind the nodes. It behaves like Stalwart 1.1.0 where that matters to the core (see
//! `dev/notes/stalwart-files.md`):
//!
//! - There is no root node: a top-level node has no parent. A directory has `myRights`, a file
//!   has none.
//! - A query takes `parentId`, `isTopLevel` and `text` filters and the sorts `nodeType` and
//!   `name`; anything else is `unsupportedFilter` or `unsupportedSort`. It gives no more ids
//!   than its limit and no more than `query_max`, and a get of more than `get_max` ids is
//!   `requestTooLarge`.
//! - A node made from an uploaded blob gets a blob of its own (a new id); the create answer does
//!   not echo `blobId`. The server does not bump `modified` on a rename or a new blob.
//! - Names are case-sensitive; a name that is taken is `alreadyExists` with `existingId`.
//! - Three accounts besides the user's own: one shares a folder, one has the file capability and
//!   shares nothing (`accountNotSupportedByMethod`, as the real server answers), one has no file
//!   capability at all.

use std::collections::{BTreeMap, HashMap};

use chrono::{DateTime, SecondsFormat};
use serde_json::{json, Map, Value};

/// Finds an uploaded blob by id: its media type and bytes.
type MailBlob<'a> = dyn Fn(&str) -> Option<(String, Vec<u8>)> + 'a;

use super::state::{now_secs, MethodError, ACCOUNT_ID};

/// The account that shares a folder with the user.
pub const SHARED_ACCOUNT: &str = "sh1";
/// An account that has the file capability and shares no file.
pub const EMPTY_SHARE_ACCOUNT: &str = "sh2";
/// An account with no file capability.
pub const MAIL_ONLY_ACCOUNT: &str = "sh3";

const FORBIDDEN_CHARS: &str = "/<>:\"\\|?*";
const RESERVED: [&str; 4] = ["CON", "PRN", "AUX", "NUL"];
const EARLY: &str = "2020-01-01T00:00:00Z";

struct Node {
    id: String,
    parent: Option<String>,
    name: String,
    dir: bool,
    /// The blob behind a file (an id of the file blobs).
    blob: Option<String>,
    media_type: String,
    modified: String,
    /// The rights of a directory, by the JMAP names.
    rights: Vec<&'static str>,
}

struct Space {
    name: &'static str,
    personal: bool,
    /// The account has the file capability in the session.
    capable: bool,
    /// The account answers the file methods (false: `accountNotSupportedByMethod`).
    serves: bool,
    may_top: bool,
    nodes: Vec<Node>,
    next: u64,
}

pub struct Files {
    pub enabled: bool,
    spaces: BTreeMap<String, Space>,
    /// Blob id to (media type, content).
    blobs: HashMap<String, (String, Vec<u8>)>,
    next_blob: u64,
    /// `(account in the URL, bytes)` of every upload received.
    pub uploads: Vec<(String, usize)>,
    /// The accounts of the download URLs received, in order.
    pub downloads: Vec<String>,
    /// The next upload is refused with HTTP 429 and this `Retry-After`.
    pub upload_429: Option<u64>,
    /// The next create, update or destroy fails with this `(type, description)`.
    pub next_set_error: Option<(&'static str, String)>,
    pub get_max: usize,
    pub query_max: usize,
}

const ALL: [&str; 7] =
    ["mayRead", "mayAddChildren", "mayRename", "mayDelete", "mayModifyContent", "mayShare", "maySetMetadata"];

fn now_string() -> String {
    DateTime::from_timestamp(now_secs(), 0).map(|t| t.to_rfc3339_opts(SecondsFormat::Secs, true)).unwrap_or_default()
}

fn merr(kind: &'static str, description: impl Into<String>) -> MethodError {
    MethodError { kind, description: description.into() }
}

fn set_error(kind: &str, description: &str, properties: &[&str]) -> Value {
    let mut e = json!({ "type": kind, "description": description });
    if !properties.is_empty() {
        e["properties"] = json!(properties);
    }
    e
}

impl Default for Files {
    fn default() -> Self {
        let space = |name, personal, capable, serves, may_top| Space {
            name,
            personal,
            capable,
            serves,
            may_top,
            nodes: Vec::new(),
            next: 1,
        };
        let mut spaces = BTreeMap::new();
        spaces.insert(ACCOUNT_ID.to_string(), space(super::state::USERNAME, true, true, true, true));
        spaces.insert(SHARED_ACCOUNT.to_string(), space("shared@kweb.test", false, true, true, false));
        spaces.insert(EMPTY_SHARE_ACCOUNT.to_string(), space("empty@kweb.test", false, true, false, false));
        spaces.insert(MAIL_ONLY_ACCOUNT.to_string(), space("mail@kweb.test", false, false, false, false));
        Files {
            enabled: false,
            spaces,
            blobs: HashMap::new(),
            next_blob: 1,
            uploads: Vec::new(),
            downloads: Vec::new(),
            upload_429: None,
            next_set_error: None,
            get_max: 500,
            query_max: 5000,
        }
    }
}

impl Files {
    /// Turns the file storage on and lays out what the shared account shares: a folder `Shared`
    /// (id 1: the user may read, add and change content), in it a folder `ReadOnly` (id 2: read
    /// only) and a file `hello.txt` (id 3).
    pub fn enable(&mut self) {
        if self.enabled {
            return;
        }
        self.enabled = true;
        let shared =
            self.add_folder(SHARED_ACCOUNT, None, "Shared", Some(&["mayRead", "mayAddChildren", "mayModifyContent"]));
        self.add_folder(SHARED_ACCOUNT, Some(&shared), "ReadOnly", Some(&["mayRead"]));
        self.add_file(SHARED_ACCOUNT, Some(&shared), "hello.txt", b"hello", "text/plain");
    }

    /// The accounts of the session besides the user's own, as `(id, account object)`.
    pub fn session_accounts(&self) -> Vec<(String, Value)> {
        self.spaces
            .iter()
            .filter(|(id, s)| id.as_str() != ACCOUNT_ID && self.enabled && (s.capable || *id == MAIL_ONLY_ACCOUNT))
            .map(|(id, s)| {
                let caps = if s.capable {
                    json!({ super::FILES_URN: { "mayCreateTopLevelFileNode": false } })
                } else {
                    json!({ "urn:ietf:params:jmap:mail": {} })
                };
                (
                    id.clone(),
                    json!({ "name": s.name, "isPersonal": false, "isReadOnly": true, "accountCapabilities": caps }),
                )
            })
            .collect()
    }

    pub fn own_may_top(&self) -> bool {
        self.spaces[ACCOUNT_ID].may_top
    }

    pub fn set_own_may_top(&mut self, allowed: bool) {
        if let Some(s) = self.spaces.get_mut(ACCOUNT_ID) {
            s.may_top = allowed;
        }
    }

    fn space(&mut self, account: &str) -> &mut Space {
        self.spaces.get_mut(account).unwrap_or_else(|| panic!("the mock has no account {account}"))
    }

    fn new_blob(&mut self, media_type: &str, bytes: Vec<u8>) -> String {
        let id = format!("fblob{}", self.next_blob);
        self.next_blob += 1;
        self.blobs.insert(id.clone(), (media_type.to_string(), bytes));
        id
    }

    pub fn blob(&self, id: &str) -> Option<(String, Vec<u8>)> {
        self.blobs.get(id).cloned()
    }

    /// Whether the account is one the mock lists (an upload or download URL may name it).
    pub fn knows(&self, account: &str) -> bool {
        self.spaces.contains_key(account)
    }

    /// `rights`: the rights of the folder; `None` is every right (own space) or read, add and
    /// change content (a shared one).
    pub fn add_folder(
        &mut self,
        account: &str,
        parent: Option<&str>,
        name: &str,
        rights: Option<&[&'static str]>,
    ) -> String {
        let personal = self.space(account).personal;
        let rights = rights.map(<[_]>::to_vec).unwrap_or_else(|| {
            if personal {
                ALL.to_vec()
            } else {
                vec!["mayRead", "mayAddChildren", "mayModifyContent"]
            }
        });
        let space = self.space(account);
        let id = space.next.to_string();
        space.next += 1;
        space.nodes.push(Node {
            id: id.clone(),
            parent: parent.map(str::to_string),
            name: name.to_string(),
            dir: true,
            blob: None,
            media_type: String::new(),
            modified: EARLY.to_string(),
            rights,
        });
        id
    }

    pub fn add_file(
        &mut self,
        account: &str,
        parent: Option<&str>,
        name: &str,
        bytes: &[u8],
        media_type: &str,
    ) -> String {
        let blob = self.new_blob(media_type, bytes.to_vec());
        let space = self.space(account);
        let id = space.next.to_string();
        space.next += 1;
        space.nodes.push(Node {
            id: id.clone(),
            parent: parent.map(str::to_string),
            name: name.to_string(),
            dir: false,
            blob: Some(blob),
            media_type: media_type.to_string(),
            modified: EARLY.to_string(),
            rights: Vec::new(),
        });
        id
    }

    pub fn set_rights(&mut self, account: &str, id: &str, rights: &[&'static str]) {
        if let Some(n) = self.space(account).nodes.iter_mut().find(|n| n.id == id) {
            n.rights = rights.to_vec();
        }
    }

    pub fn set_modified(&mut self, account: &str, id: &str, at: &str) {
        if let Some(n) = self.space(account).nodes.iter_mut().find(|n| n.id == id) {
            n.modified = at.to_string();
        }
    }

    /// Removes a node and what is under it, as another client would.
    pub fn remove(&mut self, account: &str, id: &str) {
        let space = self.space(account);
        let mut gone = vec![id.to_string()];
        let mut i = 0;
        while i < gone.len() {
            let parent = gone[i].clone();
            gone.extend(space.nodes.iter().filter(|n| n.parent.as_deref() == Some(&parent)).map(|n| n.id.clone()));
            i += 1;
        }
        space.nodes.retain(|n| !gone.contains(&n.id));
    }

    /// Every node of the account as the test would want to see it.
    pub fn dump(&self, account: &str) -> Vec<Value> {
        self.spaces[account].nodes.iter().map(|n| self.node_json(n, None)).collect()
    }

    pub fn content(&self, account: &str, id: &str) -> Option<Vec<u8>> {
        let node = self.spaces[account].nodes.iter().find(|n| n.id == id)?;
        Some(self.blobs.get(node.blob.as_ref()?)?.1.clone())
    }

    fn node_json(&self, n: &Node, properties: Option<&[String]>) -> Value {
        let mut all = Map::new();
        all.insert("id".into(), json!(n.id));
        all.insert("@type".into(), json!(if n.dir { "Directory" } else { "File" }));
        all.insert("parentId".into(), json!(n.parent));
        all.insert("name".into(), json!(n.name));
        all.insert("created".into(), json!(EARLY));
        all.insert("modified".into(), json!(n.modified));
        if n.dir {
            let rights: Map<String, Value> = ALL.iter().map(|r| (r.to_string(), json!(n.rights.contains(r)))).collect();
            all.insert("myRights".into(), Value::Object(rights));
        } else {
            let (media_type, bytes) = n.blob.as_ref().and_then(|b| self.blobs.get(b)).cloned().unwrap_or_default();
            all.insert("size".into(), json!(bytes.len()));
            all.insert("blobId".into(), json!(n.blob));
            all.insert(
                "mediaType".into(),
                json!(if n.media_type.is_empty() { media_type } else { n.media_type.clone() }),
            );
        }
        match properties {
            None => Value::Object(all),
            Some(wanted) => Value::Object(
                all.into_iter().filter(|(k, _)| k == "id" || k == "@type" || wanted.iter().any(|w| w == k)).collect(),
            ),
        }
    }

    /// Answers a `FileNode` method. `mail_blob` finds an uploaded blob, which the upload
    /// endpoint keeps with the mail blobs.
    pub fn call(&mut self, name: &str, args: &Value, mail_blob: &MailBlob<'_>) -> Result<Value, MethodError> {
        if !self.enabled {
            return Err(merr("unknownMethod", format!("the server has no {name}")));
        }
        let account = args["accountId"].as_str().unwrap_or_default().to_string();
        match self.spaces.get(&account) {
            None => return Err(merr("accountNotFound", "no such account")),
            Some(s) if !s.capable || !s.serves => {
                return Err(merr("accountNotSupportedByMethod", format!("{name} is not supported by this account")))
            }
            _ => {}
        }
        match name {
            "FileNode/get" => self.get(&account, args),
            "FileNode/query" => self.query(&account, args),
            "FileNode/set" => Ok(self.set(&account, args, mail_blob)),
            _ => Err(merr("unknownMethod", format!("the server has no {name}"))),
        }
    }

    fn get(&self, account: &str, args: &Value) -> Result<Value, MethodError> {
        let space = &self.spaces[account];
        let properties: Option<Vec<String>> =
            args["properties"].as_array().map(|p| p.iter().filter_map(|v| v.as_str().map(str::to_string)).collect());
        let ids: Option<Vec<&str>> = match &args["ids"] {
            Value::Null => None,
            ids => Some(ids.as_array().into_iter().flatten().filter_map(Value::as_str).collect()),
        };
        if ids.as_ref().is_some_and(|ids| ids.len() > self.get_max) {
            return Err(merr("requestTooLarge", "too many ids"));
        }
        let (mut list, mut not_found) = (Vec::new(), Vec::new());
        match ids {
            None => list.extend(space.nodes.iter().map(|n| self.node_json(n, properties.as_deref()))),
            Some(ids) => {
                for id in ids {
                    match space.nodes.iter().find(|n| n.id == id) {
                        Some(n) => list.push(self.node_json(n, properties.as_deref())),
                        None => not_found.push(id),
                    }
                }
            }
        }
        Ok(json!({ "accountId": account, "state": "s", "list": list, "notFound": not_found }))
    }

    /// Applies a filter condition: its keys together, or an `AND` of conditions.
    fn filter(&self, matched: &mut Vec<&Node>, filter: &Value) -> Result<(), MethodError> {
        for (key, value) in filter.as_object().into_iter().flatten() {
            match key.as_str() {
                "operator" => {
                    if value != "AND" {
                        return Err(merr("unsupportedFilter", "operator"));
                    }
                }
                "conditions" => {
                    for condition in value.as_array().into_iter().flatten() {
                        self.filter(matched, condition)?;
                    }
                }
                "parentId" => {
                    let parent = value.as_str().ok_or_else(|| merr("invalidArguments", "parentId must be a string"))?;
                    matched.retain(|n| n.parent.as_deref() == Some(parent));
                }
                "isTopLevel" => matched.retain(|n| n.parent.is_none() == value.as_bool().unwrap_or(true)),
                "name" => matched.retain(|n| Some(n.name.as_str()) == value.as_str()),
                "text" => {
                    let needle = value.as_str().unwrap_or_default().to_lowercase();
                    matched.retain(|n| {
                        let content = n.blob.as_ref().and_then(|b| self.blobs.get(b)).map(|(_, c)| c.clone());
                        let content = String::from_utf8_lossy(&content.unwrap_or_default()).to_lowercase();
                        n.name.to_lowercase().contains(&needle) || content.contains(&needle)
                    });
                }
                other => return Err(merr("unsupportedFilter", other)),
            }
        }
        Ok(())
    }

    fn query(&self, account: &str, args: &Value) -> Result<Value, MethodError> {
        let space = &self.spaces[account];
        let mut matched: Vec<&Node> = space.nodes.iter().collect();
        self.filter(&mut matched, &args["filter"])?;
        let sorts: Vec<&str> =
            args["sort"].as_array().into_iter().flatten().filter_map(|s| s["property"].as_str()).collect();
        for s in &sorts {
            if !["nodeType", "name"].contains(s) {
                return Err(merr("unsupportedSort", *s));
            }
        }
        matched.sort_by_cached_key(|n| {
            let mut key: Vec<String> = Vec::new();
            for s in &sorts {
                key.push(if *s == "nodeType" {
                    (if n.dir { "0" } else { "1" }).to_string()
                } else {
                    n.name.to_lowercase()
                });
            }
            (key, n.id.parse::<u64>().unwrap_or(0))
        });
        let total = matched.len();
        let position = args["position"].as_u64().unwrap_or(0) as usize;
        let limit = args["limit"].as_u64().map_or(self.query_max, |l| (l as usize).min(self.query_max));
        let ids: Vec<&str> = matched.iter().skip(position).take(limit).map(|n| n.id.as_str()).collect();
        let mut out = json!({ "accountId": account, "queryState": "q", "canCalculateChanges": false,
            "position": position, "ids": ids });
        if args["calculateTotal"] == true {
            out["total"] = json!(total);
        }
        Ok(out)
    }

    fn check_name(name: &str) -> Result<(), Value> {
        let bad = |d: &str| Err(set_error("invalidProperties", d, &["name"]));
        if name.is_empty() || name.len() > 255 {
            return bad("Name must be between 1 and 255 octets.");
        }
        if name.chars().any(|c| FORBIDDEN_CHARS.contains(c) || c.is_control()) {
            return bad("Name contains a forbidden character.");
        }
        let stem = name.split('.').next().unwrap_or_default().to_uppercase();
        if name == "." || name == ".." || RESERVED.contains(&stem.as_str()) {
            return bad("Name is reserved and cannot be used.");
        }
        Ok(())
    }

    /// The rights the user has over a node of a shared space; in the own space everything.
    fn may(&self, account: &str, node: Option<&Node>, parent: Option<&str>, right: &str) -> bool {
        let space = &self.spaces[account];
        if space.personal {
            return true;
        }
        let holder = match node {
            Some(n) if n.dir => Some(n),
            _ => parent.and_then(|p| space.nodes.iter().find(|n| n.id == p)),
        };
        holder.is_some_and(|h| h.rights.contains(&right_name(right)))
    }

    fn taken(&self, account: &str, parent: Option<&str>, name: &str, except: Option<&str>) -> Option<String> {
        self.spaces[account]
            .nodes
            .iter()
            .find(|n| n.parent.as_deref() == parent && n.name == name && Some(n.id.as_str()) != except)
            .map(|n| n.id.clone())
    }

    fn resolve_blob(&mut self, mail_blob: &MailBlob<'_>, id: &str) -> Option<String> {
        let (media_type, bytes) = mail_blob(id)?;
        Some(self.new_blob(&media_type, bytes))
    }

    fn set(&mut self, account: &str, args: &Value, mail_blob: &MailBlob<'_>) -> Value {
        let injected = self.next_set_error.take();
        let fail = |injected: &Option<(&'static str, String)>| injected.as_ref().map(|(k, d)| set_error(k, d, &[]));
        let (mut created, mut not_created) = (Map::new(), Map::new());
        let (mut updated, mut not_updated) = (Map::new(), Map::new());
        let (mut destroyed, mut not_destroyed) = (Vec::new(), Map::new());

        for (key, spec) in args["create"].as_object().into_iter().flatten() {
            match fail(&injected).map_or_else(|| self.create(account, spec, mail_blob), Err) {
                Ok(v) => {
                    created.insert(key.clone(), v);
                }
                Err(e) => {
                    not_created.insert(key.clone(), e);
                }
            }
        }
        for (id, patch) in args["update"].as_object().into_iter().flatten() {
            match fail(&injected).map_or_else(|| self.update(account, id, patch, mail_blob), Err) {
                Ok(v) => {
                    updated.insert(id.clone(), v);
                }
                Err(e) => {
                    not_updated.insert(id.clone(), e);
                }
            }
        }
        let remove_children = args["onDestroyRemoveChildren"] == true;
        for id in args["destroy"].as_array().into_iter().flatten().filter_map(Value::as_str) {
            match fail(&injected).map_or_else(|| self.destroy(account, id, remove_children), Err) {
                Ok(ids) => destroyed.extend(ids),
                Err(e) => {
                    not_destroyed.insert(id.to_string(), e);
                }
            }
        }
        let some = |m: Map<String, Value>| if m.is_empty() { Value::Null } else { Value::Object(m) };
        json!({
            "accountId": account, "oldState": "s", "newState": "s",
            "created": some(created), "notCreated": some(not_created),
            "updated": some(updated), "notUpdated": some(not_updated),
            "destroyed": if destroyed.is_empty() { Value::Null } else { json!(destroyed) },
            "notDestroyed": some(not_destroyed),
        })
    }

    fn parent_ok(&self, account: &str, parent: Option<&str>) -> Result<(), Value> {
        match parent {
            None => Ok(()),
            Some(p) if self.spaces[account].nodes.iter().any(|n| n.id == p && n.dir) => Ok(()),
            Some(_) => {
                Err(set_error("invalidProperties", "Parent ID does not exist or is not a folder.", &["parentId"]))
            }
        }
    }

    fn create(&mut self, account: &str, spec: &Value, mail_blob: &MailBlob<'_>) -> Result<Value, Value> {
        let dir = match spec["@type"].as_str() {
            Some("Directory") => true,
            Some("File") => false,
            Some(_) => return Err(set_error("invalidProperties", "Unsupported @type.", &["@type"])),
            None => return Err(set_error("invalidProperties", "@type is required on creation.", &["@type"])),
        };
        let name = spec["name"].as_str().ok_or_else(|| set_error("invalidProperties", "Missing name.", &["name"]))?;
        Self::check_name(name)?;
        let parent = spec["parentId"].as_str();
        self.parent_ok(account, parent)?;
        let top_ok = parent.is_some() || self.spaces[account].may_top;
        if !top_ok || !self.may(account, None, parent, "mayAddChildren") {
            return Err(set_error("forbidden", "You are not allowed to create file nodes in this folder.", &[]));
        }
        if let Some(existing) = self.taken(account, parent, name, None) {
            return Err(json!({ "type": "alreadyExists", "existingId": existing }));
        }
        let blob = if dir {
            None
        } else {
            let wanted = spec["blobId"]
                .as_str()
                .ok_or_else(|| set_error("invalidProperties", "blobId is required for File nodes.", &["blobId"]))?;
            Some(
                self.resolve_blob(mail_blob, wanted)
                    .ok_or_else(|| set_error("invalidProperties", "Field could not be set.", &["blobId"]))?,
            )
        };
        let rights = if self.spaces[account].personal { ALL.to_vec() } else { vec!["mayRead", "mayAddChildren"] };
        let media_type = spec["mediaType"].as_str().unwrap_or("application/octet-stream").to_string();
        let space = self.space(account);
        let id = space.next.to_string();
        space.next += 1;
        space.nodes.push(Node {
            id: id.clone(),
            parent: parent.map(str::to_string),
            name: name.to_string(),
            dir,
            blob,
            media_type,
            modified: now_string(),
            rights: if dir { rights } else { Vec::new() },
        });
        Ok(json!({ "id": id, "created": now_string(), "modified": now_string() }))
    }

    fn update(&mut self, account: &str, id: &str, patch: &Value, mail_blob: &MailBlob<'_>) -> Result<Value, Value> {
        let Some(node) = self.spaces[account].nodes.iter().find(|n| n.id == id) else {
            return Err(json!({ "type": "notFound" }));
        };
        let (old_parent, old_name, is_dir) = (node.parent.clone(), node.name.clone(), node.dir);
        let patch = patch.as_object().cloned().unwrap_or_default();
        for key in patch.keys() {
            if !["name", "parentId", "blobId", "modified", "mediaType"].contains(&key.as_str()) {
                return Err(set_error("invalidProperties", "Invalid property.", &[key]));
            }
        }
        let new_name = patch.get("name").and_then(Value::as_str).unwrap_or(&old_name).to_string();
        let new_parent = match patch.get("parentId") {
            None => old_parent.clone(),
            Some(v) => v.as_str().map(str::to_string),
        };
        let renames = new_name != old_name;
        let moves = new_parent != old_parent;
        if renames || moves {
            let holder = self.spaces[account].nodes.iter().find(|n| n.id == id);
            if !self.may(account, holder, old_parent.as_deref(), "mayRename") {
                return Err(set_error("forbidden", "You are not allowed to rename or move this file node.", &[]));
            }
        }
        Self::check_name(&new_name)?;
        self.parent_ok(account, new_parent.as_deref())?;
        // Not into itself, nor into what is under it.
        let mut up = new_parent.clone();
        while let Some(p) = up {
            if p == id {
                return Err(set_error("invalidProperties", "Circular reference in parent ids.", &["parentId"]));
            }
            up = self.spaces[account].nodes.iter().find(|n| n.id == p).and_then(|n| n.parent.clone());
        }
        if (renames || moves) && self.taken(account, new_parent.as_deref(), &new_name, Some(id)).is_some() {
            let existing = self.taken(account, new_parent.as_deref(), &new_name, Some(id));
            return Err(json!({ "type": "alreadyExists", "existingId": existing }));
        }
        let new_blob = match patch.get("blobId").and_then(Value::as_str) {
            None => None,
            Some(_) if is_dir => {
                return Err(set_error("invalidProperties", "blobId is only defined for File nodes.", &["blobId"]))
            }
            Some(wanted) => {
                if !self.may(account, None, old_parent.as_deref(), "mayModifyContent") {
                    return Err(set_error("forbidden", "You are not allowed to change this file.", &[]));
                }
                Some(
                    self.resolve_blob(mail_blob, wanted)
                        .ok_or_else(|| set_error("invalidProperties", "Field could not be set.", &["blobId"]))?,
                )
            }
        };
        let space = self.space(account);
        if let Some(n) = space.nodes.iter_mut().find(|n| n.id == id) {
            n.name = new_name;
            n.parent = new_parent;
            if let Some(b) = new_blob {
                n.blob = Some(b);
            }
            if let Some(m) = patch.get("modified").and_then(Value::as_str) {
                n.modified = m.to_string();
            }
            if let Some(m) = patch.get("mediaType").and_then(Value::as_str) {
                n.media_type = m.to_string();
            }
        }
        Ok(json!({ "changed": now_string() }))
    }

    fn destroy(&mut self, account: &str, id: &str, remove_children: bool) -> Result<Vec<String>, Value> {
        let Some(node) = self.spaces[account].nodes.iter().find(|n| n.id == id) else {
            return Err(json!({ "type": "notFound" }));
        };
        if !self.may(account, Some(node), node.parent.as_deref(), "mayDelete") {
            return Err(set_error("forbidden", "You do not have permission to perform this operation.", &[]));
        }
        let has_children = self.spaces[account].nodes.iter().any(|n| n.parent.as_deref() == Some(id));
        if has_children && !remove_children {
            return Err(set_error("nodeHasChildren", "Cannot delete non-empty folder.", &[]));
        }
        let before: Vec<String> = self.spaces[account].nodes.iter().map(|n| n.id.clone()).collect();
        self.remove(account, id);
        let after: Vec<String> = self.spaces[account].nodes.iter().map(|n| n.id.clone()).collect();
        Ok(before.into_iter().filter(|b| !after.contains(b)).collect())
    }
}

/// The static name of a right, for the lookups in `rights`.
fn right_name(right: &str) -> &'static str {
    ALL.iter().copied().find(|r| *r == right).unwrap_or("mayRead")
}
