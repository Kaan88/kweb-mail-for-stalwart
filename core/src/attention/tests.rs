use std::collections::{HashMap, HashSet};

use chrono::{NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;

use super::*;
use crate::model::{MessageHeader, Occurrence};

/// Tuesday 17 November 2026, 09:40 UTC.
fn now() -> i64 {
    Utc.with_ymd_and_hms(2026, 11, 17, 9, 40, 0).unwrap().timestamp()
}

fn at(day: u32, h: u32, m: u32) -> i64 {
    Utc.with_ymd_and_hms(2026, 11, day, h, m, 0).unwrap().timestamp()
}

fn event(id: &str, title: &str, start: i64, minutes: i64, place: &str) -> Occurrence {
    Occurrence {
        event_id: id.into(),
        recurrence_id: None,
        calendar_id: "c1".into(),
        title: title.into(),
        location: place.into(),
        starts_at: start,
        ends_at: start + minutes * 60,
        all_day: false,
        is_task: false,
        is_done: false,
        is_recurring: false,
        my_status: None,
    }
}

fn task(id: &str, title: &str, due: i64, all_day: bool) -> Occurrence {
    Occurrence { is_task: true, all_day, ends_at: due, ..event(id, title, due, 0, "") }
}

fn mail(id: &str, from: &str, subject: &str, preview: &str, received: i64) -> MessageHeader {
    MessageHeader {
        id: id.into(),
        thread_id: format!("t-{id}"),
        mailbox_ids: vec!["inbox".into()],
        from_name: from.split('@').next().unwrap_or("").into(),
        from_email: from.into(),
        to: vec![],
        subject: subject.into(),
        preview: preview.into(),
        received_at: received,
        is_unread: true,
        is_flagged: false,
        has_attachment: false,
        size: 1200,
    }
}

#[derive(Default)]
struct World {
    entries: Vec<Occurrence>,
    mail: Vec<MessageHeader>,
    mine: HashMap<String, i64>,
    sent: Vec<MessageHeader>,
    others: HashMap<String, i64>,
    settings: AttentionSettings,
}

impl World {
    fn run(&self) -> Vec<Finding> {
        let own: HashSet<String> = ["kaan@example.org".to_string()].into();
        let names = vec!["Kaan".to_string()];
        evaluate(&Input {
            clock: Clock { now: now(), zone: Tz::UTC },
            entries: &self.entries,
            mail: &self.mail,
            recent_mail: &self.mail,
            first_names: &names,
            own_addresses: &own,
            my_last_in_thread: &self.mine,
            sent: &self.sent,
            last_other_in_thread: &self.others,
            settings: &self.settings,
        })
    }

    fn rules(&self) -> Vec<AttentionRule> {
        self.run().into_iter().map(|f| f.rule).collect()
    }

    fn one(&self, rule: AttentionRule) -> Finding {
        self.run().into_iter().find(|f| f.rule == rule).unwrap_or_else(|| panic!("no {rule:?} in {:?}", self.rules()))
    }
}

fn world(entries: Vec<Occurrence>) -> World {
    World { entries, settings: AttentionSettings::default(), ..Default::default() }
}

#[test]
fn a_quiet_day_has_no_findings() {
    let w =
        world(vec![event("a", "Standup", at(17, 11, 0), 30, "Room 2"), event("b", "Lunch", at(17, 12, 0), 60, "Cafe")]);
    assert!(w.run().is_empty());
}

#[test]
fn overlap_names_the_later_event_and_offers_slots() {
    let w = world(vec![
        event("a", "Dentist", at(19, 14, 0), 45, "Dr. Aksoy"),
        event("b", "Standup", at(19, 14, 0), 30, "Room 2"),
    ]);
    let f = w.one(AttentionRule::Overlap);
    assert_eq!(f.names.len(), 2);
    assert!(matches!(&f.phone[0], PhoneAction::MoveEvent { .. }));
    assert!(!f.slots.is_empty() && f.slots.len() <= 4);
    // Every chip is free time of the same length, inside 08:00 to 19:00, and not at 14:00.
    for s in &f.slots {
        assert_eq!(s.end - s.start, 30 * 60);
        assert!(s.start > now());
        let hour = Clock { now: now(), zone: Tz::UTC }.local(s.start).time().format("%H:%M").to_string();
        assert!(hour.as_str() >= "08:00" && hour.as_str() <= "18:30", "{hour}");
        assert!(!(s.start < at(19, 14, 45) && s.end > at(19, 14, 0)), "chip overlaps a busy event");
    }
}

#[test]
fn declined_all_day_and_task_entries_do_not_overlap() {
    let mut declined = event("b", "Declined", at(19, 14, 0), 30, "");
    declined.my_status = Some("declined".into());
    let mut allday = event("c", "Holiday", at(19, 0, 0), 24 * 60, "");
    allday.all_day = true;
    let w = world(vec![
        event("a", "Dentist", at(19, 14, 0), 45, "x"),
        declined,
        allday,
        task("t", "Pay", at(19, 14, 0), false),
    ]);
    assert!(!w.rules().contains(&AttentionRule::Overlap));
}

#[test]
fn back_to_back_events_do_not_overlap() {
    let w = world(vec![event("a", "One", at(19, 14, 0), 30, "x"), event("b", "Two", at(19, 14, 30), 30, "x")]);
    assert!(!w.rules().contains(&AttentionRule::Overlap));
}

#[test]
fn no_place_within_two_hours_and_the_mails_that_mention_it() {
    let mut w = world(vec![event("r", "Release review", at(17, 11, 0), 60, "")]);
    w.mail = vec![mail("m1", "mira@example.com", "Release plan: review notes", "", at(12, 9, 0))];
    // Alone, a mail that shares both words gives the phone action; none shares a single word of a long title.
    let f = w.one(AttentionRule::NoPlace);
    assert_eq!(f.count, 1);
    assert!(matches!(&f.phone[0], PhoneAction::OpenMails { message_ids } if message_ids == &vec!["m1".to_string()]));
    assert_eq!(f.service, Some(ServiceAction::LookInMail));
    // Far away, or with a place, nothing.
    assert!(!world(vec![event("r", "Release review", at(17, 14, 0), 60, "")])
        .rules()
        .contains(&AttentionRule::NoPlace));
    assert!(!world(vec![event("r", "Release review", at(17, 11, 0), 60, "Room 1")])
        .rules()
        .contains(&AttentionRule::NoPlace));
}

#[test]
fn an_unanswered_invitation_says_what_it_overlaps() {
    let mut inv = event("i", "Workshop", at(19, 10, 0), 60, "Hall");
    inv.my_status = Some("needs-action".into());
    let mut accepted = event("s", "Standup", at(19, 10, 0), 30, "Room 2");
    accepted.my_status = Some("accepted".into());
    let w = world(vec![inv.clone(), accepted]);
    let f = w.one(AttentionRule::OpenInvitation);
    assert_eq!(f.names, vec!["Workshop".to_string(), "Standup".to_string()]);
    // Free then:
    let f = world(vec![inv.clone()]).one(AttentionRule::OpenInvitation);
    assert_eq!(f.names[1], "");
    // An answered one, one in the past and one too far ahead are not findings.
    inv.my_status = Some("accepted".into());
    assert!(world(vec![inv.clone()]).run().is_empty());
    let mut far = event("i", "Later", now() + 15 * 86_400, 60, "Hall");
    far.my_status = Some("needs-action".into());
    assert!(world(vec![far]).run().is_empty());
}

#[test]
fn a_busy_day_by_count_or_by_no_free_time() {
    let mut e = Vec::new();
    for i in 0..5 {
        e.push(event(&format!("e{i}"), &format!("Meeting {i}"), at(18, 9 + i, 0), 30, "Room"));
    }
    let f = world(e).one(AttentionRule::BusyDay);
    assert_eq!(f.count, 5);
    assert_eq!(f.service, Some(ServiceAction::PlanMyDay));
    assert!(!f.slots.is_empty());
    // Under 30 minutes free between 09:00 and 18:00, with events.
    let w = world(vec![
        event("a", "A", at(18, 9, 0), 4 * 60 + 40, "x"),
        event("b", "B", at(18, 13, 50), 4 * 60 + 10 - 10, "y"),
    ]);
    assert!(w.rules().contains(&AttentionRule::BusyDay), "{:?}", w.rules());
    // Two ordinary events do not make a busy day.
    assert!(!world(vec![event("a", "A", at(18, 9, 0), 60, "x"), event("b", "B", at(18, 11, 0), 60, "x")])
        .rules()
        .contains(&AttentionRule::BusyDay));
}

#[test]
fn a_tight_change_needs_two_different_physical_places() {
    let w = world(vec![
        event("a", "Dentist", at(19, 14, 0), 45, "Dr. Aksoy, Bahnhofstr. 4"),
        event("b", "Lunch", at(19, 14, 55), 60, "Cafe Nord"),
    ]);
    let f = w.one(AttentionRule::TightChange);
    assert!(matches!(&f.phone[0], PhoneAction::Directions { from, to } if from.contains("Aksoy") && to == "Cafe Nord"));
    // 15 minutes or more apart, the same place, or a link: nothing.
    for (gap, p1, p2) in [(15, "A", "B"), (5, "Cafe", "cafe"), (5, "Room", "https://meet.example.org/x"), (5, "", "B")]
    {
        let w = world(vec![
            event("a", "A", at(19, 14, 0), 45, p1),
            event("b", "B", at(19, 14, 0) + (45 + gap) * 60, 60, p2),
        ]);
        assert!(!w.rules().contains(&AttentionRule::TightChange), "{gap} {p1} {p2}");
    }
}

#[test]
fn overdue_tasks_before_today_and_the_others_today() {
    let mut w = world(vec![
        task("t1", "Send invoice 0311", at(16, 0, 0), true),
        task("t2", "Renew the domain", at(15, 0, 0), true),
        task("t3", "Pay rent", at(17, 0, 0), true),
        task("t4", "Book a table", at(17, 18, 0), false),
        task("t5", "Done already", at(14, 0, 0), true),
    ]);
    w.entries[4].is_done = true;
    let f = w.one(AttentionRule::Overdue);
    assert_eq!(f.count, 2);
    assert!(matches!(&f.phone[0], PhoneAction::MoveToday { task_ids } if task_ids.len() == 2));
    let f = w.one(AttentionRule::NoTimeToday);
    assert_eq!(f.count, 1); // Pay rent only: Book a table has a time
    assert_eq!(f.service, Some(ServiceAction::PlanMyDay));
    assert!(!w.rules().contains(&AttentionRule::TooMany));
}

#[test]
fn too_many_tasks_today() {
    let tasks = (0..6).map(|i| task(&format!("t{i}"), &format!("Task {i}"), at(17, 12 + i, 0), false)).collect();
    let f = world(tasks).one(AttentionRule::TooMany);
    assert_eq!(f.count, 6);
    // Five are not too many.
    let five = (0..5).map(|i| task(&format!("t{i}"), &format!("Task {i}"), at(17, 12 + i, 0), false)).collect();
    assert!(!world(five).rules().contains(&AttentionRule::TooMany));
}

#[test]
fn a_mail_may_finish_a_task() {
    let mut w = world(vec![task("t", "Sign Contract-v3", at(17, 0, 0), true)]);
    w.mail = vec![mail("m", "dan@example.com", "Contract-v3 signed copy", "", at(16, 18, 2))];
    let f = w.one(AttentionRule::MailMayFinish);
    assert_eq!(f.service, Some(ServiceAction::CheckTheMail));
    assert!(matches!(&f.phone[1], PhoneAction::MarkDone { task_id } if task_id == "t"));
    // A number alone links them too, a single common word does not.
    let mut w = world(vec![task("t", "Pay invoice 0311", at(17, 0, 0), true)]);
    w.mail = vec![mail("m", "x@example.com", "Re: Invoice 0311", "", at(16, 9, 0))];
    assert!(w.rules().contains(&AttentionRule::MailMayFinish));
    w.mail = vec![mail("m", "x@example.com", "Invoice for the garden", "", at(16, 9, 0))];
    assert!(!w.rules().contains(&AttentionRule::MailMayFinish));
}

#[test]
fn a_date_in_mail_english_and_turkish() {
    let mut w = world(vec![]);
    w.mail = vec![
        mail("m1", "merve@example.com", "Call this week?", "Could we talk Thu 19:30 about it", at(16, 9, 0)),
        mail("m2", "can@example.com", "Toplantı", "Cuma 19:30 olur mu", at(16, 9, 0)),
    ];
    let found = w.run();
    assert_eq!(found.len(), 2);
    assert!(found.iter().all(|f| f.rule == AttentionRule::DateInMail && f.service == Some(ServiceAction::AddDates)));
    let PhoneAction::AddToCalendar { prefill } = &found.iter().find(|f| f.message_ids[0] == "m2").unwrap().phone[0]
    else {
        panic!()
    };
    assert_eq!(prefill.start, "2026-11-20T19:30:00");
    assert!(!prefill.all_day);
}

#[test]
fn deadline_words_give_a_task_and_replace_the_date_finding() {
    let mut w = world(vec![]);
    w.mail = vec![mail("m", "billing@hetzner.example", "Invoice 1042, due 30 Nov", "", at(16, 9, 0))];
    let found = w.run();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].rule, AttentionRule::DeadlineWords);
    let PhoneAction::AddTask { prefill } = &found[0].phone[0] else { panic!() };
    assert_eq!((prefill.start.as_str(), prefill.all_day, prefill.is_task), ("2026-11-30T00:00:00", true, true));
    // An amount with the word is enough; the word alone is not.
    w.mail = vec![mail("m", "billing@x.example", "Payment reminder", "Please pay 120 EUR", at(16, 9, 0))];
    assert_eq!(w.rules(), vec![AttentionRule::DeadlineWords]);
    w.mail = vec![mail("m", "billing@x.example", "Payment received, thanks", "", at(16, 9, 0))];
    assert!(w.run().is_empty());
}

