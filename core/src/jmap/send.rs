//! Writing and sending: identities, the server copy of a draft, the submission, uploads, the
//! address book and the quotas.
//!
//! jmap-client has no methods for most of this (and none for `Quota` or `ContactCard`), so these
//! are plain JMAP requests, built and read by small pure functions that are tested here. Each
//! request goes through [`Connection::call`], which keeps the kinds of failure apart as the
//! rest of the core needs them: a 401 is recognisable with `is_unauthorized`, a refusal the
//! server will repeat is `Error::Invalid` (the message is shown to the user), anything else
//! is `Error::Temporary`.

use serde_json::{json, Map, Value};

use super::{
    errors::{from_jmap, unauthorized},
    Connection, MAIL_CAPABILITY,
};
use crate::{
    model::{Address, Draft, Identity, Quota},
    Error, Result,
};

const CORE_CAPABILITY: &str = "urn:ietf:params:jmap:core";
const SUBMISSION_CAPABILITY: &str = "urn:ietf:params:jmap:submission";
const CONTACTS_CAPABILITY: &str = "urn:ietf:params:jmap:contacts";
const QUOTA_CAPABILITY: &str = "urn:ietf:params:jmap:quota";

/// The most cards fetched into the address book.
const CONTACT_LIMIT: usize = 2000;

/// The id a request gives the one object it creates.
pub(super) const CREATE_ID: &str = "new";

/// What the server says about the copy of a message it holds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CopyState {
    /// The server has no such message.
    Gone,
    /// It is still a draft: it has not been sent.
    Draft,
    /// It is no longer marked as a draft: the submission went through.
    Sent,
}

/// The Message-ID headers of a message (without angle brackets).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct HeaderIds {
    pub message_id: Option<String>,
    pub in_reply_to: Vec<String>,
    pub references: Vec<String>,
}

/// A file the server now holds.
#[derive(Debug, Clone, PartialEq)]
pub struct Uploaded {
    pub blob_id: String,
    pub size: u64,
}

impl Connection {
    /// The largest file the server takes (bytes); 0 when the session does not say.
    pub fn max_upload(&self) -> u64 {
        self.client.session().core_capabilities().map_or(0, |c| c.max_size_upload() as u64)
    }

    /// The account that holds what `capability` is about; the mail account when the session
    /// names none.
    pub(super) fn account_for(&self, capability: &str) -> String {
        let session = self.client.session();
        let named = session.primary_accounts().find(|(c, _)| c.as_str() == capability);
        named.map_or_else(|| self.account_id.clone(), |(_, account)| account.clone())
    }

    /// One request with these method calls. Returns every response in order, `(method, arguments)`:
    /// a method such as `EmailSubmission/set` may add responses of its own (`Email/set`). A
    /// method that failed as a whole is an error here.
    pub(super) async fn call(&self, capabilities: &[&str], calls: &[(&str, Value)]) -> Result<Vec<(String, Value)>> {
        let what = calls.first().map_or("request", |(name, _)| name);
        let mut using = vec![CORE_CAPABILITY];
        using.extend_from_slice(capabilities);
        let method_calls: Vec<Value> =
            calls.iter().enumerate().map(|(i, (name, args))| json!([name, args, format!("c{i}")])).collect();
        let body = json!({ "using": using, "methodCalls": method_calls });
        let response = self
            .http
            .post(self.client.session().api_url())
            .headers(self.client.headers().clone())
            .body(body.to_string())
            .send()
            .await
            .map_err(|e| Error::temporary(format!("{what}: {e}")))?;
        let status = response.status().as_u16();
        let bytes = response.bytes().await.map_err(|e| Error::temporary(format!("{what}: {e}")))?;
        read_responses(what, status, &bytes)
    }

    /// The sender identities, in the server's order.
    pub async fn identities(&self) -> Result<Vec<Identity>> {
        let args = json!({
            "accountId": self.account_for(SUBMISSION_CAPABILITY),
            "properties": ["name", "email", "textSignature"],
        });
        let responses = self.call(&[SUBMISSION_CAPABILITY], &[("Identity/get", args)]).await?;
        Ok(identities_from(response(&responses, "Identity/get")?))
    }

