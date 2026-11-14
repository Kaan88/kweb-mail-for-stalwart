//! Folders (`Mailbox/set`) and spam reports, over plain JMAP requests.
//!
//! What Stalwart 1.1.0 does, found against the real server (`dev/notes/stalwart-mail-features.md`):
//! names are compared without regard to case among the folders of one parent; a name is
//! stored whatever it holds and is cut at 255 bytes; nesting stops at ten levels; a folder with
//! folders inside, or with mail, cannot be destroyed; and the server lets folders with a role be
//! renamed, moved and (Drafts, Sent) destroyed, so the caller has to refuse that itself. A
//! move or a spam report does not set `$junk` or `$notjunk`, so the client does.

use serde_json::{json, Value};

use super::{
    send::{response, set_error},
    Connection, MAIL_CAPABILITY,
};
use crate::{Error, Result};

const CREATE_ID: &str = "f1";
/// Messages moved by one `Email/set` when a folder is emptied.
const MOVE_BATCH: usize = 50;
const MESSAGE_GONE: &str = "That message is no longer on the server.";
const FOLDER_GONE: &str = "That folder is no longer on the server.";

impl Connection {
    /// Makes a folder and returns its id. It is made subscribed: the server's default is not,
    /// and a folder nobody is subscribed to is hidden by other mail programs.
    pub async fn mailbox_create(&self, name: &str, parent_id: Option<&str>) -> Result<String> {
        let object = json!({ "name": name, "parentId": parent_id, "isSubscribed": true });
        let args = json!({ "accountId": self.account_id(), "create": { CREATE_ID: object } });
        let responses = self.call(&[MAIL_CAPABILITY], &[("Mailbox/set", args)]).await?;
        let set = response(&responses, "Mailbox/set")?;
        if let Some(id) = set["created"][CREATE_ID]["id"].as_str() {
            return Ok(id.to_string());
        }
        match set["notCreated"].get(CREATE_ID) {
            Some(e) => Err(folder_error(e)),
            None => Err(Error::temporary("the server's answer does not say whether the folder was made")),
        }
    }

    /// Changes properties of a folder (`name`, `parentId`).
    pub async fn mailbox_update(&self, id: &str, patch: Value) -> Result<()> {
        let args = json!({ "accountId": self.account_id(), "update": { id: patch } });
        let responses = self.call(&[MAIL_CAPABILITY], &[("Mailbox/set", args)]).await?;
        let set = response(&responses, "Mailbox/set")?;
        if let Some(e) = set["notUpdated"].get(id) {
            return Err(folder_error(e));
        }
        match set["updated"].get(id) {
            Some(_) => Ok(()),
            None => Err(Error::temporary("the server's answer does not say whether the folder was changed")),
        }
    }

    /// Destroys an empty folder. One the server no longer has is as good as destroyed.
    pub async fn mailbox_destroy(&self, id: &str) -> Result<()> {
        // Never `onDestroyRemoveEmails`: the server would delete mail for good.
        let args = json!({ "accountId": self.account_id(), "destroy": [id] });
        let responses = self.call(&[MAIL_CAPABILITY], &[("Mailbox/set", args)]).await?;
        let set = response(&responses, "Mailbox/set")?;
        match set["notDestroyed"].get(id) {
            Some(e) if e["type"] != "notFound" => Err(folder_error(e)),
            Some(_) => Ok(()),
            None if set["destroyed"].as_array().is_some_and(|ids| ids.iter().any(|d| d == id)) => Ok(()),
            None => Err(Error::temporary("the server's answer does not say whether the folder was deleted")),
        }
    }

    /// Takes every message out of the folder: one that is only in it goes to `trash`, one that
    /// is in other folders too just leaves this one. Nothing is destroyed. Returns how many
    /// messages were moved. On a failure part-way the error says how far it got; the messages
    /// moved stay moved.
    pub async fn empty_mailbox(&self, mailbox_id: &str, trash_id: Option<&str>) -> Result<usize> {
        let mut moved = 0;
        loop {
            let ids = self.message_ids(mailbox_id, 0, MOVE_BATCH).await.map_err(|e| stopped(e, moved))?;
            if ids.is_empty() {
                return Ok(moved);
            }
            let before = moved;
            self.leave_mailbox(mailbox_id, trash_id, &ids, &mut moved).await.map_err(|e| stopped(e, moved))?;
            if moved == before {
                // Nothing left the folder and nothing said why: stop rather than ask forever.
                return Err(stopped(Error::temporary("the server did not move any message."), moved));
            }
        }
    }

