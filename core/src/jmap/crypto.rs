//! What the encryption feature needs from the server: the type structure of a message (cheap, no
//! body) and its raw RFC 5322 bytes (only for mail that turned out to be signed or encrypted).

use std::io::Write;

use serde_json::{json, Value};

use super::{send::response, Connection, DownloadWatch, MAIL_CAPABILITY};
use crate::{crypto::mime::TypeTree, Error, Result};

/// The largest message that is fetched whole to be checked or opened. A bigger one is reported
/// as too large, never half read: a signature over a cut message means nothing.
pub const MAX_RAW_BYTES: u64 = 40 * 1024 * 1024;

/// How deep the structure the server sends is read. A message nested deeper than this is read as far as this.
const MAX_STRUCTURE_DEPTH: usize = 12;

/// What the server says about the shape of a message.
#[derive(Debug, Clone, PartialEq)]
pub struct Structure {
    /// The blob of the whole message as received.
    pub blob_id: String,
    pub size: u64,
    pub tree: TypeTree,
}

fn tree_of(part: &Value, depth: usize) -> TypeTree {
    let media = part["type"].as_str().unwrap_or("application/octet-stream").to_ascii_lowercase();
    let children = match part["subParts"].as_array() {
        Some(parts) if depth < MAX_STRUCTURE_DEPTH => parts.iter().take(64).map(|p| tree_of(p, depth + 1)).collect(),
        _ => Vec::new(),
    };
    TypeTree { media, children }
}

/// A writer that stops at a limit, remembering that it did.
struct Limited {
    bytes: Vec<u8>,
    max: usize,
    exceeded: bool,
}

impl Write for Limited {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.bytes.len() + buf.len() > self.max {
            self.exceeded = true;
            return Err(std::io::Error::other("too large"));
        }
        self.bytes.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Connection {
    /// The media types of the parts of a message, with its blob id and size. `None` when the
    /// server does not have the message.
    pub async fn message_structure(&self, message_id: &str) -> Result<Option<Structure>> {
        let args = json!({
            "accountId": self.account_id(),
            "ids": [message_id],
            "properties": ["blobId", "size", "bodyStructure"],
            "bodyProperties": ["partId", "type"],
        });
        let responses = self.call(&[MAIL_CAPABILITY], &[("Email/get", args)]).await?;
        let get = response(&responses, "Email/get")?;
        let Some(email) = get["list"].get(0) else { return Ok(None) };
        Ok(Some(Structure {
            blob_id: email["blobId"].as_str().unwrap_or_default().to_string(),
            size: email["size"].as_u64().unwrap_or(0),
            tree: tree_of(&email["bodyStructure"], 0),
        }))
    }

    /// The raw bytes of a message. A message over `max` bytes is `Error::Invalid`.
    pub async fn raw_message(&self, blob_id: &str, max: u64) -> Result<Vec<u8>> {
        let mut out = Limited { bytes: Vec::new(), max: max as usize, exceeded: false };
        let mut progress = |_: u64, _: Option<u64>| {};
        let result = self
            .download_tracked(&self.account_id(), blob_id, &mut out, &DownloadWatch::default(), &mut progress)
            .await;
        if out.exceeded {
            return Err(Error::invalid("the message is too large to check"));
        }
        result.map(|_| out.bytes)
    }
}
