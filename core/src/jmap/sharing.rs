//! Sharing what is the user's own with other users of the server: mail folders, calendars,
//! address books and file folders. What Stalwart 1.1.0 does is in `dev/notes/stalwart-sharing.md`
//! and `dev/notes/stalwart-files.md`; the points that shape this code:
//!
//! - Each kind has a `shareWith` map from a user's id to the rights that user has. The server
//!   gives it complete (every right, true or false) to the owner. A mail folder only gives it
//!   when it is asked for by name.
//! - Only the entry of the person asked about is written: `shareWith/<id>` set to the rights of
//!   the level, or to null to take the share back. The entries of others are never sent.
//! - The user's rights are turned into two levels. Read lets the person see the content; Write
//!   lets them add, change and remove content and marks, never share, rename or delete the
//!   folder, calendar or address book itself. Anything else the server may hold (a right given
//!   by another client) is shown as the level it comes closest to.
//! - Looking users up is off by default (`Principal/query` is `forbidden`), and so is
//!   `Principal/get` for anyone who has shared nothing with the user. Then a share is shown by
//!   the person's id alone.

use serde_json::{json, Map, Value};

use super::{
    calendars::CALENDARS_CAPABILITY,
    contacts::CONTACTS_CAPABILITY,
    files::FILES_CAPABILITY,
    send::{pointer_segment, response, set_error},
    Connection, MAIL_CAPABILITY,
};
use crate::{
    model::{Person, Share, ShareKind, ShareLevel},
    Error, Result,
};

const PRINCIPALS_CAPABILITY: &str = "urn:ietf:params:jmap:principals";
/// The most people one search gives.
const MAX_PEOPLE: usize = 20;
/// How many users one lookup reads. The server's `text` filter matches whole words only
/// ("sec" does not find "second"), so the core asks for the users and matches the typed
/// text against name and address itself.
const LOOKUP_LIMIT: usize = 500;
const NO_LOOKUP: &str = "this server does not let apps look up other users, so sharing can only be changed for \
                         people it is already shared with";

/// What differs between the kinds of thing that can be shared.
struct Spec {
    /// The JMAP type, for the method names.
    object: &'static str,
    capability: &'static str,
    /// For what is said to the user.
    noun: &'static str,
    /// The rights of each level, by their names on the server.
    read: &'static [&'static str],
    write: &'static [&'static str],
    /// The rights that make a share Write when any of them is there.
    content: &'static [&'static str],
}

fn spec(kind: ShareKind) -> Spec {
    match kind {
        ShareKind::Mailbox => Spec {
            object: "Mailbox",
            capability: MAIL_CAPABILITY,
            noun: "folder",
            read: &["mayReadItems"],
            write: &["mayReadItems", "mayAddItems", "mayRemoveItems", "maySetSeen", "maySetKeywords"],
            content: &["mayAddItems", "mayRemoveItems", "maySetSeen", "maySetKeywords"],
        },
        ShareKind::Calendar => Spec {
            object: "Calendar",
            capability: CALENDARS_CAPABILITY,
            noun: "calendar",
            read: &["mayReadFreeBusy", "mayReadItems"],
            write: &["mayReadFreeBusy", "mayReadItems", "mayWriteAll", "mayWriteOwn", "mayRSVP"],
            content: &["mayWriteAll", "mayWriteOwn", "mayUpdatePrivate"],
        },
        ShareKind::AddressBook => Spec {
            object: "AddressBook",
            capability: CONTACTS_CAPABILITY,
            noun: "address book",
            read: &["mayRead"],
            write: &["mayRead", "mayWrite"],
            content: &["mayWrite"],
        },
        // A folder's rights are inherited by what is in it, so removing a file needs the right to
        // delete, which would also let the person delete the folder: Write does not give it.
        ShareKind::FileFolder => Spec {
            object: "FileNode",
            capability: FILES_CAPABILITY,
            noun: "file folder",
            read: &["mayRead"],
            write: &["mayRead", "mayAddChildren", "mayModifyContent"],
            content: &["mayAddChildren", "mayModifyContent", "mayRename", "mayDelete"],
        },
    }
}