    /// One batch of [`Connection::empty_mailbox`]. Adds to `moved` each message that is out of
    /// the folder (one the server no longer has counts: it is not in the folder), also when a
    /// later one of the batch fails.
    async fn leave_mailbox(
        &self,
        mailbox_id: &str,
        trash_id: Option<&str>,
        ids: &[String],
        moved: &mut usize,
    ) -> Result<()> {
        let get = json!({ "accountId": self.account_id(), "ids": ids, "properties": ["mailboxIds"] });
        let responses = self.call(&[MAIL_CAPABILITY], &[("Email/get", get)]).await?;
        let list = response(&responses, "Email/get")?["list"].as_array().cloned().unwrap_or_default();
        let mut update = serde_json::Map::new();
        for email in &list {
            let Some(id) = email["id"].as_str() else { continue };
            let only_here = email["mailboxIds"].as_object().is_none_or(|m| m.keys().all(|k| k == mailbox_id));
            let mut patch = serde_json::Map::new();
            patch.insert(format!("mailboxIds/{}", super::send::pointer_segment(mailbox_id)), Value::Null);
            if only_here {
                let trash = trash_id.ok_or_else(|| Error::invalid("The account has no Trash folder."))?;
                patch.insert(format!("mailboxIds/{}", super::send::pointer_segment(trash)), json!(true));
            }
            update.insert(id.to_string(), Value::Object(patch));
        }
        *moved += ids.len() - update.len();
        if update.is_empty() {
            return Ok(());
        }
        let set = json!({ "accountId": self.account_id(), "update": Value::Object(update.clone()) });
        let responses = self.call(&[MAIL_CAPABILITY], &[("Email/set", set)]).await?;
        let answer = response(&responses, "Email/set")?;
        let mut failure = None;
        for id in update.keys() {
            match answer["notUpdated"].get(id) {
                None if answer["updated"].get(id).is_some() => *moved += 1,
                None => {
                    failure
                        .get_or_insert(Error::temporary("the server's answer does not say whether the mail was moved"));
                }
                Some(e) if e["type"] == "notFound" => *moved += 1,
                Some(e) => {
                    failure.get_or_insert(set_error(e));
                }
            }
        }
        failure.map_or(Ok(()), Err)
    }

    /// Reports a message as spam (`is_spam`: into `target`, the Junk folder, with `$junk` set and
    /// `$notjunk` cleared) or as not spam (into `target`, the Inbox, the other way round), in one
    /// change. The message ends up in that folder only.
    pub async fn apply_spam(&self, message_id: &str, is_spam: bool, target: &str) -> Result<()> {
        let (set, clear) = if is_spam { ("$junk", "$notjunk") } else { ("$notjunk", "$junk") };
        let mut patch = serde_json::Map::new();
        patch.insert("mailboxIds".into(), json!({ target: true }));
        patch.insert(format!("keywords/{}", super::send::pointer_segment(set)), json!(true));
        patch.insert(format!("keywords/{}", super::send::pointer_segment(clear)), Value::Null);
        let args = json!({ "accountId": self.account_id(), "update": { message_id: Value::Object(patch) } });
        let responses = self.call(&[MAIL_CAPABILITY], &[("Email/set", args)]).await?;
        super::send::updated(response(&responses, "Email/set")?, message_id, MESSAGE_GONE)
    }
}

/// The error of a step that stopped part-way through emptying a folder, with how far it got.
fn stopped(e: Error, moved: usize) -> Error {
    let note = match moved {
        0 => "No mail was moved and the folder was not deleted.".to_string(),
        1 => "One message was moved to the Trash, the rest are still in the folder, and the folder was not deleted."
            .to_string(),
        n => format!(
            "{n} messages were moved to the Trash, the rest are still in the folder, and the folder was not deleted."
        ),
    };
    match e {
        Error::Temporary { msg } => Error::temporary(format!("{msg} {note}")),
        Error::Invalid { msg } => Error::invalid(format!("{msg} {note}")),
        other => other,
    }
}

