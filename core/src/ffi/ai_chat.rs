//! M7, stages 2 to 4 of the assistant across the bridge: the parsers of the stage 2 answers, the
//! chat, and the suggestions the user approves.
//!
//! The chat reads with tools and never changes anything by itself: `ai_chat_send` holds only a
//! reader. A change is a suggestion the core keeps; it is done by [`Core::ai_apply`] with its
//! id, which the host calls when the user taps the card, and nowhere else.
//!
//! Everything here is bound to one account: the reader and the writer name the account that was
//! signed in when the call began in every store call, and look again before each one. The only
//! code that holds a writer is `ai_apply` and `ai_undo`.

use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, MutexGuard},
    time::Duration,
};

use chrono::TimeZone;

use super::{now, Core};
use crate::{
    actions,
    assistant::{
        self,
        changes::{self, Undo, Writer},
        chat::{self, Chat, Model, Origin, State},
        prompts,
        provider::{self, Endpoint},
        tools::{Env, Reader},
        wire::{Reply, ToolSpec, ToolsRefused, Turn},
        Running,
    },
    jmap,
    model::{
        Action, Agenda, AiAbout, AiActionMode, AiApplied, AiAutoResult, AiChange, AiChatEnd, AiChatTurn, AiFinding,
        AiSettings, AiStep, Calendar, EventDetail, EventInput, Label, Mailbox, MessageBody, MessageHeader,
        SearchResult,
    },
    sync, Error, Result,
};

/// The longest one tool, an apply or an undo waits for the mail server.
const TOOL_BUDGET: Duration = Duration::from_secs(30);

/// What the id of a card of a sparkle run starts with (`sparkle~<run id>:<number>`). A chat is never
/// given an id that starts with it, so the two lists of cards never hold the same id.
pub(super) const RUN_CARD: &str = "sparkle~";

fn over() -> Error {
    Error::invalid("this conversation is over")
}

fn gone() -> Error {
    Error::invalid("this suggestion is no longer there")
}

pub(super) fn cancelled() -> Error {
    Error::invalid("cancelled")
}

/// The account `account_id` is still the one signed in.
fn still_signed_in<'a>(core: &Core, account_id: &'a str) -> Result<&'a str> {
    match core.account_id()? == account_id {
        true => Ok(account_id),
        false => Err(Error::sign_in_needed("not signed in")),
    }
}

/// What the chat reads through: the cache and the server of one account, and nothing that
/// changes mail, events or settings.
pub(super) struct CoreReader<'a> {
    pub(super) core: &'a Core,
    pub(super) account_id: String,
}

impl CoreReader<'_> {
    fn check(&self) -> Result<&str> {
        still_signed_in(self.core, &self.account_id)
    }
}

