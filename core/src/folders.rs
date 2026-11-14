//! Folders the user makes (M10): the rules that decide what may be done, and the steps that
//! keep the cache in line with the server afterwards.
//!
//! The server lets folders with a role be renamed, moved and in part destroyed, and takes any
//! name (`dev/notes/stalwart-mail-features.md`), so everything the app refuses is refused here,
//! from the cache, before the server is asked. The server's own refusals (a change made
//! elsewhere meanwhile) come back as plain words from `jmap::folders`.
//!
//! Notifications per folder are kept in the store (`mailbox_notify`) and never go to the server.

use serde_json::json;

use crate::{jmap::Connection, model::Mailbox, store::Store, Error, Result};

/// The longest folder name the server takes, in bytes.
const MAX_NAME_BYTES: usize = 255;
/// How many levels of folders the server allows (the top level is level one).
const MAX_DEPTH: usize = 10;
/// Roles whose mail never notifies. Nothing can be switched on in them.
const QUIET_ROLES: [&str; 4] = ["drafts", "sent", "trash", "junk"];
/// Folders in which no folder of the user's can be made.
const NO_CHILDREN_ROLES: [&str; 3] = ["drafts", "trash", "junk"];

fn unknown() -> Error {
    Error::invalid("That folder is not in the folder list. Refresh the list and try again.")
}

fn find<'a>(boxes: &'a [Mailbox], id: &str) -> Result<&'a Mailbox> {
    boxes.iter().find(|m| m.id == id).ok_or_else(unknown)
}

fn locked(m: &Mailbox, what: &str) -> Error {
    Error::invalid(format!(
        "\"{}\" is one of the mailbox's own folders and cannot be {what} here. Only folders you made can be.",
        m.name
    ))
}

/// The name as it will be sent: trimmed, and refused when it cannot be a folder name.
pub fn check_name(name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::invalid("A folder needs a name."));
    }
    if name.contains('/') {
        return Err(Error::invalid("A folder name cannot contain a slash (/)."));
    }
    if name.chars().any(char::is_control) {
        return Err(Error::invalid("A folder name cannot contain a line break or another control character."));
    }
    if name.len() > MAX_NAME_BYTES {
        return Err(Error::invalid(format!("That folder name is too long (at most {MAX_NAME_BYTES} bytes).")));
    }
    Ok(name.to_string())
}

/// How deep the folder is: 1 at the top level. A loop in the data counts as too deep.
fn depth_of(boxes: &[Mailbox], id: &str) -> usize {
    let mut depth = 0;
    let mut at = Some(id);
    while let Some(current) = at {
        depth += 1;
        if depth > MAX_DEPTH + boxes.len() {
            break;
        }
        at = boxes.iter().find(|m| m.id == current).and_then(|m| m.parent_id.as_deref());
    }
    depth
}

/// How many levels of folders are below this one (0 for one with none inside).
fn height_below(boxes: &[Mailbox], id: &str, guard: usize) -> usize {
    if guard > boxes.len() {
        return 0;
    }
    boxes
        .iter()
        .filter(|m| m.parent_id.as_deref() == Some(id))
        .map(|m| 1 + height_below(boxes, &m.id, guard + 1))
        .max()
        .unwrap_or(0)
}

fn is_inside(boxes: &[Mailbox], id: &str, ancestor: &str) -> bool {
    let mut at = Some(id);
    for _ in 0..=boxes.len() {
        match at {
            Some(current) if current == ancestor => return true,
            Some(current) => at = boxes.iter().find(|m| m.id == current).and_then(|m| m.parent_id.as_deref()),
            None => return false,
        }
    }
    false
}

fn check_parent(boxes: &[Mailbox], parent_id: Option<&str>) -> Result<()> {
    let Some(parent_id) = parent_id else { return Ok(()) };
    let parent = find(boxes, parent_id)?;
    if parent.role.as_deref().is_some_and(|r| NO_CHILDREN_ROLES.contains(&r)) {
        return Err(Error::invalid("Folders cannot be put inside Drafts, Trash or Junk."));
    }
    Ok(())
}

