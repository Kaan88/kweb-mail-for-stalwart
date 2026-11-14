//! File storage over plain JMAP requests (`urn:ietf:params:jmap:filenode`; jmap-client has no
//! support for it). What Stalwart 1.1.0 really does is in `dev/notes/stalwart-files.md`; the
//! points that shape this code:
//!
//! - There is no root node: a top-level node has no `parentId`. Folders are `@type` `Directory`.
//! - A file has no `myRights`; what may be done to it is what its folder allows.
//! - A shared folder lives in the owner's account. Every call here takes the account id of the
//!   space it is about; nothing is remembered between calls. An upload only works into the
//!   user's own account, and a node in a shared space may refer to that blob.
//! - The server clamps a query (5000) and refuses a get of more than 500 ids.
//! - It does not bump `modified` when the content is replaced, so the core does.

use std::{
    collections::{HashMap, HashSet},
    fs,
    io::Read,
    path::Path,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

use chrono::{DateTime, SecondsFormat};
use serde_json::{json, Value};

use super::{
    errors::unauthorized,
    send::{response, CREATE_ID},
    Connection,
};
use crate::{
    model::{FileEntry, FileKind, FileListing, FileSpace},
    Error, Result,
};

const FILES_CAPABILITY: &str = "urn:ietf:params:jmap:filenode";
const NO_FILES: &str = "the server has no file storage";
const GONE: &str = "that file or folder is no longer there";
const FOLDER_GONE: &str = "that folder is no longer there";
const EXISTS: &str = "something with this name is already there";
/// The most levels of folders the server allows; a walk up the tree stops here.
const MAX_DEPTH: usize = 64;
/// How many files and folders one listing gives up at, for a server that never ends one.
const MAX_NODES: usize = 100_000;
const SEARCH_LIMIT: usize = 200;
/// How much of a file goes into one piece of an upload.
const CHUNK: usize = 64 * 1024;
const NODE_PROPERTIES: [&str; 8] = ["id", "parentId", "name", "size", "blobId", "mediaType", "modified", "myRights"];

/// The space a call is about.
struct Space {
    id: String,
    /// The user's own storage, not a folder somebody shared.
    own: bool,
}

impl Space {
    /// What `FileEntry::account_id` says.
    fn entry_account(&self) -> Option<String> {
        (!self.own).then(|| self.id.clone())
    }
}

/// What the user may do in a folder, as far as the app uses it.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Rights {
    add: bool,
    rename: bool,
    delete: bool,
    modify: bool,
}

const ALL_RIGHTS: Rights = Rights { add: true, rename: true, delete: true, modify: true };

fn rights_of(node: &Value) -> Rights {
    let r = &node["myRights"];
    let has = |name: &str| r[name].as_bool().unwrap_or(false);
    Rights {
        add: has("mayAddChildren"),
        rename: has("mayRename"),
        delete: has("mayDelete"),
        modify: has("mayModifyContent"),
    }
}

/// A node as the app shows it. A file takes what may be done to it from `inherited`, the rights
/// of its folder. `None` when the node has no id.
fn entry_from(space: &Space, node: &Value, inherited: Rights) -> Option<FileEntry> {
    let id = node["id"].as_str()?.to_string();
    let kind = match node["@type"].as_str() {
        Some("Directory") => FileKind::Folder,
        Some("File") => FileKind::File,
        _ => FileKind::Other,
    };
    let own = if kind == FileKind::Folder { rights_of(node) } else { inherited };
    let text = |key: &str| node[key].as_str().unwrap_or_default().to_string();
    let is_file = kind == FileKind::File;
    Some(FileEntry {
        account_id: space.entry_account(),
        id,
        parent_id: node["parentId"].as_str().map(str::to_string),
        name: text("name"),
        kind,
        size: if kind == FileKind::Folder { 0 } else { node["size"].as_u64().unwrap_or(0) },
        media_type: if kind == FileKind::Folder { String::new() } else { text("mediaType") },
        blob_id: node["blobId"].as_str().filter(|_| is_file).map(str::to_string),
        modified: node["modified"].as_str().and_then(|t| DateTime::parse_from_rfc3339(t).ok()).map(|t| t.timestamp()),
        may_add: kind == FileKind::Folder && own.add,
        may_rename: own.rename,
        may_delete: own.delete,
        may_replace: is_file && own.modify,
    })
}

