//! Messages the server holds back, and the sender identities' own requests.
//!
//! A held message is an `EmailSubmission` whose envelope carries `HOLDFOR` or `HOLDUNTIL` (the
//! values are strings; Stalwart takes at most seven days). While it is `pending` it can be
//! cancelled; destroying the submission does not stop it. What Stalwart does at the moment of
//! the submission, not at the release, is the move out of Drafts that rides on it, so a
//! cancelled message has to be put back by the client (`restore_draft`).

use chrono::DateTime;
use serde_json::{json, Map, Value};

use super::{
    send::{created_id, destroyed, response, set_error, updated},
    Connection, MAIL_CAPABILITY,
};
use crate::{compose::rules::iso_utc, Error, Result};

const SUBMISSION_CAPABILITY: &str = "urn:ietf:params:jmap:submission";

/// The most the server holds a message back: seven days, in seconds.
pub const MAX_HOLD_SECS: u32 = 604_800;

/// When the server lets a message go.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Hold {
    /// This many seconds from now.
    For(u32),
    /// At this time (Unix seconds).
    Until(i64),
}

/// A submission with an envelope of its own, which is how the hold is asked for. The server
/// needs the sender and every recipient spelled out there.
#[derive(Debug, Clone, PartialEq)]
pub struct Held {
    pub from: String,
    pub to: Vec<String>,
    pub hold: Hold,
}

impl Held {
    pub(super) fn envelope(&self) -> Value {
        let parameter = match self.hold {
            Hold::For(secs) => json!({ "HOLDFOR": secs.to_string() }),
            Hold::Until(at) => json!({ "HOLDUNTIL": iso_utc(at) }),
        };
        let recipients: Vec<Value> = self.to.iter().map(|email| json!({ "email": email })).collect();
        json!({ "mailFrom": { "email": self.from, "parameters": parameter }, "rcptTo": recipients })
    }
}

/// Where a submission stands.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Undo {
    /// Held back: it can still be cancelled.
    Pending,
    /// Out of the server's hands (sent, or failed: the delivery status says which).
    Final,
    /// Taken back: it will never be sent.
    Canceled,
}

/// What the server says about one submission.
#[derive(Debug, Clone, PartialEq)]
pub struct SubmissionInfo {
    pub id: String,
    pub email_id: String,
    pub identity_id: String,
    pub undo: Undo,
    /// When it is or was released, Unix seconds.
    pub send_at: i64,
}

/// What one held message looks like to the user.
#[derive(Debug, Clone, PartialEq)]
pub struct EmailSummary {
    pub id: String,
    pub subject: String,
    /// To, then Cc, then Bcc.
    pub recipients: Vec<String>,
}

/// An identity with everything the server says about it.
#[derive(Debug, Clone, PartialEq)]
pub struct IdentityFull {
    pub id: String,
    pub name: String,
    pub email: String,
    /// The Reply-To addresses, joined with ", ".
    pub reply_to: String,
    pub signature: String,
    pub may_delete: bool,
}

const SUBMISSION_PROPERTIES: [&str; 5] = ["emailId", "identityId", "undoStatus", "sendAt", "id"];

impl Connection {
    /// The submissions that match `filter`, with their state. A message that was submitted
    /// more than once (cancelled and sent again) has one entry for each.
    async fn submissions_where(&self, filter: Value) -> Result<Vec<SubmissionInfo>> {
        let account = self.account_for(SUBMISSION_CAPABILITY);
        let query = json!({ "accountId": account, "filter": filter });
        let get = json!({
            "accountId": account,
            "#ids": { "resultOf": "c0", "name": "EmailSubmission/query", "path": "/ids" },
            "properties": SUBMISSION_PROPERTIES,
        });
        let calls = [("EmailSubmission/query", query), ("EmailSubmission/get", get)];
        let responses = self.call(&[MAIL_CAPABILITY, SUBMISSION_CAPABILITY], &calls).await?;
        Ok(submissions_from(response(&responses, "EmailSubmission/get")?))
    }

    /// Every submission of this message, held, sent or cancelled: how a send whose answer was
    /// lost is settled.
    pub async fn submissions_of(&self, email_id: &str) -> Result<Vec<SubmissionInfo>> {
        self.submissions_where(json!({ "emailIds": [email_id] })).await
    }

    /// The submissions the server holds back now (not sorted: the server cannot sort by time).
    pub async fn pending_submissions(&self) -> Result<Vec<SubmissionInfo>> {
        self.submissions_where(json!({ "undoStatus": "pending" })).await
    }

    /// The pending submissions made as one identity.
    pub async fn pending_submissions_of_identity(&self, identity_id: &str) -> Result<Vec<SubmissionInfo>> {
        self.submissions_where(json!({ "undoStatus": "pending", "identityIds": [identity_id] })).await
    }

