//! M12 stage 8, phase 1 through the `Core` object against the mock server: the findings from the cache,
//! a swipe that stays, a finding that goes when its data changes, and the switches.

use std::sync::Arc;

use crate::support::MockServer;
use kweb_core::{
    attention::{AttentionPlace, AttentionRule},
    ffi::{Core, Shade},
    secrets::MemorySecrets,
};
use serde_json::{json, Value};
use tokio::runtime::Runtime;

const ZONE: &str = "Europe/Istanbul";
use crate::support::INBOX_ID as INBOX;

struct Rig {
    core: Arc<Core>,
    server: MockServer,
    _rt: Runtime,
    _dir: tempfile::TempDir,
}

impl Rig {
    fn new() -> Rig {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        let server = rt.block_on(MockServer::start());
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("cache.db").to_string_lossy().into_owned();
        let core = Core::new(db_path, Arc::new(MemorySecrets::default())).unwrap();
        let url = core.begin_sign_in_with_redirect(server.url(), "ovh.kweb.mail:/oauth/callback".into()).unwrap();
        core.complete_sign_in(server.authorize(&url).unwrap()).unwrap();
        Rig { core, server, _rt: rt, _dir: dir }
    }

    fn rules(&self, place: AttentionPlace) -> Vec<AttentionRule> {
        self.core.attention_findings(place, ZONE.into(), now(), true).unwrap().into_iter().map(|f| f.rule).collect()
    }
}

/// Tuesday 2026-11-17 09:40 in Istanbul (UTC+3).
fn now() -> i64 {
    // 2026-11-17T00:00:00Z is 1_794_873_600.
    1_794_873_600 + 6 * 3600 + 40 * 60
}

fn event(title: &str, start: &str, minutes: u32, place: &str) -> Value {
    let mut e = json!({"@type": "Event", "title": title, "start": start, "timeZone": ZONE, "duration": format!("PT{minutes}M")});
    if !place.is_empty() {
        e["locations"] = json!({"l1": {"@type": "Location", "name": place}});
    }
    e
}

#[test]
fn findings_come_from_the_cache_and_a_swipe_stays() {
    let rig = Rig::new();
    rig.server.add_event(event("Dentist", "2026-11-19T14:00:00", 45, "Dr. Aksoy"));
    rig.server.add_event(event("Standup", "2026-11-19T14:00:00", 30, "Room 2"));
    rig.server.add_event(json!({"@type": "Task", "title": "Send invoice 0311", "due": "2026-11-16T00:00:00",
        "showWithoutTime": true, "timeZone": ZONE}));

    assert_eq!(rig.rules(AttentionPlace::Calendar), vec![AttentionRule::Overlap]);
    assert!(rig.rules(AttentionPlace::Tasks).contains(&AttentionRule::Overdue));
    let home = rig.rules(AttentionPlace::Home);
    assert!(home.contains(&AttentionRule::Overlap) && home.contains(&AttentionRule::Overdue));

    // Offline, the cache answers the same.
    rig.server.set_down(true);
    let found = rig.core.attention_findings(AttentionPlace::Calendar, ZONE.into(), now(), true).unwrap();
    assert_eq!(found.len(), 1);
    rig.server.set_down(false);

    // A swipe stays, also after the next look.
    rig.core.attention_dismiss(found[0].id.clone(), now()).unwrap();
    assert!(rig.rules(AttentionPlace::Calendar).is_empty());
    // And is undone by the reset.
    rig.core.attention_reset_dismissed().unwrap();
    assert_eq!(rig.rules(AttentionPlace::Calendar).len(), 1);
}

#[test]
fn a_finding_goes_when_its_data_changes() {
    let rig = Rig::new();
    let id = rig.server.add_event(event("Standup", "2026-11-19T14:00:00", 30, "Room 2"));
    rig.server.add_event(event("Dentist", "2026-11-19T14:00:00", 45, "Dr. Aksoy"));
    assert_eq!(rig.rules(AttentionPlace::Calendar).len(), 1);
    rig.server.patch_event(&id, json!({"start": "2026-11-19T16:00:00"}));
    assert!(rig.rules(AttentionPlace::Calendar).is_empty());
}

