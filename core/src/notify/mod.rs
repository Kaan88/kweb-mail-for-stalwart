//! Decides which notifications should exist.
//!
//! The rule: the notifications on screen must equal the unread mail, in the Inbox and in every
//! folder the user switched on for notifications, that arrived after sign-in (for such a folder:
//! after it was switched on) and that the user has not swiped away. Mail counts however it got
//! into the folder: delivered there, put there by a filter, or moved there by another client. A
//! message that is in both the Inbox and such a folder is one notification. [`plan`] compares
//! that set with what is shown and says what to post and what to cancel. It is a pure function;
//! the store supplies both sets and remembers the result.

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
/// - `desired`: unread mail of the Inbox and the folders that notify, that arrived after
///   sign-in and was not swiped away.
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
            notify: false,
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
                notify: false,
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

    // ---- folders that notify (M10) ----

    /// When the folder "work" is switched on in these tests; mail received from here on counts.
    const ON: i64 = 1_800_000_050;

    fn folder(id: &str, name: &str, role: Option<&str>) -> Mailbox {
        Mailbox {
            id: id.into(),
            name: name.into(),
            role: role.map(str::to_string),
            parent_id: None,
            sort_order: 0,
            total_emails: 0,
            unread_emails: 0,
            notify: false,
        }
    }

    fn mail_in(id: &str, received_at: i64, mailboxes: &[&str]) -> MessageHeader {
        MessageHeader { mailbox_ids: mailboxes.iter().map(|m| m.to_string()).collect(), ..mail(id, received_at) }
    }

    /// The Inbox, a folder "work" that notifies from `ON`, "later" that does not, and Junk.
    fn with_folders() -> (Store, Account) {
        let (store, account) = setup();
        let boxes =
            [folder("work", "Work", None), folder("later", "Later", None), folder("junk", "Junk", Some("junk"))];
        store.upsert_mailboxes(&account.id, &boxes).unwrap();
        assert!(store.set_mailbox_notify(&account.id, "work", true, ON).unwrap());
        (store, account)
    }

    fn pass(store: &Store, account: &Account) -> Plan {
        run(store, account, OnScreen::Unknown, NOW).unwrap()
    }

    #[test]
    fn mail_that_arrives_in_a_folder_that_notifies_is_posted_and_one_that_does_not_is_not() {
        let (store, account) = with_folders();
        let both = [mail_in("w", ON + 1, &["work"]), mail_in("l", ON + 1, &["later"])];
        store.upsert_messages(&account.id, &both).unwrap();
        assert_eq!(posted(&pass(&store, &account)), ["w"]);
        assert!(pass(&store, &account).is_empty());
    }

    #[test]
    fn a_folder_switched_on_later_does_not_announce_the_mail_that_was_already_in_it() {
        let (store, account) = with_folders();
        // Unread mail in "later" from before the user switches it on, and one that arrives after.
        store.upsert_messages(&account.id, &[mail_in("old", ON + 10, &["later"])]).unwrap();
        assert!(pass(&store, &account).is_empty());
        store.set_mailbox_notify(&account.id, "later", true, ON + 20).unwrap();
        assert!(pass(&store, &account).is_empty(), "the old mail stays quiet");
        store.upsert_messages(&account.id, &[mail_in("new", ON + 21, &["later"])]).unwrap();
        assert_eq!(posted(&pass(&store, &account)), ["new"]);
        // The same in the folder that was on from the start: mail from before that is quiet.
        store.upsert_messages(&account.id, &[mail_in("before", ON - 1, &["work"])]).unwrap();
        assert!(pass(&store, &account).is_empty());
    }

    #[test]
    fn mail_from_before_sign_in_is_quiet_in_every_folder() {
        let (store, account) = with_folders();
        store.set_mailbox_notify(&account.id, "work", true, account.signed_in_at - 100).unwrap();
        // Switching on again keeps the first time, which here is before sign-in.
        store.upsert_messages(&account.id, &[mail_in("old", account.signed_in_at - 1, &["work"])]).unwrap();
        assert!(pass(&store, &account).is_empty());
    }

    #[test]
    fn a_message_in_the_inbox_and_in_a_folder_that_notifies_is_one_notification() {
        let (store, account) = with_folders();
        store.upsert_messages(&account.id, &[mail_in("both", ON + 1, &["inbox", "work"])]).unwrap();
        let desired = store.notifiable(&account.id, "inbox", account.signed_in_at).unwrap();
        assert_eq!(desired.len(), 1);
        assert_eq!(posted(&pass(&store, &account)), ["both"]);
        assert!(pass(&store, &account).is_empty());
        // Taken out of the Inbox, it is still wanted for the folder: nothing to cancel.
        store.set_message_mailboxes(&account.id, "both", &["work".to_string()]).unwrap();
        assert!(pass(&store, &account).is_empty());
    }

    #[test]
    fn a_filter_or_a_move_elsewhere_that_brings_mail_into_the_folder_counts() {
        let (store, account) = with_folders();
        // Delivered to the Inbox, then a filter (or another client) moved it into "work": the
        // Inbox posted it first and the folder keeps it.
        store.upsert_messages(&account.id, &[mail_in("f", ON + 1, &["inbox"])]).unwrap();
        assert_eq!(posted(&pass(&store, &account)), ["f"]);
        store.set_message_mailboxes(&account.id, "f", &["work".to_string()]).unwrap();
        assert!(pass(&store, &account).is_empty());
        // New mail from "later" that is moved into "work" before this device ever saw it in the
        // Inbox is announced for "work".
        store.upsert_messages(&account.id, &[mail_in("m", ON + 2, &["later"])]).unwrap();
        assert!(pass(&store, &account).is_empty());
        store.set_message_mailboxes(&account.id, "m", &["work".to_string()]).unwrap();
        assert_eq!(posted(&pass(&store, &account)), ["m"]);
        // Mail that was received before the folder was switched on is not announced by being moved in.
        store.upsert_messages(&account.id, &[mail_in("o", ON - 5, &["later"])]).unwrap();
        store.set_message_mailboxes(&account.id, "o", &["work".to_string()]).unwrap();
        assert!(pass(&store, &account).is_empty());
    }

    #[test]
    fn what_ends_an_inbox_notification_ends_a_folders_too() {
        let (store, account) = with_folders();
        let all = [
            mail_in("read", ON + 1, &["work"]),
            mail_in("moved", ON + 2, &["work"]),
            mail_in("gone", ON + 3, &["work"]),
            mail_in("swiped", ON + 4, &["work"]),
            mail_in("junked", ON + 5, &["work"]),
        ];
        store.upsert_messages(&account.id, &all).unwrap();
        assert_eq!(pass(&store, &account).post.len(), 5);
        store.set_unread(&account.id, "read", false).unwrap();
        store.set_message_mailboxes(&account.id, "moved", &["later".to_string()]).unwrap();
        store.delete_messages(&account.id, &["gone".to_string()]).unwrap();
        swiped(&store, &account.id, "swiped").unwrap();
        store.set_message_mailboxes(&account.id, "junked", &["junk".to_string()]).unwrap();
        let plan = pass(&store, &account);
        assert!(plan.post.is_empty());
        assert_eq!(cancelled(&plan), ["gone", "junked", "moved", "read"]);
        // The swipe holds, also after the screen was emptied behind the app's back.
        let plan = run(&store, &account, OnScreen::Cleared, NOW).unwrap();
        assert!(plan.post.is_empty());
        // Unread again elsewhere: a fresh reason to notify.
        store.set_unread(&account.id, "read", true).unwrap();
        assert_eq!(posted(&pass(&store, &account)), ["read"]);
    }

    #[test]
    fn switching_a_folder_off_withdraws_its_notifications_and_on_again_starts_from_then() {
        let (store, account) = with_folders();
        store
            .upsert_messages(&account.id, &[mail_in("w", ON + 1, &["work"]), mail_in("i", ON + 1, &["inbox"])])
            .unwrap();
        assert_eq!(pass(&store, &account).post.len(), 2);
        store.set_mailbox_notify(&account.id, "work", false, ON + 2).unwrap();
        assert_eq!(cancelled(&pass(&store, &account)), ["w"], "the Inbox's stays");
        // On again: the mail that is still unread in it is old now.
        store.set_mailbox_notify(&account.id, "work", true, ON + 3).unwrap();
        assert!(pass(&store, &account).is_empty());
    }

    #[test]
    fn a_folder_with_a_role_that_never_notifies_does_not_even_with_a_row() {
        let (store, account) = with_folders();
        assert!(store.set_mailbox_notify(&account.id, "junk", true, ON).unwrap());
        store.upsert_messages(&account.id, &[mail_in("j", ON + 1, &["junk"])]).unwrap();
        assert!(pass(&store, &account).is_empty());
        assert!(!store.get_mailbox(&account.id, "junk").unwrap().unwrap().notify);
    }

    #[test]
    fn the_choice_is_kept_when_the_list_is_refreshed_and_goes_with_the_folder() {
        let (store, account) = with_folders();
        assert!(store.get_mailbox(&account.id, "work").unwrap().unwrap().notify);
        assert!(store.get_mailbox(&account.id, "inbox").unwrap().unwrap().notify, "the Inbox always does");
        assert!(!store.get_mailbox(&account.id, "later").unwrap().unwrap().notify);
        // A full refresh of the list, as the server gives it (it knows nothing of the choice).
        let fresh =
            [folder("inbox", "Inbox", Some("inbox")), folder("work", "Work", None), folder("later", "Later", None)];
        store.replace_mailboxes(&account.id, &fresh).unwrap();
        assert_eq!(store.mailbox_notify_since(&account.id, "work").unwrap(), Some(ON));
        // An update of one folder keeps it too.
        store.upsert_mailboxes(&account.id, &[folder("work", "Work 2", None)]).unwrap();
        assert!(store.get_mailbox(&account.id, "work").unwrap().unwrap().notify);
        // Switching on again does not move the time forward.
        store.set_mailbox_notify(&account.id, "work", true, ON + 500).unwrap();
        assert_eq!(store.mailbox_notify_since(&account.id, "work").unwrap(), Some(ON));
        // The folder goes: a refresh without it, or the server's word that it was destroyed.
        store.replace_mailboxes(&account.id, &[folder("inbox", "Inbox", Some("inbox"))]).unwrap();
        assert_eq!(store.mailbox_notify_since(&account.id, "work").unwrap(), None);
        store.upsert_mailboxes(&account.id, &[folder("work", "Work", None)]).unwrap();
        assert!(!store.get_mailbox(&account.id, "work").unwrap().unwrap().notify, "a new folder starts off");
        store.set_mailbox_notify(&account.id, "work", true, ON).unwrap();
        store.delete_mailboxes(&account.id, &["work".to_string()]).unwrap();
        assert_eq!(store.mailbox_notify_since(&account.id, "work").unwrap(), None);
        // A folder the cache does not know cannot be switched on.
        assert!(!store.set_mailbox_notify(&account.id, "nope", true, ON).unwrap());
    }

    #[test]
    fn a_deleted_folder_leaves_its_mail_in_the_trash_or_drops_it() {
        let (store, account) = with_folders();
        store.upsert_mailboxes(&account.id, &[folder("trash", "Trash", Some("trash"))]).unwrap();
        store
            .upsert_messages(
                &account.id,
                &[
                    mail_in("only", ON + 1, &["work"]),
                    mail_in("also", ON + 1, &["work", "later"]),
                    mail_in("kept", ON + 1, &["later"]),
                ],
            )
            .unwrap();
        store.forget_mailbox(&account.id, "work", Some("trash")).unwrap();
        let home = |id: &str| store.get_message(&account.id, id).unwrap().unwrap().mailbox_ids;
        assert_eq!(home("only"), ["trash"]);
        assert_eq!(home("also"), ["later"]);
        assert_eq!(home("kept"), ["later"]);
        assert!(store.get_mailbox(&account.id, "work").unwrap().is_none());
        assert_eq!(store.mailbox_notify_since(&account.id, "work").unwrap(), None);
        // Without a Trash the mail that was only there is dropped.
        store.forget_mailbox(&account.id, "later", None).unwrap();
        assert!(store.get_message(&account.id, "kept").unwrap().is_none());
        assert!(store.get_message(&account.id, "only").unwrap().is_some());
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