/// The level an entry of `shareWith` stands for; `None` for one that gives nothing.
fn level_of(spec: &Spec, rights: &Value) -> Option<ShareLevel> {
    if spec.content.iter().any(|r| rights[*r] == true) {
        Some(ShareLevel::Write)
    } else if rights.as_object().is_some_and(|all| all.values().any(|v| *v == true)) {
        Some(ShareLevel::Read)
    } else {
        None
    }
}

/// The people of a `shareWith` map with their levels, by id.
fn entries_from(spec: &Spec, share_with: &Value) -> Vec<(String, ShareLevel)> {
    let mut out: Vec<(String, ShareLevel)> = share_with
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(person, rights)| Some((person.clone(), level_of(spec, rights)?)))
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// The entry that gives exactly the rights of the level.
fn rights_of(spec: &Spec, level: ShareLevel) -> Value {
    let names = match level {
        ShareLevel::Read => spec.read,
        ShareLevel::Write => spec.write,
    };
    Value::Object(names.iter().map(|n| (n.to_string(), json!(true))).collect::<Map<String, Value>>())
}

/// The people of a `Principal/get` answer by id. A name is the name, else the address, else
/// the id; one the server did not give is not in the list.
fn people_from(get: &Value) -> Vec<Person> {
    let text = |v: &Value| v.as_str().unwrap_or_default().trim().to_string();
    let mut out = Vec::new();
    for p in get["list"].as_array().into_iter().flatten() {
        let id = text(&p["id"]);
        if id.is_empty() {
            continue;
        }
        let email = text(&p["email"]);
        let name =
            [text(&p["name"]), email.clone(), id.clone()].into_iter().find(|n| !n.is_empty()).unwrap_or_default();
        out.push(Person { id, name, email });
    }
    out
}

/// What is said when the server refuses a change of a share.
fn refused(e: Error, noun: &str) -> Error {
    match &e {
        Error::Invalid { msg } if msg.to_lowercase().contains("not allowed") => {
            Error::invalid(format!("you are not allowed to change who this {noun} is shared with"))
        }
        _ => e,
    }
}

impl Connection {
    /// The account that holds the user's own things of this kind.
    fn own_account(&self, kind: ShareKind, spec: &Spec) -> Result<String> {
        if !self.has_capability(spec.capability) {
            return Err(Error::invalid(format!("this server has no {}s to share", spec.noun)));
        }
        Ok(match kind {
            ShareKind::Mailbox => self.account_id.clone(),
            ShareKind::FileFolder => self.account_for(FILES_CAPABILITY),
            _ => self.account_for(spec.capability),
        })
    }

    /// Users whose name or address has `text`, at most 20, the user left out. A server that
    /// does not let apps look users up is `Error::Invalid`.
    pub async fn share_people(&self, text: &str) -> Result<Vec<Person>> {
        let text = text.trim();
        if text.is_empty() {
            return Ok(Vec::new());
        }
        if !self.has_capability(PRINCIPALS_CAPABILITY) {
            return Err(Error::invalid(NO_LOOKUP));
        }
        let account = self.account_id.as_str();
        let query = json!({ "accountId": account, "limit": LOOKUP_LIMIT });
        let get = json!({
            "accountId": account, "properties": ["name", "email", "type"],
            "#ids": { "resultOf": "c0", "name": "Principal/query", "path": "/ids" },
        });
        let responses = self
            .call_keeping_errors(&[PRINCIPALS_CAPABILITY], &[("Principal/query", query), ("Principal/get", get)])
            .await?;
        if let Some((_, e)) = responses.iter().find(|(name, _)| name == "error") {
            return Err(match e["type"].as_str() {
                Some("forbidden") => Error::invalid(NO_LOOKUP),
                kind => Error::temporary(format!("looking up users failed: {}", kind.unwrap_or("unknown"))),
            });
        }
        let me = self.username().to_lowercase();
        let mut people = people_from(response(&responses, "Principal/get")?);
        let needle = text.to_lowercase();
        people.retain(|p| {
            p.id != account
                && p.email.to_lowercase() != me
                && (p.name.to_lowercase().contains(&needle) || p.email.to_lowercase().contains(&needle))
        });
        people.sort_by_cached_key(|p| (p.name.to_lowercase(), p.id.clone()));
        people.truncate(MAX_PEOPLE);
        Ok(people)
    }

