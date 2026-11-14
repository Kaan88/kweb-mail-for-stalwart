//! Mail filters (M8): rules the user edits in the app, kept on the server as one Sieve script.
//!
//! The rules are the source of truth. They are stored as JSON in a comment at the top of the
//! script named `kweb-filters`, followed by the Sieve generated from them; the app never parses
//! Sieve. Folders are named in the script by their path and found again from the mailbox id in
//! the JSON every time the script is read, so a folder renamed elsewhere shows as stale and a
//! save writes the new path. See `dev/notes/stalwart-sieve.md` for what the server does.
//!
//! `check` and `script` are pure. This file puts them together with the server.

mod check;
mod script;

use std::collections::HashSet;

pub use check::{check_rules, is_header_name, MAX_RULES};
pub use script::{generate, read, Entry, Reading, FORMAT};

use crate::{
    jmap::{Connection, SieveScript},
    model::{FilterCondition, FilterField, FilterMatch, FilterRule, Filters, Mailbox, MessageHeader},
    Error, Result,
};

/// The error after a write that went through but could not be read back. The host reads again.
pub const SAVED_NOT_READ: &str =
    "The filters were saved, but reading them back failed. Read the filters again to see them.";

/// The one script the app owns.
pub const SCRIPT_NAME: &str = "kweb-filters";
/// The script the server keeps for the away message.
const VACATION_NAME: &str = "vacation";
const NO_FILTERS: &str = "this server has no mail filters";

/// The filters as the server has them, and the state of the scripts they were read at.
#[derive(Debug, Clone, PartialEq)]
pub struct Loaded {
    pub filters: Filters,
    pub state: String,
}

/// Where a folder is, as the script names it.
#[derive(Debug, PartialEq)]
enum Path {
    Found(String),
    Missing,
    /// A name in the path holds a "/" or a control character, which a script cannot address.
    Unaddressable(String),
}

/// The path of the folder `id`: the names from the top, joined with "/".
fn path_of(boxes: &[Mailbox], id: &str) -> Path {
    let mut names = Vec::new();
    let mut at = Some(id);
    while let Some(current) = at {
        let Some(found) = boxes.iter().find(|m| m.id == current) else { return Path::Missing };
        if found.name.contains('/') || found.name.chars().any(char::is_control) {
            return Path::Unaddressable(found.name.clone());
        }
        names.push(found.name.as_str());
        if names.len() > 64 {
            return Path::Missing;
        }
        at = found.parent_id.as_deref();
    }
    names.reverse();
    Path::Found(names.join("/"))
}

fn trash_id(boxes: &[Mailbox]) -> Option<&str> {
    boxes.iter().find(|m| m.role.as_deref() == Some("trash")).map(|m| m.id.as_str())
}

/// The folder a rule's script text names, as it is now.
fn target_now(rule: &FilterRule, boxes: &[Mailbox]) -> Option<String> {
    let id = match (&rule.move_to, rule.delete) {
        (Some(id), _) => id.as_str(),
        (None, true) => trash_id(boxes)?,
        (None, false) => return None,
    };
    match path_of(boxes, id) {
        Path::Found(path) => Some(path),
        _ => None,
    }
}

/// What a script text says about the rules: `(rules, is_stale, is_edited)`. `exists` is
/// whether the server has the script at all.
fn describe(exists: bool, text: &str, boxes: &[Mailbox]) -> (Vec<FilterRule>, bool, bool) {
    if !exists {
        return (Vec::new(), false, false);
    }
    let Reading::Rules(entries) = read(text) else { return (Vec::new(), false, true) };
    let mut stale = false;
    let mut rules = Vec::new();
    for entry in entries {
        let mut rule = entry.rule;
        rule.folder_missing = rule.move_to.as_deref().is_some_and(|id| path_of(boxes, id) == Path::Missing);
        if rule.enabled {
            // A target that cannot be named now (a "/" in its name, no Trash) is not what the
            // script says either, so it is stale too.
            stale |= match target_now(&rule, boxes) {
                Some(now) => entry.path.as_deref() != Some(now.as_str()),
                None => {
                    let wants_target = rule.move_to.is_some() || rule.delete;
                    wants_target && !rule.folder_missing
                }
            };
        }
        rules.push(rule);
    }
    (rules, stale, false)
}

