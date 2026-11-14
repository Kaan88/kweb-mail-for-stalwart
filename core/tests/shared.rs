//! M7 "shared with me" against the mock server, through the `Core` object the way the app uses
//! it: the accounts, and mail folders, calendars and address books other users share. The mock
//! behaves like Stalwart 1.1.0 where it matters (see `support/sharing.rs`); its ids repeat the
//! user's own on purpose (message `e1`, folder `mb1`, calendar `cal1`, event `ev1`, card `c1`),
//! so a call that forgets the account gives a wrong answer.

mod support;

use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

use kweb_core::{
    ffi::Core,
    model::{SharedAccount, SharedAction},
    secrets::MemorySecrets,
    Error,
};
use serde_json::json;
use support::{MockServer, ACCOUNT_ID, ALL_MAIL_RIGHTS, EMPTY_SHARE_ACCOUNT, MAIL_ONLY_ACCOUNT, SHARED_ACCOUNT};
use tokio::runtime::Runtime;

const OWNER: &str = SHARED_ACCOUNT;
const ZONE: &str = "Europe/Istanbul";
const READ: [&str; 1] = ["mayReadItems"];

struct Rig {
    // Field order is drop order: the core goes first, the mock and its runtime last.
    core: Arc<Core>,
    server: MockServer,
    dir: tempfile::TempDir,
    _rt: Runtime,
}

impl Rig {
    /// A mock server set up by `setup` before the core signs in.
    fn with(setup: impl FnOnce(&MockServer)) -> Rig {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        let server = rt.block_on(MockServer::start());
        setup(&server);
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("cache.db").to_string_lossy().into_owned();
        let core = Core::new(db_path, Arc::new(MemorySecrets::default())).unwrap();
        let url = core.begin_sign_in_with_redirect(server.url(), "ovh.kweb.mail:/oauth/callback".into()).unwrap();
        core.complete_sign_in(server.authorize(&url).unwrap()).unwrap();
        Rig { core, server, dir, _rt: rt }
    }

    /// `sh1` shares mail, a calendar and an address book; every id repeats the user's own.
    fn new() -> Rig {
        Rig::with(set_up_owner)
    }

    fn db(&self) -> PathBuf {
        self.dir.path().join("cache.db")
    }

    /// The number of rows of every table of the cache.
    fn cache(&self) -> BTreeMap<String, i64> {
        let db = rusqlite::Connection::open(self.db()).unwrap();
        let tables: Vec<String> = db
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table'")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        tables
            .into_iter()
            .map(|t| {
                let n = db.query_row(&format!("SELECT count(*) FROM \"{t}\""), [], |r| r.get(0)).unwrap();
                (t, n)
            })
            .collect()
    }

    /// How many rows of the cache belong to the account `account`.
    fn rows_of(&self, account: &str) -> i64 {
        let db = rusqlite::Connection::open(self.db()).unwrap();
        let tables: Vec<String> = db
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table'")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        let mut total = 0;
        for t in tables {
            let has_account = db
                .prepare(&format!("PRAGMA table_info(\"{t}\")"))
                .unwrap()
                .query_map([], |r| r.get::<_, String>(1))
                .unwrap()
                .any(|c| c.unwrap() == "account_id");
            if has_account {
                let sql = format!("SELECT count(*) FROM \"{t}\" WHERE account_id = ?1");
                total += db.query_row(&sql, [account], |r| r.get::<_, i64>(0)).unwrap();
            }
        }
        total
    }

    fn act(&self, id: &str, action: SharedAction) -> Result<(), Error> {
        self.core.shared_act(OWNER.into(), id.into(), action)
    }
}

/// The owner `sh1`. Mail: `mb1` "Team" (everything but nothing hidden), `mb2` "archive" (read
/// only, a child of `mb1`), `mb3` "Hidden" (not shared with the user). Messages `e1` and `e2`.
/// A calendar `cal1` "Team calendar" with a weekly event `ev1`; an address book `b` with the
/// cards `c1` and `c2`.
fn set_up_owner(server: &MockServer) {
    server.sharing(|s| {
        s.share_mail(OWNER).share_calendars(OWNER).share_contacts(OWNER);
        s.add_folder(OWNER, "mb1", "Team", None, &ALL_MAIL_RIGHTS);
        s.add_folder(OWNER, "mb2", "archive", Some("mb1"), &READ);
        s.add_folder(OWNER, "mb3", "Hidden", None, &[]);
        s.add_message(OWNER, "e1", "mb1", "Shared one", 3600);
        s.add_message(OWNER, "e2", "mb2", "Shared two", 7200);
        s.add_message(OWNER, "e3", "mb3", "Not for you", 10);
        s.calendar(OWNER).add_calendar("cal1", "Team calendar", true, true);
        s.calendar(OWNER).add(json!({"@type": "Event", "title": "Standup", "start": "2026-11-16T10:00:00",
            "timeZone": ZONE, "duration": "PT30M",
            "recurrenceRules": [{"@type": "RecurrenceRule", "frequency": "weekly"}]}));
        s.contacts(OWNER).add_address_book("b", "Team contacts", true);
        s.contacts(OWNER).add_contact(json!({"name": {"full": "Zed Zimmer"}, "addressBookIds": {"b": true},
            "emails": {"e": {"address": "zed@shared.test"}}}));
        s.contacts(OWNER).add_contact(json!({"name": {"full": "Amy Adler"}, "addressBookIds": {"b": true},
            "emails": {"e": {"address": "amy@shared.test"}}}));
    });
}