#[test]
fn the_switches_and_mail_findings() {
    let rig = Rig::new();
    rig.core.check_once(Shade::Unknown).unwrap();
    rig.server.deliver_at("Merve", "merve@example.test", "Call this week?", "Could we talk Fri 19:30?", now() - 3600);
    rig.server.deliver_at("Shop", "no-reply@shop.example", "Your ticket for Fri 19:30", "", now() - 3600);
    // The mail reaches the cache by a sync.
    rig.core.check_once(Shade::Unknown).unwrap();
    rig.core.messages(INBOX.into(), 20, 0).unwrap();
    let list = rig.core.attention_findings(AttentionPlace::MailRows, ZONE.into(), now(), true).unwrap();
    // Merve's mail and the seeded "Lunch on Friday?" are found, the shop's no-reply ticket is not.
    let senders: Vec<String> = list.iter().map(|f| f.names[0].clone()).collect();
    assert_eq!(senders, vec!["Maria Keller".to_string(), "Merve".to_string()]);
    assert!(list.iter().all(|f| f.rule == AttentionRule::DateInMail));
    // In the thread: that conversation only.
    let m = rig
        .core
        .messages(INBOX.into(), 20, 0)
        .unwrap()
        .into_iter()
        .find(|m| m.from_email.starts_with("merve"))
        .unwrap();
    assert_eq!(rig.rules(AttentionPlace::Thread { thread_id: m.thread_id.clone() }), vec![AttentionRule::DateInMail]);
    assert!(rig.rules(AttentionPlace::Thread { thread_id: "other".into() }).is_empty());

    let mut s = rig.core.attention_settings().unwrap();
    assert!(!s.home && s.mail && s.calendar && s.tasks && s.ask_each_run);
    s.mail = false;
    rig.core.attention_set_settings(s.clone()).unwrap();
    assert_eq!(rig.core.attention_settings().unwrap(), s);
    assert!(rig.rules(AttentionPlace::MailRows).is_empty());
}

#[test]
fn a_service_action_needs_a_set_up_assistant() {
    let rig = Rig::new();
    rig.server.add_event(event("Release review", "2026-11-17T11:00:00", 60, ""));
    // No place, no mail that shares the title: the only thing on offer is a button with a sparkle, so
    // with no assistant set up the finding is not shown at all.
    assert!(rig.rules(AttentionPlace::Calendar).is_empty());
}

#[test]
fn a_sent_mail_nobody_answered_is_found_in_the_cache_and_goes_with_a_reply_or_a_swipe() {
    use crate::support::SENT_ID;
    let rig = Rig::new();
    rig.core.check_once(Shade::Unknown).unwrap();
    let me = rig.core.identities().unwrap()[0].email.clone();
    let sent =
        rig.server.deliver_in_at(SENT_ID, "Me", &me, "Menu choices", "Which one would you like?", now() - 5 * 86_400);
    rig.server.set_recipients(&sent, &[("Ayse Yilmaz", "ayse@example.test")], &[]);
    rig.core.check_once(Shade::Unknown).unwrap();

    // No assistant is set up, and the finding is still there with its plain Open action.
    let found = rig.core.attention_findings(AttentionPlace::Home, ZONE.into(), now(), false).unwrap();
    let f = found.iter().find(|f| f.rule == AttentionRule::NoReply).expect("a follow-up finding");
    assert_eq!(f.names[0], "Ayse Yilmaz");
    assert_eq!(f.count, 5);
    assert!(f.service.is_none() && f.phone.len() == 1);
    let thread = f.thread_id.clone().unwrap();
    assert_eq!(rig.rules(AttentionPlace::Thread { thread_id: thread.clone() }), vec![AttentionRule::NoReply]);

    // A swipe stays away; the reset brings it back.
    rig.core.attention_dismiss(f.id.clone(), now()).unwrap();
    assert!(!rig.rules(AttentionPlace::Home).contains(&AttentionRule::NoReply));
    rig.core.attention_reset_dismissed().unwrap();
    assert!(rig.rules(AttentionPlace::Home).contains(&AttentionRule::NoReply));

    // The switch.
    let mut s = rig.core.attention_settings().unwrap();
    assert!(s.follow_up);
    s.follow_up = false;
    rig.core.attention_set_settings(s.clone()).unwrap();
    assert!(!rig.rules(AttentionPlace::Home).contains(&AttentionRule::NoReply));
    s.follow_up = true;
    rig.core.attention_set_settings(s).unwrap();

    // An answer in the same conversation, later, ends it.
    let reply =
        rig.server.deliver_at("Ayse Yilmaz", "ayse@example.test", "Re: Menu choices", "Thanks.", now() - 3 * 86_400);
    rig.server.set_thread(&reply, &thread);
    rig.core.check_once(Shade::Unknown).unwrap();
    assert!(!rig.rules(AttentionPlace::Home).contains(&AttentionRule::NoReply));
}