    /// Every address of the address book, `None` when the server has no contacts.
    pub async fn contacts(&self) -> Result<Option<Vec<Address>>> {
        if !self.has_capability(CONTACTS_CAPABILITY) {
            return Ok(None);
        }
        let account = self.account_for(CONTACTS_CAPABILITY);
        let query = json!({ "accountId": account, "limit": CONTACT_LIMIT });
        let responses = self.call(&[CONTACTS_CAPABILITY], &[("ContactCard/query", query)]).await?;
        let ids: Vec<Value> = response(&responses, "ContactCard/query")?["ids"].as_array().cloned().unwrap_or_default();
        let mut out = Vec::new();
        for chunk in ids.chunks(self.get_batch()) {
            let args = json!({ "accountId": account, "ids": chunk, "properties": ["name", "emails"] });
            let responses = self.call(&[CONTACTS_CAPABILITY], &[("ContactCard/get", args)]).await?;
            for card in response(&responses, "ContactCard/get")?["list"].as_array().into_iter().flatten() {
                out.extend(card_addresses(card));
            }
        }
        Ok(Some(out))
    }

    /// The account's limits and what is used. No quota capability, no limits: an empty list.
    pub async fn quotas(&self) -> Result<Vec<Quota>> {
        if !self.has_capability(QUOTA_CAPABILITY) {
            return Ok(Vec::new());
        }
        let args = json!({ "accountId": self.account_for(QUOTA_CAPABILITY) });
        let responses = self.call(&[QUOTA_CAPABILITY], &[("Quota/get", args)]).await?;
        Ok(quotas_from(response(&responses, "Quota/get")?))
    }

    /// Puts a file on the server. Uses the client without a time limit on the whole transfer
    /// (a big file on a slow line takes as long as it takes). A file the server finds too large
    /// is `Error::Invalid`.
    pub async fn upload(&self, bytes: Vec<u8>, mime_type: &str) -> Result<Uploaded> {
        let failed = |e: &dyn std::fmt::Display| Error::temporary(format!("upload: {e}"));
        let mut headers = self.client.headers().clone();
        let content_type = reqwest::header::HeaderValue::from_str(mime_type)
            .unwrap_or_else(|_| reqwest::header::HeaderValue::from_static("application/octet-stream"));
        headers.insert(reqwest::header::CONTENT_TYPE, content_type);
        let response =
            self.downloads.post(self.upload_url()).headers(headers).body(bytes).send().await.map_err(|e| failed(&e))?;
        let status = response.status().as_u16();
        match status {
            200..=299 => {}
            401 => return Err(unauthorized("upload")),
            413 => return Err(Error::invalid("the server does not take a file this large")),
            _ => return Err(failed(&format!("the server answered {status}"))),
        }
        let answer: Value = serde_json::from_slice(&response.bytes().await.map_err(|e| failed(&e))?)
            .map_err(|e| failed(&format!("the answer is not JSON: {e}")))?;
        match (answer["blobId"].as_str(), answer["size"].as_u64()) {
            (Some(blob_id), Some(size)) => Ok(Uploaded { blob_id: blob_id.to_string(), size }),
            _ => Err(failed(&"the answer names no blob")),
        }
    }

    /// The session's upload URL with the account filled in.
    fn upload_url(&self) -> String {
        use jmap_client::{blob::URLParameter, core::session::URLPart};
        let mut url = String::new();
        for part in self.client.upload_url() {
            match part {
                URLPart::Value(text) => url.push_str(text),
                URLPart::Parameter(URLParameter::AccountId) => url.push_str(&self.account_id),
                URLPart::Parameter(_) => {}
            }
        }
        url
    }

    /// Makes the server's copy of a draft in the Drafts mailbox and, in the same request,
    /// destroys the copy it replaces. `in_reply_to` and `references` are the Message-IDs of a
    /// message that answers another. Returns the id of the new copy.
    pub async fn save_draft_copy(
        &self,
        draft: &Draft,
        from: Option<&Identity>,
        (in_reply_to, references): (&[String], &[String]),
        drafts_id: &str,
        replaces: Option<&str>,
    ) -> Result<String> {
        let email = draft_json(draft, from, (in_reply_to, references), drafts_id);
        let mut args = json!({ "accountId": self.account_id, "create": { CREATE_ID: email } });
        if let Some(old) = replaces {
            args["destroy"] = json!([old]);
        }
        let responses = self.call(&[MAIL_CAPABILITY], &[("Email/set", args)]).await?;
        created_id(response(&responses, "Email/set")?)
    }