fn invalid(e: Error) -> String {
    match e {
        Error::Invalid { msg } => msg,
        other => panic!("expected Error::Invalid, got {other:?}"),
    }
}

fn names(accounts: &[SharedAccount]) -> Vec<(String, bool, bool, bool)> {
    accounts.iter().map(|a| (a.account_id.clone(), a.has_mail, a.has_calendars, a.has_contacts)).collect()
}

/// Unix seconds of 2026-11-16T00:00 in Istanbul and one week on.
fn week() -> (i64, i64) {
    let start = 1_794_787_200 - 3 * 3600;
    (start, start + 7 * 86_400)
}

// ---- the accounts ----

#[test]
fn an_account_is_listed_with_what_it_shares_and_nothing_else_is() {
    let rig = Rig::with(|server| {
        set_up_owner(server);
        // sh2 shares only mail. sh3 shares nothing at all (and is not in the session).
        server.sharing(|s| {
            s.share_mail(EMPTY_SHARE_ACCOUNT);
            s.add_folder(EMPTY_SHARE_ACCOUNT, "x1", "Folder", None, &READ);
        });
    });
    let found = rig.core.shared_accounts().unwrap();
    // By name: empty@kweb.test before shared@kweb.test.
    assert_eq!(
        names(&found),
        [(EMPTY_SHARE_ACCOUNT.to_string(), true, false, false), (OWNER.to_string(), true, true, true),]
    );
    assert_eq!(found[1].name, "shared@kweb.test");
    assert!(found.iter().all(|a| a.account_id != MAIL_ONLY_ACCOUNT && a.account_id != ACCOUNT_ID));
}

#[test]
fn nothing_shared_is_an_empty_list() {
    let rig = Rig::with(|_| {});
    assert!(rig.core.shared_accounts().unwrap().is_empty());
}

#[test]
fn each_account_is_asked_in_one_request() {
    let rig = Rig::with(|server| {
        set_up_owner(server);
        server.sharing(|s| {
            s.share_mail(EMPTY_SHARE_ACCOUNT);
        });
    });
    rig.server.reset_request_counts();
    rig.core.shared_accounts().unwrap();
    assert_eq!(rig.server.request_count("/jmap"), 2, "one request per account");
    assert_eq!(rig.server.request_count("Mailbox/get"), 2);
    assert_eq!(rig.server.request_count("Calendar/get"), 2);
    assert_eq!(rig.server.request_count("AddressBook/get"), 2);
}

#[test]
fn the_three_probes_that_mean_not_shared() {
    let rig = Rig::with(|server| {
        set_up_owner(server);
        server.sharing(|s| {
            // sh2: mail only; its calendar answers accountNotFound and its contacts forbidden.
            s.share_mail(EMPTY_SHARE_ACCOUNT).share_calendars(EMPTY_SHARE_ACCOUNT).share_contacts(EMPTY_SHARE_ACCOUNT);
            s.add_folder(EMPTY_SHARE_ACCOUNT, "x1", "Folder", None, &READ);
            s.refuse(EMPTY_SHARE_ACCOUNT, "calendar", Some("accountNotFound"));
            s.refuse(EMPTY_SHARE_ACCOUNT, "contacts", Some("forbidden"));
            // sh3: listed, and says "not supported" to everything: left out altogether.
            s.share_mail(MAIL_ONLY_ACCOUNT);
            s.unshare_mail(MAIL_ONLY_ACCOUNT);
        });
    });
    let found = rig.core.shared_accounts().unwrap();
    assert_eq!(
        names(&found),
        [(EMPTY_SHARE_ACCOUNT.to_string(), true, false, false), (OWNER.to_string(), true, true, true)]
    );
}

#[test]
fn any_other_failure_while_asking_is_an_error_not_a_no() {
    let rig = Rig::new();
    rig.server.sharing(|s| s.refuse(OWNER, "calendar", Some("serverFail")));
    assert!(matches!(rig.core.shared_accounts(), Err(Error::Temporary { .. })));
}

#[test]
fn a_share_given_or_taken_back_shows_at_once() {
    let rig = Rig::new();
    assert_eq!(names(&rig.core.shared_accounts().unwrap()), [(OWNER.to_string(), true, true, true)]);
    // Given after the connection was made.
    rig.server.sharing(|s| {
        s.share_mail(EMPTY_SHARE_ACCOUNT);
        s.add_folder(EMPTY_SHARE_ACCOUNT, "x1", "Folder", None, &READ);
    });
    assert_eq!(rig.core.shared_accounts().unwrap().len(), 2);
    // Taken back: the calendar, then the account.
    rig.server.sharing(|s| s.unshare_calendars(OWNER));
    assert_eq!(names(&rig.core.shared_accounts().unwrap())[1], (OWNER.to_string(), true, false, true));
    rig.server.sharing(|s| s.set_listed(OWNER, false));
    assert_eq!(names(&rig.core.shared_accounts().unwrap()), [(EMPTY_SHARE_ACCOUNT.to_string(), true, false, false)]);
}