impl Reader for CoreReader<'_> {
    fn folders(&self) -> Result<Vec<Mailbox>> {
        self.core.store.list_mailboxes(self.check()?)
    }

    fn list(&self, mailbox_id: &str, limit: u32) -> Result<Vec<MessageHeader>> {
        self.core.store.list_messages(self.check()?, mailbox_id, limit, 0)
    }

    fn search(&self, query: &str, mailbox_id: Option<&str>, limit: u32) -> Result<SearchResult> {
        let account_id = self.check()?;
        let query = query.trim();
        if query.is_empty() {
            return Ok(SearchResult { messages: Vec::new(), from_cache: false });
        }
        let store = &self.core.store;
        let found = self.core.with_connection_within(Some(TOOL_BUDGET), |conn| async move {
            sync::search(&conn, store, account_id, query, mailbox_id, limit as usize).await
        });
        match found {
            Ok(messages) => Ok(SearchResult { messages, from_cache: false }),
            Err(e @ (Error::SignInNeeded { .. } | Error::Invalid { .. })) => Err(e),
            Err(_) => {
                let messages = store.search_messages(self.check()?, query, mailbox_id, limit)?;
                Ok(SearchResult { messages, from_cache: true })
            }
        }
    }

    fn header(&self, id: &str) -> Result<Option<MessageHeader>> {
        self.core.store.get_message(self.check()?, id)
    }

    fn body(&self, id: &str) -> Result<MessageBody> {
        if let Some(body) = self.core.store.get_body(self.check()?, id)? {
            return Ok(body);
        }
        let ids = [id.to_string()];
        let ids = &ids;
        let fetched =
            self.core.with_connection_within(Some(TOOL_BUDGET), |conn| async move { conn.bodies(ids).await })?;
        let body = fetched.into_iter().next().ok_or_else(|| Error::invalid("the server has no such message"))?;
        self.core.store.put_body(self.check()?, &body)?;
        Ok(body)
    }

    fn thread(&self, thread_id: &str) -> Result<Vec<MessageHeader>> {
        self.core.store.thread_messages(self.check()?, thread_id)
    }

    fn labels(&self) -> Result<Vec<Label>> {
        self.core.label_list(self.check()?)
    }

    fn labels_of(&self, ids: &[String]) -> Result<HashMap<String, Vec<String>>> {
        self.core.store.message_keywords(self.check()?, ids)
    }

    fn calendars(&self) -> Result<Vec<Calendar>> {
        self.core.store.calendars_list(self.check()?)
    }

    fn agenda(&self, from: i64, to: i64, zone: &str) -> Result<Agenda> {
        let account_id = self.check()?;
        let zone = jmap::check_span(from, to, zone)?;
        let store = &self.core.store;
        let found = self
            .core
            .with_connection_within(Some(TOOL_BUDGET), |conn| async move { conn.agenda(from, to, zone).await });
        match found {
            Ok((calendars, entries)) => {
                let account_id = self.check()?;
                store.calendars_replace(account_id, &calendars)?;
                store.agenda_replace(account_id, from, to, &entries)?;
                Ok(Agenda { entries, from_cache: false })
            }
            Err(e @ (Error::SignInNeeded { .. } | Error::Invalid { .. })) => Err(e),
            Err(_) => {
                let mut entries = store.agenda_list(account_id, from, to)?;
                jmap::sort_agenda(&mut entries, zone);
                Ok(Agenda { entries, from_cache: true })
            }
        }
    }

    fn event(&self, id: &str, recurrence_id: Option<&str>) -> Result<EventDetail> {
        self.check()?;
        self.core
            .with_connection_within(Some(TOOL_BUDGET), |conn| async move { conn.event_detail(id, recurrence_id).await })
    }
}

/// What an approved suggestion is done with. Made by `ai_apply` and `ai_undo` only. It cannot
/// delete mail, and of events it can only add one and take that one back.
struct CoreWriter<'a> {
    core: &'a Core,
    account_id: String,
}

impl CoreWriter<'_> {
    fn check(&self) -> Result<&str> {
        still_signed_in(self.core, &self.account_id)
    }
}

impl Writer for CoreWriter<'_> {
    fn submit_all(&self, actions: &[Action]) -> Result<()> {
        let account_id = self.check()?;
        if actions.iter().any(|a| matches!(a, Action::Delete { .. } | Action::Destroy { .. })) {
            return Err(Error::invalid("the assistant does not delete mail"));
        }
        let result = actions::submit_all(&self.core.store, account_id, actions);
        self.core.poke();
        result
    }

    fn set_label(&self, ids: &[String], label_id: &str, on: bool) -> Result<()> {
        let account_id = self.check()?;
        self.core.known_label(account_id, label_id)?;
        let mut seen = HashSet::new();
        let ids: Vec<String> = ids.iter().filter(|id| seen.insert(id.as_str())).cloned().collect();
        let result = actions::submit_labels(&self.core.store, account_id, &ids, label_id, on);
        self.core.poke();
        result
    }

    fn save_event(&self, input: &EventInput) -> Result<String> {
        self.check()?;
        let saved = self
            .core
            .with_connection_within(Some(TOOL_BUDGET), |conn| async move { conn.save_event(None, input).await })?;
        self.core.store.agenda_forget_event(self.check()?, &saved)?;
        Ok(saved)
    }

    fn delete_event(&self, id: &str) -> Result<()> {
        self.check()?;
        self.core.with_connection_within(Some(TOOL_BUDGET), |conn| async move { conn.delete_event(id, None).await })?;
        self.core.store.agenda_forget_event(self.check()?, id)
    }
}

