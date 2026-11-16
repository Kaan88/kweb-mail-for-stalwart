//! What a suggestion of the chat is and does. The loop only builds one ([`propose`]); it is
//! applied ([`apply`]) and taken back ([`undo`]) through a [`Writer`], which only `ai_apply` and
//! `ai_undo` hold, called by the host when the user taps. Nothing here sends or deletes.
//!
//! No network code, no `Store` and no `Core` in this file.
//!
//! A suggestion can mark, star, tag, archive or move up to 50 messages of the cache, or add one
//! calendar entry. Mail is never moved into Trash, Junk or Drafts, and mail that is there is left
//! where it is, so that neither a change nor its Undo ever deletes or reports anything.

use std::collections::HashSet;

use chrono::{Datelike, NaiveDate, NaiveDateTime, NaiveTime};
use serde_json::Value;

use super::{
    prompts,
    tools::{
        bool_arg, check_id, id_arg, int_arg, optional_id, text_arg, zone_of, Env, Look, Reader, ACTIONS_OFF,
        MESSAGES_PER_CHANGE, NO_FOLDER, NO_MESSAGE,
    },
};
use crate::{
    compose::rules,
    model::{
        Action, Address, AiActionMode, AiApplied, AiChange, AiMessageRef, AiRisk, AiToolKind, Calendar, EventInput,
        Mailbox, MessageHeader,
    },
    Error, Result,
};

/// The only way anything is changed; held by ai_apply and ai_undo alone, never by the loop.
pub trait Writer {
    /// The implementation refuses Delete and Destroy.
    fn submit_all(&self, actions: &[Action]) -> Result<()>;
    fn set_label(&self, ids: &[String], label_id: &str, on: bool) -> Result<()>;
    /// Saves a new event and returns its id.
    fn save_event(&self, input: &EventInput) -> Result<String>;
    fn delete_event(&self, id: &str) -> Result<()>;
}

/// How an applied change is taken back.
#[derive(Debug, Clone, PartialEq)]
pub enum Undo {
    Actions(Vec<Action>),
    Label { ids: Vec<String>, label_id: String, on: bool },
    Event { id: String },
}

/// More messages than this in one change and its card warns ([`AiRisk::Confirm`]).
const CONFIRM_ABOVE: usize = 10;
/// Limits of a new calendar entry.
const TITLE_CHARS: usize = 300;
const LOCATION_CHARS: usize = 500;
const DESCRIPTION_CHARS: usize = 4_000;
/// The longest entry, in minutes (31 days).
const MOST_MINUTES: u64 = 44_640;
const LOCAL_FORMAT: &str = "%Y-%m-%dT%H:%M:%S";

const NO_IDS: &str = "name at least one message in 'message_ids'";
const NOT_A_LIST: &str = "the argument 'message_ids' must be a list of message ids";
const TOO_MANY: &str = "one suggestion can name at most 50 messages; suggest fewer at a time";
const NO_ARCHIVE: &str = "the account has no Archive folder";
const NEVER_DELETES: &str = "the assistant never deletes mail: nothing can be moved to Trash";
const NEVER_JUNK: &str = "the assistant never reports spam: nothing can be moved to Junk";
const NEVER_DRAFTS: &str = "the assistant never moves mail into Drafts";
const LEFT_ALONE: &str = "mail in Trash, Junk or Drafts is left where it is";
const NOT_ONE_FOLDER: &str = "a message that is in more than one folder cannot be moved by the assistant";
const ONE_MARK: &str = "give exactly one of 'read' and 'starred'";
const NO_LABEL: &str = "there is no such label; use an id from list_labels (the assistant cannot create a label)";
const NO_CALENDAR: &str = "there is no such calendar; use an id from list_events";
const READ_ONLY: &str = "that calendar cannot be added to";
const NO_WRITABLE: &str = "the user has no calendar that can be added to";
const BAD_START: &str = "the argument 'start' must be a local date and time such as 2026-11-12T10:00:00";
const NOT_A_CHANGE: &str = "that tool suggests no change";
const GONE: &str = "one of these messages is no longer there";

fn refuse<T>(sentence: &str) -> Result<T> {
    Err(Error::invalid(sentence))
}

/// A message as a card or a chip names it, from the cache, with the name of its first folder.
pub(crate) fn message_ref(header: &MessageHeader, folders: &[Mailbox]) -> AiMessageRef {
    let first = header.mailbox_ids.first();
    AiMessageRef {
        id: header.id.clone(),
        thread_id: header.thread_id.clone(),
        subject: header.subject.clone(),
        from: rules::address_text(&Address { name: header.from_name.clone(), email: header.from_email.clone() }),
        mailbox_name: first
            .and_then(|id| folders.iter().find(|f| &f.id == id))
            .map_or(String::new(), |f| f.name.clone()),
    }
}

/// Validates a change tool's arguments against the cache and builds the card. Err(Invalid) = a sentence for the model.
pub fn propose(reader: &dyn Reader, env: &Env<'_>, kind: AiToolKind, args: &serde_json::Value) -> Result<AiChange> {
    if env.actions != AiActionMode::AskFirst {
        return refuse(ACTIONS_OFF);
    }
    let look = Look(reader);
    match kind {
        AiToolKind::Archive | AiToolKind::Move | AiToolKind::Mark | AiToolKind::Tag => {}
        AiToolKind::AddEvent => return new_event(&look, env, args),
        AiToolKind::AddTask => return new_task(&look, env, args),
        _ => return refuse(NOT_A_CHANGE),
    }
    let ids = ids_arg(args)?;
    let folders = look.folders()?;
    let mut headers = Vec::with_capacity(ids.len());
    for id in &ids {
        headers.push(look.header(id)?.ok_or_else(|| Error::invalid(NO_MESSAGE))?);
    }
    let messages: Vec<AiMessageRef> = headers.iter().map(|h| message_ref(h, &folders)).collect();
    match kind {
        AiToolKind::Archive => {
            archive_folder(&folders)?;
            headers.iter().try_for_each(|h| movable(h, &folders))?;
            Ok(AiChange::Archive { messages })
        }
        AiToolKind::Move => {
            let target = target_folder(&folders, &id_arg(args, "folder_id")?)?;
            headers.iter().try_for_each(|h| movable(h, &folders))?;
            Ok(AiChange::Move { messages, mailbox_id: target.id.clone(), mailbox_name: target.name.clone() })
        }
        AiToolKind::Mark => match (bool_arg(args, "read")?, bool_arg(args, "starred")?) {
            (Some(read), None) => Ok(AiChange::MarkRead { messages, read }),
            (None, Some(on)) => Ok(AiChange::Star { messages, on }),
            _ => refuse(ONE_MARK),
        },
        AiToolKind::Tag => {
            let label_id = id_arg(args, "label_id")?;
            let Some(label) = look.labels()?.into_iter().find(|l| l.id == label_id) else {
                return refuse(NO_LABEL);
            };
            let Some(on) = bool_arg(args, "on")? else {
                return refuse("the argument 'on' is missing");
            };
            Ok(AiChange::Tag { messages, label_id: label.id, label_name: label.name, on })
        }
        _ => refuse(NOT_A_CHANGE),
    }
}

