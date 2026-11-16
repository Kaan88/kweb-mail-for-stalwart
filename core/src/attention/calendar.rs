//! The calendar rules: overlap, no place, open invitation, busy day, tight change.

use chrono::Duration;

use super::{
    limits::*, patterns, slots, AttentionFamily, AttentionRule, Finding, HomeTile, Input, PhoneAction, ServiceAction,
};
use crate::model::{MessageHeader, Occurrence};

fn key(e: &Occurrence) -> String {
    format!("{}/{}/{}", e.event_id, e.recurrence_id.as_deref().unwrap_or(""), e.starts_at)
}

/// Whether a mail's subject is about an event of this title.
pub fn title_matches(title: &str, subject: &str) -> bool {
    let (title, subject) =
        (patterns::bare_subject(title).to_lowercase(), patterns::bare_subject(subject).to_lowercase());
    if title.chars().count() >= 4 && subject.contains(&title) {
        return true;
    }
    let tw = patterns::significant_words(&title);
    if tw.is_empty() {
        return false;
    }
    let shared = tw.intersection(&patterns::significant_words(&subject)).count();
    shared >= tw.len().min(2)
}

fn looks_like_a_link(place: &str) -> bool {
    let low = place.to_lowercase();
    ["http", "zoom", "meet.", "teams", "video", "online", "skype", "webex"].iter().any(|w| low.contains(w))
}

pub fn rules(input: &Input<'_>, out: &mut Vec<Finding>) {
    let c = &input.clock;
    let now = c.now;
    let horizon = now + CALENDAR_DAYS * 86_400;
    let mut timed: Vec<&Occurrence> =
        input.entries.iter().filter(|e| slots::takes_time(e) && e.ends_at > now && e.starts_at < horizon).collect();
    timed.sort_by_key(|e| (e.starts_at, e.event_id.clone()));

    overlaps(input, &timed, out);
    no_place(input, &timed, out);
    invitations(input, out);
    busy_days(input, out);
    tight_changes(&timed, out);
}

fn overlaps(input: &Input<'_>, timed: &[&Occurrence], out: &mut Vec<Finding>) {
    for (i, a) in timed.iter().enumerate() {
        for b in &timed[i + 1..] {
            if b.starts_at >= a.ends_at {
                break;
            }
            if a.event_id == b.event_id {
                continue;
            }
            let mut f = Finding::new(
                AttentionRule::Overlap,
                AttentionFamily::Calendar,
                HomeTile::NextEvent,
                &format!("{}|{}", key(a), key(b)),
            );
            f.names = vec![a.title.clone(), b.title.clone()];
            f.times = vec![a.starts_at, b.starts_at];
            f.at = b.starts_at;
            f.event_ids = vec![a.event_id.clone(), b.event_id.clone()];
            // The later one is the one to move.
            f.phone = vec![
                PhoneAction::MoveEvent { event_id: b.event_id.clone(), recurrence_id: b.recurrence_id.clone() },
                PhoneAction::FindSlot { event_id: b.event_id.clone(), recurrence_id: b.recurrence_id.clone() },
            ];
            let busy = slots::busy(input.entries, Some((&b.event_id, b.starts_at)));
            f.slots = slots::free_slots(&input.clock, &busy, b.ends_at - b.starts_at, b.starts_at, 4);
            out.push(f);
        }
    }
}

fn no_place(input: &Input<'_>, timed: &[&Occurrence], out: &mut Vec<Finding>) {
    let now = input.clock.now;
    for e in timed.iter().filter(|e| e.starts_at >= now && e.starts_at < now + NO_PLACE_WITHIN_SECS) {
        if !e.location.trim().is_empty() {
            continue;
        }
        let mails: Vec<&MessageHeader> = input
            .recent_mail
            .iter()
            .filter(|m| now - m.received_at <= OPEN_MAILS_DAYS * 86_400 && title_matches(&e.title, &m.subject))
            .collect();
        let mut f = Finding::new(AttentionRule::NoPlace, AttentionFamily::Calendar, HomeTile::NextEvent, &key(e));
        f.names = vec![e.title.clone()];
        f.times = vec![e.starts_at];
        f.at = e.starts_at;
        f.count = mails.len() as u32;
        f.event_ids = vec![e.event_id.clone()];
        f.message_ids = mails.iter().map(|m| m.id.clone()).collect();
        if !mails.is_empty() {
            f.phone = vec![PhoneAction::OpenMails { message_ids: f.message_ids.clone() }];
        }
        f.service = Some(ServiceAction::LookInMail);
        out.push(f);
    }
}

