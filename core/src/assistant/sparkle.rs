//! The sparkle actions of the attention layer (M12 stage 8, phase 2), as far as they are pure: the
//! request of "Plan my day", the strict readers of the three new answers, the switches, and the
//! short read-only loop of "Look in mail".
//!
//! No network code, no `Store` and no `Core` in this file: the loop reads through the same
//! [`Reader`] as the chat and holds nothing that can write. Nothing here is acted on: a model's
//! answer is parsed strictly, and what does not fit is an error (a plain sentence), never an action.

use std::collections::HashMap;

use chrono::{NaiveDate, NaiveDateTime, TimeZone};
use chrono_tz::Tz;

use super::{
    chat::{self, Model},
    prompts,
    request::{self, Prepared},
    tools::{self, Env, Outcome, Reader, RESULT_CHARS},
    wire::{specs_chars, turn_chars, ToolCall, ToolResult, ToolSpec, ToolsRefused, Turn},
};
use crate::{
    attention::{slots, Clock, TimeSlot},
    model::{AiActionMode, AiSettings, AiSparkle, AiTask, AiVerdict, AiVerdictKind, Occurrence},
    Error, Result,
};

/// Provider requests of one "Look in mail" run, the last of them without tools.
pub const LOOK_STEPS: u32 = 6;
/// Calls taken from one turn.
pub const LOOK_CALLS_PER_STEP: usize = 4;
/// Characters sent in total by one "Look in mail" run.
pub const LOOK_RUN_CHARS: usize = 200_000;
/// The same call this often ends the lookups.
const REPEATS: usize = 2;
/// A framed result may be this much longer than [`RESULT_CHARS`].
const FRAME_SLACK: usize = 400;
/// The tools "Look in mail" may use: they read mail and nothing else.
pub const LOOK_TOOLS: [&str; 5] = ["search_mail", "read_message", "read_thread", "list_messages", "list_folders"];

/// The most planned blocks of "Plan my day".
pub const PLAN_BLOCKS: usize = 10;
/// The most events and tasks sent for "Plan my day".
const PLAN_EVENTS: usize = 40;
const PLAN_TASKS: usize = 30;
/// The longest title, place or verdict sentence read back from an answer (characters).
const TEXT_CHARS: usize = 300;

const UNREADABLE: &str = "the answer could not be read";

fn unreadable() -> Error {
    Error::temporary(UNREADABLE)
}

fn cancelled() -> Error {
    Error::invalid("cancelled")
}

/// Whether the user's switches let this run start: the assistant is on and agreed to for this
/// service, and the kind of action is on. Look in mail and Check the mail are the chat's kind of
/// work (they may read mail by themselves), so they also need the chat's switch and its own yes.
pub fn check_sparkle(s: &AiSettings, task: &AiSparkle) -> Result<()> {
    match task {
        AiSparkle::AddDates { .. } | AiSparkle::PlanMyDay { .. } => super::check_allowed(
            s,
            &AiTask::Extract { message_id: String::new(), today: String::new(), time_zone: String::new() },
        ),
        AiSparkle::ReplyIdeas { message_id } => {
            super::check_allowed(s, &AiTask::ReplyIdeas { message_id: message_id.clone() })
        }
        AiSparkle::LookInMail { .. } | AiSparkle::CheckTheMail { .. } => super::check_chat(s),
    }
}

/// Whether a card of a sparkle run may be applied under these settings: the assistant is on and
/// agreed to for this service, and changes are allowed ("ask first"). The kind of action was
/// checked when the run started.
pub fn check_apply(s: &AiSettings) -> Result<()> {
    if !s.enabled {
        return Err(Error::invalid("the assistant is switched off in the settings"));
    }
    if !s.consented {
        return Err(Error::invalid("you have not yet agreed to send mail to this service"));
    }
    if s.actions != AiActionMode::AskFirst {
        return Err(Error::invalid("actions are switched off in the settings"));
    }
    Ok(())
}

// ---- Plan my day: the request -----------------------------------------------------------------

/// What "Plan my day" sends and what it checks the answer against.
#[derive(Debug, Clone)]
pub struct PlanRequest {
    pub prepared: Prepared,
    pub day: NaiveDate,
    pub zone: Tz,
    /// The free times offered to the model (Unix seconds): a planned block must lie inside one.
    pub gaps: Vec<TimeSlot>,
    /// The titles of the open tasks that were sent (one line, unescaped): a block is a task of these.
    pub titles: Vec<String>,
    pub events: u32,
    pub tasks: u32,
}

