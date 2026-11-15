//! `kweb ai`: the assistant from the command line. The key is read from `KWEB_AI_KEY` and from
//! nowhere else (an argument would end up in the shell history and the process list), and is
//! never printed.
//!
//! Stages 2 to 4 are here too, as the way to try them by hand: reply ideas, to-dos and dates,
//! explaining, and `kweb ai chat`. The chat only reads; a change it suggests is printed in full
//! and done by `ai_apply` only after a `y` typed at the terminal.

use std::{
    io::{self, BufRead, Write},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use kweb_core::{
    assistant::settings as rules,
    ffi::{ai_parse_findings, ai_parse_ideas, AiChatListener, AiListener, Core},
    model::{
        AiActionMode, AiChange, AiChatEnd, AiChatTurn, AiEffort, AiFinding, AiLink, AiMessageRef, AiModelList,
        AiPreview, AiProposal, AiProvider, AiRisk, AiSettings, AiStep, AiTask, AiToolKind, EventInput,
    },
};

use crate::{
    args::{Ai, AiSwitches},
    commands::printable,
    timefmt,
};

/// The environment variable that holds the key.
pub const KEY_VARIABLE: &str = "KWEB_AI_KEY";

pub fn run(core: &Core, what: Ai, env_key: Option<String>) -> Result<(), String> {
    match what {
        Ai::Status => {
            println!("{}", status_text(&core.ai_settings().map_err(|e| e.to_string())?));
            Ok(())
        }
        Ai::Set { provider, base_url, model, effort, yes, switches } => {
            // What is stored for this service, whichever is in use: its effort and its consent.
            let current = core.ai_service(provider).map_err(|e| e.to_string())?;
            match plan_set(&current, provider, &base_url, &model, effort, yes, env_key)? {
                Outcome::NeedsYes(notice) => Err(notice),
                Outcome::Store { settings, key } => {
                    let settings = with_switches(settings, &switches);
                    // A change of service clears the yes, so it is given by a second save of the
                    // same values (what the user agreed to with `--yes`).
                    core.set_ai_settings(settings.clone(), key.clone()).map_err(|e| e.to_string())?;
                    let stored = core.set_ai_settings(settings, None).map_err(|e| e.to_string())?;
                    if current.has_key && !stored.has_key && key.is_none() {
                        println!(
                            "The stored key was removed because the service changed; set {KEY_VARIABLE} to store one."
                        );
                    }
                    println!("{}", status_text(&stored));
                    if stored.allow_chat && !stored.chat_consented {
                        println!("The chat needs its own yes: see `kweb ai chat-consent`.");
                    }
                    Ok(())
                }
            }
        }
        Ai::Models { provider } => {
            let in_use = core.ai_settings().map_err(|e| e.to_string())?;
            let service = core.ai_service(provider.unwrap_or(in_use.provider)).map_err(|e| e.to_string())?;
            if service.base_url.is_empty() {
                return Err("this service has no address yet: run `kweb ai set` first".into());
            }
            let key = env_key.filter(|k| !k.trim().is_empty());
            println!("{}", models_text(&core.ai_models(service, key).map_err(|e| e.to_string())?));
            Ok(())
        }
        Ai::Summary { id } => {
            task(core, AiTask::SummarizeMessage { message_id: id }, Arc::new(Printer))?;
            println!();
            Ok(())
        }
        Ai::Ideas { id } => {
            let answer = task(core, AiTask::ReplyIdeas { message_id: id }, Arc::new(Quiet))?;
            println!("{}", ideas_text(&ai_parse_ideas(answer.clone()), &answer));
            Ok(())
        }
        Ai::Found { id } => {
            // The date and the zone are part of what is sent, so "Friday" in the mail can be read.
            let zone = zone();
            let today = timefmt::day(now(), zone);
            let extract = AiTask::Extract { message_id: id, today, time_zone: zone.name().into() };
            let answer = task(core, extract, Arc::new(Quiet))?;
            println!("{}", findings_text(&ai_parse_findings(answer.clone()), &answer));
            Ok(())
        }
        Ai::Explain { id, text } => {
            task(core, AiTask::Explain { message_id: id, text }, Arc::new(Printer))?;
            println!();
            Ok(())
        }
        Ai::ChatConsent { agree } => chat_consent(core, agree),
        Ai::Chat => chat(core),
    }
}

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or_default()
}

/// The zone of the terminal, as the agenda takes it: `$TZ` if it names one, else UTC.
fn zone() -> chrono_tz::Tz {
    timefmt::zone_from_env(std::env::var("TZ").ok().as_deref())
}

/// A fresh name for a run: std has no UUID, and the `uuid` crate is not a dependency, so this
/// is the time, the process and a counter (two runs of one process never share it).
fn run_id() -> String {
    static COUNT: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or_default();
    format!("cli-{nanos:x}-{}-{}", std::process::id(), COUNT.fetch_add(1, Ordering::Relaxed))
}

fn effort_name(e: AiEffort) -> &'static str {
    match e {
        AiEffort::Default => "default",
        AiEffort::Low => "low",
        AiEffort::Medium => "medium",
        AiEffort::High => "high",
        AiEffort::XHigh => "xhigh",
        AiEffort::Max => "max",
    }
}

/// One line per model (id, name, efforts), after a line that says where the list is from.
/// The names are the service's words: control characters are left out.
pub fn models_text(list: &AiModelList) -> String {
    let mut out = String::from(if list.from_service {
        "Models from the service:"
    } else {
        "Built-in list (the service was not asked, or could not be):"
    });
    for m in &list.models {
        let efforts: Vec<&str> = m.efforts.iter().map(|e| effort_name(*e)).collect();
        let efforts = if efforts.is_empty() { "-".to_string() } else { efforts.join(",") };
        out.push_str(&format!("\n{}\t{}\t{efforts}", printable(&m.id), printable(&m.name)));
    }
    out
}

/// One task of stages 1 and 2: says what is sent and where, then runs it. Ctrl-C stops it.
fn task(core: &Core, task: AiTask, listener: Arc<dyn AiListener>) -> Result<String, String> {
    ready(&core.ai_settings().map_err(|e| e.to_string())?)?;
    eprintln!("{}", sending_text(&core.ai_preview(task.clone()).map_err(|e| e.to_string())?));
    let run = run_id();
    stoppable(core, &run, || core.ai_run(task, run.clone(), listener)).map_err(|e| e.to_string())
}