#[test]
fn an_account_of_files_only_is_not_here() {
    let rig = Rig::with(|server| {
        server.enable_files();
        server.sharing(|_| {});
    });
    // The session lists three accounts that share files or nothing; none shares mail,
    // calendars or contacts.
    assert!(rig.core.shared_accounts().unwrap().is_empty());
    assert_eq!(rig.core.file_spaces().unwrap().len(), 2, "the files side is as it was");
}

#[test]
fn an_account_that_shares_files_and_mail_is_one_account() {
    let rig = Rig::with(|server| {
        server.enable_files();
        set_up_owner(server);
    });
    assert_eq!(names(&rig.core.shared_accounts().unwrap()), [(OWNER.to_string(), true, true, true)]);
    assert_eq!(rig.core.shared_mailboxes(OWNER.into()).unwrap().len(), 2);
}

// ---- an account the user may not name ----

#[test]
fn an_account_that_is_not_shared_is_invalid_in_every_call() {
    let rig = Rig::new();
    let unshared = "this is no longer shared with you";
    for account in [ACCOUNT_ID, "nobody", MAIL_ONLY_ACCOUNT] {
        let a = || account.to_string();
        let is_unshared = |e: Error| assert_eq!(invalid(e), unshared, "{account}");
        is_unshared(rig.core.shared_mailboxes(a()).unwrap_err());
        is_unshared(rig.core.shared_messages(a(), "mb1".into(), 0, 10).unwrap_err());
        is_unshared(rig.core.shared_message(a(), "e1".into()).unwrap_err());
        is_unshared(rig.core.shared_act(a(), "e1".into(), SharedAction::MarkRead).unwrap_err());
        is_unshared(rig.core.shared_calendars(a()).unwrap_err());
        is_unshared(rig.core.shared_agenda(a(), 0, 86_400, ZONE.into()).unwrap_err());
        is_unshared(rig.core.shared_event(a(), "ev1".into(), None).unwrap_err());
        is_unshared(rig.core.shared_contacts(a(), String::new()).unwrap_err());
        is_unshared(rig.core.shared_contact(a(), "c1".into()).unwrap_err());
    }
}

#[test]
fn a_share_taken_back_between_two_calls_is_invalid_the_second_time() {
    let rig = Rig::new();
    let (from, to) = week();
    assert_eq!(rig.core.shared_mailboxes(OWNER.into()).unwrap().len(), 2);
    assert_eq!(rig.core.shared_messages(OWNER.into(), "mb1".into(), 0, 10).unwrap().total, 1);
    rig.core.shared_message(OWNER.into(), "e1".into()).unwrap();
    rig.core.shared_calendars(OWNER.into()).unwrap();
    rig.core.shared_agenda(OWNER.into(), from, to, ZONE.into()).unwrap();
    rig.core.shared_event(OWNER.into(), "ev1".into(), None).unwrap();
    rig.core.shared_contacts(OWNER.into(), String::new()).unwrap();
    rig.core.shared_contact(OWNER.into(), "c1".into()).unwrap();
    rig.server.sharing(|s| s.set_listed(OWNER, false));
    let gone = "this is no longer shared with you";
    assert_eq!(invalid(rig.core.shared_mailboxes(OWNER.into()).unwrap_err()), gone);
    assert_eq!(invalid(rig.core.shared_messages(OWNER.into(), "mb1".into(), 0, 10).unwrap_err()), gone);
    assert_eq!(invalid(rig.core.shared_message(OWNER.into(), "e1".into()).unwrap_err()), gone);
    assert_eq!(invalid(rig.act("e1", SharedAction::Flag).unwrap_err()), gone);
    assert_eq!(invalid(rig.core.shared_calendars(OWNER.into()).unwrap_err()), gone);
    assert_eq!(invalid(rig.core.shared_agenda(OWNER.into(), from, to, ZONE.into()).unwrap_err()), gone);
    assert_eq!(invalid(rig.core.shared_event(OWNER.into(), "ev1".into(), None).unwrap_err()), gone);
    assert_eq!(invalid(rig.core.shared_contacts(OWNER.into(), String::new()).unwrap_err()), gone);
    assert_eq!(invalid(rig.core.shared_contact(OWNER.into(), "c1".into()).unwrap_err()), gone);
}

#[test]
fn a_share_given_after_the_connection_was_made_can_be_read() {
    let rig = Rig::new();
    rig.core.shared_mailboxes(OWNER.into()).unwrap();
    rig.server.sharing(|s| {
        s.share_mail(EMPTY_SHARE_ACCOUNT);
        s.add_folder(EMPTY_SHARE_ACCOUNT, "x1", "Later", None, &READ);
    });
    let folders = rig.core.shared_mailboxes(EMPTY_SHARE_ACCOUNT.into()).unwrap();
    assert_eq!(folders.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["Later"]);
}