    /// Removes a message for good. One the server no longer has is as good as removed.
    pub async fn destroy_email(&self, id: &str) -> Result<()> {
        let args = json!({ "accountId": self.account_id, "destroy": [id] });
        let responses = self.call(&[MAIL_CAPABILITY], &[("Email/set", args)]).await?;
        let set = response(&responses, "Email/set")?;
        match set["notDestroyed"].get(id) {
            Some(e) if e["type"] != "notFound" => Err(set_error(e)),
            _ => Ok(()),
        }
    }

    /// Sends the message the server holds as `email_id`, as `identity_id`. When the server
    /// accepts it, it moves from Drafts to Sent and stops being a draft: the server does that
    /// in the same step, so a send that went through is never still a draft. A refusal the
    /// server will repeat (a bad recipient, too large, not allowed) is `Error::Invalid`.
    pub async fn submit(
        &self,
        identity_id: &str,
        email_id: &str,
        drafts_id: &str,
        sent_id: Option<&str>,
    ) -> Result<()> {
        let args = submission_args(&self.account_for(SUBMISSION_CAPABILITY), identity_id, email_id, drafts_id, sent_id);
        let responses = self.call(&[MAIL_CAPABILITY, SUBMISSION_CAPABILITY], &[("EmailSubmission/set", args)]).await?;
        let set = response(&responses, "EmailSubmission/set")?;
        if let Err(e) = created_id(set) {
            // The server answered, and the answer is no: the message was not sent. Whatever
            // the reason is called, the user has to see it and get the draft back; only a
            // rate limit is worth waiting out.
            return Err(match (not_created_kind(set), e) {
                (Some(kind), Error::Temporary { msg }) if kind != "rateLimit" => Error::invalid(msg),
                (_, e) => e,
            });
        }
        // The move to Sent rides on the submission and can fail on its own. The message is
        // sent either way; a copy left marked as a draft is put right, as far as that goes.
        let moved = responses.iter().any(|(name, args)| name == "Email/set" && args["updated"].get(email_id).is_some());
        if !moved {
            let _ = self.unmark_draft(email_id).await;
        }
        Ok(())
    }

    /// Whether the server holds a submission of this message: the proof that it was sent.
    pub async fn was_submitted(&self, email_id: &str) -> Result<bool> {
        let args = json!({
            "accountId": self.account_for(SUBMISSION_CAPABILITY),
            "filter": { "emailIds": [email_id] },
            "limit": 1,
        });
        let responses =
            self.call(&[MAIL_CAPABILITY, SUBMISSION_CAPABILITY], &[("EmailSubmission/query", args)]).await?;
        Ok(response(&responses, "EmailSubmission/query")?["ids"].as_array().is_some_and(|ids| !ids.is_empty()))
    }

    /// Takes the draft mark off a message that was sent.
    async fn unmark_draft(&self, email_id: &str) -> Result<()> {
        let args = json!({ "accountId": self.account_id, "update": { email_id: { "keywords/$draft": null } } });
        self.call(&[MAIL_CAPABILITY], &[("Email/set", args)]).await.map(drop)
    }

    /// Whether a message the server holds is still a draft: how a send whose answer was lost
    /// is told from one that never arrived.
    pub async fn copy_state(&self, email_id: &str) -> Result<CopyState> {
        let args = json!({ "accountId": self.account_id, "ids": [email_id], "properties": ["keywords"] });
        let responses = self.call(&[MAIL_CAPABILITY], &[("Email/get", args)]).await?;
        let get = response(&responses, "Email/get")?;
        Ok(match get["list"].get(0) {
            None => CopyState::Gone,
            Some(email) if email["keywords"]["$draft"] == true => CopyState::Draft,
            Some(_) => CopyState::Sent,
        })
    }

    /// The Message-ID, In-Reply-To and References of a message; `None` when the server does
    /// not have it.
    pub async fn header_ids(&self, email_id: &str) -> Result<Option<HeaderIds>> {
        let args = json!({
            "accountId": self.account_id,
            "ids": [email_id],
            "properties": ["messageId", "inReplyTo", "references"],
        });
        let responses = self.call(&[MAIL_CAPABILITY], &[("Email/get", args)]).await?;
        let get = response(&responses, "Email/get")?;
        Ok(get["list"].get(0).map(|email| HeaderIds {
            message_id: strings(&email["messageId"]).into_iter().next(),
            in_reply_to: strings(&email["inReplyTo"]),
            references: strings(&email["references"]),
        }))
    }

