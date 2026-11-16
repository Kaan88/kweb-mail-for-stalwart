//! The task rules: overdue, no time today, too many, a mail that may finish one.

use super::{
    limits::*, patterns, slots, AttentionFamily, AttentionRule, Finding, HomeTile, Input, PhoneAction, ServiceAction,
};
use crate::model::Occurrence;

fn first_titles(tasks: &[&Occurrence]) -> Vec<String> {
    tasks.iter().take(3).map(|t| t.title.clone()).collect()
}

fn ids_key(tasks: &[&Occurrence]) -> String {
    let mut ids: Vec<String> =
        tasks.iter().map(|t| format!("{}/{}", t.event_id, t.recurrence_id.as_deref().unwrap_or(""))).collect();
    ids.sort();
    ids.join(",")
}

pub fn rules(input: &Input<'_>, out: &mut Vec<Finding>) {
    let c = &input.clock;
    let today = c.day_start(c.now);
    let tomorrow = c.ts((c.local(today).date() + chrono::Duration::days(1)).and_hms_opt(0, 0, 0).unwrap_or_default());
    let mut open: Vec<&Occurrence> = input.entries.iter().filter(|e| e.is_task && !e.is_done).collect();
    open.sort_by_key(|t| (t.starts_at, t.event_id.clone()));

    let overdue: Vec<&Occurrence> = open.iter().copied().filter(|t| t.starts_at < today).collect();
    if !overdue.is_empty() {
        let mut f =
            Finding::new(AttentionRule::Overdue, AttentionFamily::Tasks, HomeTile::DueToday, &ids_key(&overdue));
        f.names = first_titles(&overdue);
        f.count = overdue.len() as u32;
        f.at = today - 1;
        f.times = vec![overdue[0].starts_at];
        f.task_ids = overdue.iter().map(|t| t.event_id.clone()).collect();
        f.phone = vec![PhoneAction::MoveToday { task_ids: f.task_ids.clone() }];
        f.service = Some(ServiceAction::PlanMyDay);
        out.push(f);
    }

    let due_today: Vec<&Occurrence> =
        open.iter().copied().filter(|t| t.starts_at >= today && t.starts_at < tomorrow).collect();

    // Tasks due today with no time of day, while there is free time left today.
    let untimed: Vec<&Occurrence> = due_today.iter().copied().filter(|t| t.all_day).collect();
    if !untimed.is_empty() && c.now < c.at_hour(today, 18) {
        let gaps = slots::gaps(c, &slots::busy(input.entries, None), today);
        if gaps.iter().any(|g| g.end - g.start >= 15 * 60) {
            let mut f = Finding::new(
                AttentionRule::NoTimeToday,
                AttentionFamily::Tasks,
                HomeTile::DueToday,
                &ids_key(&untimed),
            );
            f.names = first_titles(&untimed);
            f.count = untimed.len() as u32;
            f.at = today;
            f.task_ids = untimed.iter().map(|t| t.event_id.clone()).collect();
            f.slots = gaps;
            f.service = Some(ServiceAction::PlanMyDay);
            out.push(f);
        }
    }

    if due_today.len() > TOO_MANY_TASKS {
        let mut f =
            Finding::new(AttentionRule::TooMany, AttentionFamily::Tasks, HomeTile::DueToday, &today.to_string());
        f.names = first_titles(&due_today);
        f.count = due_today.len() as u32;
        f.at = today;
        f.task_ids = due_today.iter().map(|t| t.event_id.clone()).collect();
        f.service = Some(ServiceAction::PlanMyDay);
        out.push(f);
    }

    mail_may_finish(input, &open, out);
}

/// An open task that shares two significant words, a file name or a number with the subject of a
/// mail of the last days.
fn mail_may_finish(input: &Input<'_>, open: &[&Occurrence], out: &mut Vec<Finding>) {
    let c = &input.clock;
    for t in open.iter().filter(|t| t.starts_at < c.now + 14 * 86_400) {
        let words = patterns::significant_words(&t.title);
        let tokens = patterns::file_names_and_numbers(&t.title);
        let hit = input.mail.iter().filter(|m| c.now - m.received_at <= MAIL_DAYS * 86_400).find(|m| {
            let subject = patterns::bare_subject(&m.subject);
            let shared_words = patterns::shared_stems(&words, &patterns::significant_words(subject));
            shared_words >= 2 || !tokens.is_disjoint(&patterns::file_names_and_numbers(subject))
        });
        let Some(m) = hit else { continue };
        let mut f = Finding::new(
            AttentionRule::MailMayFinish,
            AttentionFamily::Tasks,
            HomeTile::DueToday,
            &format!("{}|{}", t.event_id, m.id),
        );
        f.names = vec![t.title.clone(), m.subject.clone()];
        f.times = vec![m.received_at];
        f.at = t.starts_at;
        f.task_ids = vec![t.event_id.clone()];
        f.message_ids = vec![m.id.clone()];
        f.thread_id = Some(m.thread_id.clone());
        f.phone = vec![
            PhoneAction::OpenMail { message_id: m.id.clone(), thread_id: m.thread_id.clone() },
            PhoneAction::MarkDone { task_id: t.event_id.clone() },
        ];
        f.service = Some(ServiceAction::CheckTheMail);
        out.push(f);
    }
}