/// The service as the loop sees it: one request per turn, always to the one endpoint of this
/// send, given up at once when the run is stopped. The only `block_on` of a send's model side.
struct Live<'a> {
    core: &'a Core,
    endpoint: &'a Endpoint,
    running: &'a Running<'a>,
    /// The user's own instructions, added after the app's system text of every turn.
    instructions: &'a str,
}

impl Model for Live<'_> {
    fn turn(
        &mut self,
        system: &str,
        turns: &[Turn],
        tools: &[ToolSpec],
        on_text: &mut (dyn FnMut(&str) + Send),
    ) -> Result<(Reply, ToolsRefused)> {
        if self.running.is_cancelled() {
            return Err(cancelled());
        }
        let system = &prompts::with_instructions(system, self.instructions);
        self.core.rt.block_on(async {
            tokio::select! {
                biased;
                () = self.running.cancel.notified() => Err(cancelled()),
                reply = provider::converse(self.endpoint, system, turns, tools, provider::TIMEOUTS, on_text) => reply,
            }
        })
    }

    fn cancelled(&self) -> bool {
        self.running.is_cancelled()
    }
}

/// "YYYY-MM-DD, Weekday" of `at` (Unix seconds) in `zone`.
pub(super) fn today_in(zone: chrono_tz::Tz, at: i64) -> String {
    let utc = chrono::DateTime::from_timestamp(at, 0).unwrap_or_default();
    zone.from_utc_datetime(&utc.naive_utc()).format("%Y-%m-%d, %A").to_string()
}

pub(super) fn origin_of(settings: &AiSettings) -> Origin {
    Origin { provider: settings.provider, base_url: settings.base_url.clone(), model: settings.model.clone() }
}

/// Hears one chat send. Called on a core thread; must not call a blocking Core fn.
#[uniffi::export(with_foreign)]
pub trait AiChatListener: Send + Sync {
    /// The next piece of the text being written.
    fn text(&self, piece: String);
    /// A tool is about to be used. Text heard before it was interim: the host clears it.
    fn step(&self, step: AiStep);
}

/// The ideas in a ReplyIdeas answer, at most 3. Pure.
#[uniffi::export]
pub fn ai_parse_ideas(answer: String) -> Vec<String> {
    assistant::insights::ideas(&answer)
}

/// The findings in an Extract answer, at most 20; lines that do not fit are dropped. Pure.
#[uniffi::export]
pub fn ai_parse_findings(answer: String) -> Vec<AiFinding> {
    assistant::insights::findings(&answer)
}

#[uniffi::export]
impl Core {
    /// Gives or takes back the yes for the chat, for the service in use. Giving it needs `consented`
    /// (else Invalid "you have not yet agreed to send mail to this service"). Local, sends nothing.
    pub fn ai_chat_consent(&self, agree: bool) -> Result<AiSettings> {
        assistant::chat_consent(&self.store, &*self.secrets, &self.account_id()?, agree)
    }

    /// Starts a new conversation `chat_id` (made up by the host, a UUID) for the signed-in account and
    /// drops the previous one with its suggestions and undo records. Memory only. Sends nothing.
    pub fn ai_chat_new(&self, chat_id: String) -> Result<()> {
        let account_id = self.account_id()?;
        if chat_id.trim().is_empty() || chat_id.chars().count() > 100 || chat_id.starts_with(RUN_CARD) {
            return Err(Error::invalid("the conversation needs a name"));
        }
        *self.chat_lock() = Some(Chat::new(&chat_id, &account_id));
        Ok(())
    }

    /// Drops the conversation if `chat_id` is the current one. Never fails, does not block.
    pub fn ai_chat_close(&self, chat_id: String) {
        let mut chat = self.chat_lock();
        if chat.as_ref().is_some_and(|c| c.id == chat_id) {
            *chat = None;
        }
    }