fn clean(text: &str, max: usize) -> String {
    prompts::escape_head(prompts::flatten(text).trim(), max).0
}

fn hm(clock: &Clock, ts: i64) -> String {
    clock.local(ts).format("%H:%M").to_string()
}

/// The request for the day `day`: its events (title, time, place) and the open tasks due by then
/// (title, due), from the cache entries `entries`, and the free times between 09:00 and 18:00
/// (from now on for today). Err(Invalid) when there is nothing to plan or no room to plan in.
pub fn plan_request(
    clock: &Clock,
    today: &str,
    time_zone: &str,
    day: NaiveDate,
    entries: &[Occurrence],
) -> Result<PlanRequest> {
    let start = clock.ts(day.and_hms_opt(0, 0, 0).unwrap_or_default());
    let end = clock.ts((day + chrono::Duration::days(1)).and_hms_opt(0, 0, 0).unwrap_or_default());
    let mut events: Vec<&Occurrence> = entries
        .iter()
        .filter(|e| {
            !e.is_task
                && e.my_status.as_deref() != Some("declined")
                && e.starts_at < end
                && e.ends_at.max(e.starts_at + 1) > start
        })
        .collect();
    events.sort_by_key(|e| (e.starts_at, e.event_id.clone()));
    let mut tasks: Vec<&Occurrence> = entries.iter().filter(|t| t.is_task && !t.is_done && t.starts_at < end).collect();
    tasks.sort_by_key(|t| (t.starts_at, t.event_id.clone()));
    if tasks.is_empty() {
        return Err(Error::invalid("there are no open tasks to plan"));
    }
    let gaps = slots::gaps(clock, &slots::busy(entries, None), start);
    if gaps.is_empty() {
        return Err(Error::invalid("there is no free time that day"));
    }

    let mut body = String::from("Events that are already fixed:\n");
    if events.is_empty() {
        body.push_str("- none\n");
    }
    for e in events.iter().take(PLAN_EVENTS) {
        let when = if e.all_day {
            "all day".to_string()
        } else {
            format!("{} to {}", hm(clock, e.starts_at), hm(clock, e.ends_at))
        };
        let place = clean(&e.location, 200);
        body.push_str(&format!(
            "- {when} | {}{}\n",
            clean(&e.title, TEXT_CHARS),
            if place.is_empty() { String::new() } else { format!(" | {place}") }
        ));
    }
    if events.len() > PLAN_EVENTS {
        body.push_str(&format!("- {} more were left out\n", events.len() - PLAN_EVENTS));
    }
    body.push_str("Open tasks, earliest due first:\n");
    let mut titles = Vec::new();
    for t in tasks.iter().take(PLAN_TASKS) {
        let due_day = clock.local(t.starts_at).date();
        let due = if due_day < day {
            format!("overdue, was due {due_day}")
        } else if t.all_day {
            format!("due {due_day}, no time of day")
        } else {
            format!("due {due_day} {}", hm(clock, t.starts_at))
        };
        body.push_str(&format!("- {due} | {}\n", clean(&t.title, TEXT_CHARS)));
        titles.push(prompts::flatten(&t.title).trim().to_string());
    }
    if tasks.len() > PLAN_TASKS {
        body.push_str(&format!("- {} more were left out\n", tasks.len() - PLAN_TASKS));
    }
    body.push_str("Free times, the only places where something may be planned:\n");
    for g in &gaps {
        body.push_str(&format!("- {} to {}\n", hm(clock, g.start), hm(clock, g.end)));
    }
    let heading = format!("{}, {}", day.format("%Y-%m-%d"), day.format("%A"));
    let user = format!("{}\n\n<day>\n{body}</day>", prompts::plan_line(&heading, today, time_zone));
    let cut = events.len() > PLAN_EVENTS || tasks.len() > PLAN_TASKS;
    let (e, t) = (events.len().min(PLAN_EVENTS) as u32, tasks.len().min(PLAN_TASKS) as u32);
    Ok(PlanRequest {
        prepared: request::done(prompts::plan_system(), user, 0, cut),
        day,
        zone: clock.zone,
        gaps,
        titles,
        events: e,
        tasks: t,
    })
}