/// Folders first, then files, then the rest, each by name without regard to case.
fn sort_entries(entries: &mut [FileEntry]) {
    let rank = |k: FileKind| match k {
        FileKind::Folder => 0,
        FileKind::File => 1,
        FileKind::Other => 2,
    };
    entries.sort_by_cached_key(|e| (rank(e.kind), e.name.to_lowercase(), e.id.clone()));
}

impl Connection {
    async fn files_call(&self, calls: &[(&str, Value)]) -> Result<Vec<(String, Value)>> {
        self.call(&[FILES_CAPABILITY], calls).await
    }

    /// The account that holds the user's own files, when the server has file storage for them.
    fn own_files_account(&self) -> Option<String> {
        let session = self.client.session();
        let found = session.primary_accounts().find(|(c, _)| c.as_str() == FILES_CAPABILITY);
        found.map(|(_, account)| account.clone())
    }

    fn account_has_files(&self, account: &str) -> bool {
        self.client.session().account(account).is_some_and(|a| a.capabilities().any(|c| c == FILES_CAPABILITY))
    }

    /// The space the host named: `None` is the user's own. An account the session does not
    /// list, or that has no file storage, is `Error::Invalid`.
    fn files_space(&self, account: Option<&str>) -> Result<Space> {
        let own = self.own_files_account();
        match account {
            None => own.map(|id| Space { id, own: true }).ok_or_else(|| Error::invalid(NO_FILES)),
            Some(id) if own.as_deref() == Some(id) => Ok(Space { id: id.to_string(), own: true }),
            Some(id) if self.account_has_files(id) => Ok(Space { id: id.to_string(), own: false }),
            Some(_) => Err(Error::invalid("the server has no such file space")),
        }
    }

    /// The session as the server sends it, read once per connection (the well-known URL answers
    /// with a redirect, which `session_json` follows). jmap-client keeps an account capability
    /// it has no type for as an empty object, so the limits of the file storage are read here.
    async fn raw_session(&self) -> Result<&Value> {
        self.files_session.get_or_try_init(|| self.session_json()).await
    }

    /// Whether files and folders may be made at the top level of the user's own space.
    async fn own_may_add_at_top(&self, own_account: &str) -> Result<bool> {
        let session = self.raw_session().await?;
        let caps = &session["accounts"][own_account]["accountCapabilities"][FILES_CAPABILITY];
        Ok(caps["mayCreateTopLevelFileNode"].as_bool().unwrap_or(true))
    }

    // ---- reading ----

    /// The user's own space, then every other account of the session that has file storage and
    /// really shares a folder. The session cannot tell that: an account that shares only mail
    /// has the capability too, and answers the query with `accountNotSupportedByMethod`.
    pub async fn file_spaces(&self) -> Result<Vec<FileSpace>> {
        let own = self.own_files_account();
        let mut spaces = Vec::new();
        if let Some(id) = &own {
            spaces.push(FileSpace {
                account_id: None,
                name: String::new(),
                may_add_at_top: self.own_may_add_at_top(id).await?,
                max_upload: self.max_upload(),
            });
        }
        let mut others: Vec<(String, String)> = {
            let session = self.client.session();
            let ids: Vec<&String> = session.accounts().collect();
            ids.into_iter()
                .filter(|id| Some(id.as_str()) != own.as_deref())
                .filter_map(|id| session.account(id).map(|a| (id, a)))
                .filter(|(_, a)| !a.is_personal() && a.capabilities().any(|c| c == FILES_CAPABILITY))
                .map(|(id, a)| (a.name().to_string(), id.clone()))
                .collect()
        };
        others.sort();
        for (name, id) in others {
            if self.shares_files(&id).await? {
                spaces.push(FileSpace {
                    account_id: Some(id),
                    name,
                    may_add_at_top: false,
                    max_upload: self.max_upload(),
                });
            }
        }
        Ok(spaces)
    }

    /// Whether the account has any file storage to show the user. An account that refuses the
    /// question has none.
    async fn shares_files(&self, account: &str) -> Result<bool> {
        let args = json!({ "accountId": account, "filter": { "isTopLevel": true }, "limit": 1 });
        let responses = self.call_keeping_errors(&[FILES_CAPABILITY], &[("FileNode/query", args)]).await?;
        match responses.first() {
            Some((name, answer)) if name == "error" => match answer["type"].as_str().unwrap_or("unknown") {
                "accountNotSupportedByMethod" | "forbidden" | "accountNoAccess" | "accountNotFound" => Ok(false),
                kind => Err(Error::temporary(format!("FileNode/query failed: {kind}"))),
            },
            _ => Ok(true),
        }
    }