    /// Marks the message a reply or forward was made from as answered or forwarded.
    pub async fn mark_source(&self, email_id: &str, forwarded: bool) -> Result<()> {
        let keyword = if forwarded { "$forwarded" } else { "$answered" };
        self.client.email_set_keyword(email_id, keyword, true).await.map(drop).map_err(from_jmap)
    }
}

// ---- reading answers ----

/// The responses of a request, or the reason it failed.
fn read_responses(what: &str, status: u16, body: &[u8]) -> Result<Vec<(String, Value)>> {
    if status == 401 {
        return Err(unauthorized(format!("{what}: HTTP {status}")));
    }
    if !(200..300).contains(&status) {
        return Err(Error::temporary(format!("{what}: HTTP {status}")));
    }
    let value: Value =
        serde_json::from_slice(body).map_err(|e| Error::temporary(format!("{what}: the answer is not JSON: {e}")))?;
    let mut out = Vec::new();
    for call in value["methodResponses"].as_array().into_iter().flatten() {
        let (name, args) = (call[0].as_str().unwrap_or_default(), &call[1]);
        if name == "error" {
            return Err(method_error(what, args));
        }
        out.push((name.to_string(), args.clone()));
    }
    Ok(out)
}

/// A method that failed as a whole. What no retry cures is `Invalid`.
fn method_error(what: &str, args: &Value) -> Error {
    let kind = args["type"].as_str().unwrap_or("unknown");
    let detail = args["description"].as_str().filter(|d| !d.is_empty()).unwrap_or(kind);
    match kind {
        "forbidden" | "invalidArguments" | "requestTooLarge" => Error::invalid(format!("{what}: {detail}")),
        "stateMismatch" => super::errors::state_mismatch(format!("{what}: {detail}")),
        _ => Error::temporary(format!("{what} failed: {detail}")),
    }
}

pub(super) fn response<'a>(responses: &'a [(String, Value)], name: &str) -> Result<&'a Value> {
    responses
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, args)| args)
        .ok_or_else(|| Error::temporary(format!("the server's answer has no {name}")))
}

/// Set errors that trying again cannot cure.
const REFUSED_FOR_GOOD: [&str; 12] = [
    "forbidden",
    "invalidProperties",
    "invalidEmail",
    "invalidPatch",
    "tooLarge",
    "noRecipients",
    "tooManyRecipients",
    "forbiddenFrom",
    "forbiddenToSend",
    "forbiddenMailFrom",
    "blobNotFound",
    "tooManyKeywords",
];

/// A SetError object. The text is the server's own words, for the user to read.
pub(super) fn set_error(e: &Value) -> Error {
    let kind = e["type"].as_str().unwrap_or("unknown");
    let detail = e["description"].as_str().filter(|d| !d.is_empty()).unwrap_or(kind);
    if REFUSED_FOR_GOOD.contains(&kind) {
        Error::invalid(format!("the server refused it: {detail}"))
    } else {
        Error::temporary(format!("the server could not do it now: {detail}"))
    }
}

/// The error type the server gave for not creating the one object of a `/set`.
fn not_created_kind(set: &Value) -> Option<&str> {
    set["notCreated"].get(CREATE_ID)?["type"].as_str()
}

/// The id of the one object a `/set` created, or why it did not.
pub(super) fn created_id(set: &Value) -> Result<String> {
    if let Some(id) = set["created"][CREATE_ID]["id"].as_str() {
        return Ok(id.to_string());
    }
    match set["notCreated"].get(CREATE_ID) {
        Some(e) => Err(set_error(e)),
        None => Err(Error::temporary("the server's answer does not say whether it was created")),
    }
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array().into_iter().flatten().filter_map(|s| s.as_str().map(str::to_string)).collect()
}

fn identities_from(get: &Value) -> Vec<Identity> {
    let text = |v: &Value| v.as_str().unwrap_or_default().to_string();
    let one = |i: &Value| {
        let (id, email) = (text(&i["id"]), text(&i["email"]));
        (!id.is_empty() && !email.is_empty()).then(|| Identity {
            id,
            name: text(&i["name"]),
            email,
            signature: text(&i["textSignature"]),
        })
    };
    get["list"].as_array().into_iter().flatten().filter_map(one).collect()
}