#[test]
fn the_kinds_an_account_does_not_share_are_invalid() {
    let rig = Rig::with(|server| {
        server.sharing(|s| {
            s.share_mail(EMPTY_SHARE_ACCOUNT);
            s.add_folder(EMPTY_SHARE_ACCOUNT, "x1", "Only mail", None, &READ);
        });
    });
    let account = EMPTY_SHARE_ACCOUNT.to_string();
    assert_eq!(rig.core.shared_mailboxes(account.clone()).unwrap().len(), 1);
    assert!(rig.core.shared_calendars(account.clone()).is_err());
    assert!(rig.core.shared_contacts(account.clone(), String::new()).is_err());
}

// ---- mail ----

#[test]
fn folders_by_name_with_what_the_user_may_do() {
    let rig = Rig::new();
    let found = rig.core.shared_mailboxes(OWNER.into()).unwrap();
    // "archive" before "Team" without regard to case; the hidden folder is not there.
    assert_eq!(found.iter().map(|f| f.id.as_str()).collect::<Vec<_>>(), ["mb2", "mb1"]);
    let (archive, team) = (&found[0], &found[1]);
    assert_eq!((archive.parent_id.as_deref(), team.parent_id.as_deref()), (Some("mb1"), None));
    assert_eq!((team.total, team.unread, archive.total, archive.unread), (1, 1, 1, 1));
    assert_eq!((team.may_mark, team.may_add, team.may_remove), (true, true, true));
    assert_eq!((archive.may_mark, archive.may_add, archive.may_remove), (false, false, false));
}

#[test]
fn a_parent_the_user_cannot_see_is_not_named() {
    let rig = Rig::new();
    rig.server.sharing(|s| {
        s.add_folder(OWNER, "mb9", "Orphan", Some("mb3"), &READ);
    });
    let found = rig.core.shared_mailboxes(OWNER.into()).unwrap();
    let orphan = found.iter().find(|f| f.id == "mb9").unwrap();
    assert_eq!(orphan.parent_id, None);
}

#[test]
fn each_mail_right_alone_decides_its_flag() {
    for missing in ALL_MAIL_RIGHTS {
        let rights: Vec<&'static str> = ALL_MAIL_RIGHTS.into_iter().filter(|r| *r != missing).collect();
        let rig = Rig::new();
        rig.server.sharing(|s| s.set_folder_rights(OWNER, "mb1", &rights));
        let found = rig.core.shared_mailboxes(OWNER.into()).unwrap();
        let team = found.iter().find(|f| f.id == "mb1");
        if missing == "mayReadItems" {
            assert!(team.is_none(), "a folder the user may not read is not shared");
            continue;
        }
        let team = team.unwrap();
        let expect = (
            !matches!(missing, "maySetSeen" | "maySetKeywords"),
            missing != "mayAddItems",
            missing != "mayRemoveItems",
        );
        assert_eq!((team.may_mark, team.may_add, team.may_remove), expect, "without {missing}");
    }
}

#[test]
fn messages_come_newest_first_in_pages_with_the_total() {
    let rig = Rig::new();
    rig.server.sharing(|s| {
        for i in 0..7 {
            s.add_message(OWNER, &format!("n{i}"), "mb1", &format!("Note {i}"), 100 + 10 * i);
        }
    });
    // e1 is an hour old; the notes are newer, n0 the newest.
    let page = rig.core.shared_messages(OWNER.into(), "mb1".into(), 0, 3).unwrap();
    assert_eq!(page.total, 8);
    assert_eq!(page.messages.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["n0", "n1", "n2"]);
    let page = rig.core.shared_messages(OWNER.into(), "mb1".into(), 6, 3).unwrap();
    assert_eq!(page.messages.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["n6", "e1"]);
    let past = rig.core.shared_messages(OWNER.into(), "mb1".into(), 50, 3).unwrap();
    assert_eq!((past.messages.len(), past.total), (0, 8));
    let first = &rig.core.shared_messages(OWNER.into(), "mb1".into(), 0, 1).unwrap().messages[0];
    assert_eq!(
        (first.subject.as_str(), first.is_unread, first.from_email.as_str()),
        ("Note 0", true, "owner@shared.test")
    );
    assert_eq!(first.mailbox_ids, ["mb1"]);
}

#[test]
fn a_page_is_at_most_a_hundred() {
    let rig = Rig::new();
    rig.server.sharing(|s| {
        for i in 0..130 {
            s.add_message(OWNER, &format!("m{i}"), "mb1", "Bulk", 100 + i);
        }
    });
    let page = rig.core.shared_messages(OWNER.into(), "mb1".into(), 0, 5000).unwrap();
    assert_eq!((page.messages.len(), page.total), (100, 131));
}

#[test]
fn a_folder_that_is_not_shared_has_no_messages() {
    let rig = Rig::new();
    let hidden = rig.core.shared_messages(OWNER.into(), "mb3".into(), 0, 10).unwrap();
    assert_eq!((hidden.messages.len(), hidden.total), (0, 0));
}