fn check_free(boxes: &[Mailbox], parent_id: Option<&str>, name: &str, except: Option<&str>) -> Result<()> {
    let wanted = name.to_lowercase();
    let taken = boxes.iter().any(|m| {
        m.parent_id.as_deref() == parent_id && Some(m.id.as_str()) != except && m.name.to_lowercase() == wanted
    });
    match taken {
        true => Err(Error::invalid(match parent_id {
            Some(_) => "A folder with that name is already in that folder.",
            None => "A folder with that name is already there.",
        })),
        false => Ok(()),
    }
}

fn check_depth(boxes: &[Mailbox], parent_id: Option<&str>, below: usize) -> Result<()> {
    let level = parent_id.map_or(0, |p| depth_of(boxes, p)) + 1 + below;
    match level > MAX_DEPTH {
        true => Err(Error::invalid(format!("Folders can only be nested {MAX_DEPTH} levels deep."))),
        false => Ok(()),
    }
}

/// What creating a folder needs to be true; returns the name to send.
pub fn check_create(boxes: &[Mailbox], name: &str, parent_id: Option<&str>) -> Result<String> {
    let name = check_name(name)?;
    check_parent(boxes, parent_id)?;
    check_free(boxes, parent_id, &name, None)?;
    check_depth(boxes, parent_id, 0)?;
    Ok(name)
}

/// `Ok(None)` when the name is the one it has: nothing to ask the server.
pub fn check_rename(boxes: &[Mailbox], id: &str, name: &str) -> Result<Option<String>> {
    let folder = find(boxes, id)?;
    if folder.role.is_some() {
        return Err(locked(folder, "renamed"));
    }
    let name = check_name(name)?;
    if name == folder.name {
        return Ok(None);
    }
    check_free(boxes, folder.parent_id.as_deref(), &name, Some(id))?;
    Ok(Some(name))
}

/// `Ok(false)` when the folder is already there: nothing to ask the server.
pub fn check_move(boxes: &[Mailbox], id: &str, parent_id: Option<&str>) -> Result<bool> {
    let folder = find(boxes, id)?;
    if folder.role.is_some() {
        return Err(locked(folder, "moved"));
    }
    if parent_id == folder.parent_id.as_deref() {
        return Ok(false);
    }
    if let Some(parent) = parent_id {
        if parent == id || is_inside(boxes, parent, id) {
            return Err(Error::invalid("A folder cannot be put inside itself or inside a folder in it."));
        }
    }
    check_parent(boxes, parent_id)?;
    check_free(boxes, parent_id, &folder.name, Some(id))?;
    check_depth(boxes, parent_id, height_below(boxes, id, 0))?;
    Ok(true)
}

/// Refuses a delete the app does not do; returns the Trash folder's id when there is one.
pub fn check_delete<'a>(boxes: &'a [Mailbox], id: &str, with_mail: bool) -> Result<Option<&'a str>> {
    let folder = find(boxes, id)?;
    if folder.role.is_some() {
        return Err(locked(folder, "deleted"));
    }
    if boxes.iter().any(|m| m.parent_id.as_deref() == Some(id)) {
        return Err(Error::invalid(format!("\"{}\" has folders inside it. Delete or move those first.", folder.name)));
    }
    let trash = boxes.iter().find(|m| m.role.as_deref() == Some("trash")).map(|m| m.id.as_str());
    if folder.total_emails > 0 {
        if !with_mail {
            return Err(Error::invalid(format!(
                "\"{}\" still holds mail. Move it out first, or delete the folder together with its mail (the \
                 mail goes to the Trash).",
                folder.name
            )));
        }
        if trash.is_none() {
            return Err(Error::invalid("The account has no Trash folder to move the mail to."));
        }
    }
    Ok(trash)
}