/// What is about to leave the device, before it does.
pub fn sending_text(preview: &AiPreview) -> String {
    let what = match preview.messages {
        0 => "your text".to_string(),
        1 => "1 message".to_string(),
        n => format!("{n} messages"),
    };
    let cut = if preview.is_cut { ", cut to the limits" } else { "" };
    format!(
        "Sending {} characters of {what}{cut} to {} ({}).",
        preview.characters,
        line(&preview.host),
        line(&preview.model)
    )
}

/// One line for the terminal from text that mail or a model shaped: no control characters, so
/// no escape sequence and no line break that could make up a prompt of this program.
fn line(text: &str) -> String {
    text.chars().filter(|c| !c.is_control() && !matches!(c, '\u{2028}' | '\u{2029}')).collect()
}

/// Prints the answer as it streams. The answer is shaped by mail from strangers, so control
/// characters (a terminal escape sequence) are left out.
struct Printer;

impl AiListener for Printer {
    fn text(&self, piece: String) {
        let mut out = io::stdout().lock();
        let _ = write!(out, "{}", printable(&piece));
        let _ = out.flush();
    }
}

/// For an answer that is read as a whole (ideas, findings): nothing is printed while it comes.
struct Quiet;

impl AiListener for Quiet {
    fn text(&self, _piece: String) {}
}

/// The ideas, numbered. An answer the core could read nothing from is shown as it came, so a
/// model that does not keep to the form can be seen.
pub fn ideas_text(ideas: &[String], answer: &str) -> String {
    if ideas.is_empty() {
        return format!("No reply ideas could be read from the answer:\n{}", printable(answer));
    }
    let lines: Vec<String> = ideas.iter().enumerate().map(|(i, idea)| format!("{}. {}", i + 1, line(idea))).collect();
    lines.join("\n")
}

/// One line per finding: a to-do, or a date with its day (and time, unless it is all day).
pub fn findings_text(found: &[AiFinding], answer: &str) -> String {
    if found.is_empty() {
        let answer = printable(answer);
        return if answer.trim().is_empty() {
            "Nothing was found.".into()
        } else {
            format!("No to-dos or dates could be read from the answer:\n{answer}")
        };
    }
    let lines: Vec<String> = found
        .iter()
        .map(|f| match f {
            AiFinding::Todo { text } => format!("to-do  {}", line(text)),
            AiFinding::Date { title, start, all_day } => {
                let start = line(start);
                let when = match start.split_once('T') {
                    Some((day, _)) if *all_day => format!("{day} (all day)"),
                    Some((day, time)) => format!("{day} {}", time.get(..5).unwrap_or(time)),
                    None => start.clone(),
                };
                format!("date   {when}  {}", line(title))
            }
        })
        .collect();
    lines.join("\n")
}

/// The assistant must be on and agreed to before a task can be started.
fn ready(s: &AiSettings) -> Result<(), String> {
    if s.enabled && s.consented {
        Ok(())
    } else {
        Err("the assistant is not set up: run `kweb ai set <provider> <base-url> <model> --yes`".into())
    }
}

fn provider_name(p: AiProvider) -> &'static str {
    match p {
        AiProvider::OpenAiCompatible => "OpenAI-compatible",
        AiProvider::Anthropic => "Anthropic",
    }
}

/// The settings for the user to read. The key is never in them: only whether there is one.
pub fn status_text(s: &AiSettings) -> String {
    let yes = |b: bool| if b { "yes" } else { "no" };
    let or_unset = |v: &str| if v.is_empty() { "(not set)".to_string() } else { v.to_string() };
    format!(
        "Assistant: {}\nService:   {}, {}\nModel:     {}\nEffort:    {}\nKey:       {}\nAgreed:    {}\n\
         Allowed:   summaries {}, drafts {}, rewrites {}\n\
         \x20          reply ideas {}, to-dos and dates {}, explaining {}\n\
         Chat:      {}, agreed {}\nActions:   {}",
        if s.enabled { "on" } else { "off" },
        provider_name(s.provider),
        or_unset(&s.base_url),
        or_unset(&s.model),
        effort_name(s.effort),
        if s.has_key { "stored" } else { "none" },
        yes(s.consented),
        yes(s.allow_summaries),
        yes(s.allow_drafts),
        yes(s.allow_rewrites),
        yes(s.allow_ideas),
        yes(s.allow_extract),
        yes(s.allow_explain),
        if s.allow_chat { "on" } else { "off" },
        yes(s.chat_consented),
        match s.actions {
            AiActionMode::Off => "off (the chat suggests no changes)",
            AiActionMode::AskFirst => "ask first (every change waits for your yes)",
        },
    )
}

/// The switches given to `kweb ai set` over what is stored; one left out keeps its value.
pub fn with_switches(mut s: AiSettings, switches: &AiSwitches) -> AiSettings {
    s.allow_ideas = switches.ideas.unwrap_or(s.allow_ideas);
    s.allow_extract = switches.extract.unwrap_or(s.allow_extract);
    s.allow_explain = switches.explain.unwrap_or(s.allow_explain);
    s.allow_chat = switches.chat.unwrap_or(s.allow_chat);
    s.actions = switches.actions.unwrap_or(s.actions);
    s
}

/// What the user agrees to by adding `--yes`.
pub fn consent_text(provider: AiProvider, host: &str, model: &str) -> String {
    format!(
        "Mail you ask the assistant about is sent, as text, to {host} ({}, model {model}).\n\
         Nothing is sent until you start a task, and then only the mail that task needs.\n\
         To agree, run the same command again with --yes.",
        provider_name(provider)
    )
}

#[derive(Debug, PartialEq)]
pub enum Outcome {
    /// Nothing is stored; this says what the user would agree to.
    NeedsYes(String),
    Store {
        settings: AiSettings,
        /// As `Core::set_ai_settings` takes it.
        key: Option<String>,
    },
}