/// A JSContact card's addresses, each with the card's name: `name.full`, else the name's
/// components in order.
fn card_addresses(card: &Value) -> Vec<Address> {
    let name = &card["name"];
    let full = name["full"].as_str().map(str::trim).filter(|n| !n.is_empty());
    let name = full.map(str::to_string).unwrap_or_else(|| {
        let parts = name["components"].as_array().into_iter().flatten();
        let words: Vec<&str> =
            parts.filter_map(|p| p["value"].as_str().map(str::trim)).filter(|w| !w.is_empty()).collect();
        words.join(" ")
    });
    let emails = card["emails"].as_object().into_iter().flat_map(Map::values);
    emails
        .filter_map(|e| e["address"].as_str().map(str::trim).filter(|a| !a.is_empty()))
        .map(|email| Address { name: name.clone(), email: email.to_string() })
        .collect()
}

fn quotas_from(get: &Value) -> Vec<Quota> {
    let one = |q: &Value| {
        let name = q["name"].as_str().or_else(|| q["id"].as_str())?;
        Some(Quota {
            name: name.to_string(),
            resource: q["resourceType"].as_str().unwrap_or_default().to_string(),
            used: q["used"].as_u64().unwrap_or(0),
            limit: q["hardLimit"].as_u64(),
        })
    };
    get["list"].as_array().into_iter().flatten().filter_map(one).collect()
}

// ---- building requests ----

fn address_json(a: &Address) -> Value {
    if a.name.trim().is_empty() {
        json!({ "email": a.email.trim() })
    } else {
        json!({ "name": a.name.trim(), "email": a.email.trim() })
    }
}

/// The `Email` object that makes the server's copy of a draft: in Drafts, marked `$draft` and
/// `$seen`, a plain-text body, the files by their blobs. The server writes the headers.
fn draft_json(
    draft: &Draft,
    from: Option<&Identity>,
    (in_reply_to, references): (&[String], &[String]),
    drafts_id: &str,
) -> Value {
    let mut email = json!({
        "mailboxIds": { drafts_id: true },
        "keywords": { "$draft": true, "$seen": true },
        "subject": draft.subject,
        "bodyValues": { "text": { "value": draft.body } },
    });
    if let Some(identity) = from {
        email["from"] = json!([address_json(&Address { name: identity.name.clone(), email: identity.email.clone() })]);
    }
    for (key, list) in [("to", &draft.to), ("cc", &draft.cc), ("bcc", &draft.bcc)] {
        if !list.is_empty() {
            email[key] = list.iter().map(address_json).collect();
        }
    }
    if !in_reply_to.is_empty() {
        email["inReplyTo"] = json!(in_reply_to);
    }
    if !references.is_empty() {
        email["references"] = json!(references);
    }
    let text = json!({ "partId": "text", "type": "text/plain" });
    email["bodyStructure"] = if draft.attachments.is_empty() {
        text
    } else {
        let files = draft.attachments.iter().map(|a| {
            let mut part = json!({ "blobId": a.blob_id, "type": a.mime_type, "disposition": "attachment" });
            if !a.name.is_empty() {
                part["name"] = json!(a.name);
            }
            part
        });
        json!({ "type": "multipart/mixed", "subParts": std::iter::once(text).chain(files).collect::<Vec<_>>() })
    };
    email
}

/// A JSON Pointer segment.
fn pointer_segment(s: &str) -> String {
    s.replace('~', "~0").replace('/', "~1")
}