    /// One page of nodes: the query and, by reference, the get. Gives the nodes in the order
    /// of the query, how many ids the server gave (it may give fewer than asked for) and the
    /// total it counted.
    async fn query_page(
        &self,
        space: &Space,
        filter: &Value,
        sort: Option<Value>,
        (position, limit): (usize, usize),
    ) -> Result<(Vec<Value>, usize, Option<u64>)> {
        let mut query = json!({
            "accountId": space.id, "filter": filter, "position": position, "limit": limit, "calculateTotal": true,
        });
        if let Some(sort) = sort {
            query["sort"] = sort;
        }
        let get = json!({
            "accountId": space.id, "properties": NODE_PROPERTIES,
            "#ids": { "resultOf": "c0", "name": "FileNode/query", "path": "/ids" },
        });
        let responses = self.files_call(&[("FileNode/query", query), ("FileNode/get", get)]).await?;
        let answer = response(&responses, "FileNode/query")?;
        let ids: Vec<&str> = answer["ids"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
        let mut nodes: Vec<Value> =
            response(&responses, "FileNode/get")?["list"].as_array().cloned().unwrap_or_default();
        let place = |node: &Value| ids.iter().position(|id| node["id"].as_str() == Some(id)).unwrap_or(ids.len());
        nodes.sort_by_cached_key(place);
        Ok((nodes, ids.len(), answer["total"].as_u64()))
    }

    /// Every node the filter matches, folders first and by name, page after page. The next page
    /// starts after the ids the server gave, not after the limit asked for.
    async fn all_nodes(&self, space: &Space, filter: &Value) -> Result<Vec<Value>> {
        let limit = self.get_batch();
        let sort = json!([{ "property": "nodeType" }, { "property": "name" }]);
        let (mut out, mut position) = (Vec::new(), 0_usize);
        while position < MAX_NODES {
            let (nodes, found, total) = self.query_page(space, filter, Some(sort.clone()), (position, limit)).await?;
            out.extend(nodes);
            position += found;
            if found == 0 || total.is_some_and(|total| position as u64 >= total) {
                break;
            }
        }
        Ok(out)
    }

    /// The nodes with these ids, in as many requests as the server's limit needs. Ids that are
    /// gone are left out.
    async fn get_nodes(&self, space: &Space, ids: &[String], properties: &[&str]) -> Result<Vec<Value>> {
        let mut out = Vec::with_capacity(ids.len());
        for chunk in ids.chunks(self.get_batch()) {
            let args = json!({ "accountId": space.id, "ids": chunk, "properties": properties });
            let responses = self.files_call(&[("FileNode/get", args)]).await?;
            out.extend(response(&responses, "FileNode/get")?["list"].as_array().cloned().unwrap_or_default());
        }
        Ok(out)
    }

    async fn get_node(&self, space: &Space, id: &str) -> Result<Option<Value>> {
        Ok(self.get_nodes(space, &[id.to_string()], &NODE_PROPERTIES).await?.pop())
    }

    /// The folders from the top level down to the folder `node` itself, found by walking up
    /// its parents one request per level. The walk ends at a parent the user cannot see.
    async fn folder_path(&self, space: &Space, node: &Value) -> Result<Vec<FileEntry>> {
        let mut path: Vec<FileEntry> = entry_from(space, node, Rights::default()).into_iter().collect();
        let mut seen: HashSet<String> = path.iter().map(|e| e.id.clone()).collect();
        let mut next = node["parentId"].as_str().map(str::to_string);
        while let Some(id) = next.take() {
            if path.len() > MAX_DEPTH || !seen.insert(id.clone()) {
                break;
            }
            let Some(parent) = self.get_node(space, &id).await? else { break };
            next = parent["parentId"].as_str().map(str::to_string);
            path.extend(entry_from(space, &parent, Rights::default()));
        }
        path.reverse();
        Ok(path)
    }

    /// What is in a folder (`None`: the top level of the space), and the way to it.
    pub async fn file_list(&self, account: Option<&str>, folder_id: Option<&str>) -> Result<FileListing> {
        let space = self.files_space(account)?;
        let Some(folder_id) = folder_id else {
            let may_add = space.own && self.own_may_add_at_top(&space.id).await?;
            let nodes = self.all_nodes(&space, &json!({ "isTopLevel": true })).await?;
            // The user's own top level holds files too. Another user's holds only what that
            // user shared, and that is folders.
            let inherited = if space.own { ALL_RIGHTS } else { Rights::default() };
            let mut entries: Vec<FileEntry> = nodes.iter().filter_map(|n| entry_from(&space, n, inherited)).collect();
            if !space.own {
                entries.retain(|e| e.kind == FileKind::Folder);
            }
            sort_entries(&mut entries);
            return Ok(FileListing { folder: None, path: Vec::new(), entries, may_add });
        };
        let folder = self.get_node(&space, folder_id).await?.filter(|n| n["@type"] == "Directory");
        let folder = folder.ok_or_else(|| Error::invalid(FOLDER_GONE))?;
        let rights = rights_of(&folder);
        let nodes = self.all_nodes(&space, &json!({ "parentId": folder_id })).await?;
        let mut entries: Vec<FileEntry> = nodes.iter().filter_map(|n| entry_from(&space, n, rights)).collect();
        sort_entries(&mut entries);
        let path = self.folder_path(&space, &folder).await?;
        Ok(FileListing { folder: entry_from(&space, &folder, rights), path, entries, may_add: rights.add })
    }

    /// Files and folders whose name or content has `text`, at most 200, in the server's order.
    pub async fn file_search(&self, account: Option<&str>, text: &str) -> Result<Vec<FileEntry>> {
        let space = self.files_space(account)?;
        if text.trim().is_empty() {
            return Ok(Vec::new());
        }
        let filter = json!({ "text": text.trim() });
        // Pages no larger than the server takes in one get, up to the most a search gives.
        let (mut nodes, mut position) = (Vec::new(), 0_usize);
        while nodes.len() < SEARCH_LIMIT {
            let limit = self.get_batch().min(SEARCH_LIMIT - nodes.len()).max(1);
            let (page, found, total) = self.query_page(&space, &filter, None, (position, limit)).await?;
            nodes.extend(page);
            position += found;
            if found == 0 || total.is_some_and(|t| position as u64 >= t) {
                break;
            }
        }
        nodes.truncate(SEARCH_LIMIT);
        self.entries_of(&space, &nodes).await
    }

    /// The nodes as entries. The files among them take their rights from their folders, which
    /// are asked for in one go.
    async fn entries_of(&self, space: &Space, nodes: &[Value]) -> Result<Vec<FileEntry>> {
        let mut parents: Vec<String> = Vec::new();
        for node in nodes.iter().filter(|n| n["@type"] != "Directory") {
            if let Some(p) = node["parentId"].as_str().filter(|p| !parents.iter().any(|q| q == p)) {
                parents.push(p.to_string());
            }
        }
        let known: HashMap<String, Rights> = self
            .get_nodes(space, &parents, &["id", "myRights"])
            .await?
            .iter()
            .filter_map(|p| Some((p["id"].as_str()?.to_string(), rights_of(p))))
            .collect();
        let at_top = if space.own { ALL_RIGHTS } else { Rights::default() };
        Ok(nodes
            .iter()
            .filter_map(|n| {
                let inherited = match n["parentId"].as_str() {
                    Some(p) => known.get(p).copied().unwrap_or_default(),
                    None => at_top,
                };
                entry_from(space, n, inherited)
            })
            .collect())
    }

    /// One node as the server has it now.
    async fn entry_by_id(&self, space: &Space, id: &str) -> Result<FileEntry> {
        let node = self.get_node(space, id).await?.ok_or_else(|| Error::invalid(GONE))?;
        let mut entries = self.entries_of(space, &[node]).await?;
        entries.pop().ok_or_else(|| Error::invalid(GONE))
    }

    // ---- changing ----

    /// One `FileNode/set`. Returns the answer; what became of each object is the caller's to
    /// read.
    async fn files_set(&self, space: &Space, mut args: Value) -> Result<Value> {
        args["accountId"] = json!(space.id);
        let responses = self.files_call(&[("FileNode/set", args)]).await?;
        Ok(response(&responses, "FileNode/set")?.clone())
    }

    /// `done` keeps the id of the folder once it is made, so a run repeated after a 401 does
    /// not report its own folder as already there.
    pub async fn file_create_folder(
        &self,
        account: Option<&str>,
        parent_id: Option<&str>,
        name: &str,
        done: &Mutex<Option<String>>,
    ) -> Result<FileEntry> {
        let space = self.files_space(account)?;
        let made = done.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if let Some(id) = made {
            return self.entry_by_id(&space, &id).await;
        }
        let mut node = json!({ "@type": "Directory", "name": name });
        if let Some(parent) = parent_id {
            node["parentId"] = json!(parent);
        }
        let set = self.files_set(&space, json!({ "create": { CREATE_ID: node } })).await?;
        let id = created_id(&set)?;
        *done.lock().unwrap_or_else(|e| e.into_inner()) = Some(id.clone());
        self.entry_by_id(&space, &id).await
    }

    pub async fn file_rename(&self, account: Option<&str>, id: &str, name: &str) -> Result<()> {
        let space = self.files_space(account)?;
        let set = self.files_set(&space, json!({ "update": { id: { "name": name } } })).await?;
        updated(&set, id)
    }

    pub async fn file_move(&self, account: Option<&str>, id: &str, parent_id: Option<&str>) -> Result<()> {
        let space = self.files_space(account)?;
        if parent_id == Some(id) {
            return Err(file_error(&json!({ "type": "invalidProperties", "properties": ["parentId"],
                "description": "Circular reference in parent ids." })));
        }
        let set = self.files_set(&space, json!({ "update": { id: { "parentId": parent_id } } })).await?;
        updated(&set, id)
    }

    /// Deletes a file, or a folder with everything in it. Something already gone is as good as
    /// deleted.
    pub async fn file_delete(&self, account: Option<&str>, id: &str) -> Result<()> {
        let space = self.files_space(account)?;
        let set = self.files_set(&space, json!({ "destroy": [id], "onDestroyRemoveChildren": true })).await?;
        match set["notDestroyed"].get(id) {
            Some(e) if e["type"] == "notFound" => Ok(()),
            Some(e) => Err(file_error(e)),
            None if set["destroyed"].as_array().is_some_and(|ids| ids.iter().any(|d| d == id)) => Ok(()),
            None => Err(Error::temporary("the server's answer does not say whether it was deleted")),
        }
    }

    /// The account whose blobs `account`'s space takes into its nodes, and downloads from. A
    /// download URL names the space's own account. An account that shares mail and has no file
    /// storage is good for the attachments of its messages (see `shared.rs`).
    pub async fn file_download_account(&self, account: Option<&str>) -> Result<String> {
        match (self.files_space(account), account) {
            (Ok(space), _) => Ok(space.id),
            (Err(e), Some(id)) => match self.shared_check(id).await {
                Ok(()) => Ok(id.to_string()),
                // Not shared: the space error says it best. Anything else (401, offline) must
                // reach the caller as it is, so the token is renewed and the call retried.
                Err(Error::Invalid { .. }) => Err(e),
                Err(other) => Err(other),
            },
            (Err(e), None) => Err(e),
        }
    }

    /// The node of this name in the folder, if there is one.
    async fn node_named(&self, space: &Space, parent_id: Option<&str>, name: &str) -> Result<Option<Value>> {
        let place = match parent_id {
            Some(parent) => json!({ "parentId": parent }),
            None => json!({ "isTopLevel": true }),
        };
        let filter = json!({ "operator": "AND", "conditions": [place, { "name": name }] });
        let query = json!({ "accountId": space.id, "filter": filter, "limit": 5 });
        let responses = self.files_call(&[("FileNode/query", query)]).await?;
        let ids: Vec<String> = response(&responses, "FileNode/query")?["ids"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|i| i.as_str().map(str::to_string))
            .collect();
        // The match is exact; the node is checked again in case the server was loose.
        let nodes = self.get_nodes(space, &ids, &NODE_PROPERTIES).await?;
        Ok(nodes.into_iter().find(|n| n["name"].as_str() == Some(name)))
    }

    /// Puts the file at `path` into a folder of the space. See `Core::file_upload`. `resume`
    /// outlives a repeat of the call after a 401: it keeps the blob that was uploaded and the
    /// node that was made or changed, so nothing is sent or made twice.
    pub async fn file_upload(
        &self,
        account: Option<&str>,
        parent_id: Option<&str>,
        (name, media_type): (&str, &str),
        path: &Path,
        replace: bool,
        resume: &Mutex<UploadResume>,
    ) -> Result<FileEntry> {
        let space = self.files_space(account)?;
        let state = |f: &dyn Fn(&mut UploadResume)| f(&mut resume.lock().unwrap_or_else(|e| e.into_inner()));
        let (kept_blob, kept_node) = {
            let r = resume.lock().unwrap_or_else(|e| e.into_inner());
            (r.blob.clone(), r.node.clone())
        };
        if let Some(id) = kept_node {
            return self.entry_by_id(&space, &id).await;
        }
        // An upload always goes to the user's own account: another account's upload URL
        // refuses it. A node in a shared space may refer to that blob.
        let own = self.own_files_account().ok_or_else(|| Error::invalid(NO_FILES))?;
        let meta = fs::metadata(path).map_err(read_error)?;
        if !meta.is_file() {
            return Err(Error::invalid("that is not a file"));
        }
        let (size, limit) = (meta.len(), self.max_upload());
        if limit > 0 && size > limit {
            return Err(Error::invalid(format!(
                "this file is {} and the server takes at most {}",
                megabytes(size),
                megabytes(limit)
            )));
        }
        let media_type = if media_type.trim().is_empty() { "application/octet-stream" } else { media_type.trim() };
        // Ask about the name before sending anything: the server counts every uploaded byte
        // against a budget, and a name that is taken must not use it up.
        let existing = self.node_named(&space, parent_id, name).await?;
        if let Some(node) = &existing {
            check_replaceable(node, replace)?;
        }
        let blob_id = match kept_blob {
            Some(blob) => blob,
            None => {
                let blob = self.upload_file(&own, path, size, media_type).await?;
                state(&|r| r.blob = Some(blob.clone()));
                blob
            }
        };
        if let Some(node) = existing {
            return self.replace_content(&space, &node, &blob_id, media_type, resume).await;
        }

        let mut node = json!({ "@type": "File", "name": name, "blobId": blob_id, "mediaType": media_type });
        if let Some(parent) = parent_id {
            node["parentId"] = json!(parent);
        }
        let set = self.files_set(&space, json!({ "create": { CREATE_ID: node } })).await?;
        let refused = &set["notCreated"][CREATE_ID];
        if refused["type"] != "alreadyExists" {
            let id = created_id(&set)?;
            state(&|r| r.node = Some(id.clone()));
            return self.entry_by_id(&space, &id).await;
        }
        // The name was taken after the check above. The same rules apply.
        let existing = match refused["existingId"].as_str() {
            Some(id) => self.get_node(&space, id).await?,
            None => None,
        };
        let Some(existing) = existing else { return Err(Error::invalid(EXISTS)) };
        check_replaceable(&existing, replace)?;
        self.replace_content(&space, &existing, &blob_id, media_type, resume).await
    }

    /// Gives a file new content (and its type) and sets its changed-time.
    async fn replace_content(
        &self,
        space: &Space,
        node: &Value,
        blob_id: &str,
        media_type: &str,
        resume: &Mutex<UploadResume>,
    ) -> Result<FileEntry> {
        let id = node["id"].as_str().unwrap_or_default().to_string();
        let patch = json!({ "blobId": blob_id, "mediaType": media_type, "modified": utc_now() });
        let set = self.files_set(space, json!({ "update": { id.as_str(): patch } })).await?;
        updated(&set, &id)?;
        resume.lock().unwrap_or_else(|e| e.into_inner()).node = Some(id.clone());
        self.entry_by_id(space, &id).await
    }

    /// Streams the file to the upload URL of `account` and returns the blob id. The file is
    /// read piece by piece on a thread of its own; it is never held whole in memory.
    async fn upload_file(&self, account: &str, path: &Path, size: u64, media_type: &str) -> Result<String> {
        let failed = |e: &dyn std::fmt::Display| Error::temporary(format!("upload: {e}"));
        let mut file = fs::File::open(path).map_err(read_error)?;
        let (tx, rx) = tokio::sync::mpsc::channel::<std::io::Result<Vec<u8>>>(4);
        tokio::task::spawn_blocking(move || {
            let mut buf = vec![0_u8; CHUNK];
            loop {
                let item = match file.read(&mut buf) {
                    Ok(0) => return,
                    Ok(n) => Ok(buf[..n].to_vec()),
                    Err(e) => Err(e),
                };
                let failed = item.is_err();
                // A closed channel means the request is over; nothing is left to read for.
                if tx.blocking_send(item).is_err() || failed {
                    return;
                }
            }
        });
        let body = reqwest::Body::wrap_stream(futures_util::stream::unfold(rx, |mut rx| async move {
            rx.recv().await.map(|item| (item, rx))
        }));
        let mut headers = self.client.headers().clone();
        let content_type = reqwest::header::HeaderValue::from_str(media_type)
            .unwrap_or_else(|_| reqwest::header::HeaderValue::from_static("application/octet-stream"));
        headers.insert(reqwest::header::CONTENT_TYPE, content_type);
        headers.insert(reqwest::header::CONTENT_LENGTH, size.into());
        let response = self
            .downloads
            .post(self.upload_url_for(account))
            .headers(headers)
            .body(body)
            .send()
            .await
            .map_err(|e| failed(&e))?;
        let status = response.status().as_u16();
        match status {
            200..=299 => {}
            401 => return Err(unauthorized("upload")),
            429 => {
                let wait = response.headers().get(reqwest::header::RETRY_AFTER).and_then(|v| v.to_str().ok());
                return Err(Error::temporary(upload_budget_message(wait.and_then(|v| v.trim().parse().ok()))));
            }
            413 => return Err(Error::invalid("the server does not take a file this large")),
            400 => {
                let text = response.text().await.unwrap_or_default();
                return Err(Error::invalid(if text.contains("urn:ietf:params:jmap:error:limit") {
                    "the server does not take a file this large".to_string()
                } else {
                    "the server did not accept the upload".to_string()
                }));
            }
            _ => return Err(failed(&format!("the server answered {status}"))),
        }
        let answer: Value = serde_json::from_slice(&response.bytes().await.map_err(|e| failed(&e))?)
            .map_err(|e| failed(&format!("the answer is not JSON: {e}")))?;
        answer["blobId"].as_str().map(str::to_string).ok_or_else(|| failed(&"the answer names no blob"))
    }
}

/// What a repeated upload (after a 401) keeps from the run before.
#[derive(Debug, Default)]
pub struct UploadResume {
    /// The blob already uploaded.
    blob: Option<String>,
    /// The node already made or changed.
    node: Option<String>,
}

/// Only a file may be given new content, and only when the caller said so.
fn check_replaceable(existing: &Value, replace: bool) -> Result<()> {
    match existing["@type"].as_str() {
        Some("File") if replace => Ok(()),
        Some("File") => Err(Error::invalid("a file with this name is already there")),
        Some("Directory") => Err(Error::invalid("a folder with this name is already there")),
        _ => Err(Error::invalid(EXISTS)),
    }
}

fn read_error(e: std::io::Error) -> Error {
    Error::invalid(format!("the file cannot be read: {e}"))
}

fn megabytes(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1_000_000.0)
}