    /// The names of these users, as far as the server tells them: one that it does not give
    /// (it never does for a user nothing was shared by) is shown by its id.
    async fn named(&self, ids: &[String]) -> Result<Vec<Person>> {
        let bare = |id: &String| Person { id: id.clone(), name: id.clone(), email: String::new() };
        if ids.is_empty() || !self.has_capability(PRINCIPALS_CAPABILITY) {
            return Ok(ids.iter().map(bare).collect());
        }
        let args = json!({ "accountId": self.account_id, "ids": ids, "properties": ["name", "email"] });
        let known = match self.call_keeping_errors(&[PRINCIPALS_CAPABILITY], &[("Principal/get", args)]).await {
            Ok(responses) if responses.iter().all(|(name, _)| name != "error") => {
                people_from(response(&responses, "Principal/get")?)
            }
            // Names are a courtesy: a server that refuses them still gets its shares shown.
            Ok(_) => Vec::new(),
            Err(e) if super::is_unauthorized(&e) => return Err(e),
            Err(_) => Vec::new(),
        };
        Ok(ids.iter().map(|id| known.iter().find(|p| &p.id == id).cloned().unwrap_or_else(|| bare(id))).collect())
    }

    /// Whom the user's own `id` is shared with.
    pub async fn share_list(&self, kind: ShareKind, id: &str) -> Result<Vec<Share>> {
        let spec = spec(kind);
        let (_, share_with) = self.own_object(kind, &spec, id).await?;
        let entries = entries_from(&spec, &share_with);
        let ids: Vec<String> = entries.iter().map(|(person, _)| person.clone()).collect();
        let people = self.named(&ids).await?;
        let mut out: Vec<Share> =
            people.into_iter().zip(entries).map(|(person, (_, level))| Share { person, level }).collect();
        out.sort_by_cached_key(|s| (s.person.name.to_lowercase(), s.person.id.clone()));
        Ok(out)
    }

    /// The user's own object `id` and its `shareWith`. An id that is not the user's own
    /// `kind` of thing is `Error::Invalid`.
    async fn own_object(&self, kind: ShareKind, spec: &Spec, id: &str) -> Result<(Value, Value)> {
        let account = self.own_account(kind, spec)?;
        let mut properties = vec!["shareWith"];
        match kind {
            ShareKind::Mailbox => properties.push("role"),
            ShareKind::FileFolder => properties.push("@type"),
            _ => {}
        }
        let args = json!({ "accountId": account, "ids": [id], "properties": properties });
        let method = format!("{}/get", spec.object);
        let responses = self.call(&[spec.capability], &[(method.as_str(), args)]).await?;
        let list = response(&responses, &method)?["list"].as_array();
        let own = list.and_then(|l| l.iter().find(|o| o["id"] == id)).cloned();
        let Some(object) = own.filter(|o| kind != ShareKind::FileFolder || o["@type"] == "Directory") else {
            return Err(Error::invalid(format!("that is not one of your own {}s", spec.noun)));
        };
        let share_with = object["shareWith"].clone();
        Ok((object, share_with))
    }

    /// Shares the user's own `id` with `person_id` at `level`, or with `None` takes the share
    /// back. Only that person's entry is sent. Returns the shares afterwards.
    pub async fn set_share(
        &self,
        kind: ShareKind,
        id: &str,
        person_id: &str,
        level: Option<ShareLevel>,
    ) -> Result<Vec<Share>> {
        let spec = spec(kind);
        let person_id = person_id.trim();
        if person_id.is_empty() {
            return Err(Error::invalid("choose whom to share with"));
        }
        if person_id == self.account_id {
            return Err(Error::invalid("you cannot share with yourself"));
        }
        let (object, _) = self.own_object(kind, &spec, id).await?;
        if level.is_some() {
            if let Some(why) = private_folder(&object) {
                return Err(Error::invalid(why));
            }
        }
        let entry = level.map_or(Value::Null, |l| rights_of(&spec, l));
        let patch = json!({ format!("shareWith/{}", pointer_segment(person_id)): entry });
        let args = json!({ "accountId": self.own_account(kind, &spec)?, "update": { id: patch } });
        let method = format!("{}/set", spec.object);
        let responses =
            self.call(&[spec.capability], &[(method.as_str(), args)]).await.map_err(|e| refused(e, spec.noun))?;
        let set = response(&responses, &method)?;
        if let Some(e) = set["notUpdated"].get(id) {
            return Err(match e["type"].as_str() {
                Some("notFound") => Error::invalid(format!("that is not one of your own {}s", spec.noun)),
                Some("invalidProperties") => Error::invalid("that person is not a user of this server"),
                _ => refused(set_error(e), spec.noun),
            });
        }
        if set["updated"].get(id).is_none() {
            return Err(Error::temporary("the server's answer does not say whether the share was changed"));
        }
        self.share_list(kind, id).await
    }
}