/// One block the model planned, checked: local `YYYY-MM-DDTHH:MM:00`, minutes, the task's title.
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub start: String,
    pub minutes: u32,
    pub title: String,
}

/// The blocks of a "Plan my day" answer. A line is `EVENT | YYYY-MM-DDTHH:MM | MINUTES | title`; one
/// that does not fit (another day, not inside a free time, overlapping an earlier block, a title that
/// is not a task that was sent, a length outside 15 to 240 minutes) is dropped. At most
/// [`PLAN_BLOCKS`]. The word NONE is an empty plan; an answer with no line that fits and no NONE is
/// an error (`Error::Temporary`, "the answer could not be read").
pub fn parse_plan(answer: &str, plan: &PlanRequest) -> Result<Vec<Block>> {
    let mut blocks: Vec<Block> = Vec::new();
    let mut none = false;
    for line in answer.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if is_none(line) {
            none = true;
            continue;
        }
        if blocks.len() == PLAN_BLOCKS {
            continue;
        }
        if let Some(block) = plan_line(line, plan) {
            let (s, e) = block_span(&block, plan);
            let overlaps = blocks.iter().any(|b| {
                let (bs, be) = block_span(b, plan);
                bs < e && be > s
            });
            if !overlaps {
                blocks.push(block);
            }
        }
    }
    if blocks.is_empty() && !none {
        return Err(unreadable());
    }
    Ok(blocks)
}

fn block_span(block: &Block, plan: &PlanRequest) -> (i64, i64) {
    let start = NaiveDateTime::parse_from_str(&block.start, "%Y-%m-%dT%H:%M:%S").ok();
    let start = start.and_then(|s| plan.zone.from_local_datetime(&s).earliest()).map_or(0, |d| d.timestamp());
    (start, start + i64::from(block.minutes) * 60)
}

fn plan_line(line: &str, plan: &PlanRequest) -> Option<Block> {
    let (_, line) = unmark(line);
    let mut parts = line.splitn(4, '|').map(str::trim);
    if !parts.next()?.eq_ignore_ascii_case("EVENT") {
        return None;
    }
    let (when, minutes, title) = (parts.next()?, parts.next()?, parts.next()?);
    let start = NaiveDateTime::parse_from_str(when, "%Y-%m-%dT%H:%M").ok()?;
    if start.date() != plan.day {
        return None;
    }
    let minutes: u32 = minutes.parse().ok().filter(|m| (15..=240).contains(m))?;
    let title = unescape(&prompts::flatten(title)).trim().to_string();
    if title.is_empty() || title.chars().count() > TEXT_CHARS || !plan.titles.iter().any(|t| same_text(t, &title)) {
        return None;
    }
    let block = Block { start: start.format("%Y-%m-%dT%H:%M:00").to_string(), minutes, title };
    let (s, e) = block_span(&block, plan);
    plan.gaps.iter().any(|g| g.start <= s && e <= g.end).then_some(block)
}

fn same_text(a: &str, b: &str) -> bool {
    a.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
        == b.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// The two characters the request escapes, written plain again (a model may echo them as sent).
fn unescape(text: &str) -> String {
    text.replace("&lt;", "<").replace("&gt;", ">")
}

fn is_none(line: &str) -> bool {
    line.trim().trim_end_matches('.').eq_ignore_ascii_case("NONE")
}

/// A line without a bullet or a number in front of it.
fn unmark(line: &str) -> (bool, &str) {
    let line = line.trim();
    let bullets = line.trim_start_matches(['-', '*', '\u{2022}', '\u{2013}', '\u{2014}', '\u{b7}']);
    if bullets.len() != line.len() {
        return (true, bullets.trim());
    }
    let digits = line.chars().take_while(char::is_ascii_digit).count();
    match line[digits..].strip_prefix(['.', ')']) {
        Some(rest) if (1..=2).contains(&digits) => (true, rest.trim()),
        _ => (false, line),
    }
}

// ---- Check the mail and Look in mail: the answers ---------------------------------------------

/// The verdict of a "Check the mail" answer: the first line `VERDICT | FINISHES|CHANGES|UNRELATED |
/// sentence`. Anything else is `Error::Temporary` ("the answer could not be read").
pub fn parse_verdict(answer: &str) -> Result<AiVerdict> {
    for line in answer.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let (_, line) = unmark(line);
        let mut parts = line.splitn(3, '|').map(str::trim);
        if !parts.next().is_some_and(|w| w.eq_ignore_ascii_case("VERDICT")) {
            continue;
        }
        let kind = match parts.next().map(str::to_ascii_uppercase).as_deref() {
            Some("FINISHES") => AiVerdictKind::Finishes,
            Some("CHANGES") => AiVerdictKind::Changes,
            Some("UNRELATED") => AiVerdictKind::Unrelated,
            _ => continue,
        };
        let reason: String = prompts::flatten(parts.next().unwrap_or("")).trim().chars().take(TEXT_CHARS).collect();
        return Ok(AiVerdict { kind, reason: unescape(&reason) });
    }
    Err(unreadable())
}

