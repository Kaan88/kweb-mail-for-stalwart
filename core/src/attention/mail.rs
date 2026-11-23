//! The mail rules: a date in mail, deadline words, an invitation file, a mail waiting for an answer.
//! Mail from a person only (the address is no no-reply or notification sender), never encrypted mail.

use std::collections::HashSet;

use super::{
    address_is_a_person, from_a_person, limits::*, patterns, AttentionFamily, AttentionRule, Finding, HomeTile, Input,
    PhoneAction, Prefill, ServiceAction,
};
use crate::model::MessageHeader;

fn sender(m: &MessageHeader) -> String {
    if m.from_name.trim().is_empty() {
        m.from_email.clone()
    } else {
        m.from_name.clone()
    }
}

/// Mail whose text is not in the cache: nothing to read.
fn is_encrypted(m: &MessageHeader) -> bool {
    m.preview.trim_start().starts_with("-----BEGIN PGP") || m.preview.contains("-----BEGIN PGP MESSAGE")
}

fn prefill(m: &MessageHeader, found: Option<patterns::Found>, is_task: bool) -> Prefill {
    let (start, all_day) = match found {
        Some(f) => match f.time {
            Some(t) => (f.date.and_time(t).format("%Y-%m-%dT%H:%M:%S").to_string(), false),
            None => (f.date.and_hms_opt(0, 0, 0).unwrap_or_default().format("%Y-%m-%dT%H:%M:%S").to_string(), true),
        },
        None => (String::new(), true),
    };
    Prefill { title: patterns::bare_subject(&m.subject).to_string(), start, all_day, is_task, duration_minutes: 0 }
}

fn looks_like_an_invitation_file(m: &MessageHeader) -> bool {
    if !m.has_attachment {
        return false;
    }
    let s = m.subject.to_lowercase();
    s.starts_with("invitation:")
        || s.starts_with("updated invitation")
        || s.starts_with("davet:")
        || s.starts_with("davetiye")
        || s.contains(".ics")
        || m.preview.to_lowercase().contains(".ics")
}

pub fn rules(input: &Input<'_>, out: &mut Vec<Finding>) {
    let c = &input.clock;
    let now_local = c.now_local();
    for m in input.mail {
        if c.now - m.received_at > MAIL_DAYS * 86_400 || !from_a_person(m) || is_encrypted(m) {
            continue;
        }
        if input.own_addresses.contains(&m.from_email.to_lowercase()) {
            continue;
        }
        let text = format!("{} {}", m.subject, m.preview);
        let base = |rule: AttentionRule| {
            let mut f = Finding::new(rule, AttentionFamily::Mail, HomeTile::Unread, &m.id);
            f.names = vec![sender(m), m.subject.clone()];
            f.at = m.received_at;
            f.times = vec![m.received_at];
            f.message_ids = vec![m.id.clone()];
            f.thread_id = Some(m.thread_id.clone());
            f
        };
        let open = PhoneAction::OpenMail { message_id: m.id.clone(), thread_id: m.thread_id.clone() };

        // The date: the subject first, then the preview.
        let found = patterns::find_future_date(&m.subject, now_local)
            .or_else(|| patterns::find_future_date(&m.preview, now_local));
        let deadline = patterns::has_deadline_word(&text) && (found.is_some() || patterns::has_amount(&text));

        if looks_like_an_invitation_file(m) {
            let mut f = base(AttentionRule::InvitationFile);
            f.phone = vec![open.clone()];
            out.push(f);
        } else if deadline {
            // One finding for a mail: a deadline asks for a task.
            let mut f = base(AttentionRule::DeadlineWords);
            f.times = vec![found.map(|d| c.ts(d.date.and_time(d.time.unwrap_or_default()))).unwrap_or(m.received_at)];
            f.phone = vec![PhoneAction::AddTask { prefill: prefill(m, found, true) }, open.clone()];
            f.service = Some(ServiceAction::AddDates);
            out.push(f);
        } else if let Some(d) = found {
            let mut f = base(AttentionRule::DateInMail);
            f.times = vec![c.ts(d.date.and_time(d.time.unwrap_or_default()))];
            f.phone = vec![PhoneAction::AddToCalendar { prefill: prefill(m, Some(d), false) }, open.clone()];
            f.service = Some(ServiceAction::AddDates);
            out.push(f);
        }

        // Waiting for an answer.
        let old_enough = c.now - m.received_at >= WAITING_DAYS * 86_400;
        let answered = input.my_last_in_thread.get(&m.thread_id).is_some_and(|mine| *mine > m.received_at);
        if old_enough && m.size < SHORT_MAIL_BYTES && !answered && patterns::asks_user(&text, input.first_names) {
            let mut f = base(AttentionRule::WaitingForYou);
            f.phone = vec![open];
            f.service = Some(ServiceAction::ReplyIdeas);
            out.push(f);
        }
    }
}

/// A mail the user sent 3 to 14 days ago that asks something, to at least one person, that nobody else has written
/// after in its conversation. Only the newest mail the user sent in a conversation counts (a
/// follow-up of their own starts the wait again). No encrypted mail; recipients that are the
/// user's own addresses or machines do not count as people.
pub fn follow_ups(input: &Input<'_>, out: &mut Vec<Finding>) {
    let c = &input.clock;
    let mut sent: Vec<&MessageHeader> =
        input.sent.iter().filter(|m| input.own_addresses.contains(&m.from_email.to_lowercase())).collect();
    sent.sort_by(|a, b| (b.received_at, &b.id).cmp(&(a.received_at, &a.id)));
    let mut threads = HashSet::new();
    for m in sent {
        if m.thread_id.is_empty() || !threads.insert(m.thread_id.as_str()) {
            continue;
        }
        let age = c.now - m.received_at;
        if !(FOLLOW_UP_MIN_DAYS * 86_400..=FOLLOW_UP_MAX_DAYS * 86_400).contains(&age) || is_encrypted(m) {
            continue;
        }
        // Only a mail that waits for an answer: one that asks nothing ("the notes are attached")
        // is not a missing reply, and a nudge for every mail sent would bury the rest.
        if !patterns::asks_reader(&format!("{} {}", m.subject, m.preview)) {
            continue;
        }
        let Some(to) =
            m.to.iter()
                .find(|a| address_is_a_person(&a.email) && !input.own_addresses.contains(&a.email.to_lowercase()))
        else {
            continue;
        };
        if input.last_other_in_thread.get(&m.thread_id).is_some_and(|other| *other > m.received_at) {
            continue;
        }
        let mut f = Finding::new(AttentionRule::NoReply, AttentionFamily::Mail, HomeTile::Unread, &m.id);
        f.names = vec![if to.name.trim().is_empty() { to.email.clone() } else { to.name.clone() }, m.subject.clone()];
        f.times = vec![m.received_at];
        f.count = (age / 86_400) as u32;
        f.at = m.received_at;
        f.message_ids = vec![m.id.clone()];
        f.thread_id = Some(m.thread_id.clone());
        // Opening the conversation needs no assistant. The reply-ideas action is for a mail someone
        // wrote to the user, not for one the user wrote, so it is not offered here.
        f.phone = vec![PhoneAction::OpenMail { message_id: m.id.clone(), thread_id: m.thread_id.clone() }];
        out.push(f);
    }
}