/// What `kweb ai set` does, from what is stored for that service (`current`, as
/// `Core::ai_service` gives it). A key stored for this service is removed by the core when its
/// address changes and no new key is given.
pub fn plan_set(
    current: &AiSettings,
    provider: AiProvider,
    base_url: &str,
    model: &str,
    effort: Option<AiEffort>,
    yes: bool,
    env_key: Option<String>,
) -> Result<Outcome, String> {
    let base_url = base_url.trim().trim_end_matches('/').to_string();
    let model = model.trim().to_string();
    let url = rules::check_base_url(&base_url).map_err(|e| e.to_string())?;
    if model.is_empty() {
        return Err("the model name is empty".into());
    }
    if !yes {
        return Ok(Outcome::NeedsYes(consent_text(provider, &rules::host_label(&url), &model)));
    }
    let key = env_key.filter(|k| !k.trim().is_empty());
    let effort = effort.unwrap_or(current.effort);
    let settings = AiSettings { enabled: true, provider, base_url, model, effort, consented: true, ..current.clone() };
    Ok(Outcome::Store { settings, key })
}

/// The host the service in use is reached at, as the consent texts name it.
fn host_of(s: &AiSettings) -> Result<String, String> {
    rules::check_base_url(&s.base_url).map(|url| rules::host_label(&url)).map_err(|e| e.to_string())
}

/// What the second yes is for. It is its own text because the chat is another kind of sending:
/// the model, not the user, picks what is read.
pub fn chat_consent_text(provider: AiProvider, host: &str, model: &str) -> String {
    format!(
        "In the chat the assistant itself chooses which of your mail, folders and calendar entries it\n\
         looks up, and what it looks up is sent, as text, to {} ({}, model {}).\n\
         It changes nothing by itself: a change is only suggested, shown in full, and done when you say yes.",
        line(host),
        provider_name(provider),
        line(model)
    )
}

fn chat_consent(core: &Core, agree: Option<bool>) -> Result<(), String> {
    let settings = core.ai_settings().map_err(|e| e.to_string())?;
    if agree != Some(false) {
        // Agreeing names the service and model it is for; taking the yes back needs neither.
        ready(&settings)?;
        println!("{}", chat_consent_text(settings.provider, &host_of(&settings)?, &settings.model));
    }
    match agree {
        None => println!("To agree, run `kweb ai chat-consent yes`."),
        Some(agree) => println!("{}", status_text(&core.ai_chat_consent(agree).map_err(|e| e.to_string())?)),
    }
    Ok(())
}

/// The chat needs the assistant on, both yeses and its switch. The core checks the same before
/// anything is sent; this only says what to run.
fn chat_ready(s: &AiSettings) -> Result<(), String> {
    ready(s)?;
    if !s.allow_chat {
        return Err("the chat is switched off: add `--chat on` to `kweb ai set`".into());
    }
    if !s.chat_consented {
        return Err("you have not yet agreed to let the chat read your mail and calendar: \
                    see `kweb ai chat-consent`"
            .into());
    }
    Ok(())
}

/// What a line typed into the chat means.
#[derive(Debug, PartialEq)]
pub enum Line {
    Empty,
    Quit,
    /// Take suggestion n back (numbered from 1, as they were printed).
    Undo(usize),
    /// `undo` without a number that could name a suggestion.
    UndoWhat,
    Say(String),
}

pub fn read_line(typed: &str) -> Line {
    let text = typed.trim();
    let words: Vec<&str> = text.split_whitespace().collect();
    match words[..] {
        [] => Line::Empty,
        [word] if word.eq_ignore_ascii_case("quit") || word.eq_ignore_ascii_case("exit") => Line::Quit,
        [word] if word.eq_ignore_ascii_case("undo") => Line::UndoWhat,
        [word, n] if word.eq_ignore_ascii_case("undo") => match n.parse::<usize>() {
            Ok(n) if n >= 1 => Line::Undo(n),
            _ => Line::UndoWhat,
        },
        _ => Line::Say(text.to_string()),
    }
}

/// Only a plain yes does something; anything else, and the end of the input, is a no.
pub fn said_yes(typed: &str) -> bool {
    let answer = typed.trim();
    answer.eq_ignore_ascii_case("y") || answer.eq_ignore_ascii_case("yes")
}

/// The next line of the input without its line end; `None` at the end of the input.
fn next_line(input: &mut dyn BufRead) -> Result<Option<String>, String> {
    let mut typed = String::new();
    match input.read_line(&mut typed) {
        Ok(0) => Ok(None),
        Ok(_) => Ok(Some(typed)),
        Err(e) => Err(format!("cannot read the input: {e}")),
    }
}

fn prompt(text: &str) {
    let mut err = io::stderr().lock();
    let _ = write!(err, "{text}");
    let _ = err.flush();
}

/// One conversation on the terminal. Each line is one `ai_chat_send`; nothing is changed
/// except by `ai_apply` after a `y` to the suggestion printed just above it.
fn chat(core: &Core) -> Result<(), String> {
    let settings = core.ai_settings().map_err(|e| e.to_string())?;
    chat_ready(&settings)?;
    let zone = zone().name().to_string();
    let chat_id = run_id();
    core.ai_chat_new(chat_id.clone()).map_err(|e| e.to_string())?;
    eprintln!(
        "Chat with {} ({}). What it looks up in your mail and calendar is sent there.\n\
         Times are in {zone} ($TZ). `undo <n>` takes change n back, `quit` ends, Ctrl+C stops an answer.",
        line(&host_of(&settings)?),
        line(&settings.model)
    );
    let outcome = converse(core, &chat_id, &zone, &mut io::stdin().lock());
    // The conversation, its suggestions and their undo records are memory only and go here.
    core.ai_chat_close(chat_id);
    outcome
}