#[test]
fn a_message_and_a_shared_message_with_the_same_id_are_never_mixed_up() {
    let rig = Rig::new();
    rig.server.set_bodies("e1", Some("my own text"), None);
    rig.server.sharing(|s| {
        s.share_mail(EMPTY_SHARE_ACCOUNT);
        s.add_folder(EMPTY_SHARE_ACCOUNT, "mb1", "Other", None, &READ);
        s.add_message(EMPTY_SHARE_ACCOUNT, "e1", "mb1", "From the second owner", 5);
    });
    let own = rig.core.message_body("e1".into()).unwrap();
    assert_eq!(own.text.as_deref(), Some("my own text"));
    let first = rig.core.shared_message(OWNER.into(), "e1".into()).unwrap();
    let second = rig.core.shared_message(EMPTY_SHARE_ACCOUNT.into(), "e1".into()).unwrap();
    assert_eq!(first.header.subject, "Shared one");
    assert_eq!(first.body.text.as_deref(), Some("Body of Shared one"));
    assert_eq!(second.header.subject, "From the second owner");
    assert_eq!(second.body.message_id, "e1");
    // The own message is still the own message, from the cache.
    assert_eq!(rig.core.message_body("e1".into()).unwrap().text.as_deref(), Some("my own text"));
    // A message of the own account is not there in the shared one, and the other way round.
    assert_eq!(
        invalid(rig.core.shared_message(OWNER.into(), "e4".into()).unwrap_err()),
        "that message is no longer there"
    );
}

#[test]
fn a_message_is_read_like_an_own_one() {
    let rig = Rig::new();
    rig.server.sharing(|s| {
        s.set_message_bodies(OWNER, "e1", Some("plain"), Some("<p>html</p>"));
        s.add_message_file(OWNER, "e1", "plan.pdf", "application/pdf", b"%PDF-shared");
    });
    let m = rig.core.shared_message(OWNER.into(), "e1".into()).unwrap();
    assert_eq!((m.header.id.as_str(), m.header.has_attachment), ("e1", true));
    assert_eq!((m.body.text.as_deref(), m.body.html.as_deref()), (Some("plain"), Some("<p>html</p>")));
    assert_eq!(m.body.attachments.len(), 1);
    let file = &m.body.attachments[0];
    assert_eq!((file.name.as_str(), file.mime_type.as_str()), ("plan.pdf", "application/pdf"));
    // The attachment is downloaded from the owner's account.
    let dest = rig.dir.path().join("plan.pdf");
    rig.core.file_download(Some(OWNER.into()), file.blob_id.clone(), dest.to_string_lossy().into_owned()).unwrap();
    assert_eq!(std::fs::read(dest).unwrap(), b"%PDF-shared");
    assert_eq!(rig.server.file_downloads(), [OWNER]);
}

#[test]
fn a_message_the_user_may_not_read_is_gone() {
    let rig = Rig::new();
    let gone = "that message is no longer there";
    assert_eq!(invalid(rig.core.shared_message(OWNER.into(), "e3".into()).unwrap_err()), gone);
    assert_eq!(invalid(rig.core.shared_message(OWNER.into(), "nope".into()).unwrap_err()), gone);
}

#[test]
fn reading_marks_nothing_read_and_changes_nothing() {
    let rig = Rig::new();
    rig.server.reset_request_counts();
    rig.core.shared_mailboxes(OWNER.into()).unwrap();
    rig.core.shared_messages(OWNER.into(), "mb1".into(), 0, 10).unwrap();
    rig.core.shared_message(OWNER.into(), "e1".into()).unwrap();
    assert_eq!(rig.server.request_count("Email/set"), 0);
    let keywords = rig.server.sharing(|s| s.keywords_of(OWNER, "e1"));
    assert_eq!(keywords, Some(Vec::new()), "still unread");
    assert!(rig.core.shared_message(OWNER.into(), "e1".into()).unwrap().header.is_unread);
    // The user's own mail is untouched too.
    assert!(!rig.server.is_read("e1"));
}

#[test]
fn nothing_of_a_shared_account_is_ever_cached() {
    let rig = Rig::new();
    let (from, to) = week();
    rig.core.mailboxes().unwrap();
    let before = rig.cache();
    let a = || OWNER.to_string();
    rig.core.shared_accounts().unwrap();
    assert_eq!(rig.cache(), before, "accounts");
    rig.core.shared_mailboxes(a()).unwrap();
    rig.core.shared_messages(a(), "mb1".into(), 0, 10).unwrap();
    rig.core.shared_message(a(), "e1".into()).unwrap();
    assert_eq!(rig.cache(), before, "mail");
    rig.core.shared_calendars(a()).unwrap();
    rig.core.shared_agenda(a(), from, to, ZONE.into()).unwrap();
    rig.core.shared_event(a(), "ev1".into(), None).unwrap();
    assert_eq!(rig.cache(), before, "calendars");
    rig.core.shared_contacts(a(), String::new()).unwrap();
    rig.core.shared_contacts(a(), "zed".into()).unwrap();
    rig.core.shared_contact(a(), "c1".into()).unwrap();
    assert_eq!(rig.cache(), before, "contacts");
    rig.act("e1", SharedAction::Flag).unwrap();
    rig.act("e1", SharedAction::Move { mailbox_id: "mb1".into() }).unwrap();
    assert_eq!(rig.cache(), before, "actions");
    assert_eq!(rig.rows_of(OWNER), 0);
}