/// What "Look in mail" found: the message id and the place or link (one line, at most 300
/// characters), or `None` for NONE. Anything else is `Error::Temporary`. The caller checks that the
/// message is one of the account's own.
pub fn parse_found(answer: &str) -> Result<Option<(String, String)>> {
    let mut none = false;
    for line in answer.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if is_none(line) {
            none = true;
            continue;
        }
        let (_, line) = unmark(line);
        let mut parts = line.splitn(3, '|').map(str::trim);
        if !parts.next().is_some_and(|w| w.eq_ignore_ascii_case("FOUND")) {
            continue;
        }
        let id = parts.next().unwrap_or("");
        let text: String =
            unescape(&prompts::flatten(parts.next().unwrap_or(""))).trim().chars().take(TEXT_CHARS).collect();
        if id.is_empty() || id.chars().count() > 200 || id.chars().any(char::is_control) || text.trim().is_empty() {
            continue;
        }
        return Ok(Some((id.to_string(), text.trim().to_string())));
    }
    match none {
        true => Ok(None),
        false => Err(unreadable()),
    }
}

/// Whether an answer is only the word NONE (Add dates: nothing found).
pub fn is_nothing(answer: &str) -> bool {
    let lines: Vec<&str> = answer.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    !lines.is_empty() && lines.iter().all(|l| is_none(l))
}

// ---- Look in mail: the loop -------------------------------------------------------------------

/// What a "Look in mail" run came to.
#[derive(Debug)]
pub struct Looked {
    pub answer: String,
    pub steps: u32,
}

fn error_result(call: &ToolCall, sentence: &str) -> ToolResult {
    ToolResult { id: call.id.clone(), name: call.name.clone(), content: sentence.to_string(), is_error: true }
}

fn chars(text: &str) -> usize {
    text.chars().count()
}