#[test]
fn machines_old_mail_and_encrypted_mail_are_left_alone() {
    let mut w = world(vec![]);
    w.mail = vec![
        mail("a", "no-reply@shop.example", "Your ticket for Fri 19:30", "", at(16, 9, 0)),
        mail("b", "dan@example.com", "Fri 19:30", "", at(5, 9, 0)),
        mail("c", "dan@example.com", "Fri 19:30", "-----BEGIN PGP MESSAGE-----", at(16, 9, 0)),
        mail("d", "kaan@example.org", "Fri 19:30", "", at(16, 9, 0)),
    ];
    assert!(w.run().is_empty());
    // Switched off by family.
    w.mail = vec![mail("a", "dan@example.com", "Fri 19:30", "", at(16, 9, 0))];
    assert_eq!(w.run().len(), 1);
    w.settings.mail = false;
    assert!(w.run().is_empty());
}

#[test]
fn waiting_for_you_needs_a_question_age_and_no_answer() {
    let mut w = world(vec![]);
    let old = at(14, 9, 0);
    w.mail = vec![mail("m", "ayse@example.com", "Menu", "Can you send the menu choices before Friday?", old)];
    let f = w.one(AttentionRule::WaitingForYou);
    assert_eq!(f.service, Some(ServiceAction::ReplyIdeas));
    // The same day it came: too soon. Answered later: gone. A long mail: no.
    let w2 =
        World { mail: vec![mail("m", "ayse@example.com", "Menu", "Can you send it?", now() - 3600)], ..world(vec![]) };
    assert!(!w2.rules().contains(&AttentionRule::WaitingForYou));
    w.mine.insert("t-m".into(), old + 3600);
    assert!(!w.rules().contains(&AttentionRule::WaitingForYou));
    w.mine.clear();
    w.mail[0].size = 50_000;
    assert!(!w.rules().contains(&AttentionRule::WaitingForYou));
    // The first name before "can you" is enough, in either language.
    w.mail = vec![mail("m", "ayse@example.com", "Menu", "Kaan, can you send the menu", old)];
    assert!(w.rules().contains(&AttentionRule::WaitingForYou));
}