#[test]
fn the_own_account_still_gets_its_cache_while_the_shared_one_gets_none() {
    let rig = Rig::new();
    rig.core.agenda(week().0, week().1, ZONE.into()).unwrap();
    rig.core.shared_agenda(OWNER.into(), week().0, week().1, ZONE.into()).unwrap();
    // The own cache holds the own agenda only: the server cannot be reached and the cache
    // answers with the own events, not the shared ones.
    rig.server.set_down(true);
    let own = rig.core.agenda(week().0, week().1, ZONE.into()).unwrap();
    assert!(own.from_cache && own.entries.iter().all(|e| e.title != "Standup"));
    assert!(rig.core.shared_agenda(OWNER.into(), week().0, week().1, ZONE.into()).is_err());
}

#[test]
fn an_action_does_what_it_says_in_the_shared_account_only() {
    let rig = Rig::new();
    let keywords = || rig.server.sharing(|s| s.keywords_of(OWNER, "e1")).unwrap();
    let own_before = (rig.server.keywords_of("e1"), rig.server.mailboxes_of("e1"));
    rig.act("e1", SharedAction::MarkRead).unwrap();
    assert_eq!(keywords(), ["$seen"]);
    rig.act("e1", SharedAction::Flag).unwrap();
    assert_eq!(keywords(), ["$flagged", "$seen"]);
    rig.act("e1", SharedAction::MarkUnread).unwrap();
    rig.act("e1", SharedAction::Unflag).unwrap();
    assert!(keywords().is_empty());
    // Move sets the folders to just the target.
    rig.server.sharing(|s| s.add_folder(OWNER, "mb4", "More", None, &ALL_MAIL_RIGHTS));
    rig.act("e1", SharedAction::Move { mailbox_id: "mb4".into() }).unwrap();
    assert_eq!(rig.server.sharing(|s| s.folders_of(OWNER, "e1")).unwrap(), ["mb4"]);
    rig.act("e1", SharedAction::Delete).unwrap();
    assert!(!rig.server.sharing(|s| s.message_exists(OWNER, "e1")));
    // The user's own message e1 never changed.
    assert_eq!((rig.server.keywords_of("e1"), rig.server.mailboxes_of("e1")), own_before);
    assert!(rig.server.exists("e1"));
}

#[test]
fn a_move_goes_only_to_a_folder_of_that_account() {
    let rig = Rig::new();
    let not_shared = "that folder is not shared with you";
    // mb3 is the owner's folder that is not shared; "mb5" is only a folder of the user's own.
    for target in ["mb3", "mb5", "nowhere"] {
        let e = rig.act("e1", SharedAction::Move { mailbox_id: target.into() }).unwrap_err();
        assert_eq!(invalid(e), not_shared, "{target}");
    }
    assert_eq!(rig.server.sharing(|s| s.folders_of(OWNER, "e1")).unwrap(), ["mb1"]);
}

#[test]
fn a_message_that_is_gone_is_fine_to_delete_and_invalid_otherwise() {
    let rig = Rig::new();
    rig.server.sharing(|s| s.remove_message(OWNER, "e1"));
    rig.act("e1", SharedAction::Delete).unwrap();
    let gone = "that message is no longer there";
    for action in [
        SharedAction::MarkRead,
        SharedAction::MarkUnread,
        SharedAction::Flag,
        SharedAction::Unflag,
        SharedAction::Move { mailbox_id: "mb1".into() },
    ] {
        assert_eq!(invalid(rig.act("e1", action).unwrap_err()), gone);
    }
}