/// Said for HTTP 429: the server's upload budget is used up for some minutes.
fn upload_budget_message(retry_after_secs: Option<u64>) -> String {
    match retry_after_secs {
        Some(secs) => {
            let minutes = secs.div_ceil(60).max(1);
            let unit = if minutes == 1 { "minute" } else { "minutes" };
            format!("the server takes no more uploads for now; try again in {minutes} {unit}")
        }
        None => "the server takes no more uploads for now; try again in a few minutes".to_string(),
    }
}

/// Now as the server writes a time: UTC, to the second.
fn utc_now() -> String {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
    DateTime::from_timestamp(secs, 0).map(|t| t.to_rfc3339_opts(SecondsFormat::Secs, true)).unwrap_or_default()
}

/// The id of the one object a `/set` created, or why it did not.
fn created_id(set: &Value) -> Result<String> {
    if let Some(id) = set["created"][CREATE_ID]["id"].as_str() {
        return Ok(id.to_string());
    }
    match set["notCreated"].get(CREATE_ID) {
        Some(e) => Err(file_error(e)),
        None => Err(Error::temporary("the server's answer does not say whether it was created")),
    }
}

/// Whether a `/set` changed the object, or why not.
fn updated(set: &Value, id: &str) -> Result<()> {
    if let Some(e) = set["notUpdated"].get(id) {
        return Err(file_error(e));
    }
    match set["updated"].get(id) {
        Some(_) => Ok(()),
        None => Err(Error::temporary("the server's answer does not say whether it changed")),
    }
}

