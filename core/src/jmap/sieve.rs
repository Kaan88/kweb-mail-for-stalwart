//! Filter scripts (RFC 9661): `SieveScript` over plain JMAP requests.
//!
//! What Stalwart 1.1.0 does, found against the real server (`dev/notes/stalwart-sieve.md`): a
//! script is made from an uploaded blob (media type `application/sieve`); the blob id the
//! server hands back after a create or update differs from the one uploaded; the text is read
//! through the download URL; `SieveScript/validate` takes a blob id; one script is active at a
//! time, and activating one deactivates the other, the away message's script included (the
//! `set` answer does not say so); the error names are `invalidScript` and `scriptIsActive`
//! where the RFC says `invalidSieve` and `sieveIsActive`, and both are understood here. The
//! server does not check `require` against its own extension list, so the caller must.

use serde_json::{json, Value};

use super::{
    send::{response, set_error},
    Connection, MAIL_CAPABILITY,
};
use crate::{Error, Result};

pub const SIEVE_CAPABILITY: &str = "urn:ietf:params:jmap:sieve";
const CREATE_ID: &str = "s1";

/// What the account's Sieve capability says.
#[derive(Debug, Clone, PartialEq)]
pub struct SieveLimits {
    /// The extensions a script may `require`.
    pub extensions: Vec<String>,
    /// The largest script in bytes; `None` when the server does not say.
    pub max_size: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SieveScript {
    pub id: String,
    pub name: String,
    pub blob_id: String,
    pub is_active: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SieveList {
    pub scripts: Vec<SieveScript>,
    /// The state of the scripts, which the away message shares.
    pub state: String,
}

/// What the host is told when the scripts moved under it.
pub const CHANGED: &str = "The filters changed elsewhere. Read them again.";

impl Connection {
    pub fn has_sieve(&self) -> bool {
        self.has_capability(SIEVE_CAPABILITY)
    }

    fn sieve_account(&self) -> String {
        self.account_for(SIEVE_CAPABILITY)
    }

    /// The extensions and the size limit of the account's scripts.
    pub async fn sieve_limits(&self) -> Result<SieveLimits> {
        // Read afresh: the extensions and the limit can change while the app is signed in.
        let session = self.session_json().await?;
        let caps = &session["accounts"][self.sieve_account()]["accountCapabilities"][SIEVE_CAPABILITY];
        let extensions = caps["sieveExtensions"].as_array().into_iter().flatten();
        Ok(SieveLimits {
            extensions: extensions.filter_map(|e| e.as_str().map(str::to_string)).collect(),
            max_size: caps["maxSizeScript"].as_u64(),
        })
    }

    /// Every script of the account, with the state they were read at.
    pub async fn sieve_scripts(&self) -> Result<SieveList> {
        let args = json!({ "accountId": self.sieve_account() });
        let responses = self.call(&[SIEVE_CAPABILITY], &[("SieveScript/get", args)]).await?;
        let get = response(&responses, "SieveScript/get")?;
        let scripts = get["list"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|s| {
                Some(SieveScript {
                    id: s["id"].as_str()?.to_string(),
                    name: s["name"].as_str().unwrap_or_default().to_string(),
                    blob_id: s["blobId"].as_str()?.to_string(),
                    is_active: s["isActive"] == true,
                })
            })
            .collect();
        let state =
            get["state"].as_str().ok_or_else(|| Error::temporary("the server gave no state for its scripts"))?;
        Ok(SieveList { scripts, state: state.to_string() })
    }

    /// The text of a script.
    pub async fn sieve_text(&self, script: &SieveScript) -> Result<String> {
        let mut bytes = Vec::new();
        self.download_from(&self.sieve_account(), &script.blob_id, &mut bytes).await?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    /// Puts script text on the server as a blob, for a validation or a write.
    pub async fn sieve_upload(&self, text: &str) -> Result<String> {
        let uploaded = self.upload(text.as_bytes().to_vec(), "application/sieve").await?;
        Ok(uploaded.blob_id)
    }

    /// The server's reason for refusing the uploaded script, or `None` when it is fine.
    pub async fn sieve_validate(&self, blob_id: &str) -> Result<Option<String>> {
        let args = json!({ "accountId": self.sieve_account(), "blobId": blob_id });
        let responses = self.call(&[SIEVE_CAPABILITY], &[("SieveScript/validate", args)]).await?;
        let answer = response(&responses, "SieveScript/validate")?;
        let error = &answer["error"];
        if error.is_null() {
            return Ok(None);
        }
        let kind = error["type"].as_str().unwrap_or("unknown");
        let detail = error["description"].as_str().filter(|d| !d.is_empty()).unwrap_or(kind);
        match kind {
            "invalidScript" | "invalidSieve" | "tooLarge" => Ok(Some(detail.to_string())),
            _ => Err(Error::temporary(format!("the server could not check the script: {detail}"))),
        }
    }

    /// Writes the uploaded script as `name`: a new script, or the text of `existing`. With
    /// `activate` the server also makes it the active one. `state` is what the scripts were at:
    /// a change since is refused as "changed elsewhere".
    pub async fn sieve_write(
        &self,
        name: &str,
        existing: Option<&str>,
        blob_id: &str,
        state: &str,
        activate: bool,
    ) -> Result<()> {
        let mut args = json!({ "accountId": self.sieve_account(), "ifInState": state });
        match existing {
            Some(id) => {
                args["update"] = json!({ id: { "blobId": blob_id } });
                if activate {
                    args["onSuccessActivateScript"] = json!(id);
                }
            }
            None => {
                args["create"] = json!({ CREATE_ID: { "name": name, "blobId": blob_id } });
                if activate {
                    args["onSuccessActivateScript"] = json!(format!("#{CREATE_ID}"));
                }
            }
        }
        let responses = self.call(&[SIEVE_CAPABILITY], &[("SieveScript/set", args)]).await.map_err(moved)?;
        let set = response(&responses, "SieveScript/set")?;
        let (refused, done) = match existing {
            Some(id) => (set["notUpdated"].get(id), set["updated"].get(id).is_some()),
            None => (set["notCreated"].get(CREATE_ID), set["created"].get(CREATE_ID).is_some()),
        };
        if let Some(e) = refused {
            return Err(script_error(e));
        }
        match done {
            true => Ok(()),
            false => Err(Error::temporary("the server's answer does not say whether the script was written")),
        }
    }

    /// Makes `id` the active script, or with `None` leaves no script active. The server's
    /// answer does not say what it displaced: read the scripts again.
    pub async fn sieve_activate(&self, id: Option<&str>, state: &str) -> Result<()> {
        let mut args = json!({ "accountId": self.sieve_account(), "ifInState": state });
        match id {
            Some(id) => args["onSuccessActivateScript"] = json!(id),
            None => args["onSuccessDeactivateScript"] = json!(true),
        }
        let responses = self.call(&[SIEVE_CAPABILITY], &[("SieveScript/set", args)]).await.map_err(moved)?;
        response(&responses, "SieveScript/set").map(drop)
    }

    /// The List-Id header of a message, as text; `None` when it has none or the server does
    /// not have the message.
    pub async fn list_id(&self, email_id: &str) -> Result<Option<String>> {
        let property = "header:list-id:asText";
        let args = json!({ "accountId": self.account_id(), "ids": [email_id], "properties": [property] });
        let responses = self.call(&[MAIL_CAPABILITY], &[("Email/get", args)]).await?;
        let get = response(&responses, "Email/get")?;
        let value = get["list"].get(0).and_then(|e| e[property].as_str()).map(str::trim);
        Ok(value.filter(|v| !v.is_empty()).map(str::to_string))
    }
}

/// A request refused because the scripts moved on (`ifInState`) says so in words.
fn moved(e: Error) -> Error {
    if super::errors::is_state_mismatch(&e) {
        Error::invalid(CHANGED)
    } else {
        e
    }
}

/// A refused script write, in words for the user.
fn script_error(e: &Value) -> Error {
    let kind = e["type"].as_str().unwrap_or("unknown");
    let detail = e["description"].as_str().filter(|d| !d.is_empty()).unwrap_or(kind);
    match kind {
        "stateMismatch" | "alreadyExists" => Error::invalid(CHANGED),
        "invalidScript" | "invalidSieve" | "tooLarge" => {
            Error::invalid(format!("The server does not accept the filters: {detail}"))
        }
        _ => set_error(e),
    }
}
