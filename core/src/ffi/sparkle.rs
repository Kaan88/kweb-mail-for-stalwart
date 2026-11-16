//! M12 stage 8, phase 2 across the bridge: the sparkle actions of the attention layer. A finding
//! names one of five actions ([`crate::attention::ServiceAction`]); the host turns it into an
//! [`AiSparkle`] and calls [`Core::ai_sparkle_preview`] (what would be sent), then, after the user's
//! yes, [`Core::ai_sparkle_run`].
//!
//! What a run sends is exactly what its task names (the mails by id, the one event, the one task and
//! mail, the one day), made by the same code for the preview and for the run. A run makes no change:
//! the cards it returns are suggestions the core keeps under the run, and each is done only by
//! [`Core::ai_apply`] (or [`Core::ai_apply_all`]) on the user's tap, through the chat's apply path
//! and its checks. They are never applied by themselves, not even in a chat the user switched to
//! automatic (that switch belongs to the chat). No tool here sends, deletes or reaches contacts,
//! files or a shared account; the one run that reads by itself ("Look in mail") holds a reader of
//! the signed-in account's own cache and server and a short list of read tools.
//!
//! Everything in an answer is data: it is parsed strictly, and what does not fit is an error.

use std::collections::HashSet;

use chrono::NaiveDate;
use serde_json::json;

use super::{
    ai_chat::{cancelled, origin_of, CoreReader, RUN_CARD},
    now, Core,
};
use crate::{
    assistant::{
        self,
        changes::{self, message_ref},
        chat::{Entry, Model, Origin, State},
        insights, prompts,
        provider::{self, Endpoint, Usage},
        request::{self, Prepared},
        sparkle::{self as logic, PlanRequest},
        tools::{Env, Reader},
        wire::{Reply, ToolSpec, ToolsRefused, Turn},
        Running,
    },
    attention::Clock,
    compose::rules,
    jmap,
    model::{
        Address, AiActionMode, AiBatchApplied, AiBatchFailure, AiBatchItem, AiBatchUndone, AiChange, AiFinding,
        AiFound, AiProposal, AiSparkle, AiSparklePreview, AiSparkleResult, AiTask, AiToolKind, AiUsage, Occurrence,
    },
    Error, Result,
};

/// The most cards `ai_apply_all` takes at once.
const BATCH_MOST: usize = 20;
/// The most cards of sparkle runs that are kept; the oldest go first.
const KEPT_CARDS: usize = 60;
/// The longest name of a run.
const RUN_NAME_CHARS: usize = 100;
/// How far from `today` the agenda is read to find the one event or task a run names (days).
const LOOK_BACK_DAYS: i64 = 300;
const LOOK_AHEAD_DAYS: i64 = 60;

/// What `ai_apply_all` did, so that its Undo can take all of it back: (undo id, proposal ids).
#[derive(Default)]
pub(super) struct Batches {
    next: u64,
    list: Vec<(String, Vec<String>)>,
}

/// What a run needs to check the answer against.
enum Kind {
    Dates,
    Day(PlanRequest),
    Look,
    Check,
    Ideas,
}

/// A run made ready: the request as it will be sent, and what the preview says about it.
struct Prep {
    prepared: Prepared,
    kind: Kind,
    senders: Vec<String>,
    events: u32,
    tasks: u32,
    reads_mail: bool,
    time_zone: String,
}

/// The service as the short read-only loop sees it: one request per turn to the one endpoint of this
/// run, given up at once when the run is stopped; the tokens it reported are added up.
struct SparkleModel<'a> {
    core: &'a Core,
    endpoint: &'a Endpoint,
    running: &'a Running<'a>,
    instructions: &'a str,
    usage: Usage,
}

impl Model for SparkleModel<'_> {
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
        let (reply, refused, usage) = self.core.rt.block_on(async {
            tokio::select! {
                biased;
                () = self.running.cancel.notified() => Err(cancelled()),
                reply = provider::converse_with_usage(
                    self.endpoint, system, turns, tools, provider::TIMEOUTS, on_text, true,
                ) => reply,
            }
        })?;
        self.usage = self.usage.plus(usage);
        Ok((reply, refused))
    }

    fn cancelled(&self) -> bool {
        self.running.is_cancelled()
    }
}

fn unreadable() -> Error {
    Error::temporary("the answer could not be read")
}