/// 1 to 50 distinct ids, in the order given.
fn ids_arg(args: &Value) -> Result<Vec<String>> {
    let list = match args.get("message_ids") {
        Some(Value::Array(list)) => list,
        None | Some(Value::Null) => return refuse(NO_IDS),
        Some(_) => return refuse(NOT_A_LIST),
    };
    // Not even looked at: a list this long is no honest suggestion.
    if list.len() > 4 * MESSAGES_PER_CHANGE {
        return refuse(TOO_MANY);
    }
    let (mut seen, mut ids) = (HashSet::new(), Vec::new());
    for item in list {
        let Some(text) = item.as_str() else {
            return refuse(NOT_A_LIST);
        };
        let id = check_id(text, "message_ids")?;
        if seen.insert(id.clone()) {
            ids.push(id);
        }
    }
    match ids.len() {
        0 => refuse(NO_IDS),
        n if n > MESSAGES_PER_CHANGE => refuse(TOO_MANY),
        _ => Ok(ids),
    }
}

fn archive_folder(folders: &[Mailbox]) -> Result<&Mailbox> {
    folders.iter().find(|f| role_of(f).as_deref() == Some("archive")).ok_or_else(|| Error::invalid(NO_ARCHIVE))
}

fn role_of(folder: &Mailbox) -> Option<String> {
    folder.role.as_deref().map(|r| r.trim().to_ascii_lowercase())
}

/// Why nothing goes into this folder, if nothing does.
fn kept_out(folder: &Mailbox) -> Option<&'static str> {
    match role_of(folder).as_deref() {
        Some("trash") => Some(NEVER_DELETES),
        Some("junk") => Some(NEVER_JUNK),
        Some("drafts") => Some(NEVER_DRAFTS),
        _ => None,
    }
}

/// The folder a move goes to: it is there, and it is not Trash, Junk or Drafts.
fn target_folder<'a>(folders: &'a [Mailbox], id: &str) -> Result<&'a Mailbox> {
    let target = folders.iter().find(|f| f.id == id).ok_or_else(|| Error::invalid(NO_FOLDER))?;
    match kept_out(target) {
        Some(sentence) => refuse(sentence),
        None => Ok(target),
    }
}

/// A message can be archived or moved when it is in exactly one folder (so that Undo can put
/// it back) and that folder is not Trash, Junk or Drafts (so that Undo never moves into one).
fn movable(header: &MessageHeader, folders: &[Mailbox]) -> Result<()> {
    let [only] = header.mailbox_ids.as_slice() else {
        return refuse(NOT_ONE_FOLDER);
    };
    match folders.iter().find(|f| &f.id == only) {
        None => refuse("the folder of one of these messages is not known"),
        Some(folder) if kept_out(folder).is_some() => refuse(LEFT_ALONE),
        Some(_) => Ok(()),
    }
}

/// The calendar a new entry goes to: the one named, else the default, else the first the user
/// may write to.
fn calendar_for<'a>(calendars: &'a [Calendar], id: Option<&str>) -> Result<&'a Calendar> {
    match id {
        Some(id) => match calendars.iter().find(|c| c.id == id) {
            None => refuse(NO_CALENDAR),
            Some(c) if !c.may_write => refuse(READ_ONLY),
            Some(c) => Ok(c),
        },
        None => calendars
            .iter()
            .find(|c| c.is_default && c.may_write)
            .or_else(|| calendars.iter().find(|c| c.may_write))
            .ok_or_else(|| Error::invalid(NO_WRITABLE)),
    }
}