#[test]
fn an_invitation_file_by_its_cues() {
    let mut w = world(vec![]);
    let mut m = mail("m", "org@example.com", "Invitation: Workshop @ Thu", "", at(16, 9, 0));
    m.has_attachment = true;
    w.mail = vec![m];
    assert_eq!(w.rules(), vec![AttentionRule::InvitationFile]);
}

#[test]
fn ids_are_stable_and_follow_the_data() {
    let a = world(vec![event("a", "A", at(19, 14, 0), 45, "x"), event("b", "B", at(19, 14, 0), 30, "y")]);
    let again = world(vec![event("b", "B", at(19, 14, 0), 30, "y"), event("a", "A", at(19, 14, 0), 45, "x")]);
    assert_eq!(a.one(AttentionRule::Overlap).id, again.one(AttentionRule::Overlap).id);
    // The event moved: another finding (or none), never the old id.
    let moved = world(vec![event("a", "A", at(19, 14, 0), 45, "x"), event("b", "B", at(19, 15, 0), 30, "y")]);
    assert!(moved.run().is_empty());
    let _ = NaiveDate::from_ymd_opt(2026, 11, 17);
}

/// A mail the user sent to `to`, `days` days ago, in the conversation `thread`.
fn sent_mail(id: &str, thread: &str, to: &str, days: i64) -> MessageHeader {
    MessageHeader {
        thread_id: thread.into(),
        mailbox_ids: vec!["sent".into()],
        to: vec![crate::model::Address { name: "Ayse Yilmaz".into(), email: to.into() }],
        is_unread: false,
        ..mail(id, "kaan@example.org", "Menu choices", "Which one would you like?", now() - days * 86_400)
    }
}

