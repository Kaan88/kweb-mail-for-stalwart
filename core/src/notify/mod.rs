//! Decides which notifications should exist.
//!
//! The rule: the notifications on screen must equal the unread Inbox mail that arrived after
//! sign-in and that the user has not swiped away. [`plan`] compares that set with what is
//! shown and says what to post and what to cancel. It is a pure function; the store supplies
//! both sets and remembers the result.

use std::collections::HashSet;

use crate::{
    model::{Account, MessageHeader},
    store::Store,
    Result,
};

/// Android ids below this are kept for the app's own entries (the "Connected" entry, the
/// group summary), so a mail notification can never replace one of them.
const FIRST_MAIL_ID: u32 = 1000;

/// One mail notification for Android to show.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct MailNotification {
    pub message_id: String,
    /// Derived from the message id, so posting twice updates instead of duplicating.
    pub android_id: i32,
    /// Sender's name, or the address when there is no name.
    pub from: String,
    pub subject: String,
    pub preview: String,
    pub received_at: i64,
    /// True when this mail was already announced once: put it back without sound or vibration.
    pub silent: bool,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Cancel {
    pub message_id: String,
    pub android_id: i32,
}

/// What Android has to do so the screen matches the rule. Cancelling is always silent.
#[derive(Debug, Clone, Default, PartialEq, uniffi::Record)]
pub struct Plan {
    pub post: Vec<MailNotification>,
    pub cancel: Vec<Cancel>,
}

impl Plan {
    pub fn is_empty(&self) -> bool {
        self.post.is_empty() && self.cancel.is_empty()
    }
}

/// What the host can tell about the notification shade.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OnScreen<'a> {
    /// Nothing is known: trust the stored "shown" set.
    Unknown,
    /// The shade was emptied behind the app's back (the phone restarted).
    Cleared,
    /// The Android ids that are really there right now.
    Ids(&'a [i32]),
}