    /// One user message. Blocks until the answer is complete; one run at a time (the stage 1 slot);
    /// `ai_cancel(run_id)` stops it. `time_zone`: the phone's IANA zone. Errors as `ai_run`
    /// ("cancelled", "the assistant is already working on something"), plus Invalid
    /// "this conversation is over" (unknown chat, other account, or the service or model changed),
    /// "the chat is switched off in the settings",
    /// "you have not yet agreed to let the chat read your mail and calendar",
    /// "write something first" / "that message is too long". Every refusal happens before anything is
    /// sent. On any Err the conversation is exactly as before the call.
    pub fn ai_chat_send(
        &self,
        chat_id: String,
        text: String,
        time_zone: String,
        run_id: String,
        listener: Arc<dyn AiChatListener>,
    ) -> Result<AiChatTurn> {
        // The slot first, as in `ai_run`: from here on `ai_cancel` and `sign_out` see this run.
        let running = self.ai.begin(&run_id)?;
        let account_id = self.account_id()?;
        let settings = assistant::load(&self.store, &*self.secrets, &account_id)?;
        assistant::check_chat(&settings)?;
        chat::check_text(&text)?;
        let zone = jmap::check_span(0, 1, &time_zone)?;
        // The one endpoint of this send: every turn, and a retry without tools, goes there.
        let endpoint = assistant::endpoint(&settings, &*self.secrets)?;
        let origin = origin_of(&settings);
        // The run works on a copy. How many tap outcomes the copy holds (they are told to the
        // model and cleared) is noted, so that the commit keeps the ones that come meanwhile.
        let (mut copy, told) = match &*self.chat_lock() {
            Some(c)
                if c.id == chat_id
                    && c.account_id == account_id
                    && (c.origin.is_none() || c.origin.as_ref() == Some(&origin)) =>
            {
                (c.clone(), c.events.len())
            }
            _ => return Err(over()),
        };
        // Automatic changes were switched on for one service and model; for another they are off.
        if copy.auto.is_some() && copy.auto.as_ref() != Some(&origin) {
            copy.auto = None;
        }
        copy.origin = Some(origin);

        let at = now();
        let env = Env { zone: &time_zone, now: at, actions: settings.actions };
        let today = today_in(zone, at);
        let reader = CoreReader { core: self, account_id: account_id.clone() };
        let mut model =
            Live { core: self, endpoint: &endpoint, running: &running, instructions: &settings.instructions };
        let mut on_text = |piece: &str| listener.text(piece.to_string());
        let mut on_step = |step: AiStep| listener.step(step);
        let sent = chat::send(&mut copy, &text, &env, &today, &mut model, &reader, &mut on_text, &mut on_step);
        // What the service said about tools is remembered for this address and model, so that the
        // settings can say so. A failure to remember it does not fail the send.
        let tools = match &sent {
            Err(Error::ToolsUnsupported) => Some(true),
            Ok(turn) if turn.tools_off => Some(true),
            Ok(turn) if turn.end == AiChatEnd::Answered || turn.steps > 0 => Some(false),
            Ok(_) | Err(_) => None,
        };
        if let (Some(unsupported), Some(origin)) = (tools, &copy.origin) {
            assistant::note_tools(&self.store, &*self.secrets, &account_id, origin, unsupported).ok();
        }
        let mut turn = sent?;

        // Commit, and only into the conversation the run began in.
        let mut stored = self.chat_lock();
        match stored.as_mut() {
            Some(c) if c.id == chat_id && c.account_id == account_id => {
                // Where a suggestion stands is what the stored conversation says (a tap may have
                // come meanwhile); the run itself only ever withdraws one that still waits.
                for entry in &mut copy.entries {
                    match c.entries.iter().find(|e| e.proposal.id == entry.proposal.id) {
                        Some(tapped) if tapped.state == State::Pending && entry.state == State::Declined => {}
                        Some(tapped) => entry.state = tapped.state.clone(),
                        None => {}
                    }
                }
                copy.events.extend(c.events.iter().skip(told).cloned());
                // The switch is the user's, made through `ai_chat_auto` only; a send keeps what is
                // stored now, or switches it off when this send saw another service or model.
                copy.auto = if copy.auto.is_some() { c.auto.clone() } else { None };
                *c = copy;
            }
            _ => return Err(over()),
        }
        drop(stored);

        // Automatic changes: the loop only proposed. What the user allowed is applied here, through
        // the same path and checks as a tap, one at a time, and shown as done cards. Stop ends it,
        // and what is left waits for a tap.
        let itself: Vec<bool> = {
            let chat = self.chat_lock();
            let chat = chat.as_ref().filter(|c| c.id == chat_id);
            turn.proposals.iter().map(|p| chat.is_some_and(|c| c.applies_itself(p))).collect()
        };
        let by_itself: Vec<_> = turn.proposals.iter().zip(&itself).filter(|(_, itself)| **itself).collect();
        for (proposal, _) in by_itself.into_iter().take(chat::AUTO_PER_RUN) {
            if running.is_cancelled() {
                break;
            }
            let (result, left_open) = self.apply_entry(&proposal.id, true);
            turn.automatic.push(match result {
                Ok(applied) => AiAutoResult {
                    proposal_id: proposal.id.clone(),
                    applied: true,
                    can_undo: applied.can_undo,
                    left_open: false,
                    message: String::new(),
                },
                Err(e) => AiAutoResult {
                    proposal_id: proposal.id.clone(),
                    applied: false,
                    can_undo: false,
                    left_open,
                    message: e.to_string(),
                },
            });
        }
        Ok(turn)
    }