/// Switches a folder's notifications on or off, here only. Switching on counts from just after
/// the newest mail the cache has of the folder (else from `now`). The Inbox cannot be switched off;
/// Drafts, Sent, Trash and Junk cannot be switched on.
pub fn set_notify(store: &Store, account_id: &str, id: &str, on: bool, now: i64) -> Result<()> {
    let folder = store.get_mailbox(account_id, id)?.ok_or_else(unknown)?;
    match (folder.role.as_deref(), on) {
        (Some("inbox"), true) => return Ok(()),
        (Some("inbox"), false) => return Err(Error::invalid("The Inbox always notifies; it cannot be switched off.")),
        (Some(role), true) if QUIET_ROLES.contains(&role) => {
            return Err(Error::invalid(format!("\"{}\" never notifies; it cannot be switched on.", folder.name)))
        }
        _ => {}
    }
    // "From now on" is told by the server's clock, not the phone's: the phone's `now` is only
    // used when the cache holds no mail of the folder to tell it by. Mail the cache already has
    // is older than the switch; mail that comes after it is newer than all of it.
    let since = match store.newest_received_in(account_id, id)? {
        Some(newest) => newest.saturating_add(1),
        None => now,
    };
    store.set_mailbox_notify(account_id, id, on, since)?;
    Ok(())
}

/// Makes the folder on the server and puts it in the cache.
pub async fn create(
    conn: &Connection,
    store: &Store,
    account_id: &str,
    name: &str,
    parent_id: Option<&str>,
) -> Result<Mailbox> {
    let boxes = store.list_mailboxes(account_id)?;
    let name = check_create(&boxes, name, parent_id)?;
    let id = conn.mailbox_create(&name, parent_id).await?;
    // The server made it. What it holds beyond this (the order) is read when it can be; failing
    // to read it is not a failure to make the folder.
    let made = Mailbox {
        id: id.clone(),
        name,
        role: None,
        parent_id: parent_id.map(str::to_string),
        sort_order: 0,
        total_emails: 0,
        unread_emails: 0,
        notify: false,
    };
    let made = match conn.mailboxes_by_id(std::slice::from_ref(&id)).await {
        Ok(read) => read.into_iter().next().unwrap_or(made),
        Err(_) => made,
    };
    store.upsert_mailboxes(account_id, std::slice::from_ref(&made))?;
    Ok(store.get_mailbox(account_id, &id)?.unwrap_or(made))
}

pub async fn rename(conn: &Connection, store: &Store, account_id: &str, id: &str, name: &str) -> Result<()> {
    let boxes = store.list_mailboxes(account_id)?;
    let Some(name) = check_rename(&boxes, id, name)? else { return Ok(()) };
    conn.mailbox_update(id, json!({ "name": name })).await?;
    let folder = Mailbox { name, ..find(&boxes, id)?.clone() };
    store.upsert_mailboxes(account_id, &[folder])
}

pub async fn move_to(
    conn: &Connection,
    store: &Store,
    account_id: &str,
    id: &str,
    parent_id: Option<&str>,
) -> Result<()> {
    let boxes = store.list_mailboxes(account_id)?;
    if !check_move(&boxes, id, parent_id)? {
        return Ok(());
    }
    conn.mailbox_update(id, json!({ "parentId": parent_id })).await?;
    let folder = Mailbox { parent_id: parent_id.map(str::to_string), ..find(&boxes, id)?.clone() };
    store.upsert_mailboxes(account_id, &[folder])
}