/// A SetError of the file storage in words for the user. The server's own description is only
/// passed on for an error this does not know, with its type.
fn file_error(e: &Value) -> Error {
    let kind = e["type"].as_str().unwrap_or("unknown");
    let description = e["description"].as_str().unwrap_or_default();
    let has = |needle: &str| description.to_lowercase().contains(needle);
    let property = |name: &str| e["properties"].as_array().is_some_and(|p| p.iter().any(|p| p == name));
    let words = match kind {
        "alreadyExists" => EXISTS.to_string(),
        "notFound" => GONE.to_string(),
        "forbidden" => "you are not allowed to do this here".to_string(),
        "overQuota" if has("folders") => "there are too many folders; delete some first".to_string(),
        "overQuota" if has("files") => "there are too many files; delete some first".to_string(),
        "overQuota" => "the storage is full; delete something first".to_string(),
        "nodeHasChildren" => "the folder is not empty".to_string(),
        "invalidProperties" if property("name") && has("forbidden character") => {
            "this name has a character that is not allowed (/ < > : \" \\ | ? * or a control character)".to_string()
        }
        "invalidProperties" if property("name") && has("reserved") => "the server does not allow this name".to_string(),
        "invalidProperties" if property("name") && has("octets") => {
            "a name must be between 1 and 255 bytes long".to_string()
        }
        "invalidProperties" if property("parentId") && has("circular") => {
            "a folder cannot go into itself or into a folder inside it".to_string()
        }
        "invalidProperties" if property("parentId") && has("depth") => "the folders are nested too deeply".to_string(),
        "invalidProperties" if property("parentId") => "the folder is gone, or it is not a folder".to_string(),
        "invalidProperties" if property("blobId") => "the server did not accept the uploaded content".to_string(),
        "serverFail" | "rateLimit" => return Error::temporary(format!("the server could not do it now ({kind})")),
        _ if description.is_empty() => format!("the server could not do it ({kind})"),
        _ => format!("the server could not do it ({kind}): {description}"),
    };
    Error::invalid(words)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(e: Value) -> String {
        match file_error(&e) {
            Error::Invalid { msg } => msg,
            other => panic!("not invalid: {other:?}"),
        }
    }

    #[test]
    fn set_errors_become_words() {
        assert!(words(json!({ "type": "alreadyExists", "existingId": "c" })).contains("already there"));
        let name = |d: &str| json!({ "type": "invalidProperties", "description": d, "properties": ["name"] });
        assert!(words(name("Name contains a forbidden character.")).contains("character"));
        assert!(words(name("Name is reserved and cannot be used.")).contains("does not allow"));
        assert!(words(name("Name must be between 1 and 255 octets.")).contains("255"));
        let parent = |d: &str| json!({ "type": "invalidProperties", "description": d, "properties": ["parentId"] });
        assert!(words(parent("Circular reference in parent ids.")).contains("into itself"));
        assert!(words(parent("Maximum folder depth exceeded.")).contains("nested"));
        let quota = |d: &str| json!({ "type": "overQuota", "description": d });
        assert!(words(quota("There are too many folders, please ...")).contains("folders"));
        assert!(words(quota("There are too many files, please ...")).contains("files"));
        assert!(words(quota("The disk quota has been exceeded.")).contains("full"));
        assert!(
            words(json!({ "type": "forbidden", "description": "You do not have permission" })).contains("not allowed")
        );
    }

    #[test]
    fn an_unknown_set_error_keeps_its_type() {
        let msg = words(json!({ "type": "somethingNew", "description": "odd" }));
        assert!(msg.contains("somethingNew") && msg.contains("odd"));
        assert!(matches!(file_error(&json!({ "type": "serverFail" })), Error::Temporary { .. }));
    }

    #[test]
    fn the_upload_budget_is_told_in_minutes() {
        assert!(upload_budget_message(Some(213)).contains("4 minutes"));
        assert!(upload_budget_message(Some(10)).contains("1 minute"));
        assert!(upload_budget_message(None).contains("a few minutes"));
    }

    #[test]
    fn a_file_takes_its_rights_from_its_folder() {
        let space = Space { id: "a".into(), own: true };
        let file = json!({ "id": "f", "@type": "File", "name": "x", "size": 3, "blobId": "b", "mediaType": "text/plain",
            "modified": "2026-11-14T09:53:47Z" });
        let rights = Rights { add: true, rename: true, delete: false, modify: true };
        let entry = entry_from(&space, &file, rights).unwrap();
        assert!(entry.may_rename && !entry.may_delete && entry.may_replace && !entry.may_add);
        assert_eq!(entry.modified, Some(1_794_650_027));
        let link = json!({ "id": "l", "@type": "SymLink", "name": "l" });
        let entry = entry_from(&space, &link, ALL_RIGHTS).unwrap();
        assert_eq!((entry.kind, entry.blob_id, entry.may_replace), (FileKind::Other, None, false));
    }
}