fn follow_up_world(sent: Vec<MessageHeader>) -> World {
    World { sent, ..world(vec![]) }
}

#[test]
fn a_sent_mail_nobody_answered_is_a_follow_up() {
    let w = follow_up_world(vec![sent_mail("s", "th", "ayse@example.com", 5)]);
    let f = w.one(AttentionRule::NoReply);
    assert_eq!(f.family, AttentionFamily::Mail);
    assert_eq!(f.names, vec!["Ayse Yilmaz".to_string(), "Menu choices".to_string()]);
    assert_eq!(f.count, 5);
    assert_eq!(f.thread_id.as_deref(), Some("th"));
    // Opening the conversation works without an assistant: there is no service action.
    assert_eq!(f.service, None);
    assert_eq!(f.phone, vec![PhoneAction::OpenMail { message_id: "s".into(), thread_id: "th".into() }]);
    // The id follows the mail, so a swipe is remembered under it.
    assert_eq!(f.id, w.one(AttentionRule::NoReply).id);
}

#[test]
fn a_sent_mail_that_asks_nothing_is_not_a_follow_up() {
    let mut m = sent_mail("s", "th", "ayse@example.com", 5);
    m.preview = "Here are the options, the notes are attached.".into();
    assert!(!follow_up_world(vec![m]).rules().contains(&AttentionRule::NoReply));
}