fn invitations(input: &Input<'_>, out: &mut Vec<Finding>) {
    let now = input.clock.now;
    for e in input.entries.iter().filter(|e| {
        !e.is_task
            && e.my_status.as_deref() == Some("needs-action")
            && e.starts_at >= now
            && e.starts_at < now + INVITATION_DAYS * 86_400
    }) {
        // "Overlaps Standup": another event of the user that is not itself waiting for an answer.
        let clash = input.entries.iter().find(|o| {
            slots::takes_time(o)
                && slots::takes_time(e)
                && o.my_status.as_deref() != Some("needs-action")
                && !(o.event_id == e.event_id && o.starts_at == e.starts_at)
                && o.starts_at < e.ends_at
                && o.ends_at > e.starts_at
        });
        let mut f =
            Finding::new(AttentionRule::OpenInvitation, AttentionFamily::Calendar, HomeTile::NextEvent, &key(e));
        f.names = vec![e.title.clone(), clash.map(|o| o.title.clone()).unwrap_or_default()];
        f.times = vec![e.starts_at];
        f.at = e.starts_at;
        f.event_ids = vec![e.event_id.clone()];
        f.phone = vec![PhoneAction::Respond { event_id: e.event_id.clone(), recurrence_id: e.recurrence_id.clone() }];
        out.push(f);
    }
}

fn busy_days(input: &Input<'_>, out: &mut Vec<Finding>) {
    let c = &input.clock;
    let today = c.now_local().date();
    for k in 0..2i64 {
        let date = today + Duration::days(k);
        let start = c.ts(date.and_hms_opt(0, 0, 0).unwrap_or_default());
        let end = c.ts((date + Duration::days(1)).and_hms_opt(0, 0, 0).unwrap_or_default());
        // What is left of today; all of tomorrow.
        if k == 0 && c.now >= c.at_hour(start, 18) {
            continue;
        }
        let day: Vec<&Occurrence> = input
            .entries
            .iter()
            .filter(|e| slots::takes_time(e) && e.starts_at >= start && e.starts_at < end && e.ends_at > c.now)
            .collect();
        let busy = slots::busy(input.entries, None);
        let gaps = slots::gaps(c, &busy, start);
        let free: i64 = gaps.iter().map(|g| g.end - g.start).sum::<i64>() / 60;
        let crowded = day.len() >= BUSY_EVENTS;
        let squeezed = day.len() >= 2 && free < BUSY_FREE_MINUTES;
        if !crowded && !squeezed {
            continue;
        }
        let mut f =
            Finding::new(AttentionRule::BusyDay, AttentionFamily::Calendar, HomeTile::NextEvent, &date.to_string());
        f.times = vec![start];
        f.at = start;
        f.count = day.len() as u32;
        f.event_ids = day.iter().map(|e| e.event_id.clone()).collect();
        f.slots = gaps;
        f.phone = vec![PhoneAction::FreeTime];
        f.service = Some(ServiceAction::PlanMyDay);
        out.push(f);
    }
}

fn tight_changes(timed: &[&Occurrence], out: &mut Vec<Finding>) {
    for (i, a) in timed.iter().enumerate() {
        for b in &timed[i + 1..] {
            let gap = b.starts_at - a.ends_at;
            if gap >= TIGHT_CHANGE_MINUTES * 60 {
                break;
            }
            if gap < 0 || a.event_id == b.event_id {
                continue;
            }
            let (pa, pb) = (a.location.trim(), b.location.trim());
            if pa.is_empty()
                || pb.is_empty()
                || pa.eq_ignore_ascii_case(pb)
                || looks_like_a_link(pa)
                || looks_like_a_link(pb)
            {
                continue;
            }
            let mut f = Finding::new(
                AttentionRule::TightChange,
                AttentionFamily::Calendar,
                HomeTile::NextEvent,
                &format!("{}|{}", key(a), key(b)),
            );
            f.names = vec![a.title.clone(), b.title.clone(), pa.to_string(), pb.to_string()];
            f.times = vec![a.starts_at, b.starts_at];
            f.at = b.starts_at;
            f.event_ids = vec![a.event_id.clone(), b.event_id.clone()];
            f.phone = vec![PhoneAction::Directions { from: pa.to_string(), to: pb.to_string() }];
            out.push(f);
        }
    }
}
