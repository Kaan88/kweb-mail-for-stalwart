//! A message the app wrote itself (signed or encrypted, M9) is put on the server whole, as the
//! finished RFC 5322 bytes: `Email/set` would write the headers and the MIME structure itself, and
//! a signature covers exact bytes.

use serde_json::json;

use super::{
    send::{created_id, response, CREATE_ID},
    Connection, MAIL_CAPABILITY,
};
use crate::Result;

impl Connection {
    /// Uploads `raw` and makes it the server's copy of a draft in Drafts (marked `$draft` and `$seen`), and, in the
    /// same request, destroys the copy it replaces. Returns the id of the new copy. A copy the server will not
    /// take is `Error::Invalid`; anything else that goes wrong is a failure that may pass, and the same bytes
    /// are uploaded again by the next try.
    pub async fn import_draft_copy(&self, raw: &[u8], drafts_id: &str, replaces: Option<&str>) -> Result<String> {
        let blob = self.upload(raw.to_vec(), "message/rfc822").await?;
        let email = json!({
            "blobId": blob.blob_id,
            "mailboxIds": { drafts_id: true },
            "keywords": { "$draft": true, "$seen": true },
        });
        let import = json!({ "accountId": self.account_id, "emails": { CREATE_ID: email } });
        let mut calls = vec![("Email/import", import)];
        if let Some(old) = replaces {
            calls.push(("Email/set", json!({ "accountId": self.account_id, "destroy": [old] })));
        }
        let responses = self.call(&[MAIL_CAPABILITY], &calls).await?;
        created_id(response(&responses, "Email/import")?)
    }
}