fn parse_day(text: &str) -> Result<NaiveDate> {
    NaiveDate::parse_from_str(text.trim(), "%Y-%m-%d")
        .ok()
        .filter(|_| insights::is_day(text.trim()))
        .ok_or_else(|| Error::invalid("the day must be written like 2026-11-15"))
}

/// The `AiUsage` of a run; `None` when the service reported nothing.
fn usage_of(usage: Usage) -> Option<AiUsage> {
    (usage.input.is_some() || usage.output.is_some())
        .then_some(AiUsage { input_tokens: usage.input, output_tokens: usage.output })
}

#[uniffi::export]
impl Core {
    /// What `task` would send, without sending anything: the host, the model, how many mails,
    /// events and tasks, who the mails are from, and the request itself (`text`), made by the very
    /// code the run uses. Reads the cache, and the server for a mail body or an agenda the cache
    /// does not have. Errors as the run's own checks that come before anything is sent: Invalid
    /// "no service is set up for the assistant yet", an id that is not in this account's cache (a
    /// message or entry of a shared account is never), "there are no open tasks to plan", "there is no
    /// free time that day". The switches and the yes are not looked at here (the run does).
    pub fn ai_sparkle_preview(&self, task: AiSparkle) -> Result<AiSparklePreview> {
        let account_id = self.account_id()?;
        let settings = assistant::load(&self.store, &*self.secrets, &account_id)?;
        if settings.base_url.is_empty() {
            return Err(Error::invalid("no service is set up for the assistant yet"));
        }
        let host = assistant::settings::host_label(&assistant::settings::check_base_url(&settings.base_url)?);
        let prep = self.sparkle_prepare(&account_id, &task)?;
        Ok(AiSparklePreview {
            host,
            model: settings.model,
            messages: prep.prepared.messages,
            events: prep.events,
            tasks: prep.tasks,
            characters: prep.prepared.characters,
            is_cut: prep.prepared.is_cut,
            senders: prep.senders,
            reads_mail: prep.reads_mail,
            text: prep.prepared.user,
        })
    }

