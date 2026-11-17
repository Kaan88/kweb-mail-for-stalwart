//! The public-link API of the Stalwart fork with share links (github.com/Kaan88/stalwart,
//! `FORK.md`): `POST /api/sharelink/{limits,list,create,revoke}`, signed in like JMAP, JSON in
//! and out. Official Stalwart has no such route and answers 404, which is what this mock does
//! until a test turns it on. Like the fork, it checks `expectName`/`expectSize` against the
//! file, keeps expiry within one hour and one year, and allows 20 live links per file.

use std::collections::HashMap;

use serde_json::{json, Value};

pub const MIN_EXPIRY: u64 = 3600;
pub const MAX_EXPIRY: u64 = 365 * 86_400;
pub const MAX_PER_FILE: usize = 20;

#[derive(Default)]
pub struct PublicLinks {
    pub enabled: bool,
    /// File id to its links.
    links: HashMap<String, Vec<Link>>,
    next: u64,
    /// The bodies of the requests received, by action, in order.
    pub requests: Vec<(String, Value)>,
}

struct Link {
    id: String,
    token: String,
    created: u64,
    expires: u64,
}

impl PublicLinks {
    /// Answers one call. `file` is the name and size of the file `fileId` names in the
    /// user's own space, if there is one. Returns the HTTP status and body.
    pub fn call(&mut self, action: &str, body: &Value, file: Option<(String, u64)>, now: u64) -> (u16, Value) {
        self.requests.push((action.to_string(), body.clone()));
        if !self.enabled {
            return (404, json!({ "type": "about:blank", "status": 404, "title": "Not Found" }));
        }
        if action == "limits" {
            return (
                200,
                json!({ "defaultExpiresIn": 604_800, "minExpiresIn": MIN_EXPIRY, "maxExpiresIn": MAX_EXPIRY,
                        "maxLinksPerFile": MAX_PER_FILE }),
            );
        }
        let Some(file_id) = body["fileId"].as_str() else {
            return bad("fileId is required.");
        };
        let Some((name, size)) = file else {
            return (404, json!({ "type": "about:blank", "status": 404, "title": "Not Found" }));
        };
        if body["expectName"].as_str().is_some_and(|n| n != name)
            || body["expectSize"].as_u64().is_some_and(|s| s != size)
        {
            return bad("The file does not match; links can only be made for your own files.");
        }
        let links = self.links.entry(file_id.to_string()).or_default();
        links.retain(|l| l.expires > now);
        match action {
            "list" => (200, Value::Array(links.iter().map(|l| view(file_id, l)).collect())),
            "create" => {
                if links.len() >= MAX_PER_FILE {
                    return (429, json!({ "status": 429, "detail": "Too many links for this file." }));
                }
                self.next += 1;
                let expires_in = body["expiresIn"].as_u64().unwrap_or(604_800).clamp(MIN_EXPIRY, MAX_EXPIRY);
                let link = Link {
                    id: (1000 + self.next).to_string(),
                    token: format!("{:056x}", self.next),
                    created: now + self.next,
                    expires: now + expires_in,
                };
                let answer = view(file_id, &link);
                links.push(link);
                (200, answer)
            }
            "revoke" => {
                let before = links.len();
                let id = body["linkId"].as_str().unwrap_or_default();
                links.retain(|l| l.id != id);
                (200, json!({ "revoked": links.len() != before }))
            }
            _ => (404, json!({ "status": 404 })),
        }
    }

    /// The live links of a file (for tests).
    pub fn count(&self, file_id: &str) -> usize {
        self.links.get(file_id).map_or(0, Vec::len)
    }
}

fn view(file_id: &str, link: &Link) -> Value {
    let path = format!("/s/{}", link.token);
    json!({ "id": link.id, "fileId": file_id, "path": path,
            "url": format!("http://host.invalid{path}"), "created": link.created, "expires": link.expires })
}

fn bad(detail: &str) -> (u16, Value) {
    (400, json!({ "type": "about:blank", "status": 400, "title": "Invalid parameters", "detail": detail }))
}