    /// Tells the chat which conversation it is about (`Some(thread id)`), or takes that back
    /// (`None`). Only before the first message: the text goes to the service with it, once, and
    /// what has been sent cannot be taken back (Invalid "start a new conversation..."). Reads the
    /// cache (a body it lacks from the server) and sends nothing. The text is the one a thread
    /// summary sends, within its limits, framed as data. Refused like a send when the chat is off
    /// or not agreed to. Only the signed-in account's own mail can be named.
    pub fn ai_chat_about(&self, chat_id: String, thread_id: Option<String>) -> Result<Option<AiAbout>> {
        let account_id = self.account_id()?;
        let settings = assistant::load(&self.store, &*self.secrets, &account_id)?;
        assistant::check_chat(&settings)?;
        let still_empty = |chat: &Option<Chat>| match chat {
            Some(c) if c.id == chat_id && c.account_id == account_id => match c.turns.is_empty() {
                true => Ok(()),
                false => Err(Error::invalid("start a new conversation to ask about another mail")),
            },
            _ => Err(over()),
        };
        still_empty(&self.chat_lock())?;
        let about = match thread_id {
            None => None,
            Some(thread_id) => {
                let ((mails, earlier), subject) = self.thread_mails(&account_id, &thread_id)?;
                let (block, messages, is_cut) = assistant::request::about_block(mails, earlier)?;
                Some(chat::About { thread_id, subject, messages, is_cut, block })
            }
        };
        let mut stored = self.chat_lock();
        still_empty(&stored)?;
        let shown = about.as_ref().map(|a| AiAbout {
            thread_id: a.thread_id.clone(),
            subject: a.subject.clone(),
            messages: a.messages,
            is_cut: a.is_cut,
        });
        if let Some(c) = stored.as_mut() {
            c.about = about;
        }
        Ok(shown)
    }

    /// Switches automatic changes on or off for this chat. Its own call: nothing the model or a
    /// mail says can reach it. On: suggestions to mark, star, tag, archive or move mail and to add
    /// an entry to the user's own calendar are applied right away, each shown as done with an Undo;
    /// anything else still asks. It holds for the service and model in use now, ends when either
    /// changes, and with `ai_chat_new`, `ai_chat_close`, a reset and sign-out. Refused (Invalid)
    /// unless the chat is on and agreed to, and actions are on ("ask first").
    pub fn ai_chat_auto(&self, chat_id: String, on: bool) -> Result<()> {
        let account_id = self.account_id()?;
        let settings = assistant::load(&self.store, &*self.secrets, &account_id)?;
        assistant::check_chat(&settings)?;
        if on && settings.actions != AiActionMode::AskFirst {
            return Err(Error::invalid("actions are switched off in the settings"));
        }
        let mut stored = self.chat_lock();
        match stored.as_mut() {
            Some(c) if c.id == chat_id && c.account_id == account_id => {
                c.auto = on.then(|| origin_of(&settings));
                Ok(())
            }
            _ => Err(over()),
        }
    }