/// The Android notification id of a message: FNV-1a over the id, folded into the positive
/// range above [`FIRST_MAIL_ID`]. It has to give the same number in every run and every
/// version of the app, so it does not use the standard library's hasher.
pub fn android_id(message_id: &str) -> i32 {
    let mut hash: u32 = 0x811c_9dc5;
    for byte in message_id.bytes() {
        hash ^= u32::from(byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    let span = i32::MAX as u32 - FIRST_MAIL_ID;
    (FIRST_MAIL_ID + hash % span) as i32
}

/// Compares what should be shown with what is shown.
///
/// - `desired`: unread Inbox mail that arrived after sign-in and was not swiped away.
/// - `shown`: the message ids the core last told Android to show.
/// - `on_screen`: what the host knows about the shade. A mail that is in `shown` but not on
///   screen is posted again silently: it was announced once already.
pub fn plan(desired: &[MessageHeader], shown: &[String], on_screen: OnScreen) -> Plan {
    let shown: HashSet<&str> = shown.iter().map(String::as_str).collect();
    let wanted: HashSet<&str> = desired.iter().map(|m| m.id.as_str()).collect();
    let visible = |id: &str| match on_screen {
        OnScreen::Unknown => true,
        OnScreen::Cleared => false,
        OnScreen::Ids(ids) => ids.contains(&android_id(id)),
    };

    let post = desired
        .iter()
        .filter_map(|m| {
            let announced = shown.contains(m.id.as_str());
            if announced && visible(&m.id) {
                return None;
            }
            Some(MailNotification {
                message_id: m.id.clone(),
                android_id: android_id(&m.id),
                from: if m.from_name.is_empty() { m.from_email.clone() } else { m.from_name.clone() },
                subject: m.subject.clone(),
                preview: m.preview.clone(),
                received_at: m.received_at,
                silent: announced,
            })
        })
        .collect();

    // Everything recorded as shown that should no longer be there is cancelled, also when the
    // host says it is not on screen: cancelling a missing notification does nothing, and the
    // caller uses this list to forget the entry.
    let mut cancel: Vec<Cancel> = shown
        .iter()
        .filter(|id| !wanted.contains(*id))
        .map(|id| Cancel { message_id: id.to_string(), android_id: android_id(id) })
        .collect();
    cancel.sort_by(|a, b| a.message_id.cmp(&b.message_id));

    Plan { post, cancel }
}

/// Runs the rule against the cache and records the outcome, so a killed process does not
/// forget what is on screen. Call it after every sync, on reconnect, on the 15-minute check,
/// when the app comes to the foreground and after boot (with [`OnScreen::Cleared`]).
///
/// Before the first sync has found the Inbox there is nothing to compare: the plan is empty
/// and nothing on screen is touched.
pub fn run(store: &Store, account: &Account, on_screen: OnScreen, now: i64) -> Result<Plan> {
    let Some(inbox) = store.mailbox_by_role(&account.id, "inbox")? else {
        return Ok(Plan::default());
    };
    let desired = store.notifiable(&account.id, &inbox.id, account.signed_in_at)?;
    let shown: Vec<String> = store.shown_list(&account.id)?.into_iter().map(|(id, _)| id).collect();
    let plan = plan(&desired, &shown, on_screen);
    for n in &plan.post {
        store.shown_add(&account.id, &n.message_id, now)?;
    }
    for c in &plan.cancel {
        store.shown_remove(&account.id, &c.message_id)?;
    }
    // Swipes for mail that has since been read or removed are no longer needed.
    store.prune_dismissed(&account.id)?;
    Ok(plan)
}

/// The user swiped a notification away: that mail is not posted again.
pub fn swiped(store: &Store, account_id: &str, message_id: &str) -> Result<()> {
    store.dismissed_add(account_id, message_id)
}

#[cfg(test)]
mod store_tests {
    use super::*;
    use crate::model::Mailbox;

    const NOW: i64 = 1_800_000_100;

    fn setup() -> (Store, Account) {
        let store = Store::open_in_memory().unwrap();
        let account = Account {
            id: "acc".into(),
            server_url: "https://mail.example.org".into(),
            username: "kaan".into(),
            signed_in_at: 1_800_000_000,
        };
        store.save_account(&account).unwrap();
        let inbox = Mailbox {
            id: "inbox".into(),
            name: "Inbox".into(),
            role: Some("inbox".into()),
            parent_id: None,
            sort_order: 0,
            total_emails: 0,
            unread_emails: 0,
        };
        store.upsert_mailboxes(&account.id, &[inbox]).unwrap();
        (store, account)
    }

    fn mail(id: &str, received_at: i64) -> MessageHeader {
        MessageHeader {
            id: id.into(),
            thread_id: format!("t-{id}"),
            mailbox_ids: vec!["inbox".into()],
            from_name: "Merve".into(),
            from_email: "merve@example.org".into(),
            to: Vec::new(),
            subject: id.into(),
            preview: String::new(),
            received_at,
            is_unread: true,
            is_flagged: false,
            has_attachment: false,
            size: 1,
        }
    }

    fn posted(plan: &Plan) -> Vec<&str> {
        plan.post.iter().map(|n| n.message_id.as_str()).collect()
    }

    fn cancelled(plan: &Plan) -> Vec<&str> {
        plan.cancel.iter().map(|c| c.message_id.as_str()).collect()
    }

    #[test]
    fn posts_once_then_cancels_when_read_elsewhere() {
        let (store, account) = setup();
        store.upsert_messages(&account.id, &[mail("a", NOW)]).unwrap();
        assert_eq!(posted(&run(&store, &account, OnScreen::Unknown, NOW).unwrap()), ["a"]);
        // A second run with nothing changed must not post again.
        assert!(run(&store, &account, OnScreen::Unknown, NOW).unwrap().is_empty());

        store.set_unread(&account.id, "a", false).unwrap();
        assert_eq!(cancelled(&run(&store, &account, OnScreen::Unknown, NOW).unwrap()), ["a"]);
        assert!(run(&store, &account, OnScreen::Unknown, NOW).unwrap().is_empty());
    }

    #[test]
    fn archived_or_deleted_elsewhere_is_cancelled() {
        let (store, account) = setup();
        store.upsert_messages(&account.id, &[mail("a", NOW), mail("b", NOW)]).unwrap();
        run(&store, &account, OnScreen::Unknown, NOW).unwrap();
        store.set_message_mailboxes(&account.id, "a", &["archive".to_string()]).unwrap();
        store.delete_messages(&account.id, &["b".to_string()]).unwrap();
        assert_eq!(cancelled(&run(&store, &account, OnScreen::Unknown, NOW).unwrap()), ["a", "b"]);
    }

    #[test]
    fn mail_from_before_sign_in_is_never_posted() {
        let (store, account) = setup();
        store.upsert_messages(&account.id, &[mail("old", account.signed_in_at - 1)]).unwrap();
        assert!(run(&store, &account, OnScreen::Unknown, NOW).unwrap().is_empty());
    }

    #[test]
    fn a_swiped_notification_does_not_come_back() {
        let (store, account) = setup();
        store.upsert_messages(&account.id, &[mail("a", NOW)]).unwrap();
        run(&store, &account, OnScreen::Unknown, NOW).unwrap();
        swiped(&store, &account.id, "a").unwrap();

        store.upsert_messages(&account.id, &[mail("b", NOW + 1)]).unwrap();
        let plan = run(&store, &account, OnScreen::Unknown, NOW).unwrap();
        assert_eq!(posted(&plan), ["b"]);
        assert!(plan.cancel.is_empty());
        // Not after a restart either.
        assert_eq!(posted(&run(&store, &account, OnScreen::Cleared, NOW).unwrap()), ["b"]);
    }

    #[test]
    fn the_shown_set_survives_a_reopened_store() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cache.db");
        let account = {
            let store = Store::open(&path).unwrap();
            let (seed, account) = setup();
            drop(seed);
            store.save_account(&account).unwrap();
            let inbox = Mailbox {
                id: "inbox".into(),
                name: "Inbox".into(),
                role: Some("inbox".into()),
                parent_id: None,
                sort_order: 0,
                total_emails: 0,
                unread_emails: 0,
            };
            store.upsert_mailboxes(&account.id, &[inbox]).unwrap();
            store.upsert_messages(&account.id, &[mail("a", NOW)]).unwrap();
            assert_eq!(posted(&run(&store, &account, OnScreen::Unknown, NOW).unwrap()), ["a"]);
            account
        };
        // The process was killed and started again: "a" is still known to be on screen.
        let store = Store::open(&path).unwrap();
        assert!(run(&store, &account, OnScreen::Unknown, NOW).unwrap().is_empty());
    }

    #[test]
    fn nothing_happens_before_the_inbox_is_known() {
        let store = Store::open_in_memory().unwrap();
        let (_, account) = setup();
        store.save_account(&account).unwrap();
        assert!(run(&store, &account, OnScreen::Unknown, NOW).unwrap().is_empty());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mail(id: &str) -> MessageHeader {
        MessageHeader {
            id: id.into(),
            thread_id: format!("t-{id}"),
            mailbox_ids: vec!["inbox".into()],
            from_name: "Merve Aydın".into(),
            from_email: "merve@example.org".into(),
            to: Vec::new(),
            subject: format!("Subject {id}"),
            preview: "Hi".into(),
            received_at: 1_800_000_000,
            is_unread: true,
            is_flagged: false,
            has_attachment: false,
            size: 1024,
        }
    }

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn posted(plan: &Plan) -> Vec<(&str, bool)> {
        plan.post.iter().map(|n| (n.message_id.as_str(), n.silent)).collect()
    }

    fn cancelled(plan: &Plan) -> Vec<&str> {
        plan.cancel.iter().map(|c| c.message_id.as_str()).collect()
    }

    #[test]
    fn new_mail_is_posted_with_sound() {
        let p = plan(&[mail("a")], &[], OnScreen::Unknown);
        assert_eq!(posted(&p), [("a", false)]);
        assert!(p.cancel.is_empty());
    }

    #[test]
    fn nothing_to_do_when_screen_matches() {
        let p = plan(&[mail("a"), mail("b")], &ids(&["a", "b"]), OnScreen::Unknown);
        assert!(p.is_empty());
    }

    #[test]
    fn mail_read_elsewhere_is_cancelled() {
        // "a" was read, archived or deleted on another device: it is no longer desired.
        let p = plan(&[mail("b")], &ids(&["a", "b"]), OnScreen::Unknown);
        assert!(p.post.is_empty());
        assert_eq!(cancelled(&p), ["a"]);
    }

    #[test]
    fn airplane_mode_three_arrive_one_read_elsewhere() {
        // Offline while three mails arrive and one of them is read on the desktop: back
        // online, the store only offers the two unread ones.
        let p = plan(&[mail("m1"), mail("m3")], &[], OnScreen::Unknown);
        assert_eq!(posted(&p), [("m1", false), ("m3", false)]);
        assert!(p.cancel.is_empty());
    }

    #[test]
    fn after_a_restart_everything_comes_back_silently() {
        let p = plan(&[mail("a"), mail("b")], &ids(&["a"]), OnScreen::Cleared);
        // "a" was announced before the restart; "b" arrived while the phone was off.
        assert_eq!(posted(&p), [("a", true), ("b", false)]);
    }

    #[test]
    fn a_notification_missing_from_the_shade_is_put_back_silently() {
        let on_screen = [android_id("a")];
        let p = plan(&[mail("a"), mail("b")], &ids(&["a", "b"]), OnScreen::Ids(&on_screen));
        assert_eq!(posted(&p), [("b", true)]);
    }

    #[test]
    fn stale_entries_are_cancelled_even_when_not_on_screen() {
        let p = plan(&[], &ids(&["b", "a"]), OnScreen::Cleared);
        assert_eq!(cancelled(&p), ["a", "b"]);
    }

    #[test]
    fn sender_falls_back_to_the_address() {
        let mut m = mail("a");
        m.from_name.clear();
        let p = plan(&[m], &[], OnScreen::Unknown);
        assert_eq!(p.post[0].from, "merve@example.org");
    }

    #[test]
    fn android_ids_are_stable_and_out_of_the_reserved_range() {
        // Pinned values: a change here would orphan notifications across an app update.
        assert_eq!(android_id(""), (FIRST_MAIL_ID + 0x811c_9dc5 % (i32::MAX as u32 - FIRST_MAIL_ID)) as i32);
        assert_eq!(android_id("a"), android_id("a"));
        assert_ne!(android_id("a"), android_id("b"));
        for id in ["", "a", "M0123456789abcdef", "ÿ", "zzzzzzzzzzzzzzzzzzzzzzzz"] {
            assert!(android_id(id) >= FIRST_MAIL_ID as i32, "{id}");
        }
    }
}