    /// Runs `task`: sends the request of [`Core::ai_sparkle_preview`] to the service, reads the answer
    /// strictly and returns it. Blocks (one run at a time, the assistant's one slot); [`Core::ai_cancel`]
    /// with the same `run_id` stops it (Invalid "cancelled"). A change is never made here: `proposals`
    /// are cards kept under this run, done only by `ai_apply` / `ai_apply_all` on the user's tap.
    ///
    /// Refused before anything is sent, with Invalid: the assistant is off or not agreed to
    /// ("you have not yet agreed to send mail to this service"), the kind is off in the settings
    /// (Add dates and Plan my day: "finding to-dos and dates"; Reply ideas: "reply ideas"; Look in
    /// mail and Check the mail: "the chat", with its own yes "you have not yet agreed to let the chat
    /// read your mail and calendar"), a bad id, day or zone, nothing to plan. An answer that does not
    /// fit its form is `Error::Temporary` "the answer could not be read" and makes no card. `usage`
    /// is what the service reported, `None` when it said nothing (the answer and the key are never in
    /// an error or a log).
    pub fn ai_sparkle_run(&self, task: AiSparkle, run_id: String) -> Result<AiSparkleResult> {
        if run_id.trim().is_empty() || run_id.chars().count() > RUN_NAME_CHARS {
            return Err(Error::invalid("the run needs a name"));
        }
        // The slot first: from here on `ai_cancel` and `sign_out` see this run.
        let running = self.ai.begin(&run_id)?;
        let account_id = self.account_id()?;
        let settings = assistant::load(&self.store, &*self.secrets, &account_id)?;
        logic::check_sparkle(&settings, &task)?;
        let prep = self.sparkle_prepare(&account_id, &task)?;
        let endpoint = assistant::endpoint(&settings, &*self.secrets)?;
        let host = assistant::settings::host_label(&assistant::settings::check_base_url(&settings.base_url)?);
        let reader = CoreReader { core: self, account_id: account_id.clone() };

        let (answer, usage, steps) = match prep.kind {
            Kind::Look => {
                let env = Env { zone: &prep.time_zone, now: now(), actions: AiActionMode::Off };
                let mut model = SparkleModel {
                    core: self,
                    endpoint: &endpoint,
                    running: &running,
                    instructions: &settings.instructions,
                    usage: Usage::default(),
                };
                let looked =
                    logic::look(&mut model, &reader, &env, &prep.prepared.system, &prep.prepared.user, &mut |_| {})?;
                (looked.answer, model.usage, looked.steps)
            }
            _ => {
                let system = prompts::with_instructions(&prep.prepared.system, &settings.instructions);
                let mut quiet = |_: &str| {};
                let (answer, usage) = self.rt.block_on(async {
                    tokio::select! {
                        biased;
                        () = running.cancel.notified() => Err(cancelled()),
                        done = provider::complete_with_usage(
                            &endpoint,
                            &system,
                            &prep.prepared.user,
                            provider::TIMEOUTS,
                            &mut quiet,
                            true,
                        ) => done,
                    }
                })?;
                (answer, usage, 0)
            }
        };

        let mut result = AiSparkleResult {
            text: String::new(),
            proposals: Vec::new(),
            ideas: Vec::new(),
            found: None,
            verdict: None,
            changes_allowed: settings.actions == AiActionMode::AskFirst,
            usage: usage_of(usage),
            host,
            model: settings.model.clone(),
            messages: prep.prepared.messages,
            characters: prep.prepared.characters,
            is_cut: prep.prepared.is_cut,
            steps,
        };
        // Cards are proposed with the "ask first" rules whatever the switch says: the switch is looked
        // at when a card is applied, and `changes_allowed` tells the host.
        let env = Env { zone: &prep.time_zone, now: now(), actions: AiActionMode::AskFirst };
        if matches!(prep.kind, Kind::Dates | Kind::Day(_)) {
            // The calendars a card is made for are known from a read of the agenda; the cache has them
            // only after one. Best effort: without them the cards are refused with their own reason.
            if reader.calendars().map_or(true, |c| c.is_empty()) {
                let _ = reader.agenda(env.now, env.now + 86_400, &prep.time_zone);
            }
        }
        let changes = match &prep.kind {
            Kind::Dates => {
                let found = insights::findings(&answer);
                if found.is_empty() && !logic::is_nothing(&answer) {
                    return Err(unreadable());
                }
                if found.is_empty() {
                    result.text = "Nothing to add was found in these mails.".into();
                }
                cards_of_findings(&reader, &env, &found)?
            }
            Kind::Day(plan) => {
                let blocks = logic::parse_plan(&answer, plan)?;
                if blocks.is_empty() {
                    result.text = "Nothing could be planned.".into();
                }
                let mut changes = Vec::new();
                let mut first_refusal = None;
                for block in &blocks {
                    let args = json!({"title": block.title, "start": block.start, "duration_minutes": block.minutes});
                    match changes::propose(&reader, &env, AiToolKind::AddEvent, &args) {
                        Ok(change) => changes.push(change),
                        Err(e @ Error::Invalid { .. }) => first_refusal = first_refusal.or(Some(e)),
                        Err(e) => return Err(e),
                    }
                }
                match (changes.is_empty(), first_refusal) {
                    (true, Some(e)) => return Err(e),
                    _ => changes,
                }
            }
            Kind::Look => {
                match logic::parse_found(&answer)? {
                    Some((id, text)) => {
                        // Only a mail of this account's own cache: an id that is not there is not an answer.
                        let header = self.store.get_message(&account_id, &id)?.ok_or_else(unreadable)?;
                        let folders = self.store.list_mailboxes(&account_id)?;
                        result.found = Some(AiFound { message: message_ref(&header, &folders), text });
                    }
                    None => result.text = "Nothing certain was found in your mail.".into(),
                }
                Vec::new()
            }
            Kind::Check => {
                result.verdict = Some(logic::parse_verdict(&answer)?);
                Vec::new()
            }
            Kind::Ideas => {
                let ideas = insights::ideas(&answer);
                if ideas.is_empty() {
                    return Err(unreadable());
                }
                result.ideas = ideas;
                Vec::new()
            }
        };
        result.proposals = self.register_run(&account_id, origin_of(&settings), &run_id, changes);
        Ok(result)
    }