fn converse(core: &Core, chat_id: &str, zone: &str, input: &mut dyn BufRead) -> Result<(), String> {
    // The ids of the suggestions in the order they were printed: `undo <n>` names one.
    let mut shown: Vec<String> = Vec::new();
    loop {
        prompt("> ");
        let Some(typed) = next_line(input)? else { return Ok(()) };
        let text = match read_line(&typed) {
            Line::Empty => continue,
            Line::Quit => return Ok(()),
            Line::UndoWhat => {
                eprintln!("undo needs the number of a suggestion, as in `undo 1`");
                continue;
            }
            Line::Undo(n) => {
                match shown.get(n - 1) {
                    None => eprintln!("there is no suggestion {n}"),
                    Some(id) => match core.ai_undo(id.clone()) {
                        Ok(()) => println!("Suggestion {n} was taken back."),
                        Err(e) => eprintln!("kweb: {e}"),
                    },
                }
                continue;
            }
            Line::Say(text) => text,
        };
        let run = run_id();
        let sent = stoppable(core, &run, || {
            core.ai_chat_send(chat_id.to_string(), text, zone.to_string(), run.clone(), Arc::new(Steps))
        });
        let turn = match sent {
            Ok(turn) => turn,
            Err(e) => {
                let problem = e.to_string();
                if problem.contains("this conversation is over") {
                    return Err(problem);
                }
                // Stopped, refused or failed: the conversation is as it was before this line.
                eprintln!("kweb: {problem}");
                continue;
            }
        };
        println!("{}", answer_text(&turn));
        let mut asking = true;
        for proposal in &turn.proposals {
            shown.push(proposal.id.clone());
            let n = shown.len();
            println!("\n{}", proposal_text(n, proposal));
            let yes = asking && {
                prompt("Do exactly this? [y/N] ");
                match next_line(input)? {
                    Some(answer) => said_yes(&answer),
                    None => {
                        // The input ended: nobody is there to say yes to the rest either.
                        asking = false;
                        false
                    }
                }
            };
            if !yes {
                core.ai_decline(proposal.id.clone());
                println!("Not done.");
                continue;
            }
            match core.ai_apply(proposal.id.clone()) {
                Ok(applied) => {
                    let event = applied.event_id.map(|id| format!(" The entry is {}.", line(&id))).unwrap_or_default();
                    let undo = if applied.can_undo { format!(" `undo {n}` takes it back.") } else { String::new() };
                    println!("Done.{event}{undo}");
                }
                Err(e) => eprintln!("kweb: {e}"),
            }
        }
        if !asking {
            return Ok(());
        }
    }
}

/// Hears a chat send. The text heard on the way is interim (the turn's own text is the answer),
/// and a terminal cannot take it back, so only the steps are shown as they come.
struct Steps;

impl AiChatListener for Steps {
    fn text(&self, _piece: String) {}

    fn step(&self, step: AiStep) {
        eprintln!("{}", step_text(&step));
    }
}

fn tool_name(kind: AiToolKind) -> &'static str {
    match kind {
        AiToolKind::SearchMail => "search mail",
        AiToolKind::ListMessages => "list messages",
        AiToolKind::ReadMessage => "read a message",
        AiToolKind::ReadThread => "read a conversation",
        AiToolKind::ListFolders => "list folders",
        AiToolKind::ListLabels => "list labels",
        AiToolKind::ListEvents => "list calendar entries",
        AiToolKind::ReadEvent => "read a calendar entry",
        AiToolKind::OpenMessage => "point to a message",
        AiToolKind::DraftReply => "write a reply draft",
        AiToolKind::Archive => "suggest archiving",
        AiToolKind::Move => "suggest moving",
        AiToolKind::Mark => "suggest marking",
        AiToolKind::Tag => "suggest a label",
        AiToolKind::AddEvent => "suggest a calendar entry",
    }
}

/// One tool the model uses, as a line: what it does, and what with.
pub fn step_text(step: &AiStep) -> String {
    let detail = line(&step.detail);
    if detail.is_empty() {
        format!("  [{}]", tool_name(step.tool))
    } else {
        format!("  [{}] {detail}", tool_name(step.tool))
    }
}

/// The answer of one turn: its text, how the run ended if not with an answer, and the links.
pub fn answer_text(turn: &AiChatTurn) -> String {
    let text = printable(&turn.text);
    let mut out = if text.trim().is_empty() { "(no answer)".to_string() } else { text };
    if turn.tools_off {
        out.push_str(
            "\n(This service takes no tools: nothing was looked up, the answer is from the conversation only.)",
        );
    }
    match turn.end {
        AiChatEnd::Answered => {}
        AiChatEnd::StepLimit => out.push_str(&format!(
            "\n(Stopped at the limit of {} steps for one message; the answer may be incomplete.)",
            kweb_core::assistant::chat::STEPS
        )),
        AiChatEnd::SizeLimit => {
            out.push_str("\n(Stopped at the limit of what one message may send; the answer may be incomplete.)")
        }
    }
    if !turn.links.is_empty() {
        out.push_str("\nLinks (nothing is opened by itself):");
        for link in &turn.links {
            match link {
                AiLink::Message { message } => out.push_str(&format!("\n  message {}", message_text(message))),
                AiLink::ReplyDraft { message, text } => {
                    out.push_str(&format!("\n  reply draft to {}:\n{}", message_text(message), indented(text)))
                }
            }
        }
    }
    out
}

/// Text of several lines under a card or a link, each line set in.
fn indented(text: &str) -> String {
    let lines: Vec<String> = printable(text).lines().map(|l| format!("      {}", l.trim_end())).collect();
    lines.join("\n")
}

fn message_text(m: &AiMessageRef) -> String {
    let subject = line(&m.subject);
    let subject = if subject.trim().is_empty() { "(no subject)".to_string() } else { subject };
    format!("[{}] {subject} | from {} | in {}", line(&m.id), line(&m.from), line(&m.mailbox_name))
}

fn count(messages: &[AiMessageRef]) -> String {
    match messages.len() {
        1 => "1 message".into(),
        n => format!("{n} messages"),
    }
}

/// A suggestion in full: what it would do, to which folder, label or calendar, and every
/// message or every field of the entry. This is what the user says yes or no to.
pub fn proposal_text(n: usize, proposal: &AiProposal) -> String {
    let named = |name: &str, id: &str| format!("\"{}\" [{}]", line(name), line(id));
    let (title, messages) = match &proposal.change {
        AiChange::Archive { messages } => (format!("archive {}", count(messages)), Some(messages)),
        AiChange::Move { messages, mailbox_id, mailbox_name } => {
            (format!("move {} to the folder {}", count(messages), named(mailbox_name, mailbox_id)), Some(messages))
        }
        AiChange::MarkRead { messages, read } => {
            (format!("mark {} as {}", count(messages), if *read { "read" } else { "unread" }), Some(messages))
        }
        AiChange::Star { messages, on: true } => (format!("star {}", count(messages)), Some(messages)),
        AiChange::Star { messages, on: false } => (format!("take the star off {}", count(messages)), Some(messages)),
        AiChange::Tag { messages, label_id, label_name, on } => {
            let label = named(label_name, label_id);
            let title = if *on {
                format!("put the label {label} on {}", count(messages))
            } else {
                format!("take the label {label} off {}", count(messages))
            };
            (title, Some(messages))
        }
        AiChange::AddEvent { input, calendar_name } => {
            let kind = if input.is_task { "a task" } else { "an event" };
            (format!("add {kind} to the calendar {}", named(calendar_name, &input.calendar_id)), None)
        }
    };
    let mut out = format!("Suggestion {n}: {title}");
    if proposal.risk == AiRisk::Confirm {
        out.push_str("\n  Careful: this changes many messages at once.");
    }
    if let Some(messages) = messages {
        for message in messages {
            out.push_str(&format!("\n  {}", message_text(message)));
        }
    }
    if let AiChange::AddEvent { input, .. } = &proposal.change {
        out.push_str(&event_text(input));
    }
    out
}