fn find<'a>(scripts: &'a [SieveScript], name: &str) -> Option<&'a SieveScript> {
    scripts.iter().find(|s| s.name.eq_ignore_ascii_case(name))
}

/// Reads the filters from the server: the scripts, the app's script and what it says, the
/// folders (fresh, so a folder renamed a minute ago counts) and the away message.
pub async fn load(conn: &Connection) -> Result<Loaded> {
    if !conn.has_sieve() {
        return Err(Error::invalid(NO_FILTERS));
    }
    let list = conn.sieve_scripts().await?;
    let (boxes, _) = conn.mailboxes().await?;
    let ours = find(&list.scripts, SCRIPT_NAME);
    let text = match ours {
        Some(script) => conn.sieve_text(script).await?,
        None => String::new(),
    };
    let (rules, is_stale, is_edited) = describe(ours.is_some(), &text, &boxes);
    let away_script = find(&list.scripts, VACATION_NAME).is_some_and(|s| s.is_active);
    let away_is_on = away_script || conn.away_is_on().await?;
    let other = list.scripts.iter().find(|s| {
        s.is_active && !s.name.eq_ignore_ascii_case(SCRIPT_NAME) && !s.name.eq_ignore_ascii_case(VACATION_NAME)
    });
    let other_text = match other {
        Some(script) => Some(conn.sieve_text(script).await?),
        None => None,
    };
    let filters = Filters {
        rules,
        is_active: ours.is_some_and(|s| s.is_active),
        is_stale,
        is_edited,
        text,
        away_is_on,
        other_active: other.map(|s| s.name.clone()),
        other_text,
    };
    Ok(Loaded { filters, state: list.state })
}

/// Gives every rule an id and finds the folder path each enabled rule writes into the script.
fn entries_for(rules: &[FilterRule], boxes: &[Mailbox]) -> Result<Vec<Entry>> {
    let mut used: HashSet<String> = rules.iter().filter(|r| !r.id.is_empty()).map(|r| r.id.clone()).collect();
    let mut entries = Vec::new();
    for (index, rule) in rules.iter().enumerate() {
        let who = check::label(rule, index);
        let mut rule = rule.clone();
        rule.folder_missing = false;
        if rule.id.is_empty() {
            rule.id = new_id(&used)?;
            used.insert(rule.id.clone());
        }
        let wanted = match (&rule.move_to, rule.delete) {
            (Some(id), _) => {
                Some((id.as_str(), "moves mail to a folder that no longer exists. Choose another folder."))
            }
            (None, true) => {
                trash_id(boxes).map(|id| (id, "")).or(Some(("", "deletes mail, but there is no Trash folder.")))
            }
            (None, false) => None,
        };
        let path = match wanted {
            None => None,
            Some((id, gone)) => match path_of(boxes, id) {
                Path::Found(path) => Some(path),
                // A rule that is off does not write the folder, so it does not need it.
                _ if !rule.enabled => None,
                Path::Missing => return Err(Error::invalid(format!("{who} {gone}"))),
                Path::Unaddressable(name) => {
                    return Err(Error::invalid(format!(
                        "{who} points at the folder \"{name}\", whose name has a \"/\" or a control character in it; \
                         a filter cannot name such a folder."
                    )))
                }
            },
        };
        entries.push(Entry { rule, path });
    }
    Ok(entries)
}

fn new_id(used: &HashSet<String>) -> Result<String> {
    loop {
        let mut bytes = [0u8; 8];
        getrandom::fill(&mut bytes).map_err(|e| Error::temporary(format!("random numbers: {e}")))?;
        let id: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        if !used.contains(&id) {
            return Ok(id);
        }
    }
}

fn check_state(base: Option<&str>, now: &str) -> Result<()> {
    match base {
        Some(base) if base != now => Err(Error::invalid(crate::jmap::SIEVE_CHANGED)),
        _ => Ok(()),
    }
}