    /// One submission; `None` when the server has none with this id.
    pub async fn submission(&self, id: &str) -> Result<Option<SubmissionInfo>> {
        let args = json!({
            "accountId": self.account_for(SUBMISSION_CAPABILITY),
            "ids": [id],
            "properties": SUBMISSION_PROPERTIES,
        });
        let responses = self.call(&[MAIL_CAPABILITY, SUBMISSION_CAPABILITY], &[("EmailSubmission/get", args)]).await?;
        Ok(submissions_from(response(&responses, "EmailSubmission/get")?).into_iter().next())
    }

    /// Takes a held message back: it will not be sent. One that has left already is
    /// `Error::Invalid`. Destroying the submission would not do this: the message would still go.
    pub async fn cancel_submission(&self, id: &str) -> Result<()> {
        let args = json!({
            "accountId": self.account_for(SUBMISSION_CAPABILITY),
            "update": { id: { "undoStatus": "canceled" } },
        });
        let responses = self.call(&[MAIL_CAPABILITY, SUBMISSION_CAPABILITY], &[("EmailSubmission/set", args)]).await?;
        let set = response(&responses, "EmailSubmission/set")?;
        if let Some(e) = set["notUpdated"].get(id) {
            return Err(match e["type"].as_str() {
                Some("cannotUnsend") => Error::invalid("too late: the server has already sent it"),
                Some("notFound") => Error::invalid("the server holds no such message"),
                _ => set_error(e),
            });
        }
        match set["updated"].get(id) {
            Some(_) => Ok(()),
            None => Err(Error::temporary("the server's answer does not say whether it was cancelled")),
        }
    }

    /// Makes a message that was handed over a draft again: back in Drafts and marked `$draft`
    /// (and so out of Sent, where the submission put it). A message the server no longer has is
    /// `Error::Invalid`.
    pub async fn restore_draft(&self, email_id: &str, drafts_id: &str) -> Result<()> {
        let patch = json!({ "mailboxIds": { drafts_id: true }, "keywords/$draft": true });
        let args = json!({ "accountId": self.account_id, "update": { email_id: patch } });
        let responses = self.call(&[MAIL_CAPABILITY], &[("Email/set", args)]).await?;
        updated(response(&responses, "Email/set")?, email_id, "the message is gone from the server")
    }

    /// The subject and the recipients of messages, for the list of what is held back.
    pub async fn email_summaries(&self, ids: &[String]) -> Result<Vec<EmailSummary>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let args = json!({ "accountId": self.account_id, "ids": ids, "properties": ["subject", "to", "cc", "bcc"] });
        let responses = self.call(&[MAIL_CAPABILITY], &[("Email/get", args)]).await?;
        let get = response(&responses, "Email/get")?;
        let addresses = |email: &Value, key: &str| -> Vec<String> {
            let list = email[key].as_array().into_iter().flatten();
            list.filter_map(|a| a["email"].as_str().map(str::to_string)).collect()
        };
        Ok(get["list"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|email| {
                Some(EmailSummary {
                    id: email["id"].as_str()?.to_string(),
                    subject: email["subject"].as_str().unwrap_or_default().to_string(),
                    recipients: ["to", "cc", "bcc"].iter().flat_map(|key| addresses(email, key)).collect(),
                })
            })
            .collect())
    }

    // ---- identities ----

    /// The identities with all that can be changed, in the server's order.
    pub async fn identities_full(&self) -> Result<Vec<IdentityFull>> {
        let args = json!({
            "accountId": self.account_for(SUBMISSION_CAPABILITY),
            "properties": ["name", "email", "replyTo", "textSignature", "mayDelete"],
        });
        let responses = self.call(&[SUBMISSION_CAPABILITY], &[("Identity/get", args)]).await?;
        Ok(identities_full_from(response(&responses, "Identity/get")?))
    }

    /// Makes an identity. Only these fields are sent: the HTML signature stays what it is.
    pub async fn identity_create(
        &self,
        name: &str,
        email: &str,
        reply_to: &[String],
        signature: &str,
    ) -> Result<String> {
        let mut identity = json!({ "name": name, "email": email, "textSignature": signature });
        if !reply_to.is_empty() {
            identity["replyTo"] = reply_to_json(reply_to);
        }
        let args = json!({
            "accountId": self.account_for(SUBMISSION_CAPABILITY),
            "create": { super::send::CREATE_ID: identity },
        });
        let responses = self.call(&[SUBMISSION_CAPABILITY], &[("Identity/set", args)]).await?;
        created_id(response(&responses, "Identity/set")?)
    }

    /// Changes an identity: `patch` holds only `name`, `replyTo` and `textSignature` (the server
    /// refuses the address and `mayDelete`, and the HTML signature is not ours).
    pub async fn identity_update(&self, id: &str, patch: Map<String, Value>) -> Result<()> {
        let args = json!({
            "accountId": self.account_for(SUBMISSION_CAPABILITY),
            "update": { id: Value::Object(patch) },
        });
        let responses = self.call(&[SUBMISSION_CAPABILITY], &[("Identity/set", args)]).await?;
        updated(response(&responses, "Identity/set")?, id, "this sender no longer exists")
    }

    pub async fn identity_destroy(&self, id: &str) -> Result<()> {
        let args = json!({ "accountId": self.account_for(SUBMISSION_CAPABILITY), "destroy": [id] });
        let responses = self.call(&[SUBMISSION_CAPABILITY], &[("Identity/set", args)]).await?;
        destroyed(response(&responses, "Identity/set")?, id)
    }
}