/// The short read-only run: the model may ask for the tools of [`LOOK_TOOLS`] (at most
/// [`LOOK_CALLS_PER_STEP`] per turn, [`LOOK_STEPS`] requests, the last without tools) and then
/// answers. It holds a [`Reader`] and nothing that writes; a call of any other tool is answered
/// with a fixed sentence and not run. What a tool gives is data for the model and never an order to
/// this loop.
pub fn look(
    model: &mut dyn Model,
    reader: &dyn Reader,
    env: &Env<'_>,
    system: &str,
    user: &str,
    on_step: &mut dyn FnMut(&ToolCall),
) -> Result<Looked> {
    let offered: Vec<ToolSpec> =
        tools::specs(AiActionMode::Off).into_iter().filter(|s| LOOK_TOOLS.contains(&s.name)).collect();
    let mut turns = vec![Turn::User(user.to_string())];
    let mut asked: HashMap<(String, String), usize> = HashMap::new();
    let (mut sent, mut steps) = (0usize, 0u32);
    for request in 1..=LOOK_STEPS {
        // The last request is made without tools: the model has to answer with what it has.
        let specs: &[ToolSpec] = if request == LOOK_STEPS { &[] } else { &offered };
        let fixed = chars(system) + specs_chars(specs);
        if !chat::fit(&mut turns, fixed) {
            return Err(Error::temporary("the mail was too long to look through"));
        }
        let size = fixed + turns.iter().map(turn_chars).sum::<usize>();
        if sent + size > LOOK_RUN_CHARS {
            return Err(Error::temporary("the mail was too long to look through"));
        }
        if model.cancelled() {
            return Err(cancelled());
        }
        sent += size;
        let (reply, refused) = model.turn(system, &turns, specs, &mut |_| {})?;
        if refused == ToolsRefused::Yes {
            return Err(Error::invalid("this model takes no tools, so it cannot look through your mail"));
        }
        let calls = if specs.is_empty() { Vec::new() } else { reply.calls.clone() };
        if calls.is_empty() {
            if reply.text.trim().is_empty() {
                return Err(Error::temporary("the service sent an empty answer"));
            }
            return Ok(Looked { answer: reply.text, steps });
        }
        let mut results = Vec::with_capacity(calls.len());
        for (at, call) in calls.iter().enumerate() {
            let times = asked.entry((call.name.clone(), call.args.to_string())).or_default();
            *times += 1;
            if at >= LOOK_CALLS_PER_STEP {
                results.push(error_result(call, "too many calls at once"));
                continue;
            }
            if *times > REPEATS {
                results.push(error_result(call, "this was asked already; nothing more is looked up for it"));
                continue;
            }
            if !LOOK_TOOLS.contains(&call.name.as_str()) {
                results.push(error_result(call, "that tool is not available here"));
                continue;
            }
            if model.cancelled() {
                return Err(cancelled());
            }
            on_step(call);
            let ran = tools::run(reader, env, call)?;
            steps += 1;
            let (content, is_error) = match ran.outcome {
                Outcome::Text(text) => (text, false),
                Outcome::Error(sentence) => (sentence, true),
                // The tools of the list read; nothing else can come out of them.
                Outcome::Link(..) | Outcome::Proposal(..) => ("that tool is not available here".to_string(), true),
            };
            let (content, is_error) = match chars(&content) > RESULT_CHARS + FRAME_SLACK {
                true => ("the result was too long and was left out".to_string(), true),
                false => (content, is_error),
            };
            results.push(ToolResult { id: call.id.clone(), name: call.name.clone(), content, is_error });
        }
        turns.push(Turn::Assistant { text: reply.text, calls, raw: reply.raw });
        turns.push(Turn::Results(results));
    }
    // Not reached: the last request has no tools, so it ends the loop above.
    Err(Error::temporary("the service did not answer"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::AiSettings;

    fn base() -> AiSettings {
        AiSettings {
            enabled: false,
            service_id: String::new(),
            service_name: String::new(),
            base_url: String::new(),
            model: String::new(),
            effort: crate::model::AiEffort::Default,
            has_key: false,
            consented: false,
            allow_summaries: false,
            allow_drafts: false,
            allow_rewrites: false,
            allow_ideas: false,
            allow_extract: false,
            allow_explain: false,
            allow_chat: false,
            chat_consented: false,
            actions: AiActionMode::AskFirst,
            tools_unsupported: false,
            instructions: String::new(),
            services: Vec::new(),
        }
    }

    fn plan(titles: &[&str], gaps: &[(i64, i64)]) -> PlanRequest {
        PlanRequest {
            prepared: request::done(String::new(), String::new(), 0, false),
            day: NaiveDate::from_ymd_opt(2026, 11, 17).unwrap(),
            zone: chrono_tz::UTC,
            gaps: gaps.iter().map(|&(start, end)| TimeSlot { start, end }).collect(),
            titles: titles.iter().map(|t| t.to_string()).collect(),
            events: 0,
            tasks: titles.len() as u32,
        }
    }

    /// 2026-11-17T00:00:00Z.
    const DAY: i64 = 1_794_873_600;

    fn h(hours: f64) -> i64 {
        DAY + (hours * 3600.0) as i64
    }

    #[test]
    fn a_plan_keeps_only_blocks_that_fit_and_are_of_listed_tasks() {
        let p = plan(&["Send invoice", "Call <Dan>"], &[(h(10.5), h(12.0)), (h(14.0), h(15.0))]);
        let answer = "\
EVENT | 2026-11-17T10:30 | 45 | Send invoice
- EVENT | 2026-11-17T11:15 | 30 | Call <Dan>
EVENT | 2026-11-17T11:30 | 30 | Send invoice
EVENT | 2026-11-17T09:00 | 30 | Send invoice
EVENT | 2026-11-18T10:30 | 30 | Send invoice
EVENT | 2026-11-17T14:00 | 90 | Send invoice
EVENT | 2026-11-17T14:00 | 30 | Something else
EVENT | 2026-11-17T14:00 | 10 | Send invoice
EVENT | 2026-11-17T14:00 | x | Send invoice
Here is more text
";
        let blocks = parse_plan(answer, &p).unwrap();
        assert_eq!(
            blocks,
            vec![
                Block { start: "2026-11-17T10:30:00".into(), minutes: 45, title: "Send invoice".into() },
                Block { start: "2026-11-17T11:15:00".into(), minutes: 30, title: "Call <Dan>".into() },
            ]
        );
    }

    #[test]
    fn a_plan_that_says_none_is_empty_and_one_that_says_nothing_usable_is_an_error() {
        let p = plan(&["Send invoice"], &[(h(10.0), h(12.0))]);
        assert!(parse_plan("NONE", &p).unwrap().is_empty());
        assert!(parse_plan("none.\n", &p).unwrap().is_empty());
        assert!(matches!(parse_plan("I would start with the invoice.", &p), Err(Error::Temporary { .. })));
        assert!(matches!(parse_plan("", &p), Err(Error::Temporary { .. })));
        // Every line is outside the free time: nothing usable.
        assert!(matches!(parse_plan("EVENT | 2026-11-17T13:00 | 30 | Send invoice", &p), Err(Error::Temporary { .. })));
    }

    #[test]
    fn a_verdict_is_one_of_three_and_anything_else_is_an_error() {
        let v = parse_verdict("VERDICT | finishes | Dan says the contract is signed.").unwrap();
        assert_eq!((v.kind, v.reason.as_str()), (AiVerdictKind::Finishes, "Dan says the contract is signed."));
        assert_eq!(parse_verdict("- VERDICT | CHANGES | New date").unwrap().kind, AiVerdictKind::Changes);
        assert_eq!(parse_verdict("VERDICT | UNRELATED |").unwrap().reason, "");
        for bad in ["", "FINISHES", "VERDICT | MAYBE | x", "It finishes it.", "VERDICT"] {
            assert!(matches!(parse_verdict(bad), Err(Error::Temporary { .. })), "{bad}");
        }
    }

    #[test]
    fn found_is_an_id_and_a_one_line_text() {
        assert_eq!(
            parse_found("FOUND | m42 | https://meet.example.test/x?a=1|2").unwrap(),
            Some(("m42".to_string(), "https://meet.example.test/x?a=1|2".to_string()))
        );
        assert_eq!(parse_found("NONE").unwrap(), None);
        assert_eq!(parse_found("none.").unwrap(), None);
        for bad in ["", "FOUND | | Room 2", "FOUND | m42 |", "Room 2 is where it is.", "FOUND m42 Room 2"] {
            assert!(matches!(parse_found(bad), Err(Error::Temporary { .. })), "{bad}");
        }
        let long = format!("FOUND | m1 | {}", "x".repeat(900));
        assert_eq!(parse_found(&long).unwrap().unwrap().1.chars().count(), TEXT_CHARS);
    }

    #[test]
    fn the_switches_follow_the_kind_of_action() {
        let on = AiSettings {
            enabled: true,
            consented: true,
            allow_extract: true,
            allow_ideas: true,
            allow_chat: true,
            chat_consented: true,
            ..base()
        };
        let dates = AiSparkle::AddDates { message_ids: vec![], today: String::new(), time_zone: String::new() };
        let look = AiSparkle::LookInMail {
            event_id: "e".into(),
            recurrence_id: None,
            today: String::new(),
            time_zone: String::new(),
        };
        let ideas = AiSparkle::ReplyIdeas { message_id: "m".into() };
        assert!(
            check_sparkle(&on, &dates).is_ok()
                && check_sparkle(&on, &look).is_ok()
                && check_sparkle(&on, &ideas).is_ok()
        );
        let no_extract = AiSettings { allow_extract: false, ..on.clone() };
        assert!(check_sparkle(&no_extract, &dates).is_err() && check_sparkle(&no_extract, &look).is_ok());
        let no_chat_yes = AiSettings { chat_consented: false, ..on.clone() };
        assert!(check_sparkle(&no_chat_yes, &look).is_err() && check_sparkle(&no_chat_yes, &dates).is_ok());
        let no_ideas = AiSettings { allow_ideas: false, ..on.clone() };
        assert!(check_sparkle(&no_ideas, &ideas).is_err());
        let not_agreed = AiSettings { consented: false, ..on };
        assert!(check_sparkle(&not_agreed, &dates).is_err() && check_sparkle(&not_agreed, &look).is_err());
    }
}