#[test]
fn what_the_user_may_not_do_is_said_in_plain_words() {
    let change = "you are not allowed to change messages in this folder";
    let delete = "you are not allowed to delete messages in this folder";
    let into = "you are not allowed to move messages into that folder";
    let out_of = "you are not allowed to move messages out of this folder";
    // Each right missing in turn from the folder the message is in (mb1); mb2 is the target.
    for missing in ALL_MAIL_RIGHTS {
        let rig = Rig::new();
        let rights: Vec<&'static str> = ALL_MAIL_RIGHTS.into_iter().filter(|r| *r != missing).collect();
        rig.server.sharing(|s| {
            s.set_folder_rights(OWNER, "mb1", &rights);
            s.set_folder_rights(OWNER, "mb2", &ALL_MAIL_RIGHTS);
        });
        let to_mb2 = || SharedAction::Move { mailbox_id: "mb2".into() };
        let outcome = |action| rig.act("e1", action).map_err(invalid);
        match missing {
            "mayReadItems" => {
                // The folder is not shared any more, so the message is not there for the user.
                assert_eq!(outcome(SharedAction::MarkRead), Err("that message is no longer there".to_string()));
                assert_eq!(outcome(SharedAction::Delete), Ok(()));
            }
            "mayAddItems" => {
                assert_eq!(outcome(SharedAction::MarkRead), Ok(()));
                assert_eq!(outcome(SharedAction::Delete), Ok(()));
            }
            "mayRemoveItems" => {
                assert_eq!(outcome(SharedAction::MarkRead), Ok(()));
                assert_eq!(outcome(to_mb2()), Err(out_of.to_string()));
                assert_eq!(outcome(SharedAction::Delete), Err(delete.to_string()));
            }
            "maySetSeen" => {
                assert_eq!(outcome(SharedAction::MarkRead), Err(change.to_string()));
                assert_eq!(outcome(SharedAction::MarkUnread), Err(change.to_string()));
                assert_eq!(outcome(SharedAction::Flag), Ok(()));
            }
            _ => {
                assert_eq!(outcome(SharedAction::Flag), Err(change.to_string()));
                assert_eq!(outcome(SharedAction::Unflag), Err(change.to_string()));
                assert_eq!(outcome(SharedAction::MarkRead), Ok(()));
            }
        }
    }
    // Into a folder that may not be added to.
    let rig = Rig::new();
    let e = rig.act("e1", SharedAction::Move { mailbox_id: "mb2".into() }).unwrap_err();
    assert_eq!(invalid(e), into);
    // Nothing changed where it was refused.
    assert_eq!(rig.server.sharing(|s| s.folders_of(OWNER, "e1")).unwrap(), ["mb1"]);
    // A read only folder: nothing may be done to its message.
    assert_eq!(invalid(rig.act("e2", SharedAction::Delete).unwrap_err()), delete);
    assert_eq!(invalid(rig.act("e2", SharedAction::Flag).unwrap_err()), change);
    assert!(rig.server.sharing(|s| s.message_exists(OWNER, "e2")));
}

// ---- calendars ----

#[test]
fn calendars_as_the_users_own_are_given_with_the_rights_there() {
    let rig = Rig::new();
    rig.server.sharing(|s| {
        s.calendar(OWNER).add_calendar("cal2", "Read only", true, false);
        // As Stalwart gives a calendar shared with the user: not subscribed until the user says.
        s.calendar(OWNER).patch_calendar("cal2", "isSubscribed", json!(false));
    });
    let found = rig.core.shared_calendars(OWNER.into()).unwrap();
    let flags: Vec<_> = found.iter().map(|c| (c.id.as_str(), c.name.as_str(), c.may_write, c.is_visible)).collect();
    assert_eq!(flags, [("cal1", "Team calendar", true, true), ("cal2", "Read only", false, true)]);
    // The user's own calendar has the same id and is another one.
    let own = rig.core.calendars().unwrap();
    assert_eq!(own[0].name, "Personal");
}

#[test]
fn the_agenda_of_a_shared_calendar_expands_repeating_events_and_is_not_the_users_own() {
    let rig = Rig::new();
    rig.server.add_event(json!({"@type": "Event", "title": "My own", "start": "2026-11-17T09:00:00",
        "timeZone": ZONE, "duration": "PT1H"}));
    let (from, to) = week();
    let shared = rig.core.shared_agenda(OWNER.into(), from, to, ZONE.into()).unwrap();
    assert!(!shared.from_cache);
    // The weekly event of 2026-11-16 once in the week; its id is the own account's id too.
    assert_eq!(
        shared.entries.iter().map(|e| (e.event_id.as_str(), e.title.as_str())).collect::<Vec<_>>(),
        [("ev1", "Standup")]
    );
    let own = rig.core.agenda(from, to, ZONE.into()).unwrap();
    assert_eq!(own.entries.iter().map(|e| e.title.as_str()).collect::<Vec<_>>(), ["My own"]);
    // Two weeks: the series comes twice.
    let two = rig.core.shared_agenda(OWNER.into(), from, to + 7 * 86_400, ZONE.into()).unwrap();
    assert_eq!(two.entries.len(), 2);
    assert!(two.entries.iter().all(|e| e.is_recurring && e.recurrence_id.is_some()));
}

#[test]
fn a_shared_agenda_is_asked_for_with_the_same_rules_as_the_own() {
    let rig = Rig::new();
    let (from, to) = week();
    assert!(rig.core.shared_agenda(OWNER.into(), to, from, ZONE.into()).is_err());
    assert!(rig.core.shared_agenda(OWNER.into(), from, to, "Nowhere/Land".into()).is_err());
    assert!(rig.core.shared_agenda(OWNER.into(), from, from + 400 * 86_400, ZONE.into()).is_err());
}

#[test]
fn a_shared_agenda_without_a_connection_is_an_error_not_an_old_list() {
    let rig = Rig::new();
    let (from, to) = week();
    assert_eq!(rig.core.shared_agenda(OWNER.into(), from, to, ZONE.into()).unwrap().entries.len(), 1);
    rig.server.set_down(true);
    assert!(rig.core.shared_agenda(OWNER.into(), from, to, ZONE.into()).is_err());
}