/// Checks the rules, has the server check the script, and only then writes it. `base_state` is
/// the state the host's last read was at, when it is known. Nothing is written when anything
/// fails.
pub async fn save(
    conn: &Connection,
    base_state: Option<&str>,
    rules: &[FilterRule],
    activate: bool,
    overwrite: bool,
) -> Result<Loaded> {
    if !conn.has_sieve() {
        return Err(Error::invalid(NO_FILTERS));
    }
    let limits = conn.sieve_limits().await?;
    check_rules(rules, &limits.extensions)?;
    let (boxes, _) = conn.mailboxes().await?;
    let entries = entries_for(rules, &boxes)?;
    let text = generate(&entries);
    if let Some(max) = limits.max_size.filter(|max| text.len() as u64 > *max) {
        return Err(Error::invalid(format!(
            "These filters are too big for the server: the script would be {} bytes and the server takes {max}. \
             Remove or shorten some rules.",
            text.len()
        )));
    }
    let list = conn.sieve_scripts().await?;
    check_state(base_state, &list.state)?;
    let ours = find(&list.scripts, SCRIPT_NAME);
    if let Some(script) = ours {
        if !overwrite && read(&conn.sieve_text(script).await?) == Reading::Edited {
            return Err(Error::invalid(
                "The filter script on the server was changed by hand or by another program. \
                 Saving would replace it; save again with overwrite to do that.",
            ));
        }
    }
    let blob_id = conn.sieve_upload(&text).await?;
    if let Some(reason) = conn.sieve_validate(&blob_id).await? {
        return Err(Error::invalid(format!("The server does not accept these filters: {reason}")));
    }
    conn.sieve_write(SCRIPT_NAME, ours.map(|s| s.id.as_str()), &blob_id, &list.state, activate).await?;
    read_back(conn).await
}

/// Switches the app's script on or off. Off does nothing unless the app's script is the
/// active one: another script, or the away message, is left alone.
pub async fn set_active(conn: &Connection, base_state: Option<&str>, active: bool) -> Result<Loaded> {
    if !conn.has_sieve() {
        return Err(Error::invalid(NO_FILTERS));
    }
    let list = conn.sieve_scripts().await?;
    check_state(base_state, &list.state)?;
    let ours = find(&list.scripts, SCRIPT_NAME).ok_or_else(|| Error::invalid("There are no filters saved yet."))?;
    match (active, ours.is_active) {
        (true, false) => conn.sieve_activate(Some(&ours.id), &list.state).await?,
        (false, true) => conn.sieve_activate(None, &list.state).await?,
        _ => {}
    }
    read_back(conn).await
}

/// Reads the filters after a write that succeeded. A failure is tried once more and then
/// reported as "saved, not read back" (never as a failed save, and never as an error that makes
/// the caller run the write again).
async fn read_back(conn: &Connection) -> Result<Loaded> {
    match load(conn).await {
        Ok(loaded) => Ok(loaded),
        Err(_) => load(conn).await.map_err(|_| Error::temporary(SAVED_NOT_READ)),
    }
}

/// A rule to start from: the sender's address, or the mailing list's id when the message has
/// one, and no action.
pub fn rule_from(message: &MessageHeader, list_id: Option<&str>) -> Result<FilterRule> {
    let (name, condition) = match list_id.map(list_address).filter(|l| !l.is_empty()) {
        Some(list) => (
            list.clone(),
            FilterCondition {
                field: FilterField::Header { name: "List-Id".into() },
                how: FilterMatch::Contains,
                value: list,
            },
        ),
        None => {
            let address = message.from_email.trim();
            if address.is_empty() || address.chars().any(char::is_control) {
                return Err(Error::invalid("That message has no sender address to make a rule from."));
            }
            let name = match message.from_name.trim() {
                "" => address.to_string(),
                shown if !shown.chars().any(char::is_control) => shown.to_string(),
                _ => address.to_string(),
            };
            (name, FilterCondition { field: FilterField::From, how: FilterMatch::Contains, value: address.to_string() })
        }
    };
    Ok(FilterRule {
        id: String::new(),
        name,
        enabled: true,
        match_all: true,
        conditions: vec![condition],
        move_to: None,
        mark_read: false,
        star: false,
        delete: false,
        stop: false,
        folder_missing: false,
    })
}