    /// Does what the suggestion shows, once. Blocking (an event is saved over the network). Invalid
    /// "this suggestion is no longer there" (unknown, used, declined, other account, chat closed) and
    /// "actions are switched off in the settings"; the chat gates are checked again. A failed event save
    /// leaves the suggestion open; a failed mail change closes it and keeps an Undo for what was done.
    pub fn ai_apply(&self, proposal_id: String) -> Result<AiApplied> {
        self.apply_entry(&proposal_id, false).0
    }

    /// Marks it declined. Never fails, does not block.
    pub fn ai_decline(&self, proposal_id: String) {
        let mut chat = self.book_lock(&proposal_id);
        let Some(chat) = chat.as_mut() else { return };
        let entry = chat.entries.iter_mut().find(|e| e.proposal.id == proposal_id && e.state == State::Pending);
        if let Some(entry) = entry {
            entry.state = State::Declined;
            chat.events.push(format!("suggestion {}: declined", chat::number_of(&proposal_id)));
        }
    }

    /// Takes an applied suggestion back, once. Blocking. Invalid "this can no longer be undone".
    pub fn ai_undo(&self, proposal_id: String) -> Result<()> {
        let past = || Error::invalid("this can no longer be undone");
        let signed_in = self.account_id().ok();
        let (account_id, undo) = {
            let mut chat = self.book_lock(&proposal_id);
            let chat = chat.as_mut().filter(|c| Some(&c.account_id) == signed_in.as_ref()).ok_or_else(past)?;
            let account_id = chat.account_id.clone();
            let entry = chat.entries.iter_mut().find(|e| e.proposal.id == proposal_id).ok_or_else(past)?;
            // Single use: the record is taken out here, so a second tap finds none.
            let undo = match &mut entry.state {
                State::Applied(undo) | State::Failed(undo) => undo.take().ok_or_else(past)?,
                _ => return Err(past()),
            };
            (account_id, undo)
        };
        let writer = CoreWriter { core: self, account_id };
        match changes::undo(&writer, &undo) {
            Ok(()) => {
                self.settle(&proposal_id, State::Undone, Some("taken back by the user"));
                Ok(())
            }
            Err(e) => {
                // An event that could not be deleted is still there and nothing else was done, so
                // that one can be tried again. Mail that was partly moved back cannot.
                if let Undo::Event { .. } = undo {
                    let mut chat = self.book_lock(&proposal_id);
                    let entry = chat.as_mut().and_then(|c| c.entries.iter_mut().find(|e| e.proposal.id == proposal_id));
                    if let Some(State::Applied(record) | State::Failed(record)) = entry.map(|e| &mut e.state) {
                        *record = Some(undo);
                    }
                }
                Err(e)
            }
        }
    }
}

impl Core {
    /// The one way a suggestion is applied: a tap (`automatic` false) and the automatic changes
    /// of a send (true) both come here, with the same gates. For an automatic one the chat must
    /// still have automatic changes on for this service and model and the kind must be in
    /// [`changes::automatic`] and be in view (`Chat::in_view`), else it is left as it is (it waits for a tap) and the second value
    /// says so (true: still open).
    pub(super) fn apply_entry(&self, proposal_id: &str, automatic: bool) -> (Result<AiApplied>, bool) {
        // Asked before the chat is locked, so the two locks are never held together.
        let signed_in = self.account_id().ok();
        let claimed = (|| {
            let mut chat = self.book_lock(proposal_id);
            let chat = chat.as_mut().filter(|c| Some(&c.account_id) == signed_in.as_ref()).ok_or_else(gone)?;
            let (account_id, origin) = (chat.account_id.clone(), chat.origin.clone());
            let proposal = chat
                .entries
                .iter()
                .find(|e| e.proposal.id == proposal_id && e.state == State::Pending)
                .map(|e| e.proposal.clone())
                .ok_or_else(gone)?;
            let allowed = chat.applies_itself(&proposal);
            let entry = chat
                .entries
                .iter_mut()
                .find(|e| e.proposal.id == proposal_id && e.state == State::Pending)
                .ok_or_else(gone)?;
            if automatic && !allowed {
                return Err(Error::invalid("this suggestion waits for the user"));
            }
            // Single use: a second tap finds it no longer pending.
            entry.state = State::Applying;
            Ok((account_id, origin, entry.proposal.change.clone()))
        })();
        let (account_id, origin, change) = match claimed {
            Ok(claimed) => claimed,
            Err(e) => return (Err(e), automatic),
        };
        let result = self.apply_claimed(proposal_id, account_id, origin, change, automatic);
        let open = self
            .book_lock(proposal_id)
            .as_ref()
            .is_some_and(|c| c.entries.iter().any(|e| e.proposal.id == proposal_id && e.state == State::Pending));
        (result, open)
    }