    /// Does what each of these cards shows, one after the other, each by the very path and checks of
    /// [`Core::ai_apply`] (the user's tap on the card; for cards of sparkle runs and of the chat).
    /// One that fails does not stop the others and does not undo them: it is in `failed` with the
    /// reason, and an event that could not be saved stays open to be tried again. At most 20 ids,
    /// each counted once. `undo_id` takes back everything that was done and can be taken back, with
    /// [`Core::ai_undo_batch`]; each card also keeps its own Undo (`ai_undo`). Blocking.
    pub fn ai_apply_all(&self, proposal_ids: Vec<String>) -> Result<AiBatchApplied> {
        let mut seen = HashSet::new();
        let ids: Vec<String> = proposal_ids.into_iter().filter(|id| seen.insert(id.clone())).collect();
        if ids.is_empty() {
            return Err(Error::invalid("there is nothing to add"));
        }
        if ids.len() > BATCH_MOST {
            return Err(Error::invalid("at most 20 suggestions can be done at once"));
        }
        let (mut applied, mut failed, mut undoable) = (Vec::new(), Vec::new(), Vec::new());
        for id in ids {
            match self.apply_entry(&id, false).0 {
                Ok(done) => {
                    if done.can_undo {
                        undoable.push(id.clone());
                    }
                    applied.push(AiBatchItem { proposal_id: id, applied: done });
                }
                Err(e) => failed.push(AiBatchFailure { proposal_id: id, message: e.to_string() }),
            }
        }
        let undo_id = (!undoable.is_empty()).then(|| {
            let mut batches = self.batches.lock().unwrap_or_else(|e| e.into_inner());
            batches.next += 1;
            let undo_id = format!("batch-{}", batches.next);
            batches.list.push((undo_id.clone(), undoable));
            // The newest few only: an old Undo is not needed any more.
            let extra = batches.list.len().saturating_sub(20);
            batches.list.drain(..extra);
            undo_id
        });
        Ok(AiBatchApplied { applied, failed, undo_id })
    }

    /// Takes back what `ai_apply_all` did under `undo_id`, newest first, each by the single Undo of its
    /// card. One that cannot be taken back is in `failed`; it stays under the same `undo_id` so the
    /// call can be repeated for it. Blocking. Invalid "this can no longer be undone" for an id that
    /// is not (or no longer) there.
    pub fn ai_undo_batch(&self, undo_id: String) -> Result<AiBatchUndone> {
        let ids = {
            let mut batches = self.batches.lock().unwrap_or_else(|e| e.into_inner());
            let at = batches
                .list
                .iter()
                .position(|(id, _)| *id == undo_id)
                .ok_or_else(|| Error::invalid("this can no longer be undone"))?;
            batches.list.remove(at).1
        };
        let (mut undone, mut failed) = (Vec::new(), Vec::new());
        for id in ids.into_iter().rev() {
            match self.ai_undo(id.clone()) {
                Ok(()) => undone.push(id),
                Err(e) => failed.push(AiBatchFailure { proposal_id: id, message: e.to_string() }),
            }
        }
        if !failed.is_empty() {
            let again: Vec<String> = failed.iter().rev().map(|f| f.proposal_id.clone()).collect();
            self.batches.lock().unwrap_or_else(|e| e.into_inner()).list.push((undo_id, again));
        }
        Ok(AiBatchUndone { undone, failed })
    }

    /// Drops the cards of the run `run_id` that still wait (or were declined or taken back): the host
    /// calls it when the user dismisses the result. A card that was done keeps its Undo. Never fails.
    pub fn ai_run_forget(&self, run_id: String) {
        let prefix = format!("{RUN_CARD}{run_id}:");
        if let Some(book) = self.runs_lock().as_mut() {
            book.entries.retain(|e| {
                !(e.proposal.id.starts_with(&prefix)
                    && matches!(e.state, State::Pending | State::Declined | State::Undone))
            });
        }
    }
}

/// The cards for what Add dates found: a new task for a to-do, a new event for a date. A finding the
/// rules refuse (no calendar to write to, a title that is too long) is left out; when every one is
/// refused the first reason is the error.
fn cards_of_findings(reader: &dyn Reader, env: &Env<'_>, found: &[AiFinding]) -> Result<Vec<AiChange>> {
    let mut cards = Vec::new();
    let mut first_refusal = None;
    for finding in found {
        let (kind, args) = match finding {
            AiFinding::Todo { text } => (AiToolKind::AddTask, json!({"title": text})),
            AiFinding::Date { title, start, all_day } => {
                (AiToolKind::AddEvent, json!({"title": title, "start": start, "all_day": all_day}))
            }
        };
        match changes::propose(reader, env, kind, &args) {
            Ok(change) => cards.push(change),
            Err(e @ Error::Invalid { .. }) => first_refusal = first_refusal.or(Some(e)),
            Err(e) => return Err(e),
        }
    }
    match (cards.is_empty(), first_refusal) {
        (true, Some(e)) => Err(e),
        _ => Ok(cards),
    }
}