/// Every field of an entry the chat suggests, also the ones that should be empty.
fn event_text(e: &EventInput) -> String {
    let zone = match &e.time_zone {
        Some(zone) => line(zone),
        None => "no zone: the same clock time everywhere".into(),
    };
    let length = match (e.is_task, e.all_day, e.duration_secs) {
        (true, _, _) => "none (a task)".to_string(),
        (_, true, secs) => match secs / 86_400 {
            1 => "all day, 1 day".to_string(),
            days => format!("all day, {days} days"),
        },
        (_, false, secs) => format!("{} h {:02} min", secs / 3600, secs % 3600 / 60),
    };
    let alerts = if e.alert_minutes.is_empty() {
        "none".to_string()
    } else {
        let minutes: Vec<String> = e.alert_minutes.iter().map(u32::to_string).collect();
        format!("{} minutes before", minutes.join(", "))
    };
    let repeats = match &e.repeat {
        None => "no".to_string(),
        Some(rule) => line(&format!("{rule:?}")),
    };
    let mut out = format!(
        "\n  Title:     {}\n  {}{} ({zone})\n  Length:    {length}\n  Where:     {}\n  Reminders: {alerts}\n  Repeats:   {repeats}\n  Invited:   nobody",
        line(&e.title),
        if e.is_task { "Due:       " } else { "Starts:    " },
        line(&e.start),
        line(&e.location),
    );
    if e.description.trim().is_empty() {
        out.push_str("\n  Notes:     none");
    } else {
        out.push_str(&format!("\n  Notes:\n{}", indented(&e.description)));
    }
    out
}

/// Runs `work` (one `ai_run` or `ai_chat_send` named `run_id`) so that Ctrl-C stops that run
/// through `ai_cancel` and not the program. A stop that comes before the core has begun the run
/// is not lost: the core keeps it for that id.
fn stoppable<T>(core: &Core, run_id: &str, work: impl FnOnce() -> T) -> T {
    /// Ends the watcher also when `work` panics.
    struct Over<'a>(&'a AtomicBool);
    impl Drop for Over<'_> {
        fn drop(&mut self) {
            interrupt::watch(false);
            self.0.store(true, Ordering::SeqCst);
        }
    }
    let over = AtomicBool::new(false);
    thread::scope(|scope| {
        scope.spawn(|| {
            while !over.load(Ordering::SeqCst) {
                if interrupt::taken() {
                    core.ai_cancel(run_id.to_string());
                }
                thread::sleep(Duration::from_millis(40));
            }
        });
        let _over = Over(&over);
        interrupt::watch(true);
        work()
    })
}