/// `2026-11-12T10:00:00`, `2026-11-12T10:00` or the bare date; the flag says it was only a date.
fn parse_start(text: &str) -> Option<(NaiveDateTime, bool)> {
    let at = match text.len() {
        19 => (NaiveDateTime::parse_from_str(text, LOCAL_FORMAT).ok()?, false),
        16 => (NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M").ok()?, false),
        10 => (NaiveDate::parse_from_str(text, "%Y-%m-%d").ok()?.and_time(NaiveTime::MIN), true),
        _ => return None,
    };
    (1970..=2200).contains(&at.0.year()).then_some(at)
}

fn new_event(look: &Look<'_>, env: &Env<'_>, args: &Value) -> Result<AiChange> {
    zone_of(env.zone)?;
    let short = |key: &str, most: usize, long: &str| -> Result<String> {
        let text = prompts::flatten(text_arg(args, key)?.unwrap_or("")).trim().to_string();
        if text.chars().count() > most {
            return refuse(long);
        }
        Ok(text)
    };
    let title = short("title", TITLE_CHARS, "the title is too long")?;
    if title.is_empty() {
        return refuse("the argument 'title' is missing");
    }
    let location = short("location", LOCATION_CHARS, "the location is too long")?;
    let description = text_arg(args, "description")?.unwrap_or("").trim().to_string();
    if description.chars().count() > DESCRIPTION_CHARS {
        return refuse("the description is too long; write at most 4,000 characters");
    }
    let is_task = bool_arg(args, "is_task")?.unwrap_or(false);
    let Some((start, only_date)) = parse_start(text_arg(args, "start")?.unwrap_or("").trim()) else {
        return refuse(BAD_START);
    };
    let all_day = bool_arg(args, "all_day")?.unwrap_or(only_date);
    let minutes = int_arg(args, "duration_minutes")?;
    if minutes.is_some_and(|m| !(1..=MOST_MINUTES).contains(&m)) {
        return refuse("the argument 'duration_minutes' must be between 1 and 44640");
    }
    // All day: whole days from midnight. A task has no length.
    let start = if all_day { start.date().and_time(NaiveTime::MIN) } else { start };
    let duration_secs = match (is_task, all_day) {
        (true, _) => 0,
        (false, true) => minutes.map_or(1, |m| m.div_ceil(1_440)) * 86_400,
        (false, false) => minutes.unwrap_or(60) * 60,
    };
    let calendars = look.calendars()?;
    let calendar = calendar_for(&calendars, optional_id(args, "calendar_id")?.as_deref())?;
    Ok(AiChange::AddEvent {
        input: EventInput {
            calendar_id: calendar.id.clone(),
            title,
            location,
            description,
            start: start.format(LOCAL_FORMAT).to_string(),
            time_zone: Some(env.zone.to_string()),
            duration_secs,
            all_day,
            is_task,
            repeat: None,
            alert_minutes: Vec::new(),
        },
        calendar_name: calendar.name.clone(),
    })
}

/// A new task: a title, a due date or time if there is one, no repeat, nobody invited, in a
/// calendar the user may write to. The same change as an event with `is_task`, so it is applied,
/// taken back and (in a chat the user switched to automatic) done by the same path.
fn new_task(look: &Look<'_>, env: &Env<'_>, args: &Value) -> Result<AiChange> {
    zone_of(env.zone)?;
    let title = prompts::flatten(text_arg(args, "title")?.unwrap_or("")).trim().to_string();
    if title.is_empty() {
        return refuse("the argument 'title' is missing");
    }
    if title.chars().count() > TITLE_CHARS {
        return refuse("the title is too long");
    }
    let description = text_arg(args, "description")?.unwrap_or("").trim().to_string();
    if description.chars().count() > DESCRIPTION_CHARS {
        return refuse("the description is too long; write at most 4,000 characters");
    }
    // No due time at all is a task without a date.
    let (start, all_day) = match text_arg(args, "due")?.map(str::trim).filter(|t| !t.is_empty()) {
        None => (String::new(), false),
        Some(due) => match parse_start(due) {
            Some((at, only_date)) => (at.format(LOCAL_FORMAT).to_string(), only_date),
            None => {
                return refuse("the argument 'due' must be a local date or date and time such as 2026-11-12T10:00:00")
            }
        },
    };
    let calendars = look.calendars()?;
    let calendar = calendar_for(&calendars, optional_id(args, "calendar_id")?.as_deref())?;
    Ok(AiChange::AddEvent {
        input: EventInput {
            calendar_id: calendar.id.clone(),
            title,
            location: String::new(),
            description,
            start,
            time_zone: Some(env.zone.to_string()),
            duration_secs: 0,
            all_day,
            is_task: true,
            repeat: None,
            alert_minutes: Vec::new(),
        },
        calendar_name: calendar.name.clone(),
    })
}

fn messages_of(change: &AiChange) -> &[AiMessageRef] {
    match change {
        AiChange::Archive { messages }
        | AiChange::Move { messages, .. }
        | AiChange::MarkRead { messages, .. }
        | AiChange::Star { messages, .. }
        | AiChange::Tag { messages, .. } => messages,
        AiChange::AddEvent { .. } => &[],
    }
}

/// Confirm above 10 messages, else Change.
pub fn risk(change: &AiChange) -> AiRisk {
    if messages_of(change).len() > CONFIRM_ABOVE {
        AiRisk::Confirm
    } else {
        AiRisk::Change
    }
}

/// Whether a change may be applied without a tap when the user switched automatic changes on for
/// the chat. An explicit allow-list: a kind that is not named here asks, in every mode. Only what
/// stays inside the mailbox, is not mailed to anyone and is taken back by [`undo`] is named, and
/// not a change of more than [`CONFIRM_ABOVE`] messages (its card asks first, as always).
///
/// A new calendar entry is named too (owner), a new task included (`add_task` makes the same
/// `AddEvent` with `is_task`): it is made in the user's own calendar, has no
/// participants (so saving mails no invitation: `EventInput` has no way to name any), does not
/// repeat, and its Undo deletes it again. Changing or deleting an existing entry, an entry with
/// guests and anything in a shared calendar are not changes of the chat at all, so they cannot be
/// automatic; a new kind asks until it is named here.
pub fn automatic(change: &AiChange, risk: AiRisk) -> bool {
    risk == AiRisk::Change
        && matches!(
            change,
            AiChange::MarkRead { .. }
                | AiChange::Star { .. }
                | AiChange::Tag { .. }
                | AiChange::Archive { .. }
                | AiChange::Move { .. }
                | AiChange::AddEvent { .. }
        )
}

/// The one mail action a change means for one message; None for a tag and a calendar entry.
fn action_for(change: &AiChange, id: &str) -> Option<Action> {
    let message_id = id.to_string();
    Some(match change {
        AiChange::Archive { .. } => Action::Archive { message_id },
        AiChange::Move { mailbox_id, .. } => Action::Move { message_id, mailbox_id: mailbox_id.clone() },
        AiChange::MarkRead { read: true, .. } => Action::MarkRead { message_id },
        AiChange::MarkRead { read: false, .. } => Action::MarkUnread { message_id },
        AiChange::Star { on: true, .. } => Action::Flag { message_id },
        AiChange::Star { on: false, .. } => Action::Unflag { message_id },
        AiChange::Tag { .. } | AiChange::AddEvent { .. } => return None,
    })
}

/// The mail actions a change stands for: only MarkRead, MarkUnread, Archive, Flag, Unflag, Move.
pub fn actions_of(change: &AiChange) -> Vec<Action> {
    messages_of(change).iter().filter_map(|m| action_for(change, &m.id)).collect()
}

/// What [`apply`] did.
pub struct Applied {
    pub result: Result<AiApplied>,
    /// What to take back, also after a failed mail change.
    pub undo: Option<Undo>,
}

/// What applying a change comes to, worked out from the state as it is now.
enum Plan {
    /// Everything is already as the change wants it.
    Nothing,
    Mail {
        actions: Vec<Action>,
        back: Vec<Action>,
    },
    Label {
        ids: Vec<String>,
        label_id: String,
        on: bool,
    },
    Event(EventInput),
}

/// Reads the state now, validates again, acts once.
pub fn apply(reader: &dyn Reader, writer: &dyn Writer, change: &AiChange) -> Applied {
    let done = |event_id: Option<String>, undo: Option<Undo>| Applied {
        result: Ok(AiApplied { can_undo: undo.is_some(), event_id }),
        undo,
    };
    // The inverse is complete before anything is done.
    match plan(&Look(reader), change) {
        Err(e) => Applied { result: Err(e), undo: None },
        Ok(Plan::Nothing) => done(None, None),
        Ok(Plan::Mail { actions, back }) => {
            let undo = Some(Undo::Actions(back));
            match writer.submit_all(&actions) {
                Ok(()) => done(None, undo),
                // The core stops at the first error; what came before it stays done.
                Err(e) => Applied { result: Err(e), undo },
            }
        }
        Ok(Plan::Label { ids, label_id, on }) => {
            let result = writer.set_label(&ids, &label_id, on);
            let undo = Some(Undo::Label { ids, label_id, on: !on });
            match result {
                Ok(()) => done(None, undo),
                Err(e) => Applied { result: Err(e), undo },
            }
        }
        Ok(Plan::Event(input)) => match writer.save_event(&input) {
            Ok(id) => done(Some(id.clone()), Some(Undo::Event { id })),
            Err(e) => Applied { result: Err(e), undo: None },
        },
    }
}

fn plan(look: &Look<'_>, change: &AiChange) -> Result<Plan> {
    if let AiChange::AddEvent { input, .. } = change {
        // The card promised an entry that does not repeat, in a calendar that is still there.
        if input.repeat.is_some() {
            return refuse(NOT_A_CHANGE);
        }
        calendar_for(&look.calendars()?, Some(&input.calendar_id))?;
        return Ok(Plan::Event(input.clone()));
    }
    let named = messages_of(change);
    if named.is_empty() {
        return refuse(NO_IDS);
    }
    if named.len() > MESSAGES_PER_CHANGE {
        return refuse(TOO_MANY);
    }
    let mut now = Vec::with_capacity(named.len());
    for message in named {
        now.push(look.header(&message.id)?.ok_or_else(|| Error::invalid(GONE))?);
    }
    let changing: Vec<&MessageHeader> = match change {
        AiChange::Archive { .. } | AiChange::Move { .. } => {
            let folders = look.folders()?;
            let target = match change {
                AiChange::Move { mailbox_id, .. } => target_folder(&folders, mailbox_id)?,
                _ => archive_folder(&folders)?,
            };
            let elsewhere: Vec<&MessageHeader> =
                now.iter().filter(|h| h.mailbox_ids.as_slice() != std::slice::from_ref(&target.id)).collect();
            elsewhere.iter().try_for_each(|h| movable(h, &folders))?;
            elsewhere
        }
        AiChange::MarkRead { read, .. } => now.iter().filter(|h| h.is_unread == *read).collect(),
        AiChange::Star { on, .. } => now.iter().filter(|h| h.is_flagged != *on).collect(),
        AiChange::Tag { label_id, on, .. } => {
            if !look.labels()?.iter().any(|l| &l.id == label_id) {
                return refuse(NO_LABEL);
            }
            let ids: Vec<String> = now.iter().map(|h| h.id.clone()).collect();
            let have = look.labels_of(&ids)?;
            let tagged = |id: &String| have.get(id).is_some_and(|keywords| keywords.contains(label_id));
            let ids: Vec<String> = ids.into_iter().filter(|id| tagged(id) != *on).collect();
            return Ok(if ids.is_empty() {
                Plan::Nothing
            } else {
                Plan::Label { ids, label_id: label_id.clone(), on: *on }
            });
        }
        AiChange::AddEvent { .. } => Vec::new(),
    };
    let (mut actions, mut back) = (Vec::new(), Vec::new());
    for header in changing {
        let Some(action) = action_for(change, &header.id) else { continue };
        let message_id = header.id.clone();
        back.push(match &action {
            Action::MarkRead { .. } => Action::MarkUnread { message_id },
            Action::MarkUnread { .. } => Action::MarkRead { message_id },
            Action::Flag { .. } => Action::Unflag { message_id },
            Action::Unflag { .. } => Action::Flag { message_id },
            // `movable` saw that it is in exactly one folder.
            Action::Archive { .. } | Action::Move { .. } => match header.mailbox_ids.first() {
                Some(folder) => Action::Move { message_id, mailbox_id: folder.clone() },
                None => return refuse(NOT_ONE_FOLDER),
            },
            Action::Delete { .. } | Action::Destroy { .. } => return refuse(NEVER_DELETES),
        });
        actions.push(action);
    }
    Ok(if actions.is_empty() { Plan::Nothing } else { Plan::Mail { actions, back } })
}

/// Takes an applied change back, once.
pub fn undo(writer: &dyn Writer, undo: &Undo) -> Result<()> {
    match undo {
        Undo::Actions(actions) => {
            if actions.iter().any(|a| matches!(a, Action::Delete { .. } | Action::Destroy { .. })) {
                return refuse(NEVER_DELETES);
            }
            writer.submit_all(actions)
        }
        Undo::Label { ids, label_id, on } => writer.set_label(ids, label_id, *on),
        Undo::Event { id } => writer.delete_event(id),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        super::tools::fake::{calendar, message, Call, Down, Fake},
        *,
    };

    const ZONE: &str = "Europe/Istanbul";
    const ENV: Env<'static> = Env { zone: ZONE, now: 1_794_000_000, actions: AiActionMode::AskFirst };

    fn suggest(fake: &Fake, kind: AiToolKind, args: Value) -> AiChange {
        propose(fake, &ENV, kind, &args).expect("a suggestion")
    }

    fn refused(fake: &Fake, kind: AiToolKind, args: Value) -> String {
        match propose(fake, &ENV, kind, &args) {
            Err(Error::Invalid { msg }) => msg,
            Err(e) => panic!("another error: {e}"),
            Ok(change) => panic!("not refused: {change:?}"),
        }
    }

    fn ids(n: usize) -> Vec<String> {
        (1..=n).map(|i| format!("m{i}")).collect()
    }

    fn named(change: &AiChange) -> Vec<&str> {
        messages_of(change).iter().map(|m| m.id.as_str()).collect()
    }

    fn submitted(fake: &Fake) -> Vec<Call> {
        fake.calls.borrow().clone()
    }

    fn invalid(applied: &Applied) -> &str {
        match &applied.result {
            Err(Error::Invalid { msg }) => msg,
            _ => panic!("not refused"),
        }
    }

    #[test]
    fn a_suggestion_names_every_message_from_the_cache_and_writes_nothing() {
        let fake = Fake::with_mail(3);
        fake.edit("m2", |m| m.mailbox_ids = vec!["work".into()]);
        let change = suggest(&fake, AiToolKind::Archive, json!({ "message_ids": ["m1", "m2", "m1"] }));
        let AiChange::Archive { messages } = &change else { panic!("{change:?}") };
        assert_eq!(
            messages[..],
            [
                AiMessageRef {
                    id: "m1".into(),
                    thread_id: "t-m1".into(),
                    subject: "Subject m1".into(),
                    from: "Ann <ann@example.test>".into(),
                    mailbox_name: "Inbox".into(),
                },
                AiMessageRef {
                    id: "m2".into(),
                    thread_id: "t-m2".into(),
                    subject: "Subject m2".into(),
                    from: "Ann <ann@example.test>".into(),
                    mailbox_name: "Work".into(),
                },
            ]
        );
        let moved = suggest(&fake, AiToolKind::Move, json!({ "message_ids": ["m3"], "folder_id": "work" }));
        assert!(
            matches!(&moved, AiChange::Move { mailbox_id, mailbox_name, .. } if mailbox_id == "work" && mailbox_name == "Work")
        );
        let read = suggest(&fake, AiToolKind::Mark, json!({ "message_ids": ["m1"], "read": false }));
        assert!(matches!(read, AiChange::MarkRead { read: false, .. }));
        let star = suggest(&fake, AiToolKind::Mark, json!({ "message_ids": ["m1"], "starred": true }));
        assert!(matches!(star, AiChange::Star { on: true, .. }));
        let tag = suggest(&fake, AiToolKind::Tag, json!({ "message_ids": ["m1"], "label_id": "todo", "on": true }));
        assert!(
            matches!(&tag, AiChange::Tag { label_id, label_name, on: true, .. } if label_id == "todo" && label_name == "To do")
        );
        assert!(submitted(&fake).is_empty());
        assert_eq!(fake.get("m1"), message("m1", "in"), "the cache is as it was");
    }

    #[test]
    fn what_must_not_be_suggested_is_refused_with_a_sentence() {
        let fake = Fake::with_mail(4);
        fake.edit("m2", |m| m.mailbox_ids = vec!["in".into(), "work".into()]);
        fake.edit("m3", |m| m.mailbox_ids = vec!["bin".into()]);
        fake.edit("m4", |m| m.mailbox_ids = vec!["junk".into()]);
        let to = |folder: &str| json!({ "message_ids": ["m1"], "folder_id": folder });
        let cases = [
            // Never into Trash, Junk or Drafts.
            (AiToolKind::Move, to("bin"), NEVER_DELETES),
            (AiToolKind::Move, to("junk"), NEVER_JUNK),
            (AiToolKind::Move, to("dr"), NEVER_DRAFTS),
            (AiToolKind::Move, to("nope"), NO_FOLDER),
            (AiToolKind::Move, json!({ "message_ids": ["m1"] }), "the argument 'folder_id' is missing"),
            // Not out of more than one folder, and not out of Trash or Junk.
            (AiToolKind::Move, json!({ "message_ids": ["m1", "m2"], "folder_id": "work" }), NOT_ONE_FOLDER),
            (AiToolKind::Archive, json!({ "message_ids": ["m2"] }), NOT_ONE_FOLDER),
            (AiToolKind::Archive, json!({ "message_ids": ["m3"] }), LEFT_ALONE),
            (AiToolKind::Move, json!({ "message_ids": ["m4"], "folder_id": "in" }), LEFT_ALONE),
            // Only messages of the cache, and at least one.
            (AiToolKind::Archive, json!({ "message_ids": ["m1", "gone"] }), NO_MESSAGE),
            (AiToolKind::Archive, json!({ "message_ids": [] }), NO_IDS),
            (AiToolKind::Archive, json!({}), NO_IDS),
            (AiToolKind::Archive, json!({ "message_ids": "m1" }), NOT_A_LIST),
            (AiToolKind::Archive, json!({ "message_ids": ["m1", 2] }), NOT_A_LIST),
            // Exactly one mark.
            (AiToolKind::Mark, json!({ "message_ids": ["m1"] }), ONE_MARK),
            (AiToolKind::Mark, json!({ "message_ids": ["m1"], "read": true, "starred": true }), ONE_MARK),
            (
                AiToolKind::Mark,
                json!({ "message_ids": ["m1"], "read": "yes" }),
                "the argument 'read' must be true or false",
            ),
            // Only labels the user has; none is created.
            (AiToolKind::Tag, json!({ "message_ids": ["m1"], "label_id": "new", "on": true }), NO_LABEL),
            (AiToolKind::Tag, json!({ "message_ids": ["m1"], "label_id": "todo" }), "the argument 'on' is missing"),
            // A tool that is no change tool builds no card.
            (AiToolKind::ReadMessage, json!({ "message_ids": ["m1"] }), NOT_A_CHANGE),
            (AiToolKind::DraftReply, json!({ "message_id": "m1", "text": "x" }), NOT_A_CHANGE),
        ];
        for (kind, args, sentence) in cases {
            assert_eq!(refused(&fake, kind, args.clone()), sentence, "{kind:?} {args}");
        }
        // Marks and tags do not move anything, so they may name mail anywhere.
        suggest(&fake, AiToolKind::Mark, json!({ "message_ids": ["m2", "m3"], "read": true }));
        // Roles are compared whatever their case.
        fake.folders.borrow_mut()[2].role = Some("Trash".into());
        assert_eq!(refused(&fake, AiToolKind::Move, to("bin")), NEVER_DELETES);
        // No Archive folder, no archiving.
        fake.folders.borrow_mut().retain(|f| f.id != "arch");
        assert_eq!(refused(&fake, AiToolKind::Archive, json!({ "message_ids": ["m1"] })), NO_ARCHIVE);
        // With actions off nothing is suggested at all.
        let off = Env { zone: ZONE, now: 0, actions: AiActionMode::Off };
        let said = propose(&fake, &off, AiToolKind::Mark, &json!({ "message_ids": ["m1"], "read": true }));
        assert!(matches!(said, Err(Error::Invalid { msg }) if msg == ACTIONS_OFF));
        assert!(submitted(&fake).is_empty());
    }

    #[test]
    fn what_the_reader_says_never_becomes_the_sentence() {
        let fake = Fake::with_mail(1);
        fake.down.set(Down::Invalid);
        let said = refused(&fake, AiToolKind::Archive, json!({ "message_ids": ["m1"] }));
        assert!(!said.contains("server said"), "{said}");
        fake.down.set(Down::SignIn);
        let stop = propose(&fake, &ENV, AiToolKind::Archive, &json!({ "message_ids": ["m1"] }));
        assert!(matches!(stop, Err(Error::SignInNeeded { .. })));
    }

    #[test]
    fn eleven_messages_need_confirming_and_fifty_one_are_refused() {
        let fake = Fake::with_mail(60);
        let mark = |n: usize| json!({ "message_ids": ids(n), "read": true });
        assert_eq!(risk(&suggest(&fake, AiToolKind::Mark, mark(1))), AiRisk::Change);
        assert_eq!(risk(&suggest(&fake, AiToolKind::Mark, mark(10))), AiRisk::Change);
        assert_eq!(risk(&suggest(&fake, AiToolKind::Mark, mark(11))), AiRisk::Confirm);
        let fifty = suggest(&fake, AiToolKind::Archive, json!({ "message_ids": ids(50) }));
        assert_eq!((named(&fifty).len(), risk(&fifty)), (50, AiRisk::Confirm));
        assert_eq!(refused(&fake, AiToolKind::Archive, json!({ "message_ids": ids(51) })), TOO_MANY);
        assert_eq!(refused(&fake, AiToolKind::Mark, mark(51)), TOO_MANY);
        // The same id over and over is one message; a flood of them is not counted at all.
        let same = suggest(&fake, AiToolKind::Mark, json!({ "message_ids": vec!["m1"; 60], "read": true }));
        assert_eq!(named(&same), ["m1"]);
        assert_eq!(refused(&fake, AiToolKind::Mark, json!({ "message_ids": vec!["m1"; 201], "read": true })), TOO_MANY);
        // A record with too many messages is not applied either.
        let Applied { result, undo } = apply(
            &fake,
            &fake,
            &AiChange::MarkRead {
                messages: fake.mail.borrow().iter().take(51).map(|h| message_ref(h, &[])).collect(),
                read: true,
            },
        );
        assert!(matches!(result, Err(Error::Invalid { msg }) if msg == TOO_MANY) && undo.is_none());
        assert!(submitted(&fake).is_empty());
        let event = suggest(&fake, AiToolKind::AddEvent, json!({ "title": "Lunch", "start": "2026-11-20T12:00:00" }));
        assert_eq!(risk(&event), AiRisk::Change);
    }

    fn refs(fake: &Fake, ids: &[&str]) -> Vec<AiMessageRef> {
        ids.iter().map(|id| message_ref(&fake.get(id), &fake.folders.borrow())).collect()
    }

    fn every_change(fake: &Fake) -> Vec<AiChange> {
        let messages = refs(fake, &["m1", "m2"]);
        vec![
            AiChange::Archive { messages: messages.clone() },
            AiChange::Move { messages: messages.clone(), mailbox_id: "work".into(), mailbox_name: "Work".into() },
            AiChange::Move { messages: messages.clone(), mailbox_id: "bin".into(), mailbox_name: "Trash".into() },
            AiChange::MarkRead { messages: messages.clone(), read: true },
            AiChange::MarkRead { messages: messages.clone(), read: false },
            AiChange::Star { messages: messages.clone(), on: true },
            AiChange::Star { messages: messages.clone(), on: false },
            AiChange::Tag { messages: messages.clone(), label_id: "todo".into(), label_name: "To do".into(), on: true },
            AiChange::Tag { messages, label_id: "todo".into(), label_name: "To do".into(), on: false },
            suggest(fake, AiToolKind::AddEvent, json!({ "title": "Lunch", "start": "2026-11-20T12:00:00" })),
        ]
    }

    #[test]
    fn no_change_stands_for_a_delete() {
        let fake = Fake::with_mail(2);
        let m = |id: &str| id.to_string();
        let changes = every_change(&fake);
        for change in &changes {
            for action in actions_of(change) {
                assert!(!matches!(action, Action::Delete { .. } | Action::Destroy { .. }), "{change:?}");
            }
        }
        assert_eq!(
            actions_of(&changes[0]),
            [Action::Archive { message_id: m("m1") }, Action::Archive { message_id: m("m2") }]
        );
        assert_eq!(actions_of(&changes[1])[1], Action::Move { message_id: m("m2"), mailbox_id: m("work") });
        assert_eq!(actions_of(&changes[3])[0], Action::MarkRead { message_id: m("m1") });
        assert_eq!(actions_of(&changes[4])[0], Action::MarkUnread { message_id: m("m1") });
        assert_eq!(actions_of(&changes[5])[0], Action::Flag { message_id: m("m1") });
        assert_eq!(actions_of(&changes[6])[0], Action::Unflag { message_id: m("m1") });
        assert!(actions_of(&changes[7]).is_empty() && actions_of(&changes[9]).is_empty());
        // Whatever is applied, and whatever is taken back, the writer never hears a delete; a
        // record that names Trash is refused when it is applied, too.
        for change in &changes {
            let fake = Fake::with_mail(2);
            let applied = apply(&fake, &fake, change);
            if let Some(record) = &applied.undo {
                undo(&fake, record).unwrap();
            }
            for call in submitted(&fake) {
                let Call::Submit(actions) = call else { continue };
                assert!(actions.iter().all(|a| !matches!(a, Action::Delete { .. } | Action::Destroy { .. })));
                let into_trash = |a: &Action| matches!(a, Action::Move { mailbox_id, .. } if mailbox_id == "bin");
                assert!(!actions.iter().any(into_trash), "{change:?}");
            }
        }
        let fake = Fake::with_mail(2);
        assert_eq!(invalid(&apply(&fake, &fake, &changes[2])), NEVER_DELETES);
        assert!(submitted(&fake).is_empty());
        // An undo record cannot be made to delete.
        let forged = Undo::Actions(vec![Action::Delete { message_id: m("m1") }]);
        assert!(undo(&fake, &forged).is_err() && submitted(&fake).is_empty());
    }

    #[test]
    fn applying_skips_what_is_already_so_and_undo_puts_it_back() {
        let m = |id: &str| id.to_string();
        // Read: m2 is read already.
        let fake = Fake::with_mail(2);
        fake.edit("m2", |h| h.is_unread = false);
        let before = fake.mail.borrow().clone();
        let change = AiChange::MarkRead { messages: refs(&fake, &["m1", "m2"]), read: true };
        let Applied { result, undo: record } = apply(&fake, &fake, &change);
        assert_eq!(result.unwrap(), AiApplied { can_undo: true, event_id: None });
        assert_eq!(submitted(&fake), [Call::Submit(vec![Action::MarkRead { message_id: m("m1") }])]);
        assert_eq!(record, Some(Undo::Actions(vec![Action::MarkUnread { message_id: m("m1") }])));
        assert!(!fake.get("m1").is_unread);
        undo(&fake, &record.unwrap()).unwrap();
        assert_eq!(*fake.mail.borrow(), before);

        // Star off: only m1 has one.
        let fake = Fake::with_mail(2);
        fake.edit("m1", |h| h.is_flagged = true);
        let before = fake.mail.borrow().clone();
        let change = AiChange::Star { messages: refs(&fake, &["m1", "m2"]), on: false };
        let record = apply(&fake, &fake, &change).undo.unwrap();
        assert_eq!(record, Undo::Actions(vec![Action::Flag { message_id: m("m1") }]));
        assert!(!fake.get("m1").is_flagged);
        undo(&fake, &record).unwrap();
        assert_eq!(*fake.mail.borrow(), before);

        // Archive: m2 is there already, m3 comes from Work and goes back to Work.
        let fake = Fake::with_mail(3);
        fake.edit("m2", |h| h.mailbox_ids = vec![m("arch")]);
        fake.edit("m3", |h| h.mailbox_ids = vec![m("work")]);
        let before = fake.mail.borrow().clone();
        let change = AiChange::Archive { messages: refs(&fake, &["m1", "m2", "m3"]) };
        let record = apply(&fake, &fake, &change).undo.unwrap();
        assert_eq!(
            submitted(&fake),
            [Call::Submit(vec![Action::Archive { message_id: m("m1") }, Action::Archive { message_id: m("m3") }])]
        );
        assert_eq!(
            record,
            Undo::Actions(vec![
                Action::Move { message_id: m("m1"), mailbox_id: m("in") },
                Action::Move { message_id: m("m3"), mailbox_id: m("work") },
            ])
        );
        assert_eq!(fake.get("m3").mailbox_ids, ["arch"]);
        undo(&fake, &record).unwrap();
        assert_eq!(*fake.mail.borrow(), before);

        // Move: the state at the tap counts, not the state when it was suggested.
        let fake = Fake::with_mail(2);
        let change = suggest(&fake, AiToolKind::Move, json!({ "message_ids": ["m1", "m2"], "folder_id": "work" }));
        fake.edit("m1", |h| h.mailbox_ids = vec![m("arch")]);
        fake.edit("m2", |h| h.mailbox_ids = vec![m("work")]);
        let record = apply(&fake, &fake, &change).undo.unwrap();
        assert_eq!(record, Undo::Actions(vec![Action::Move { message_id: m("m1"), mailbox_id: m("arch") }]));
        undo(&fake, &record).unwrap();
        assert_eq!((fake.get("m1").mailbox_ids, fake.get("m2").mailbox_ids), (vec![m("arch")], vec![m("work")]));

        // Tag: m1 has the label already; only m2 gets it, and only m2 loses it again.
        let fake = Fake::with_mail(2);
        fake.keywords.borrow_mut().insert(m("m1"), vec![m("$seen"), m("todo")]);
        let tag = |on| AiChange::Tag {
            messages: refs(&fake, &["m1", "m2"]),
            label_id: m("todo"),
            label_name: m("To do"),
            on,
        };
        let Applied { result, undo: record } = apply(&fake, &fake, &tag(true));
        assert!(result.unwrap().can_undo);
        assert_eq!(submitted(&fake), [Call::Label(vec![m("m2")], m("todo"), true)]);
        assert_eq!(record, Some(Undo::Label { ids: vec![m("m2")], label_id: m("todo"), on: false }));
        undo(&fake, &record.unwrap()).unwrap();
        assert_eq!(fake.keywords.borrow()["m1"], [m("$seen"), m("todo")]);
        assert!(fake.keywords.borrow()["m2"].is_empty());
        let off = apply(&fake, &fake, &tag(false)).undo.unwrap();
        assert_eq!(off, Undo::Label { ids: vec![m("m1")], label_id: m("todo"), on: true });
    }

    #[test]
    fn a_change_that_changes_nothing_has_no_undo_and_reaches_no_writer() {
        let fake = Fake::with_mail(2);
        fake.edit("m1", |h| (h.is_unread, h.mailbox_ids) = (false, vec!["arch".into()]));
        fake.edit("m2", |h| (h.is_unread, h.mailbox_ids) = (false, vec!["arch".into()]));
        let messages = refs(&fake, &["m1", "m2"]);
        let idle = [
            AiChange::MarkRead { messages: messages.clone(), read: true },
            AiChange::Star { messages: messages.clone(), on: false },
            AiChange::Archive { messages: messages.clone() },
            AiChange::Move { messages: messages.clone(), mailbox_id: "arch".into(), mailbox_name: "Archive".into() },
            AiChange::Tag { messages, label_id: "todo".into(), label_name: "To do".into(), on: false },
        ];
        for change in idle {
            let Applied { result, undo } = apply(&fake, &fake, &change);
            assert_eq!(result.unwrap(), AiApplied { can_undo: false, event_id: None }, "{change:?}");
            assert_eq!(undo, None);
        }
        assert!(submitted(&fake).is_empty());
    }

    #[test]
    fn the_state_is_checked_again_at_the_tap() {
        type Break = fn(&Fake);
        type Build<'a> = &'a dyn Fn(&Fake) -> AiChange;
        let archive = |f: &Fake| AiChange::Archive { messages: refs(f, &["m1", "m2"]) };
        let to_work =
            |f: &Fake| suggest(f, AiToolKind::Move, json!({ "message_ids": ["m1", "m2"], "folder_id": "work" }));
        let tag =
            |f: &Fake| suggest(f, AiToolKind::Tag, json!({ "message_ids": ["m1"], "label_id": "todo", "on": true }));
        let mark = |f: &Fake| suggest(f, AiToolKind::Mark, json!({ "message_ids": ["m1", "m2"], "read": true }));
        let event =
            |f: &Fake| suggest(f, AiToolKind::AddEvent, json!({ "title": "Lunch", "start": "2026-11-20T12:00:00" }));
        let cases: [(Build, Break, &str); 10] = [
            (&mark, |f| f.mail.borrow_mut().retain(|h| h.id != "m2"), GONE),
            (&archive, |f| f.mail.borrow_mut().retain(|h| h.id != "m2"), GONE),
            (&archive, |f| f.folders.borrow_mut().retain(|x| x.id != "arch"), NO_ARCHIVE),
            (&archive, |f| f.edit("m2", |h| h.mailbox_ids.push("work".into())), NOT_ONE_FOLDER),
            (&archive, |f| f.edit("m2", |h| h.mailbox_ids = vec!["bin".into()]), LEFT_ALONE),
            (&to_work, |f| f.folders.borrow_mut().retain(|x| x.id != "work"), NO_FOLDER),
            (&to_work, |f| f.folders.borrow_mut()[5].role = Some("junk".into()), NEVER_JUNK),
            (&tag, |f| f.labels.borrow_mut().clear(), NO_LABEL),
            (&event, |f| f.calendars.borrow_mut()[1].may_write = false, READ_ONLY),
            (&event, |f| drop(f.calendars.borrow_mut().remove(1)), NO_CALENDAR),
        ];
        for (build, spoil, sentence) in cases {
            let fake = Fake::with_mail(2);
            let change = build(&fake);
            spoil(&fake);
            let before = fake.mail.borrow().clone();
            let applied = apply(&fake, &fake, &change);
            assert_eq!(invalid(&applied), sentence, "{change:?}");
            assert_eq!(applied.undo, None);
            // Not even the messages that were fine were touched.
            assert!(submitted(&fake).is_empty(), "{change:?}");
            assert_eq!(*fake.mail.borrow(), before);
        }
        // The reader failing is no change either, and its words are not passed on.
        let fake = Fake::with_mail(2);
        let change = mark(&fake);
        fake.down.set(Down::Offline);
        let applied = apply(&fake, &fake, &change);
        assert!(matches!(&applied.result, Err(Error::Temporary { msg }) if !msg.contains("secret")));
        assert!(applied.undo.is_none() && submitted(&fake).is_empty());
    }

    #[test]
    fn a_failed_write_gives_the_error_and_what_to_take_back() {
        let m = |id: &str| id.to_string();
        let fake = Fake::with_mail(2);
        let before = fake.mail.borrow().clone();
        fake.fail_write.set(true);
        let change = AiChange::Archive { messages: refs(&fake, &["m1", "m2"]) };
        let Applied { result, undo: record } = apply(&fake, &fake, &change);
        assert!(matches!(result, Err(Error::Temporary { .. })));
        // The first went through before the error: the inverse covers everything that was tried.
        assert_eq!((fake.get("m1").mailbox_ids, fake.get("m2").mailbox_ids), (vec![m("arch")], vec![m("in")]));
        let record = record.expect("an inverse");
        assert_eq!(
            record,
            Undo::Actions(vec![
                Action::Move { message_id: m("m1"), mailbox_id: m("in") },
                Action::Move { message_id: m("m2"), mailbox_id: m("in") },
            ])
        );
        fake.fail_write.set(false);
        undo(&fake, &record).unwrap();
        assert_eq!(*fake.mail.borrow(), before);

        // A tag that fails keeps its inverse too.
        let fake = Fake::with_mail(1);
        fake.fail_write.set(true);
        let change = suggest(&fake, AiToolKind::Tag, json!({ "message_ids": ["m1"], "label_id": "todo", "on": true }));
        let Applied { result, undo: record } = apply(&fake, &fake, &change);
        assert!(result.is_err());
        assert_eq!(record, Some(Undo::Label { ids: vec![m("m1")], label_id: m("todo"), on: false }));

        // An entry that was not saved leaves nothing to take back.
        let change = suggest(&fake, AiToolKind::AddEvent, json!({ "title": "Lunch", "start": "2026-11-20T12:00:00" }));
        let Applied { result, undo: record } = apply(&fake, &fake, &change);
        assert!(matches!(result, Err(Error::Temporary { .. })) && record.is_none());
    }

    fn event(fake: &Fake, args: Value) -> (EventInput, String) {
        match suggest(fake, AiToolKind::AddEvent, args) {
            AiChange::AddEvent { input, calendar_name } => (input, calendar_name),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_new_entry_has_the_shape_the_calendar_takes() {
        let fake = Fake::new();
        let (input, calendar_name) =
            event(&fake, json!({ "title": " Lunch\nwith Ann ", "start": "2026-11-20T12:30:00" }));
        assert_eq!(
            input,
            EventInput {
                calendar_id: "home".into(),
                title: "Lunch with Ann".into(),
                location: String::new(),
                description: String::new(),
                start: "2026-11-20T12:30:00".into(),
                time_zone: Some(ZONE.into()),
                duration_secs: 3_600,
                all_day: false,
                is_task: false,
                repeat: None,
                alert_minutes: vec![],
            }
        );
        assert_eq!(calendar_name, "Home");

        let (input, calendar_name) = event(
            &fake,
            json!({
                "title": "Review", "start": "2026-11-20T09:00", "duration_minutes": 45, "location": "Room 2",
                "description": "Line one\nline two", "calendar_id": "team",
            }),
        );
        assert_eq!((input.start.as_str(), input.duration_secs), ("2026-11-20T09:00:00", 2_700));
        assert_eq!((input.location.as_str(), input.description.as_str()), ("Room 2", "Line one\nline two"));
        assert_eq!((input.calendar_id.as_str(), calendar_name.as_str()), ("team", "Team"));

        // All day: whole days from midnight, whatever time was given.
        let all_day = |args: Value| {
            let (input, _) = event(&fake, args);
            assert!(input.all_day && !input.is_task);
            (input.start, input.duration_secs)
        };
        let midnight = "2026-11-20T00:00:00".to_string();
        assert_eq!(
            all_day(json!({ "title": "Trip", "start": "2026-11-20T15:30:00", "all_day": true })),
            (midnight.clone(), 86_400)
        );
        assert_eq!(all_day(json!({ "title": "Trip", "start": "2026-11-20" })), (midnight.clone(), 86_400));
        assert_eq!(
            all_day(json!({ "title": "Trip", "start": "2026-11-20", "all_day": true, "duration_minutes": 2_000 })),
            (midnight.clone(), 2 * 86_400)
        );
        assert_eq!(
            all_day(json!({ "title": "Trip", "start": "2026-11-20", "duration_minutes": 2_880 })),
            (midnight, 2 * 86_400)
        );

        // A task is due at its start and has no length.
        let (task, _) = event(
            &fake,
            json!({ "title": "Pay rent", "start": "2026-11-20T17:00:00", "is_task": true, "duration_minutes": 30 }),
        );
        assert_eq!((task.is_task, task.all_day, task.duration_secs), (true, false, 0));
        assert_eq!(task.start, "2026-11-20T17:00:00");
        let (day_task, _) = event(&fake, json!({ "title": "Pay rent", "start": "2026-11-20", "is_task": true }));
        assert_eq!(
            (day_task.all_day, day_task.duration_secs, day_task.start.as_str()),
            (true, 0, "2026-11-20T00:00:00")
        );
        // Never a repeat, never a reminder, always the phone's zone.
        for input in [&task, &day_task] {
            assert_eq!((&input.repeat, &input.alert_minutes, input.time_zone.as_deref()), (&None, &vec![], Some(ZONE)));
        }

        // Without a default the first calendar the user may write to.
        fake.calendars.borrow_mut()[1].is_default = false;
        fake.calendars.borrow_mut()[0].is_default = true;
        assert_eq!(event(&fake, json!({ "title": "x", "start": "2026-11-20" })).1, "Home");
        fake.calendars.borrow_mut().remove(1);
        assert_eq!(event(&fake, json!({ "title": "x", "start": "2026-11-20" })).1, "Team");
    }

    #[test]
    fn a_new_entry_that_cannot_be_is_refused() {
        let fake = Fake::new();
        let with = |key: &str, value: Value| {
            let mut args = json!({ "title": "Lunch", "start": "2026-11-20T12:00:00" });
            args[key] = value;
            refused(&fake, AiToolKind::AddEvent, args)
        };
        assert_eq!(with("title", json!("  ")), "the argument 'title' is missing");
        assert_eq!(with("title", json!("x".repeat(301))), "the title is too long");
        assert_eq!(with("title", json!(5)), "the argument 'title' must be text");
        for bad in [
            "tomorrow",
            "",
            "2026-11-20T10:00:00Z",
            "2026-11-20T10:00:00+03:00",
            "2026-13-01",
            "2026-11-20 10:00",
            "0026-11-20",
        ] {
            assert_eq!(with("start", json!(bad)), BAD_START, "{bad}");
        }
        assert!(with("duration_minutes", json!(0)).contains("between 1 and 44640"));
        assert!(with("duration_minutes", json!(44_641)).contains("between 1 and 44640"));
        assert_eq!(with("duration_minutes", json!(-5)), "the argument 'duration_minutes' must be a whole number");
        assert_eq!(with("all_day", json!("maybe")), "the argument 'all_day' must be true or false");
        assert!(with("description", json!("x".repeat(4_001))).starts_with("the description is too long"));
        assert_eq!(with("location", json!("x".repeat(501))), "the location is too long");
        assert_eq!(with("calendar_id", json!("hol")), READ_ONLY);
        assert_eq!(with("calendar_id", json!("nope")), NO_CALENDAR);
        *fake.calendars.borrow_mut() = vec![calendar("hol", "Holidays", true, false)];
        assert_eq!(with("location", json!("here")), NO_WRITABLE);
        let mars = Env { zone: "Mars/Base", now: 0, actions: AiActionMode::AskFirst };
        let said = propose(&Fake::new(), &mars, AiToolKind::AddEvent, &json!({ "title": "x", "start": "2026-11-20" }));
        assert!(matches!(said, Err(Error::Invalid { msg }) if msg == "the time zone of the phone is not known"));
    }

    #[test]
    fn a_new_entry_is_saved_as_shown_and_undo_removes_it() {
        let fake = Fake::new();
        let change = suggest(&fake, AiToolKind::AddEvent, json!({ "title": "Lunch", "start": "2026-11-20T12:00:00" }));
        let AiChange::AddEvent { input, .. } = &change else { panic!() };
        let Applied { result, undo: record } = apply(&fake, &fake, &change);
        assert_eq!(result.unwrap(), AiApplied { can_undo: true, event_id: Some("ev-new".into()) });
        assert_eq!(submitted(&fake), [Call::SaveEvent(input.clone())]);
        assert_eq!(record, Some(Undo::Event { id: "ev-new".into() }));
        undo(&fake, &record.unwrap()).unwrap();
        assert_eq!(submitted(&fake)[1], Call::DeleteEvent("ev-new".into()));
        // A record that repeats is not what a card can show: it is not saved.
        let mut odd = input.clone();
        odd.repeat = Some(crate::model::RepeatRule {
            frequency: crate::model::Frequency::Daily,
            interval: 1,
            by_day: vec![],
            until: None,
            count: None,
        });
        let applied = apply(&fake, &fake, &AiChange::AddEvent { input: odd, calendar_name: "Home".into() });
        assert!(applied.result.is_err() && applied.undo.is_none());
        assert_eq!(submitted(&fake).len(), 2);
    }

    /// Every kind a change can be, and whether it is applied without a tap. A new kind has no
    /// entry in `automatic`, so it asks until someone decides here.
    #[test]
    fn only_undoable_changes_inside_the_mailbox_are_automatic() {
        let messages = vec![AiMessageRef {
            id: "m1".into(),
            thread_id: "t1".into(),
            subject: "s".into(),
            from: "f".into(),
            mailbox_name: "Inbox".into(),
        }];
        let kinds = [
            (AiChange::MarkRead { messages: messages.clone(), read: true }, true),
            (AiChange::Star { messages: messages.clone(), on: true }, true),
            (
                AiChange::Tag { messages: messages.clone(), label_id: "l".into(), label_name: "L".into(), on: true },
                true,
            ),
            (AiChange::Archive { messages: messages.clone() }, true),
            (
                AiChange::Move { messages: messages.clone(), mailbox_id: "work".into(), mailbox_name: "Work".into() },
                true,
            ),
            (
                AiChange::AddEvent {
                    input: EventInput {
                        calendar_id: "c".into(),
                        title: "t".into(),
                        location: String::new(),
                        description: String::new(),
                        start: "2026-11-20T10:00:00".into(),
                        time_zone: None,
                        duration_secs: 3600,
                        all_day: false,
                        is_task: false,
                        repeat: None,
                        alert_minutes: vec![],
                    },
                    calendar_name: "Home".into(),
                },
                true,
            ),
        ];
        for (change, allowed) in kinds {
            assert_eq!(automatic(&change, AiRisk::Change), allowed, "{change:?}");
            // A change that needs the user's eye (more than ten messages) asks, whatever it is.
            assert!(!automatic(&change, AiRisk::Confirm), "{change:?}");
        }
    }
}