/// Why a folder is not shared, for those that hold what is not for others; `None` for any
/// other.
fn private_folder(object: &Value) -> Option<&'static str> {
    match object["role"].as_str() {
        Some("drafts") => Some("the Drafts folder cannot be shared: it holds mail you have not finished"),
        Some("trash") => Some("the Trash cannot be shared: it holds what you threw away"),
        Some("junk") => Some("the Junk folder cannot be shared: it holds mail the server thinks is spam"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_share_is_write_when_content_may_be_changed() {
        let mail = spec(ShareKind::Mailbox);
        let rights = |on: &[&str]| Value::Object(on.iter().map(|r| (r.to_string(), json!(true))).collect());
        assert_eq!(level_of(&mail, &rights(&["mayReadItems"])), Some(ShareLevel::Read));
        assert_eq!(level_of(&mail, &rights(&["mayReadItems", "maySetSeen"])), Some(ShareLevel::Write));
        assert_eq!(level_of(&mail, &rights(&["mayReadItems", "mayShare"])), Some(ShareLevel::Read));
        assert_eq!(level_of(&mail, &json!({ "mayReadItems": false })), None);
        assert_eq!(level_of(&mail, &json!({})), None);
        let books = spec(ShareKind::AddressBook);
        assert_eq!(level_of(&books, &rights(&["mayRead", "mayWrite"])), Some(ShareLevel::Write));
        let calendars = spec(ShareKind::Calendar);
        assert_eq!(level_of(&calendars, &rights(&["mayReadItems", "mayRSVP"])), Some(ShareLevel::Read));
        assert_eq!(level_of(&calendars, &rights(&["mayWriteOwn"])), Some(ShareLevel::Write));
    }

    #[test]
    fn the_rights_of_a_level_never_reach_the_container() {
        for kind in [ShareKind::Mailbox, ShareKind::Calendar, ShareKind::AddressBook, ShareKind::FileFolder] {
            let spec = spec(kind);
            for level in [ShareLevel::Read, ShareLevel::Write] {
                let rights = rights_of(&spec, level);
                for never in ["mayShare", "mayDelete", "mayRename", "maySetMetadata", "mayCreateChild"] {
                    assert!(rights.get(never).is_none(), "{never} is given by {level:?}");
                }
                assert_eq!(level_of(&spec, &rights), Some(level), "a level reads back as itself");
            }
        }
    }

    #[test]
    fn people_are_named_by_name_then_address_then_id() {
        let get = json!({"list": [
            {"id": "d", "name": "Second", "email": "second@x.test"},
            {"id": "e", "name": "", "email": "e@x.test"},
            {"id": "f"},
            {"name": "no id"},
        ]});
        let names: Vec<(String, String)> = people_from(&get).into_iter().map(|p| (p.name, p.email)).collect();
        assert_eq!(
            names,
            [
                ("Second".into(), "second@x.test".into()),
                ("e@x.test".into(), "e@x.test".into()),
                ("f".into(), "".into())
            ]
        );
    }

    #[test]
    fn drafts_trash_and_junk_stay_private() {
        for role in ["drafts", "trash", "junk"] {
            assert!(private_folder(&json!({ "role": role })).is_some());
        }
        assert!(private_folder(&json!({ "role": "inbox" })).is_none());
        assert!(private_folder(&json!({ "role": null })).is_none());
    }
}