impl Core {
    /// Makes the request of `task` from the account's own cache (and server), and what the answer is
    /// checked against. Every id is looked up in the signed-in account's cache: an id of a shared
    /// account is not there (ids repeat between accounts, so this account's lookup is the only one).
    fn sparkle_prepare(&self, account_id: &str, task: &AiSparkle) -> Result<Prep> {
        let time_zone = match task {
            AiSparkle::AddDates { time_zone, .. }
            | AiSparkle::PlanMyDay { time_zone, .. }
            | AiSparkle::LookInMail { time_zone, .. }
            | AiSparkle::CheckTheMail { time_zone, .. } => time_zone.as_str(),
            AiSparkle::ReplyIdeas { .. } => "UTC",
        };
        let zone = jmap::check_span(0, 1, time_zone)?;
        let usable = |today: &str| match insights::is_day(today) {
            true => Ok(()),
            false => Err(Error::invalid("today's date must be written like 2026-11-15")),
        };
        let reader = CoreReader { core: self, account_id: account_id.to_string() };
        let sender = |h: &crate::model::MessageHeader| {
            rules::address_text(&Address { name: h.from_name.clone(), email: h.from_email.clone() })
        };
        let mut prep = Prep {
            prepared: request::done(String::new(), String::new(), 0, false),
            kind: Kind::Dates,
            senders: Vec::new(),
            events: 0,
            tasks: 0,
            reads_mail: false,
            time_zone: time_zone.to_string(),
        };
        match task {
            AiSparkle::AddDates { message_ids, today, time_zone } => {
                usable(today)?;
                let mut seen = HashSet::new();
                let ids: Vec<&String> = message_ids.iter().filter(|id| seen.insert(id.as_str())).collect();
                if ids.is_empty() {
                    return Err(Error::invalid("there is no mail to look through"));
                }
                if ids.len() > request::SPARKLE_MAILS {
                    return Err(Error::invalid("at most 5 mails can be looked through at once"));
                }
                let mut mails = Vec::new();
                for id in ids {
                    let (header, body) = self.source(account_id, id)?;
                    prep.senders.push(sender(&header));
                    mails.push(request::mail_input(&header, &body));
                }
                // Oldest first, as a conversation is sent.
                let mut order: Vec<usize> = (0..mails.len()).collect();
                order.sort_by_key(|&i| mails[i].date);
                prep.senders = order.iter().map(|&i| prep.senders[i].clone()).collect();
                let mails = order.iter().map(|&i| mails[i].clone()).collect();
                prep.prepared = request::build_extract_mails(mails, today, time_zone)?;
                prep.kind = Kind::Dates;
            }
            AiSparkle::PlanMyDay { day, today, time_zone } => {
                usable(today)?;
                let day = parse_day(day)?;
                let anchor = parse_day(today)?;
                let at = now();
                let midnight = |d: NaiveDate| Clock { now: at, zone }.ts(d.and_hms_opt(0, 0, 0).unwrap_or_default());
                // Today is planned from now on; a later day from its start; a day gone is not planned.
                let clock_now = match day.cmp(&anchor) {
                    std::cmp::Ordering::Equal => at,
                    std::cmp::Ordering::Greater => midnight(day),
                    std::cmp::Ordering::Less => midnight(day + chrono::Duration::days(1)),
                };
                let (from, to) = (midnight(day) - 60 * 86_400, midnight(day + chrono::Duration::days(1)));
                let agenda = reader.agenda(from, to, time_zone)?;
                let plan =
                    logic::plan_request(&Clock { now: clock_now, zone }, today, time_zone, day, &agenda.entries)?;
                prep.events = plan.events;
                prep.tasks = plan.tasks;
                prep.prepared = plan.prepared.clone();
                prep.kind = Kind::Day(plan);
            }
            AiSparkle::LookInMail { event_id, recurrence_id, today, time_zone } => {
                usable(today)?;
                let found = self.entry_named(&reader, today, time_zone, zone, event_id, false, |o| {
                    o.recurrence_id.as_deref() == recurrence_id.as_deref()
                })?;
                let clock = Clock { now: found.starts_at, zone };
                let (from, to) = (clock.local(found.starts_at), clock.local(found.ends_at.max(found.starts_at)));
                let when = if found.all_day {
                    format!("{}, all day", from.format("%Y-%m-%d"))
                } else {
                    format!("{} to {}", from.format("%Y-%m-%d %H:%M"), to.format("%H:%M"))
                };
                let (title, title_cut) = prompts::escape_cut(prompts::flatten(&found.title).trim(), 300);
                let user = format!(
                    "{}\n\n<event>\ntitle: {title}\ntime: {when}\ntime zone: {}\n</event>",
                    prompts::look_line(),
                    prompts::escape_head(&prompts::flatten(time_zone), 64).0
                );
                prep.events = 1;
                prep.reads_mail = true;
                prep.prepared = request::done(prompts::look_system(today, time_zone), user, 0, title_cut);
                prep.kind = Kind::Look;
            }
            AiSparkle::CheckTheMail { task_id, message_id, today, time_zone } => {
                usable(today)?;
                let found = self.entry_named(&reader, today, time_zone, zone, task_id, true, |_| true)?;
                let (header, body) = self.source(account_id, message_id)?;
                prep.senders.push(sender(&header));
                prep.tasks = 1;
                prep.prepared = request::build_check(&found.title, &request::mail_input(&header, &body))?;
                prep.kind = Kind::Check;
            }
            AiSparkle::ReplyIdeas { message_id } => {
                let task = AiTask::ReplyIdeas { message_id: message_id.clone() };
                let (header, _) = self.source(account_id, message_id)?;
                prep.senders.push(sender(&header));
                prep.prepared = self.ai_prepare(account_id, &task)?;
                prep.kind = Kind::Ideas;
            }
        }
        Ok(prep)
    }