/// Deletes the folder. With `with_mail` its mail goes to the Trash first, in batches, and only
/// when all of it is out is the empty folder destroyed: a failure part-way leaves the folder,
/// and the error says how many messages had been moved.
pub async fn delete(conn: &Connection, store: &Store, account_id: &str, id: &str, with_mail: bool) -> Result<()> {
    // The role, the folders inside and the mail are judged on what the server says now, before
    // any mail is moved: the cache may not know yet what another client did.
    find(&store.list_mailboxes(account_id)?, id)?;
    let (fresh, _) = conn.mailboxes().await?;
    if !fresh.iter().any(|m| m.id == id) {
        // Gone elsewhere already: as good as deleted.
        return store.forget_mailbox(account_id, id, None);
    }
    let trash = check_delete(&fresh, id, with_mail)?.map(str::to_string);
    let mut moved = 0;
    if with_mail {
        // The cache may say the folder is empty while it is not (mail arrived since): the
        // server is asked either way.
        moved = conn.empty_mailbox(id, trash.as_deref()).await?;
    }
    let mut result = conn.mailbox_destroy(id).await;
    if with_mail && matches!(result, Err(Error::Invalid { .. })) {
        // Mail may have been delivered between the last batch and the destroy: once more.
        moved += conn.empty_mailbox(id, trash.as_deref()).await?;
        result = conn.mailbox_destroy(id).await;
    }
    // Nothing is lost: the mail is in the Trash and the folder is still there.
    let note = "The mail was moved to the Trash, but the folder was not deleted.";
    result.map_err(|e| match e {
        Error::Temporary { msg } if with_mail => Error::temporary(format!("{msg} {note}")),
        Error::Invalid { msg } if moved > 0 => Error::invalid(format!("{msg} {note}")),
        other => other,
    })?;
    store.forget_mailbox(account_id, id, trash.as_deref().filter(|_| with_mail))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(id: &str, name: &str, parent: Option<&str>, role: Option<&str>, total: u32) -> Mailbox {
        Mailbox {
            id: id.into(),
            name: name.into(),
            role: role.map(str::to_string),
            parent_id: parent.map(str::to_string),
            sort_order: 0,
            total_emails: total,
            unread_emails: 0,
            notify: false,
        }
    }

    fn boxes() -> Vec<Mailbox> {
        vec![
            folder("in", "Inbox", None, Some("inbox"), 3),
            folder("tr", "Trash", None, Some("trash"), 0),
            folder("dr", "Drafts", None, Some("drafts"), 0),
            folder("jk", "Junk", None, Some("junk"), 0),
            folder("w", "Work", None, None, 0),
            folder("b", "Bills", Some("w"), None, 2),
            folder("p", "Private", None, None, 0),
        ]
    }

    fn refusal(r: Result<impl std::fmt::Debug>) -> String {
        match r {
            Err(Error::Invalid { msg }) => msg,
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    #[test]
    fn a_name_is_trimmed_and_refused_when_it_cannot_be_one() {
        assert_eq!(check_name("  Receipts ").unwrap(), "Receipts");
        assert_eq!(check_name("Türkçe 日本語 😀").unwrap(), "Türkçe 日本語 😀");
        assert!(refusal(check_name("   ")).contains("needs a name"));
        assert!(refusal(check_name("a/b")).contains("slash"));
        assert!(refusal(check_name("a\nb")).contains("control character"));
        assert!(refusal(check_name("a\u{0}b")).contains("control character"));
        assert!(check_name(&"x".repeat(255)).is_ok());
        assert!(refusal(check_name(&"x".repeat(256))).contains("too long"));
        // Bytes, not characters.
        assert!(refusal(check_name(&"é".repeat(128))).contains("too long"));
    }

    #[test]
    fn a_new_folder_needs_a_free_name_and_a_parent_that_takes_folders() {
        let b = boxes();
        assert_eq!(check_create(&b, "Receipts", Some("w")).unwrap(), "Receipts");
        assert!(refusal(check_create(&b, "work", None)).contains("already there"));
        assert!(refusal(check_create(&b, " INBOX ", None)).contains("already there"));
        assert!(refusal(check_create(&b, "bills", Some("w"))).contains("already in that folder"));
        // The same name somewhere else is fine.
        assert!(check_create(&b, "Bills", None).is_ok());
        for parent in ["dr", "tr", "jk"] {
            assert!(refusal(check_create(&b, "x", Some(parent))).contains("Drafts, Trash or Junk"));
        }
        assert!(check_create(&b, "x", Some("in")).is_ok());
        assert!(refusal(check_create(&b, "x", Some("nope"))).contains("not in the folder list"));
    }

    #[test]
    fn nesting_stops_at_ten_levels() {
        let mut b = vec![folder("d1", "d1", None, None, 0)];
        for i in 2..=10 {
            b.push(folder(&format!("d{i}"), &format!("d{i}"), Some(&format!("d{}", i - 1)), None, 0));
        }
        assert!(check_create(&b, "x", Some("d9")).is_ok(), "the tenth level");
        assert!(refusal(check_create(&b, "x", Some("d10"))).contains("10 levels"));
        // Moving a folder with folders in it counts them.
        b.push(folder("t1", "t1", None, None, 0));
        b.push(folder("t2", "t2", Some("t1"), None, 0));
        assert!(check_move(&b, "t1", Some("d8")).unwrap(), "t1 and t2 take levels 9 and 10");
        assert!(refusal(check_move(&b, "t1", Some("d9"))).contains("10 levels"));
    }

    #[test]
    fn only_folders_the_user_made_are_renamed_moved_or_deleted() {
        let b = boxes();
        for id in ["in", "tr", "dr", "jk"] {
            assert!(refusal(check_rename(&b, id, "x")).contains("own folders"));
            assert!(refusal(check_move(&b, id, Some("p"))).contains("own folders"));
            assert!(refusal(check_delete(&b, id, true)).contains("own folders"));
        }
        assert_eq!(check_rename(&b, "p", "Personal").unwrap(), Some("Personal".to_string()));
        assert_eq!(check_rename(&b, "p", " Private ").unwrap(), None, "the same name asks nothing");
        assert_eq!(check_rename(&b, "p", "PRIVATE").unwrap(), Some("PRIVATE".to_string()), "a change of case is one");
        assert!(refusal(check_rename(&b, "p", "work")).contains("already there"));
        assert!(refusal(check_rename(&b, "p", "")).contains("needs a name"));
        assert!(refusal(check_rename(&b, "zz", "x")).contains("not in the folder list"));
    }

    #[test]
    fn a_folder_does_not_go_inside_itself() {
        let b = boxes();
        assert!(refusal(check_move(&b, "w", Some("w"))).contains("inside itself"));
        assert!(refusal(check_move(&b, "w", Some("b"))).contains("inside itself"));
        assert!(check_move(&b, "b", None).unwrap());
        assert!(!check_move(&b, "b", Some("w")).unwrap(), "already there");
        assert!(refusal(check_move(&b, "b", Some("jk"))).contains("Drafts, Trash or Junk"));
        // A name that is taken at the target.
        let mut c = boxes();
        c.push(folder("p2", "Bills", None, None, 0));
        assert!(refusal(check_move(&c, "b", None)).contains("already there"));
    }

    #[test]
    fn a_delete_is_refused_for_folders_inside_and_for_mail_unless_asked() {
        let b = boxes();
        assert!(refusal(check_delete(&b, "w", false)).contains("folders inside"));
        assert!(refusal(check_delete(&b, "w", true)).contains("folders inside"));
        assert!(refusal(check_delete(&b, "b", false)).contains("still holds mail"));
        assert_eq!(check_delete(&b, "b", true).unwrap(), Some("tr"));
        assert_eq!(check_delete(&b, "p", false).unwrap(), Some("tr"));
        let no_trash: Vec<Mailbox> = b.iter().filter(|m| m.id != "tr").cloned().collect();
        assert!(refusal(check_delete(&no_trash, "b", true)).contains("no Trash"));
        assert!(check_delete(&no_trash, "p", true).is_ok(), "an empty folder needs no Trash");
    }

    #[test]
    fn notify_is_refused_where_the_contract_says() {
        let store = Store::open_in_memory().unwrap();
        let account = crate::model::Account {
            id: "a".into(),
            server_url: "https://x.example".into(),
            username: "u".into(),
            signed_in_at: 0,
        };
        store.save_account(&account).unwrap();
        store.upsert_mailboxes("a", &boxes()).unwrap();
        assert!(refusal(set_notify(&store, "a", "in", false, 5)).contains("always notifies"));
        assert!(set_notify(&store, "a", "in", true, 5).is_ok());
        for id in ["dr", "tr", "jk"] {
            assert!(refusal(set_notify(&store, "a", id, true, 5)).contains("never notifies"));
            assert!(set_notify(&store, "a", id, false, 5).is_ok());
        }
        assert!(refusal(set_notify(&store, "a", "zz", true, 5)).contains("not in the folder list"));
        set_notify(&store, "a", "p", true, 5).unwrap();
        assert!(store.get_mailbox("a", "p").unwrap().unwrap().notify);
        set_notify(&store, "a", "p", false, 6).unwrap();
        assert!(!store.get_mailbox("a", "p").unwrap().unwrap().notify);
    }
}