/// `replyTo` as the server takes it: a list of addresses.
pub fn reply_to_json(addresses: &[String]) -> Value {
    addresses.iter().map(|email| json!({ "email": email })).collect()
}

fn submissions_from(get: &Value) -> Vec<SubmissionInfo> {
    let text = |v: &Value| v.as_str().unwrap_or_default().to_string();
    let one = |s: &Value| {
        let id = s["id"].as_str().filter(|id| !id.is_empty())?;
        let undo = match s["undoStatus"].as_str() {
            Some("pending") => Undo::Pending,
            Some("canceled") => Undo::Canceled,
            _ => Undo::Final,
        };
        let send_at =
            s["sendAt"].as_str().and_then(|t| DateTime::parse_from_rfc3339(t).ok()).map_or(0, |t| t.timestamp());
        Some(SubmissionInfo {
            id: id.to_string(),
            email_id: text(&s["emailId"]),
            identity_id: text(&s["identityId"]),
            undo,
            send_at,
        })
    };
    get["list"].as_array().into_iter().flatten().filter_map(one).collect()
}

fn identities_full_from(get: &Value) -> Vec<IdentityFull> {
    let text = |v: &Value| v.as_str().unwrap_or_default().to_string();
    let one = |i: &Value| {
        let (id, email) = (text(&i["id"]), text(&i["email"]));
        if id.is_empty() || email.is_empty() {
            return None;
        }
        let reply_to: Vec<String> = i["replyTo"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|a| a["email"].as_str().map(str::to_string))
            .collect();
        Some(IdentityFull {
            id,
            name: text(&i["name"]),
            email,
            reply_to: reply_to.join(", "),
            signature: text(&i["textSignature"]),
            // A server that does not say lets the user try: it refuses by itself.
            may_delete: i["mayDelete"].as_bool().unwrap_or(true),
        })
    };
    get["list"].as_array().into_iter().flatten().filter_map(one).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hold_is_asked_for_in_the_envelope_as_strings() {
        let held = Held { from: "me@x.org".into(), to: vec!["a@x.org".into(), "b@x.org".into()], hold: Hold::For(10) };
        assert_eq!(
            held.envelope(),
            json!({
                "mailFrom": { "email": "me@x.org", "parameters": { "HOLDFOR": "10" } },
                "rcptTo": [{ "email": "a@x.org" }, { "email": "b@x.org" }],
            })
        );
        let at = Held { hold: Hold::Until(1_767_323_045), ..held };
        assert_eq!(at.envelope()["mailFrom"]["parameters"], json!({ "HOLDUNTIL": "2026-01-02T03:04:05Z" }));
    }

    #[test]
    fn submissions_are_read_with_their_state_and_time() {
        let get = json!({ "list": [
            { "id": "s1", "emailId": "e1", "identityId": "i1", "undoStatus": "pending", "sendAt": "2026-01-02T03:04:05Z" },
            { "id": "s2", "emailId": "e1", "undoStatus": "canceled", "sendAt": "2026-01-02T04:00:00+01:00" },
            { "id": "s3", "emailId": "e2", "undoStatus": "final" },
            { "emailId": "nameless" },
        ]});
        let got = submissions_from(&get);
        assert_eq!(got.len(), 3);
        assert_eq!((got[0].undo, got[0].send_at, got[0].identity_id.as_str()), (Undo::Pending, 1_767_323_045, "i1"));
        assert_eq!((got[1].undo, got[1].send_at), (Undo::Canceled, 1_767_322_800));
        assert_eq!((got[2].undo, got[2].send_at), (Undo::Final, 0));
    }

    #[test]
    fn identities_carry_their_reply_to_and_whether_they_can_go() {
        let get = json!({ "list": [
            { "id": "i1", "name": "Me", "email": "me@x.org", "textSignature": "Bye", "mayDelete": false,
              "replyTo": [{ "name": "Desk", "email": "desk@x.org" }, { "email": "b@x.org" }] },
            { "id": "i2", "email": "alias@x.org" },
            { "id": "i3", "name": "No address" },
        ]});
        let got = identities_full_from(&get);
        assert_eq!(got.len(), 2);
        assert_eq!(
            (got[0].reply_to.as_str(), got[0].signature.as_str(), got[0].may_delete),
            ("desk@x.org, b@x.org", "Bye", false)
        );
        assert_eq!((got[1].reply_to.as_str(), got[1].may_delete), ("", true));
        assert_eq!(reply_to_json(&["a@x.org".into()]), json!([{ "email": "a@x.org" }]));
    }
}