    /// The one calendar entry (`task` false) or task (true) with this id, from the agenda of the
    /// account's own calendars around `today`. Err(Invalid) when it is not there.
    #[allow(clippy::too_many_arguments)]
    fn entry_named(
        &self,
        reader: &CoreReader<'_>,
        today: &str,
        time_zone: &str,
        zone: chrono_tz::Tz,
        id: &str,
        task: bool,
        also: impl Fn(&Occurrence) -> bool,
    ) -> Result<Occurrence> {
        let anchor = parse_day(today)?;
        let at = |d: NaiveDate| Clock { now: 0, zone }.ts(d.and_hms_opt(0, 0, 0).unwrap_or_default());
        let (from, to) =
            (at(anchor - chrono::Duration::days(LOOK_BACK_DAYS)), at(anchor + chrono::Duration::days(LOOK_AHEAD_DAYS)));
        let agenda = reader.agenda(from, to, time_zone)?;
        agenda
            .entries
            .into_iter()
            .find(|o| o.event_id == id && o.is_task == task && !(task && o.is_done) && also(o))
            .ok_or_else(|| {
                Error::invalid(match task {
                    true => "that task is not in the calendar",
                    false => "that calendar entry is not in the calendar",
                })
            })
    }

    /// Keeps the cards of a run under its name and gives them ids. The list belongs to the account and
    /// service in use: for another it starts again. Old cards beyond [`KEPT_CARDS`] go, the ones that
    /// were not done first.
    fn register_run(&self, account_id: &str, origin: Origin, run_id: &str, cards: Vec<AiChange>) -> Vec<AiProposal> {
        if cards.is_empty() {
            return Vec::new();
        }
        let mut book = self.runs_lock();
        let keep = book.as_ref().is_some_and(|b| b.account_id == account_id && b.origin.as_ref() == Some(&origin));
        if !keep {
            let mut fresh = assistant::chat::Chat::new("sparkle", account_id);
            fresh.origin = Some(origin);
            *book = Some(fresh);
        }
        let Some(book) = book.as_mut() else { return Vec::new() };
        book.events.clear();
        let mut out = Vec::new();
        for change in cards {
            let number = book.next;
            book.next += 1;
            let proposal =
                AiProposal { id: format!("{RUN_CARD}{run_id}:{number}"), risk: changes::risk(&change), change };
            book.entries.push(Entry { proposal: proposal.clone(), state: State::Pending });
            out.push(proposal);
        }
        while book.entries.len() > KEPT_CARDS {
            let at = book
                .entries
                .iter()
                .position(|e| matches!(e.state, State::Pending | State::Declined | State::Undone))
                .or_else(|| book.entries.iter().position(|e| !matches!(e.state, State::Applying)))
                .unwrap_or(0);
            book.entries.remove(at);
        }
        out
    }
}