    fn apply_claimed(
        &self,
        proposal_id: &str,
        account_id: String,
        origin: Option<Origin>,
        change: AiChange,
        automatic: bool,
    ) -> Result<AiApplied> {
        // Outside the lock, the gates again: what was allowed when it was suggested may not be now.
        let of_a_run = proposal_id.starts_with(RUN_CARD);
        let allowed = assistant::load(&self.store, &*self.secrets, &account_id).and_then(|settings| {
            // A card of a sparkle run was asked for with the yes of its own kind of action; the tap on
            // it needs the assistant on and agreed to, and changes allowed. A chat's card needs the chat's yes.
            match of_a_run {
                true => assistant::sparkle::check_apply(&settings)?,
                false => assistant::check_chat(&settings)?,
            }
            if settings.actions != AiActionMode::AskFirst {
                return Err(Error::invalid("actions are switched off in the settings"));
            }
            // A suggestion belongs to the service and model it was made with.
            match origin == Some(origin_of(&settings)) {
                true => Ok(()),
                false => Err(gone()),
            }
        });
        if let Err(e) = allowed {
            self.settle(proposal_id, State::Pending, None);
            if automatic {
                // The user's yes was for something that no longer holds.
                if let Some(chat) = self.chat_lock().as_mut() {
                    chat.auto = None;
                }
            }
            return Err(e);
        }
        let reader = CoreReader { core: self, account_id: account_id.clone() };
        let writer = CoreWriter { core: self, account_id };
        let applied = changes::apply(&reader, &writer, &change);
        let done = if automatic { "done automatically, as the user allowed" } else { "done" };
        let (state, line) = match (&applied.result, &change) {
            (Ok(_), _) => (State::Applied(applied.undo), Some(done)),
            // Nothing was saved: the suggestion stays open and can be tried again.
            (Err(_), AiChange::AddEvent { .. }) => (State::Pending, None),
            (Err(_), _) => (State::Failed(applied.undo), Some("could not be done")),
        };
        self.settle(proposal_id, state, line);
        applied.result
    }

    fn chat_lock(&self) -> MutexGuard<'_, Option<Chat>> {
        self.chat.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The list of cards that holds `proposal_id`: a sparkle run's, or the chat's.
    fn book_lock(&self, proposal_id: &str) -> MutexGuard<'_, Option<Chat>> {
        match proposal_id.starts_with(RUN_CARD) {
            true => self.runs_lock(),
            false => self.chat_lock(),
        }
    }

    pub(super) fn runs_lock(&self) -> MutexGuard<'_, Option<Chat>> {
        self.runs.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Writes down where a suggestion stands now and, with `line`, tells the model with the next
    /// message. A suggestion of a conversation that is gone meanwhile is not found: nothing is kept.
    fn settle(&self, proposal_id: &str, state: State, line: Option<&str>) {
        let mut chat = self.book_lock(proposal_id);
        let Some(chat) = chat.as_mut() else { return };
        if let Some(entry) = chat.entries.iter_mut().find(|e| e.proposal.id == proposal_id) {
            entry.state = state;
            if let Some(line) = line {
                chat.events.push(format!("suggestion {}: {line}", chat::number_of(proposal_id)));
            }
        }
    }

    /// Sign-out: the conversation, its suggestions and undo records go.
    pub(super) fn ai_chat_forget(&self) {
        *self.chat_lock() = None;
        *self.runs_lock() = None;
        *self.batches.lock().unwrap_or_else(|e| e.into_inner()) = Default::default();
    }
}