/// The rule "Blocked: <address>": From contains the address, delete, stop.
pub fn block_rule(message: &MessageHeader) -> Result<FilterRule> {
    let mut rule = rule_from(message, None)?;
    let address = rule.conditions[0].value.clone();
    rule.name = format!("Blocked: {address}");
    rule.delete = true;
    rule.stop = true;
    Ok(rule)
}

/// Whether `rule` is what [`block_rule`] makes for this address (the address compared without
/// regard to case) and is on.
fn blocks(rule: &FilterRule, address: &str) -> bool {
    rule.enabled
        && rule.delete
        && rule.match_all
        && rule.move_to.is_none()
        && matches!(rule.conditions.as_slice(), [c] if c.field == FilterField::From
            && c.how == FilterMatch::Contains && c.value.eq_ignore_ascii_case(address))
}

/// Puts the rule that blocks the sender first in the app's filters and switches the filters on.
/// `own` are the user's own addresses (lower case): blocking one is refused. A sender that is
/// blocked already changes nothing (except that a script that was switched off is switched on).
/// Refused, with words for the user, when the server has no filters, when the away message or
/// another script is active, or when the app's script was edited by hand.
pub async fn block(conn: &Connection, message: &MessageHeader, own: &HashSet<String>) -> Result<Loaded> {
    let rule = block_rule(message)?;
    let address = rule.conditions[0].value.clone();
    let mut own = own.clone();
    match conn.identities().await {
        Ok(identities) => own.extend(identities.iter().map(|i| i.email.trim().to_lowercase())),
        Err(e) if crate::jmap::is_unauthorized(&e) => return Err(e),
        // The cached identities and the sign-in name stand in.
        Err(_) => {}
    }
    // The rule is "From contains", so an own address that merely ends in this one (joann@ for
    // ann@) would be caught by it too.
    let wanted = address.to_lowercase();
    if own.iter().any(|o| o.contains(&wanted)) {
        return Err(Error::invalid(
            "That is one of your own addresses. Blocking it would send your own mail to the Trash.",
        ));
    }
    if !conn.has_sieve() {
        return Err(Error::invalid("This server has no mail filters, so a sender cannot be blocked here."));
    }
    let loaded = load(conn).await?;
    let f = &loaded.filters;
    if f.away_is_on {
        return Err(Error::invalid(
            "The away message is on, and the server runs it instead of any filter. Decide in Filters what \
             should be on, then block the sender.",
        ));
    }
    if let Some(other) = &f.other_active {
        return Err(Error::invalid(format!(
            "Another filter script, \"{other}\", is the one the server runs, and blocking would switch it off. \
             Decide in Filters what should be on, then block the sender."
        )));
    }
    if f.is_edited {
        return Err(Error::invalid(
            "The filter script on the server was changed by hand, so the app cannot add to it. Open Filters to \
             decide what to do with it.",
        ));
    }
    if f.rules.iter().any(|r| blocks(r, &address)) {
        return match f.is_active {
            true => Ok(loaded),
            false => set_active(conn, Some(&loaded.state), true).await,
        };
    }
    if let Some((index, other)) = f.rules.iter().enumerate().find(|(_, r)| r.enabled && r.folder_missing) {
        return Err(Error::invalid(format!(
            "Blocking has to save all the filters, and {} moves mail to a folder that no longer exists. \
             Open Filters, choose another folder or switch that rule off, then block the sender.",
            check::label(other, index)
        )));
    }
    let mut rules = vec![rule];
    rules.extend(f.rules.iter().cloned());
    save(conn, Some(&loaded.state), &rules, true, false).await
}

