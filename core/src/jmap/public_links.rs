//! Public links to files: anyone with the link can download the file, until the link
//! expires. Only servers built from the Stalwart fork with share links have them
//! (github.com/Kaan88/stalwart, `FORK.md`); its API is plain JSON over
//! `POST /api/sharelink/{limits,list,create,revoke}`, signed in like JMAP. Official
//! Stalwart answers 404, and then the app offers no links.
//!
//! - Only the user's own files: node ids repeat between accounts, and the server only
//!   lets the owner make links. Every request also names the file's name and size, and
//!   the server refuses one that does not match, so a link is never made for another file.
//! - A link is built from the server's own address and the path the server returns, so
//!   the app shows exactly what it would open.
//! - The link is a secret: it is never logged, put into an error or cached. The links
//!   are read from the server each time.

use serde_json::{json, Value};

use super::{
    errors::{forbidden, unauthorized},
    Connection,
};
use crate::{
    model::{FileEntry, FileKind, PublicLink, PublicLinkLimits},
    Error, Result,
};

const NOT_OWN: &str = "public links can only be made for your own files";
const NOT_FILE: &str = "only files can have a public link, not folders";

impl Connection {
    /// What the server allows, or `None` when it has no public links (official Stalwart).
    pub(crate) async fn public_link_limits(&self) -> Result<Option<PublicLinkLimits>> {
        let Some(body) = self.share_link_call("limits", json!({}), true).await? else {
            return Ok(None);
        };
        let number = |key: &str| body[key].as_u64().ok_or_else(|| malformed(key));
        Ok(Some(PublicLinkLimits {
            default_expires_in: number("defaultExpiresIn")?,
            min_expires_in: number("minExpiresIn")?,
            max_expires_in: number("maxExpiresIn")?,
            max_per_file: u32::try_from(number("maxLinksPerFile")?).unwrap_or(u32::MAX),
        }))
    }

    /// The live links of one of the user's own files, newest first.
    pub(crate) async fn public_links(&self, file: &FileEntry) -> Result<Vec<PublicLink>> {
        let body = self.share_link_call("list", file_ref(file)?, false).await?.unwrap_or(Value::Null);
        let mut links = body
            .as_array()
            .ok_or_else(|| malformed("list"))?
            .iter()
            .map(|link| self.to_link(link))
            .collect::<Result<Vec<_>>>()?;
        links.sort_by_key(|link| std::cmp::Reverse(link.created));
        Ok(links)
    }

    /// Makes a link valid for `expires_in` seconds (the server keeps it within its limits).
    pub(crate) async fn public_link_create(&self, file: &FileEntry, expires_in: u64) -> Result<PublicLink> {
        let mut body = file_ref(file)?;
        body["expiresIn"] = json!(expires_in);
        let answer = self.share_link_call("create", body, false).await?.unwrap_or(Value::Null);
        self.to_link(&answer)
    }

    /// Revokes a link: it stops working at once. Revoking one that is gone is not an error.
    pub(crate) async fn public_link_revoke(&self, file: &FileEntry, link_id: &str) -> Result<()> {
        let mut body = file_ref(file)?;
        body["linkId"] = json!(link_id);
        self.share_link_call("revoke", body, false).await.map(|_| ())
    }

    /// One call. With `missing_ok`, a server without the API (404) is `Ok(None)`.
    async fn share_link_call(&self, action: &str, body: Value, missing_ok: bool) -> Result<Option<Value>> {
        let failed = |e: &dyn std::fmt::Display| Error::temporary(format!("public links: {e}"));
        let url = format!("{}/api/sharelink/{action}", self.origin);
        let answer = self
            .http
            .post(url)
            .headers(self.client.headers().clone())
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body.to_string())
            .send()
            .await
            .map_err(|e| failed(&e.without_url()))?;
        let status = answer.status().as_u16();
        let bytes = answer.bytes().await.map_err(|e| failed(&e.without_url()))?;
        match status {
            200..=299 => serde_json::from_slice(&bytes).map(Some).map_err(|_| malformed(action)),
            404 if missing_ok => Ok(None),
            401 => Err(unauthorized("api/sharelink: HTTP 401")),
            403 => Err(forbidden("api/sharelink: HTTP 403")),
            404 => Err(Error::invalid("this server has no public links, or the file is gone")),
            400 | 429 => {
                // The server's own words ("The file does not match …", "Too many links …").
                let detail: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
                let words = detail["detail"].as_str().unwrap_or("the server refused the request");
                Err(Error::invalid(words.chars().take(200).collect::<String>()))
            }
            _ => Err(failed(&format!("HTTP {status}"))),
        }
    }

    fn to_link(&self, link: &Value) -> Result<PublicLink> {
        let path = link["path"].as_str().filter(|p| p.starts_with("/s/")).ok_or_else(|| malformed("path"))?;
        Ok(PublicLink {
            id: link["id"].as_str().ok_or_else(|| malformed("id"))?.to_string(),
            url: format!("{}{}", self.origin, path),
            created: link["created"].as_i64().ok_or_else(|| malformed("created"))?,
            expires: link["expires"].as_i64().ok_or_else(|| malformed("expires"))?,
        })
    }
}

/// The file as the request names it: id, and the name and size the server checks.
fn file_ref(file: &FileEntry) -> Result<Value> {
    if file.account_id.is_some() {
        return Err(Error::invalid(NOT_OWN));
    }
    if file.kind != FileKind::File {
        return Err(Error::invalid(NOT_FILE));
    }
    Ok(json!({ "fileId": file.id, "expectName": file.name, "expectSize": file.size }))
}

fn malformed(what: &str) -> Error {
    Error::temporary(format!("public links: the server's answer has no valid {what}"))
}