#[test]
fn a_reply_from_anyone_else_after_it_ends_the_wait() {
    let mut w = follow_up_world(vec![sent_mail("s", "th", "ayse@example.com", 5)]);
    // An answer from before the sent mail does not count; one after it does.
    w.others.insert("th".into(), now() - 6 * 86_400);
    assert!(w.rules().contains(&AttentionRule::NoReply));
    w.others.insert("th".into(), now() - 4 * 86_400);
    assert!(!w.rules().contains(&AttentionRule::NoReply));
}

#[test]
fn a_follow_up_waits_three_days_and_ends_after_fourteen() {
    assert!(!follow_up_world(vec![sent_mail("s", "th", "ayse@example.com", 2)])
        .rules()
        .contains(&AttentionRule::NoReply));
    assert!(follow_up_world(vec![sent_mail("s", "th", "ayse@example.com", 3)])
        .rules()
        .contains(&AttentionRule::NoReply));
    assert!(follow_up_world(vec![sent_mail("s", "th", "ayse@example.com", 14)])
        .rules()
        .contains(&AttentionRule::NoReply));
    assert!(!follow_up_world(vec![sent_mail("s", "th", "ayse@example.com", 15)])
        .rules()
        .contains(&AttentionRule::NoReply));
}

#[test]
fn machines_the_users_own_address_and_encrypted_mail_are_not_waited_for() {
    for to in ["no-reply@example.com", "notifications@example.com", "newsletter@example.com", "kaan@example.org"] {
        assert!(!follow_up_world(vec![sent_mail("s", "th", to, 5)]).rules().contains(&AttentionRule::NoReply), "{to}");
    }
    let mut secret = sent_mail("s", "th", "ayse@example.com", 5);
    secret.preview = "-----BEGIN PGP MESSAGE-----".into();
    assert!(!follow_up_world(vec![secret]).rules().contains(&AttentionRule::NoReply));
    // Someone else's mail in the Sent list (not from the user's address) is not the user's.
    let mut other = sent_mail("s", "th", "ayse@example.com", 5);
    other.from_email = "bob@example.com".into();
    assert!(!follow_up_world(vec![other]).rules().contains(&AttentionRule::NoReply));
}

#[test]
fn only_the_newest_mail_of_a_conversation_counts() {
    // A follow-up of the user's own, two days ago: the wait starts again, so no finding yet.
    let w = follow_up_world(vec![
        sent_mail("old", "th", "ayse@example.com", 8),
        sent_mail("new", "th", "ayse@example.com", 2),
    ]);
    assert!(!w.rules().contains(&AttentionRule::NoReply));
    // Both old enough: one finding, for the newest.
    let w = follow_up_world(vec![
        sent_mail("old", "th", "ayse@example.com", 8),
        sent_mail("new", "th", "ayse@example.com", 4),
    ]);
    let all: Vec<_> = w.run().into_iter().filter(|f| f.rule == AttentionRule::NoReply).collect();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].message_ids, vec!["new".to_string()]);
}

#[test]
fn the_follow_up_has_its_own_switch_and_follows_the_mail_switch() {
    let mut w = follow_up_world(vec![sent_mail("s", "th", "ayse@example.com", 5)]);
    assert!(AttentionSettings::default().follow_up);
    w.settings.follow_up = false;
    assert!(!w.rules().contains(&AttentionRule::NoReply));
    w.settings.follow_up = true;
    w.settings.mail = false;
    assert!(!w.rules().contains(&AttentionRule::NoReply));
}