/// The id inside the angle brackets of a List-Id (`Name <list.example.org>`), else the whole
/// value. Control characters would be refused by the rule check, so they are dropped here.
fn list_address(value: &str) -> String {
    let inside = value.rfind('<').and_then(|open| value[open + 1..].split_once('>')).map(|(id, _)| id);
    inside.unwrap_or(value).chars().filter(|c| !c.is_control()).collect::<String>().trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mailbox(id: &str, name: &str, parent: Option<&str>, role: Option<&str>) -> Mailbox {
        Mailbox {
            id: id.into(),
            name: name.into(),
            role: role.map(str::to_string),
            parent_id: parent.map(str::to_string),
            sort_order: 0,
            total_emails: 0,
            unread_emails: 0,
            notify: false,
        }
    }

    #[test]
    fn a_path_is_the_names_from_the_top() {
        let boxes = vec![
            mailbox("a", "Work", None, None),
            mailbox("b", "Bills", Some("a"), None),
            mailbox("c", "A/B", None, None),
            mailbox("d", "Loop", Some("e"), None),
            mailbox("e", "Loop2", Some("d"), None),
        ];
        assert_eq!(path_of(&boxes, "b"), Path::Found("Work/Bills".into()));
        assert_eq!(path_of(&boxes, "zz"), Path::Missing);
        assert_eq!(path_of(&boxes, "c"), Path::Unaddressable("A/B".into()));
        assert_eq!(path_of(&boxes, "d"), Path::Missing);
    }

    #[test]
    fn a_block_rule_is_the_senders_address_deleted_and_stopped() {
        let message = MessageHeader {
            id: "m".into(),
            thread_id: "t".into(),
            mailbox_ids: vec![],
            from_name: "Ann Example".into(),
            from_email: " Ann@Example.org ".into(),
            to: vec![],
            subject: String::new(),
            preview: String::new(),
            received_at: 0,
            is_unread: true,
            is_flagged: false,
            has_attachment: false,
            size: 0,
        };
        let rule = block_rule(&message).unwrap();
        assert_eq!(rule.name, "Blocked: Ann@Example.org");
        assert!(rule.enabled && rule.delete && rule.stop && rule.move_to.is_none());
        assert_eq!(rule.conditions[0].field, FilterField::From);
        assert_eq!(rule.conditions[0].value, "Ann@Example.org");
        assert!(blocks(&rule, "ann@example.org"));
        let mut off = rule.clone();
        off.enabled = false;
        assert!(!blocks(&off, "ann@example.org"));
        let mut moves = rule.clone();
        moves.delete = false;
        moves.move_to = Some("x".into());
        assert!(!blocks(&moves, "ann@example.org"));
        assert!(!blocks(&rule, "bob@example.org"));
        let nobody = MessageHeader { from_email: String::new(), ..message };
        assert!(matches!(block_rule(&nobody), Err(Error::Invalid { .. })));
    }

    #[test]
    fn a_list_id_is_taken_from_its_brackets() {
        assert_eq!(list_address("Dev list <dev.lists.example.org>"), "dev.lists.example.org");
        assert_eq!(list_address("  news.example.org "), "news.example.org");
        assert_eq!(list_address("<a.b>"), "a.b");
    }

    #[test]
    fn a_rule_starts_from_the_sender_or_the_list() {
        let message = MessageHeader {
            id: "m".into(),
            thread_id: "t".into(),
            mailbox_ids: vec![],
            from_name: "Ann Example".into(),
            from_email: "ann@example.org".into(),
            to: vec![],
            subject: String::new(),
            preview: String::new(),
            received_at: 0,
            is_unread: true,
            is_flagged: false,
            has_attachment: false,
            size: 0,
        };
        let rule = rule_from(&message, None).unwrap();
        assert_eq!(rule.name, "Ann Example");
        assert_eq!(rule.conditions[0].value, "ann@example.org");
        assert!(rule.conditions[0].field == FilterField::From && rule.enabled && rule.id.is_empty());
        let list = rule_from(&message, Some("Dev <dev.example.org>")).unwrap();
        assert_eq!(list.name, "dev.example.org");
        assert_eq!(list.conditions[0].field, FilterField::Header { name: "List-Id".into() });
        let nobody = MessageHeader { from_email: " ".into(), ..message };
        assert!(matches!(rule_from(&nobody, None), Err(Error::Invalid { .. })));
    }
}