/// The `EmailSubmission/set` that sends a message and, when it is accepted, takes it out of
/// Drafts (into Sent when there is one) and clears `$draft`.
fn submission_args(
    account_id: &str,
    identity_id: &str,
    email_id: &str,
    drafts_id: &str,
    sent_id: Option<&str>,
) -> Value {
    let mut update = Map::new();
    update.insert("keywords/$draft".into(), Value::Null);
    if let Some(sent) = sent_id {
        update.insert(format!("mailboxIds/{}", pointer_segment(drafts_id)), Value::Null);
        update.insert(format!("mailboxIds/{}", pointer_segment(sent)), json!(true));
    }
    json!({
        "accountId": account_id,
        "create": { CREATE_ID: { "identityId": identity_id, "emailId": email_id } },
        "onSuccessUpdateEmail": { format!("#{CREATE_ID}"): update },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Attachment;

    fn draft() -> Draft {
        Draft {
            id: "d1".into(),
            identity_id: "i1".into(),
            to: vec![Address { name: "Bo".into(), email: "bo@x.org".into() }],
            cc: Vec::new(),
            bcc: vec![Address { name: String::new(), email: "hid@x.org".into() }],
            subject: "Hello".into(),
            body: "Text\n".into(),
            attachments: Vec::new(),
            source_message_id: None,
            is_forward: false,
            server_id: None,
            send_error: None,
            updated_at: 0,
        }
    }

    fn ident() -> Identity {
        Identity { id: "i1".into(), name: "Me".into(), email: "me@x.org".into(), signature: String::new() }
    }

    #[test]
    fn a_draft_is_made_in_drafts_with_a_text_body() {
        let email = draft_json(&draft(), Some(&ident()), (&[], &[]), "mb4");
        assert_eq!(email["mailboxIds"], json!({ "mb4": true }));
        assert_eq!(email["keywords"], json!({ "$draft": true, "$seen": true }));
        assert_eq!(email["from"], json!([{ "name": "Me", "email": "me@x.org" }]));
        assert_eq!(email["to"], json!([{ "name": "Bo", "email": "bo@x.org" }]));
        assert_eq!(email["bcc"], json!([{ "email": "hid@x.org" }]));
        assert_eq!(email["subject"], "Hello");
        assert_eq!(email["bodyValues"]["text"]["value"], "Text\n");
        assert_eq!(email["bodyStructure"], json!({ "partId": "text", "type": "text/plain" }));
        // Nothing that is empty is sent.
        for key in ["cc", "inReplyTo", "references"] {
            assert!(email.get(key).is_none(), "{key}");
        }
    }

    #[test]
    fn files_go_in_by_blob_and_an_answer_carries_the_ids() {
        let mut d = draft();
        d.attachments = vec![
            Attachment {
                blob_id: "b1".into(),
                name: "a.pdf".into(),
                mime_type: "application/pdf".into(),
                size: 3,
                content_id: None,
                is_inline: false,
            },
            Attachment {
                blob_id: "b2".into(),
                name: String::new(),
                mime_type: "image/png".into(),
                size: 3,
                content_id: Some("c".into()),
                is_inline: true,
            },
        ];
        let (in_reply_to, references) = (vec!["m1@x".to_string()], vec!["m0@x".to_string(), "m1@x".to_string()]);
        let email = draft_json(&d, None, (&in_reply_to, &references), "mb4");
        assert!(email.get("from").is_none());
        assert_eq!(email["inReplyTo"], json!(["m1@x"]));
        assert_eq!(email["references"], json!(["m0@x", "m1@x"]));
        assert_eq!(
            email["bodyStructure"],
            json!({ "type": "multipart/mixed", "subParts": [
                { "partId": "text", "type": "text/plain" },
                { "blobId": "b1", "type": "application/pdf", "disposition": "attachment", "name": "a.pdf" },
                { "blobId": "b2", "type": "image/png", "disposition": "attachment" },
            ]})
        );
    }

    #[test]
    fn the_submission_moves_the_message_to_sent_and_clears_the_draft_flag() {
        let args = submission_args("a1", "i1", "e9", "mb4", Some("mb5"));
        assert_eq!(args["create"]["new"], json!({ "identityId": "i1", "emailId": "e9" }));
        assert_eq!(
            args["onSuccessUpdateEmail"]["#new"],
            json!({ "keywords/$draft": null, "mailboxIds/mb4": null, "mailboxIds/mb5": true })
        );
        // Without a Sent mailbox the message stays where it is, but is no longer a draft.
        let args = submission_args("a1", "i1", "e9", "mb4", None);
        assert_eq!(args["onSuccessUpdateEmail"]["#new"], json!({ "keywords/$draft": null }));
        // An id with a slash cannot break the pointer.
        let args = submission_args("a1", "i1", "e9", "a/b", Some("c~d"));
        assert!(args["onSuccessUpdateEmail"]["#new"].get("mailboxIds/a~1b").is_some());
        assert!(args["onSuccessUpdateEmail"]["#new"].get("mailboxIds/c~0d").is_some());
    }

    #[test]
    fn a_created_object_gives_its_id_and_a_refusal_its_kind() {
        let ok = json!({ "created": { "new": { "id": "e5" } } });
        assert_eq!(created_id(&ok).unwrap(), "e5");
        for kind in ["invalidEmail", "tooLarge", "forbiddenFrom", "noRecipients", "blobNotFound"] {
            let e = created_id(&json!({ "notCreated": { "new": { "type": kind, "description": "why" } } }));
            assert!(matches!(&e, Err(Error::Invalid { msg }) if msg.contains("why")), "{kind}: {e:?}");
        }
        for kind in ["overQuota", "rateLimit", "serverFail"] {
            let e = created_id(&json!({ "notCreated": { "new": { "type": kind } } }));
            assert!(matches!(&e, Err(Error::Temporary { msg }) if msg.contains(kind)), "{kind}: {e:?}");
        }
        assert!(matches!(created_id(&json!({})), Err(Error::Temporary { .. })));
    }

    #[test]
    fn answers_keep_their_kind_of_failure() {
        use super::super::is_unauthorized;
        assert!(is_unauthorized(&read_responses("x", 401, b"").unwrap_err()));
        let e = read_responses("x", 503, b"").unwrap_err();
        assert!(matches!(e, Error::Temporary { .. }) && !is_unauthorized(&e));
        assert!(matches!(read_responses("x", 200, b"<html>"), Err(Error::Temporary { .. })));
        let method = |kind: &str| json!({ "methodResponses": [["error", { "type": kind }, "c0"]] }).to_string();
        assert!(matches!(read_responses("x", 200, method("forbidden").as_bytes()), Err(Error::Invalid { .. })));
        assert!(matches!(read_responses("x", 200, method("serverFail").as_bytes()), Err(Error::Temporary { .. })));
        let two = json!({ "methodResponses": [["A/set", { "n": 1 }, "c0"], ["B/set", { "n": 2 }, "c0"]] });
        let got = read_responses("x", 200, two.to_string().as_bytes()).unwrap();
        assert_eq!(response(&got, "B/set").unwrap()["n"], 2);
        assert!(response(&got, "C/set").is_err());
    }

    #[test]
    fn identities_need_an_id_and_an_address() {
        let get = json!({ "list": [
            { "id": "i1", "name": "Me", "email": "me@x.org", "textSignature": "Bye" },
            { "id": "i2", "email": "alias@x.org" },
            { "id": "i3", "name": "No address" },
        ]});
        let got = identities_from(&get);
        assert_eq!(got.len(), 2);
        assert_eq!((got[0].signature.as_str(), got[1].name.as_str()), ("Bye", ""));
    }

    #[test]
    fn a_card_gives_every_address_with_its_name() {
        let full = json!({ "name": { "full": " Ann Keller " },
            "emails": { "a": { "address": "ann@x.org" }, "b": { "address": " ann@work.org " }, "c": {} } });
        let got = card_addresses(&full);
        let emails: Vec<_> = got.iter().map(|a| (a.name.as_str(), a.email.as_str())).collect();
        assert_eq!(emails, [("Ann Keller", "ann@x.org"), ("Ann Keller", "ann@work.org")]);
        let parts = json!({ "name": { "components": [
            { "kind": "given", "value": "Bo" }, { "kind": "surname", "value": "Roth" }, { "kind": "x", "value": "" } ] },
            "emails": { "a": { "address": "bo@x.org" } } });
        assert_eq!(card_addresses(&parts)[0].name, "Bo Roth");
        // No name is fine; no address means no entry.
        assert_eq!(card_addresses(&json!({ "emails": { "a": { "address": "z@x.org" } } }))[0].name, "");
        assert!(card_addresses(&json!({ "name": { "full": "No Mail" } })).is_empty());
    }

    #[test]
    fn quotas_name_what_is_counted() {
        let get = json!({ "list": [
            { "id": "q1", "name": "Storage", "resourceType": "octets", "used": 5, "hardLimit": 100 },
            { "id": "q2", "resourceType": "count", "used": 2 },
        ]});
        let got = quotas_from(&get);
        assert_eq!(got[0], Quota { name: "Storage".into(), resource: "octets".into(), used: 5, limit: Some(100) });
        assert_eq!(got[1], Quota { name: "q2".into(), resource: "count".into(), used: 2, limit: None });
    }
}