/// A SetError of a folder change, in words for the user.
fn folder_error(e: &Value) -> Error {
    let kind = e["type"].as_str().unwrap_or("unknown");
    let detail = e["description"].as_str().unwrap_or_default();
    let lower = detail.to_lowercase();
    let first = e["properties"].get(0).and_then(Value::as_str).unwrap_or_default();
    match kind {
        "alreadyExists" => Error::invalid("A folder with that name is already there."),
        "mailboxHasChild" => Error::invalid("That folder has folders inside it. Delete or move those first."),
        "mailboxHasEmail" => Error::invalid("That folder still holds mail. Move or delete the mail first."),
        "notFound" => Error::invalid(FOLDER_GONE),
        "forbidden" => Error::invalid("The server does not allow that change to this folder."),
        "invalidProperties" if lower.contains("too deep") => {
            Error::invalid("Folders can only be nested ten levels deep.")
        }
        "invalidProperties" if lower.contains("itself") => Error::invalid("A folder cannot be put inside itself."),
        "invalidProperties" if lower.contains("parent") => Error::invalid("The folder to put it in is gone."),
        "invalidProperties" if first == "name" && lower.contains("empty") => Error::invalid("A folder needs a name."),
        "invalidProperties" if first == "name" && lower.contains("long") => {
            Error::invalid("That folder name is too long.")
        }
        "invalidProperties" => Error::invalid(match detail {
            "" => "The server does not accept that folder change.".to_string(),
            d => format!("The server does not accept that folder change: {d}"),
        }),
        _ => set_error(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn invalid_text(e: Value) -> String {
        match folder_error(&e) {
            Error::Invalid { msg } => msg,
            other => panic!("not a refusal: {other:?}"),
        }
    }

    #[test]
    fn the_servers_refusals_become_plain_words() {
        let t = |v: Value| invalid_text(v);
        assert_eq!(
            t(json!({"type":"alreadyExists","description":"A mailbox with name 'x' already exists.","existingId":"b"})),
            "A folder with that name is already there."
        );
        assert!(t(json!({"type":"mailboxHasChild"})).contains("folders inside"));
        assert!(t(json!({"type":"mailboxHasEmail"})).contains("still holds mail"));
        assert!(t(json!({"type":"invalidProperties","description":"Mailbox parent-child relationship is too deep.","properties":["parentId"]}))
            .contains("ten levels"));
        assert!(t(json!({"type":"invalidProperties","description":"Mailbox cannot be a parent of itself.","properties":["parentId"]}))
            .contains("inside itself"));
        assert!(t(json!({"type":"invalidProperties","description":"Parent ID does not exist."})).contains("is gone"));
        assert_eq!(
            t(json!({"type":"invalidProperties","description":"Mailbox name is too long.","properties":["name"]})),
            "That folder name is too long."
        );
        assert_eq!(
            t(json!({"type":"invalidProperties","description":"Mailbox name cannot be empty.","properties":["name"]})),
            "A folder needs a name."
        );
        assert!(t(
            json!({"type":"forbidden","description":"You are not allowed to delete Inbox, Junk or Trash folders."})
        )
        .contains("does not allow"));
        assert_eq!(t(json!({"type":"notFound"})), FOLDER_GONE);
        // A passing trouble is not a refusal.
        assert!(matches!(folder_error(&json!({"type":"overQuota"})), Error::Temporary { .. }));
    }

    #[test]
    fn a_stopped_move_says_how_far_it_got() {
        let e = stopped(Error::temporary("the server is busy"), 0);
        assert_eq!(e, Error::temporary("the server is busy No mail was moved and the folder was not deleted."));
        let Error::Temporary { msg } = stopped(Error::temporary("x"), 120) else { panic!() };
        assert!(msg.contains("120 messages were moved to the Trash") && msg.contains("folder was not deleted"));
        assert!(matches!(stopped(Error::invalid("x"), 1), Error::Invalid { .. }));
        let signed_out = Error::sign_in_needed("x");
        assert_eq!(stopped(signed_out.clone(), 5), signed_out);
    }
}