/// Ctrl-C. While a run is watched it only raises a flag (the handler runs where nothing may
/// block, so the cancel itself is called by `stoppable`'s thread); otherwise it ends the program
/// as it always did. std has no way to hear Ctrl-C and the workspace takes no new dependency,
/// so this declares the one system call of each platform itself.
mod interrupt {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Once,
    };

    static WATCHED: AtomicBool = AtomicBool::new(false);
    static HIT: AtomicBool = AtomicBool::new(false);

    /// Starts or ends watching; the first call installs the handler.
    pub fn watch(on: bool) {
        static INSTALL: Once = Once::new();
        INSTALL.call_once(sys::install);
        HIT.store(false, Ordering::SeqCst);
        WATCHED.store(on, Ordering::SeqCst);
    }

    /// Whether Ctrl-C was pressed since the last look.
    pub fn taken() -> bool {
        HIT.swap(false, Ordering::SeqCst)
    }

    /// From the handler: true when a run is watched and the press was taken for it.
    fn pressed() -> bool {
        let watched = WATCHED.load(Ordering::SeqCst);
        if watched {
            HIT.store(true, Ordering::SeqCst);
        }
        watched
    }

    #[cfg(windows)]
    mod sys {
        type Handler = unsafe extern "system" fn(u32) -> i32;

        #[link(name = "kernel32")]
        extern "system" {
            fn SetConsoleCtrlHandler(handler: Option<Handler>, add: i32) -> i32;
        }

        /// Windows calls this on a thread of its own. 0 is CTRL_C_EVENT; answering 0 passes the
        /// event on to the default handler, which ends the program.
        unsafe extern "system" fn on_event(kind: u32) -> i32 {
            i32::from(kind == 0 && super::pressed())
        }

        pub fn install() {
            // SAFETY: the documented signature of the kernel32 call; the handler is a plain
            // function that lives as long as the program and only touches two atomics.
            unsafe {
                SetConsoleCtrlHandler(Some(on_event), 1);
            }
        }
    }

    #[cfg(unix)]
    mod sys {
        extern "C" {
            fn signal(signum: i32, handler: extern "C" fn(i32)) -> usize;
            fn _exit(status: i32) -> !;
        }

        /// A signal handler: it may only touch atomics and call `_exit`.
        extern "C" fn on_signal(_signum: i32) {
            if !super::pressed() {
                // SAFETY: `_exit` is async-signal-safe and takes any status; 130 is what a shell
                // reports for a program ended by Ctrl-C.
                unsafe { _exit(130) }
            }
        }

        pub fn install() {
            // SAFETY: the C library's `signal` with SIGINT (2 on every Unix) and a handler that
            // lives as long as the program.
            unsafe {
                signal(2, on_signal);
            }
        }
    }

    #[cfg(not(any(windows, unix)))]
    mod sys {
        pub fn install() {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn current() -> AiSettings {
        AiSettings {
            enabled: false,
            provider: AiProvider::OpenAiCompatible,
            base_url: String::new(),
            model: String::new(),
            effort: AiEffort::Default,
            has_key: false,
            consented: false,
            allow_summaries: true,
            allow_drafts: false,
            allow_rewrites: true,
            allow_ideas: false,
            allow_extract: false,
            allow_explain: false,
            allow_chat: false,
            chat_consented: false,
            actions: kweb_core::model::AiActionMode::AskFirst,
        }
    }

    #[test]
    fn status_shows_whether_there_is_a_key_and_never_the_key() {
        let s = AiSettings { has_key: true, ..current() };
        let text = status_text(&s);
        assert!(text.contains("Key:       stored") && text.contains("Assistant: off"));
        assert!(text.contains("Service:   OpenAI-compatible, (not set)"));
        assert!(text.contains("summaries yes, drafts no, rewrites yes"));
        assert!(status_text(&current()).contains("Key:       none"));
    }

    #[test]
    fn without_yes_nothing_is_stored_and_the_notice_names_the_host_and_model() {
        let plan = plan_set(
            &current(),
            AiProvider::Anthropic,
            "https://api.example.com/v1/",
            "m-1",
            None,
            false,
            Some("k".into()),
        );
        let Ok(Outcome::NeedsYes(notice)) = plan else { panic!("{plan:?}") };
        assert!(notice.contains("api.example.com (Anthropic, model m-1)") && notice.contains("--yes"));
        assert!(!notice.contains("k\n"));
    }

    #[test]
    fn with_yes_it_enables_and_agrees_and_keeps_the_other_switches() {
        let plan = plan_set(
            &current(),
            AiProvider::OpenAiCompatible,
            " http://127.0.0.1:11434/v1/ ",
            " llama ",
            None,
            true,
            None,
        );
        let Ok(Outcome::Store { settings, key }) = plan else { panic!("{plan:?}") };
        assert!(settings.enabled && settings.consented);
        assert_eq!((settings.base_url.as_str(), settings.model.as_str()), ("http://127.0.0.1:11434/v1", "llama"));
        assert!(!settings.allow_drafts, "the other switches are kept");
        assert_eq!(key, None);
    }

    #[test]
    fn effort_is_taken_from_the_option_or_kept_from_what_is_stored() {
        let stored = AiSettings { effort: AiEffort::High, ..current() };
        let effort = |given: Option<AiEffort>| match plan_set(
            &stored,
            AiProvider::Anthropic,
            "https://a.example/v1",
            "m",
            given,
            true,
            None,
        ) {
            Ok(Outcome::Store { settings, .. }) => settings.effort,
            other => panic!("{other:?}"),
        };
        assert_eq!(effort(None), AiEffort::High);
        assert_eq!(effort(Some(AiEffort::Max)), AiEffort::Max);
        assert_eq!(effort(Some(AiEffort::Default)), AiEffort::Default);
        assert!(status_text(&stored).contains("Effort:    high"));
    }

    #[test]
    fn the_model_list_says_where_it_is_from_and_never_shows_control_characters() {
        use kweb_core::model::AiModel;
        let model = |id: &str, name: &str, efforts: &[AiEffort]| AiModel {
            id: id.into(),
            name: name.into(),
            efforts: efforts.to_vec(),
        };
        let list = AiModelList {
            models: vec![model("a", "A\u{1b}[31m", &[AiEffort::Low, AiEffort::XHigh]), model("b", "b", &[])],
            from_service: true,
        };
        let text = models_text(&list);
        assert_eq!(text, "Models from the service:\na\tA[31m\tlow,xhigh\nb\tb\t-");
        assert!(models_text(&AiModelList { from_service: false, ..list }).starts_with("Built-in list"));
    }

    #[test]
    fn run_ids_are_never_repeated() {
        let ids: std::collections::HashSet<String> = (0..100).map(|_| run_id()).collect();
        assert_eq!(ids.len(), 100);
    }

    #[test]
    fn the_key_comes_from_the_environment_only_and_an_empty_one_is_no_key() {
        let with = |env: Option<&str>| match plan_set(
            &current(),
            AiProvider::Anthropic,
            "https://a.example/v1",
            "m",
            None,
            true,
            env.map(String::from),
        ) {
            Ok(Outcome::Store { key, .. }) => key,
            other => panic!("{other:?}"),
        };
        assert_eq!(with(Some("sk-1")), Some("sk-1".into()));
        assert_eq!(with(Some("  ")), None);
        assert_eq!(with(None), None);
    }

    #[test]
    fn bad_addresses_and_models_are_refused_before_anything_else() {
        for (url, model) in [("http://api.example.com/v1", "m"), ("nonsense", "m"), ("https://a.example/v1", "  ")] {
            for yes in [false, true] {
                assert!(
                    plan_set(&current(), AiProvider::Anthropic, url, model, None, yes, None).is_err(),
                    "{url} {model}"
                );
            }
        }
    }

    #[test]
    fn a_task_needs_the_assistant_on_and_agreed_to() {
        assert!(ready(&current()).is_err());
        assert!(ready(&AiSettings { enabled: true, ..current() }).is_err());
        assert!(ready(&AiSettings { enabled: true, consented: true, ..current() }).is_ok());
    }

    #[test]
    fn status_shows_the_new_switches_the_chat_yes_and_the_action_mode() {
        let off = status_text(&current());
        assert!(off.contains("reply ideas no, to-dos and dates no, explaining no"), "{off}");
        assert!(off.contains("Chat:      off, agreed no") && off.contains("Actions:   ask first"), "{off}");
        let on = AiSettings {
            allow_ideas: true,
            allow_extract: true,
            allow_explain: true,
            allow_chat: true,
            chat_consented: true,
            actions: AiActionMode::Off,
            ..current()
        };
        let on = status_text(&on);
        assert!(on.contains("reply ideas yes, to-dos and dates yes, explaining yes"), "{on}");
        assert!(on.contains("Chat:      on, agreed yes") && on.contains("Actions:   off"), "{on}");
        assert!(on.lines().all(|l| !l.ends_with(' ')), "{on:?}");
    }

    #[test]
    fn a_switch_given_is_set_and_one_left_out_keeps_what_is_stored() {
        let stored = AiSettings { allow_ideas: true, allow_chat: true, actions: AiActionMode::Off, ..current() };
        assert_eq!(with_switches(stored.clone(), &AiSwitches::default()), stored);
        let given = AiSwitches {
            ideas: Some(false),
            extract: Some(true),
            explain: None,
            chat: None,
            actions: Some(AiActionMode::AskFirst),
        };
        let new = with_switches(stored.clone(), &given);
        assert!(!new.allow_ideas && new.allow_extract && !new.allow_explain && new.allow_chat);
        assert_eq!(new.actions, AiActionMode::AskFirst);
        // The yes for the chat is never given by a switch.
        let all = AiSwitches { chat: Some(true), ..given };
        assert!(!with_switches(current(), &all).chat_consented);
    }

    #[test]
    fn the_chat_needs_both_yeses_and_its_switch_and_says_what_to_run() {
        let agreed = AiSettings { enabled: true, consented: true, ..current() };
        assert!(chat_ready(&current()).unwrap_err().contains("kweb ai set"));
        assert!(chat_ready(&agreed).unwrap_err().contains("--chat on"));
        let on = AiSettings { allow_chat: true, ..agreed };
        assert!(chat_ready(&on).unwrap_err().contains("kweb ai chat-consent"));
        assert!(chat_ready(&AiSettings { chat_consented: true, ..on.clone() }).is_ok());
        // The chat's yes alone is not enough.
        assert!(chat_ready(&AiSettings { chat_consented: true, consented: false, ..on }).is_err());
    }

    #[test]
    fn the_chat_consent_names_the_host_and_model_and_says_who_chooses() {
        let text = chat_consent_text(AiProvider::Anthropic, "api.example.com", "m-1\u{1b}[2J");
        assert!(text.contains("api.example.com (Anthropic, model m-1[2J)"), "{text}");
        assert!(text.contains("itself chooses") && text.contains("when you say yes"));
    }

    #[test]
    fn what_is_sent_is_said_before_it_is_sent() {
        let preview = |messages, is_cut| AiPreview {
            host: "a.example".into(),
            model: "m".into(),
            messages,
            characters: 120,
            is_cut,
        };
        assert_eq!(sending_text(&preview(1, false)), "Sending 120 characters of 1 message to a.example (m).");
        assert_eq!(
            sending_text(&preview(3, true)),
            "Sending 120 characters of 3 messages, cut to the limits to a.example (m)."
        );
        assert_eq!(sending_text(&preview(0, false)), "Sending 120 characters of your text to a.example (m).");
    }

    #[test]
    fn ideas_are_numbered_and_an_answer_without_any_is_shown_as_it_came() {
        assert_eq!(
            ideas_text(&["Say yes".into(), "Ask\u{1b}[31m for\nmore time".into()], "x"),
            "1. Say yes\n2. Ask[31m formore time"
        );
        assert_eq!(ideas_text(&[], "free\u{7} text"), "No reply ideas could be read from the answer:\nfree text");
    }

    #[test]
    fn findings_are_one_line_each_with_the_day_and_the_time() {
        let found = [
            AiFinding::Todo { text: "Send the\nreport".into() },
            AiFinding::Date { title: "Review".into(), start: "2026-11-20T14:30:00".into(), all_day: false },
            AiFinding::Date { title: "Fair".into(), start: "2026-12-01T00:00:00".into(), all_day: true },
            AiFinding::Date { title: "Odd".into(), start: "soon".into(), all_day: false },
        ];
        assert_eq!(
            findings_text(&found, ""),
            "to-do  Send thereport\ndate   2026-11-20 14:30  Review\ndate   2026-12-01 (all day)  Fair\ndate   soon  Odd"
        );
        assert_eq!(findings_text(&[], " \n"), "Nothing was found.");
        assert!(findings_text(&[], "no form").ends_with(":\nno form"));
    }

    #[test]
    fn a_typed_line_is_a_message_unless_it_is_quit_or_undo_with_a_number() {
        assert_eq!(read_line("  \r\n"), Line::Empty);
        assert_eq!(read_line("quit\n"), Line::Quit);
        assert_eq!(read_line(" EXIT "), Line::Quit);
        assert_eq!(read_line("undo 2\r\n"), Line::Undo(2));
        assert_eq!(read_line("Undo  10"), Line::Undo(10));
        assert_eq!(read_line("undo"), Line::UndoWhat);
        assert_eq!(read_line("undo 0"), Line::UndoWhat);
        assert_eq!(read_line("undo last"), Line::UndoWhat);
        assert_eq!(read_line("undo the last change"), Line::Say("undo the last change".into()));
        assert_eq!(read_line(" quit smoking mail? \n"), Line::Say("quit smoking mail?".into()));
    }

    #[test]
    fn only_a_plain_yes_approves_a_suggestion() {
        for yes in ["y", "Y\r\n", " yes \n", "YES"] {
            assert!(said_yes(yes), "{yes:?}");
        }
        for no in ["", "\n", "n", "no", "ok", "yes please", "yy", "j", "1", "y\u{1b}"] {
            assert!(!said_yes(no), "{no:?}");
        }
    }

    #[test]
    fn the_end_of_the_input_is_not_a_line() {
        let mut input = io::Cursor::new(b"first\nsecond".to_vec());
        assert_eq!(next_line(&mut input), Ok(Some("first\n".into())));
        assert_eq!(next_line(&mut input), Ok(Some("second".into())));
        assert_eq!(next_line(&mut input), Ok(None));
    }

    fn message(id: &str, subject: &str) -> AiMessageRef {
        AiMessageRef {
            id: id.into(),
            thread_id: format!("t-{id}"),
            subject: subject.into(),
            from: "Ada <ada@example.com>".into(),
            mailbox_name: "Inbox".into(),
        }
    }

    fn proposal(risk: AiRisk, change: AiChange) -> AiProposal {
        AiProposal { id: "p1".into(), risk, change }
    }

    #[test]
    fn a_suggestion_lists_every_message_and_names_the_target() {
        let messages: Vec<AiMessageRef> = (1..=12).map(|i| message(&format!("m{i}"), &format!("Offer {i}"))).collect();
        let text = proposal_text(
            3,
            &proposal(
                AiRisk::Confirm,
                AiChange::Move { messages: messages.clone(), mailbox_id: "mb7".into(), mailbox_name: "Offers".into() },
            ),
        );
        assert!(text.starts_with("Suggestion 3: move 12 messages to the folder \"Offers\" [mb7]\n"), "{text}");
        assert!(text.contains("Careful: this changes many messages at once."));
        for i in 1..=12 {
            assert!(text.contains(&format!("\n  [m{i}] Offer {i} | from Ada <ada@example.com> | in Inbox")), "{text}");
        }
        let one = vec![message("m1", " ")];
        let title = |change| proposal_text(1, &proposal(AiRisk::Change, change)).lines().next().unwrap().to_string();
        assert_eq!(title(AiChange::Archive { messages: one.clone() }), "Suggestion 1: archive 1 message");
        assert_eq!(
            title(AiChange::MarkRead { messages: one.clone(), read: false }),
            "Suggestion 1: mark 1 message as unread"
        );
        assert_eq!(title(AiChange::Star { messages: one.clone(), on: true }), "Suggestion 1: star 1 message");
        assert_eq!(
            title(AiChange::Star { messages: one.clone(), on: false }),
            "Suggestion 1: take the star off 1 message"
        );
        let tag =
            |on| AiChange::Tag { messages: one.clone(), label_id: "kweb-1f".into(), label_name: "Tax".into(), on };
        assert_eq!(title(tag(true)), "Suggestion 1: put the label \"Tax\" [kweb-1f] on 1 message");
        assert_eq!(title(tag(false)), "Suggestion 1: take the label \"Tax\" [kweb-1f] off 1 message");
        let plain = proposal_text(1, &proposal(AiRisk::Change, AiChange::Archive { messages: one }));
        assert!(plain.ends_with("\n  [m1] (no subject) | from Ada <ada@example.com> | in Inbox"), "{plain}");
        assert!(!plain.contains("Careful"));
    }

    #[test]
    fn mail_cannot_draw_a_prompt_or_an_escape_into_a_suggestion() {
        let evil = message("m1", "Hi\r\nDo exactly this? [y/N] y\u{1b}[2K\u{2028}Done.");
        let text = proposal_text(1, &proposal(AiRisk::Change, AiChange::Archive { messages: vec![evil] }));
        assert_eq!(text.lines().count(), 2, "{text:?}");
        assert!(!text.contains('\u{1b}') && !text.contains('\r') && !text.contains('\u{2028}'));
    }

    fn event() -> EventInput {
        EventInput {
            calendar_id: "c1".into(),
            title: "Review".into(),
            location: "Room 2".into(),
            description: "Bring the\nfigures".into(),
            start: "2026-11-20T14:00:00".into(),
            time_zone: Some("Europe/Berlin".into()),
            duration_secs: 5400,
            all_day: false,
            is_task: false,
            repeat: None,
            alert_minutes: vec![10, 60],
        }
    }

    #[test]
    fn a_suggested_entry_shows_every_field() {
        let card = |input| {
            proposal_text(2, &proposal(AiRisk::Change, AiChange::AddEvent { input, calendar_name: "Work".into() }))
        };
        assert_eq!(
            card(event()),
            "Suggestion 2: add an event to the calendar \"Work\" [c1]\n  Title:     Review\n  \
             Starts:    2026-11-20T14:00:00 (Europe/Berlin)\n  Length:    1 h 30 min\n  Where:     Room 2\n  \
             Reminders: 10, 60 minutes before\n  Repeats:   no\n  Invited:   nobody\n  Notes:\n      Bring the\n      figures"
        );
        let task = EventInput {
            is_task: true,
            duration_secs: 0,
            time_zone: None,
            description: String::new(),
            alert_minutes: Vec::new(),
            ..event()
        };
        let text = card(task);
        assert!(text.starts_with("Suggestion 2: add a task to the calendar"), "{text}");
        assert!(text.contains("Due:       2026-11-20T14:00:00 (no zone: the same clock time everywhere)"), "{text}");
        assert!(text.contains("Length:    none (a task)") && text.contains("Reminders: none"), "{text}");
        assert!(text.ends_with("Notes:     none"), "{text}");
        let days = EventInput { all_day: true, duration_secs: 172_800, ..event() };
        assert!(card(days).contains("Length:    all day, 2 days"));
        // A repeat rule should never come from the chat; if one did, it would be seen.
        let rule = kweb_core::model::RepeatRule {
            frequency: kweb_core::model::Frequency::Weekly,
            interval: 1,
            by_day: Vec::new(),
            until: None,
            count: None,
        };
        assert!(card(EventInput { repeat: Some(rule), ..event() }).contains("Repeats:   RepeatRule"));
    }

    fn turn() -> AiChatTurn {
        AiChatTurn {
            text: "Two offers came.\n".into(),
            proposals: Vec::new(),
            links: Vec::new(),
            steps: 2,
            end: AiChatEnd::Answered,
            tools_off: false,
        }
    }

    #[test]
    fn an_answer_says_how_it_ended_and_lists_its_links() {
        assert_eq!(answer_text(&turn()), "Two offers came.\n");
        assert_eq!(answer_text(&AiChatTurn { text: " \n".into(), ..turn() }), "(no answer)");
        let limit = answer_text(&AiChatTurn { end: AiChatEnd::StepLimit, ..turn() });
        assert!(limit.contains("limit of 30 steps"), "{limit}");
        assert!(answer_text(&AiChatTurn { end: AiChatEnd::SizeLimit, ..turn() }).contains("may be incomplete"));
        assert!(answer_text(&AiChatTurn { tools_off: true, ..turn() }).contains("takes no tools"));
        let links = vec![
            AiLink::Message { message: message("m1", "Offer") },
            AiLink::ReplyDraft { message: message("m2", "Re: Offer"), text: "Dear Ada,\n\u{1b}[31mthank you.".into() },
        ];
        let text = answer_text(&AiChatTurn { links, ..turn() });
        assert!(text.contains("Links (nothing is opened by itself):\n  message [m1] Offer | from"), "{text}");
        assert!(text.ends_with("  reply draft to [m2] Re: Offer | from Ada <ada@example.com> | in Inbox:\n      Dear Ada,\n      [31mthank you."), "{text}");
    }

    #[test]
    fn a_step_is_one_line_without_control_characters() {
        let step = |tool, detail: &str| AiStep { tool, risk: AiRisk::Read, detail: detail.into() };
        assert_eq!(step_text(&step(AiToolKind::SearchMail, "offer\n2026\u{1b}[0m")), "  [search mail] offer2026[0m");
        assert_eq!(step_text(&step(AiToolKind::ListFolders, "")), "  [list folders]");
    }

    #[test]
    fn ctrl_c_is_taken_only_while_a_run_is_watched_and_only_once() {
        // The handler itself cannot be pressed from a test; this is the flag it raises.
        interrupt::watch(true);
        assert!(!interrupt::taken());
        interrupt::watch(false);
        assert!(!interrupt::taken());
    }
}