#[test]
fn a_shared_event_may_be_changed_as_far_as_its_calendar_may() {
    let rig = Rig::new();
    let detail = rig.core.shared_event(OWNER.into(), "ev1".into(), None).unwrap();
    assert_eq!((detail.event_id.as_str(), detail.input.title.as_str()), ("ev1", "Standup"));
    assert!(detail.may_write, "the calendar is writable");
    let (from, _) = week();
    let rec = rig.core.shared_agenda(OWNER.into(), from, from + 14 * 86_400, ZONE.into()).unwrap();
    let second = rec.entries[1].recurrence_id.clone();
    let occurrence = rig.core.shared_event(OWNER.into(), "ev1".into(), second).unwrap();
    assert!(occurrence.may_write);
    rig.server.sharing(|s| {
        s.calendar(OWNER).add_calendar("cal2", "Read only", true, false);
        s.calendar(OWNER).add(json!({"@type": "Event", "title": "Look only", "start": "2026-11-18T10:00:00",
            "timeZone": ZONE, "duration": "PT30M", "calendarIds": {"cal2": true}}));
    });
    assert!(!rig.core.shared_event(OWNER.into(), "ev2".into(), None).unwrap().may_write);
    assert_eq!(
        invalid(rig.core.shared_event(OWNER.into(), "ev99".into(), None).unwrap_err()),
        "the event no longer exists on the server"
    );
}

// ---- contacts ----

#[test]
fn contacts_by_name_narrowed_by_the_query() {
    let rig = Rig::new();
    let all = rig.core.shared_contacts(OWNER.into(), String::new()).unwrap();
    assert_eq!(all.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["Amy Adler", "Zed Zimmer"]);
    assert_eq!(all[0].email, "amy@shared.test");
    let found = rig.core.shared_contacts(OWNER.into(), "zed".into()).unwrap();
    assert_eq!(found.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), ["c1"]);
    assert!(rig.core.shared_contacts(OWNER.into(), "nobody".into()).unwrap().is_empty());
}

#[test]
fn shared_contacts_are_not_the_users_own_and_never_suggested() {
    let rig = Rig::new();
    rig.server.add_contact(json!({"name": {"full": "My Friend"}, "emails": {"e": {"address": "friend@own.test"}}}));
    rig.core.shared_contacts(OWNER.into(), String::new()).unwrap();
    let own = rig.core.contacts(String::new()).unwrap();
    assert_eq!(own.rows.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["My Friend"]);
    assert_eq!(rig.core.shared_contact(OWNER.into(), "c1".into()).unwrap().contact.name, "Zed Zimmer");
    assert_eq!(rig.core.contact("c1".into()).unwrap().contact.name, "My Friend");
}

#[test]
fn a_shared_contact_may_be_changed_as_far_as_its_address_book_may() {
    let rig = Rig::new();
    let detail = rig.core.shared_contact(OWNER.into(), "c2".into()).unwrap();
    assert_eq!((detail.id.as_str(), detail.contact.name.as_str(), detail.may_write), ("c2", "Amy Adler", true));
    rig.server.sharing(|s| {
        s.contacts(OWNER).add_address_book("ro", "Read only", false);
        s.contacts(OWNER).add_contact(json!({"name": {"full": "Ro Reader"}, "addressBookIds": {"ro": true}}));
    });
    assert!(!rig.core.shared_contact(OWNER.into(), "c3".into()).unwrap().may_write);
    let books = rig.core.shared_address_books(OWNER.into()).unwrap();
    assert_eq!(books.iter().map(|b| (b.id.as_str(), b.may_write)).collect::<Vec<_>>(), [("b", true), ("ro", false)]);
    assert_eq!(detail.contact.emails[0].value, "amy@shared.test");
    let e = rig.core.shared_contact(OWNER.into(), "c9".into()).unwrap_err();
    assert_eq!(invalid(e), "the contact no longer exists on the server");
}

// ---- connection ----

#[test]
fn an_expired_token_is_renewed_and_the_call_repeated() {
    let rig = Rig::new();
    rig.core.shared_mailboxes(OWNER.into()).unwrap();
    rig.server.expire_access_tokens();
    assert_eq!(rig.core.shared_mailboxes(OWNER.into()).unwrap().len(), 2);
    rig.server.expire_access_tokens();
    rig.act("e1", SharedAction::MarkRead).unwrap();
    assert!(!rig.server.sharing(|s| s.keywords_of(OWNER, "e1")).unwrap().is_empty());
}

#[test]
fn without_a_connection_nothing_is_done_and_nothing_is_queued() {
    let rig = Rig::new();
    rig.server.set_down(true);
    assert!(matches!(rig.core.shared_mailboxes(OWNER.into()), Err(Error::Temporary { .. })));
    assert!(matches!(rig.act("e1", SharedAction::Delete), Err(Error::Temporary { .. })));
    rig.server.set_down(false);
    assert!(rig.server.sharing(|s| s.message_exists(OWNER, "e1")), "the delete was not kept for later");
    assert_eq!(rig.core.shared_messages(OWNER.into(), "mb1".into(), 0, 10).unwrap().total, 1);
}

#[test]
fn signed_out_is_sign_in_needed_or_invalid_not_a_panic() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("c.db").to_string_lossy().into_owned();
    let core = Core::new(path, Arc::new(MemorySecrets::default())).unwrap();
    assert!(core.shared_accounts().is_err());
    assert!(core.shared_mailboxes("sh1".into()).is_err());
}
