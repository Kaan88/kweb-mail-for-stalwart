//! M7 sharing against the mock server, through the `Core` object the way the app uses it: the
//! user sharing their own folders, calendars, address books and file folders with others (see
//! `support/shares.rs`), and working in what others share (writing events and contacts,
//! answering and forwarding mail; `support/sharing.rs`). The mock is written from
//! `dev/notes/stalwart-sharing.md`; its ids repeat the user's own on purpose.

mod support;

use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

use kweb_core::{
    ffi::{Core, Shade},
    model::{Contact, DraftKind, EventInput, Person, Share, ShareKind, ShareLevel},
    secrets::MemorySecrets,
    Error,
};
use serde_json::json;
use support::{MockServer, ACCOUNT_ID, ALL_MAIL_RIGHTS, ARCHIVE_ID, DRAFTS_ID, INBOX_ID, SHARED_ACCOUNT, TRASH_ID};
use tokio::runtime::Runtime;

const OWNER: &str = SHARED_ACCOUNT;
const ZONE: &str = "Europe/Istanbul";
const JUNK_ID: &str = "mb6";

struct Rig {
    // Field order is drop order: the core goes first, the mock and its runtime last.
    core: Arc<Core>,
    server: MockServer,
    dir: tempfile::TempDir,
    _rt: Runtime,
}

impl Rig {
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

    /// The user's own side: users to share with, and the file storage.
    fn new() -> Rig {
        Rig::with(|server| {
            server.enable_files();
            server.own_shares(|s| {
                s.add_person("p2", "Second Person", "second@example.test");
                s.add_person("p3", "Third Person", "third@example.test");
            });
        })
    }

    /// `sh1` shares mail, a calendar and an address book, each in a writable and a read-only
    /// kind; the user's own side is as in [`Rig::new`].
    fn shared() -> Rig {
        let rig = Rig::new();
        set_up_owner(&rig.server);
        rig
    }

    fn db(&self) -> PathBuf {
        self.dir.path().join("cache.db")
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

    /// The row counts of every table of the cache.
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

    fn sync(&self) {
        self.core.check_once(Shade::Unknown).expect("a pass");
    }

    fn level(&self, kind: ShareKind, id: &str, person: &str) -> Option<ShareLevel> {
        let shares = self.core.shares(kind, id.into()).unwrap();
        shares.iter().find(|s| s.person.id == person).map(|s| s.level)
    }
}

/// The owner `sh1`: mail `mb1` "Team" (everything) and `mb2` "archive" (read only); a calendar
/// `cal1` (writable) with a weekly event `ev1` and `cal2` (read only) with `ev2`; an address
/// book `b` (writable) with the cards `c1` and `c2` and `ro` (read only) with `c3`.
fn set_up_owner(server: &MockServer) {
    server.sharing(|s| {
        s.share_mail(OWNER).share_calendars(OWNER).share_contacts(OWNER);
        s.add_folder(OWNER, "mb1", "Team", None, &ALL_MAIL_RIGHTS);
        s.add_folder(OWNER, "mb2", "archive", Some("mb1"), &["mayReadItems"]);
        s.add_folder(OWNER, "mb3", "Hidden", None, &[]);
        s.add_message(OWNER, "e1", "mb1", "Shared one", 3600);
        s.add_message(OWNER, "e2", "mb2", "Shared two", 7200);
        s.add_message(OWNER, "e3", "mb3", "Not for you", 10);
        s.calendar(OWNER).add_calendar("cal1", "Team calendar", true, true);
        s.calendar(OWNER).add_calendar("cal2", "Read only", true, false);
        s.calendar(OWNER).add(json!({"@type": "Event", "title": "Standup", "start": "2026-11-16T10:00:00",
        "timeZone": ZONE, "duration": "PT30M", "description": "Daily", "x-keep": "stays",
        "recurrenceRules": [{"@type": "RecurrenceRule", "frequency": "weekly"}]}));
        s.calendar(OWNER).add(json!({"@type": "Event", "title": "Look only", "start": "2026-11-17T10:00:00",
        "timeZone": ZONE, "duration": "PT30M", "calendarIds": {"cal2": true}}));
        s.contacts(OWNER).add_address_book("b", "Team contacts", true);
        s.contacts(OWNER).add_address_book("ro", "Read only", false);
        s.contacts(OWNER).add_contact(json!({"name": {"full": "Zed Zimmer"}, "addressBookIds": {"b": true},
        "emails": {"e": {"address": "zed@shared.test"}}, "x-keep": "stays"}));
        s.contacts(OWNER).add_contact(json!({"name": {"full": "Amy Adler"}, "addressBookIds": {"b": true},
        "emails": {"e": {"address": "amy@shared.test"}}}));
        s.contacts(OWNER).add_contact(json!({"name": {"full": "Ro Reader"}, "addressBookIds": {"ro": true},
        "emails": {"e": {"address": "ro@shared.test"}}}));
    });
}

fn invalid(e: Error) -> String {
    match e {
        Error::Invalid { msg } => msg,
        other => panic!("expected Error::Invalid, got {other:?}"),
    }
}

fn person(id: &str, name: &str, email: &str) -> Person {
    Person { id: id.into(), name: name.into(), email: email.into() }
}

fn input(title: &str, start: &str, calendar: &str) -> EventInput {
    EventInput {
        calendar_id: calendar.into(),
        title: title.into(),
        location: String::new(),
        description: String::new(),
        start: start.into(),
        time_zone: Some(ZONE.into()),
        duration_secs: 3600,
        all_day: false,
        is_task: false,
        repeat: None,
        alert_minutes: Vec::new(),
    }
}

fn a_contact(name: &str, book: &str) -> Contact {
    Contact {
        address_book_id: book.into(),
        name: name.into(),
        given_name: String::new(),
        family_name: String::new(),
        organization: String::new(),
        emails: Vec::new(),
        phones: Vec::new(),
        addresses: Vec::new(),
        notes: String::new(),
    }
}

fn sorted(mut rights: Vec<String>) -> Vec<String> {
    rights.sort();
    rights
}

// ---- finding people ----

#[test]
fn people_are_found_by_text_and_the_user_is_not_one_of_them() {
    let rig = Rig::new();
    let found = rig.core.share_people("person".into()).unwrap();
    assert_eq!(
        found,
        [person("p2", "Second Person", "second@example.test"), person("p3", "Third Person", "third@example.test")]
    );
    // The user matches "alice" and is left out.
    assert!(rig.core.share_people("alice".into()).unwrap().is_empty());
    assert_eq!(rig.core.share_people("THIRD@".into()).unwrap().len(), 1);
    assert!(rig.core.share_people("  ".into()).unwrap().is_empty(), "a blank text asks nobody");
    assert!(rig.core.share_people("nobody".into()).unwrap().is_empty());
    // The server matches whole words only; a part of a name or address is still found.
    assert_eq!(rig.core.share_people("sec".into()).unwrap().len(), 1);
    assert_eq!(rig.core.share_people("second@exa".into()).unwrap().len(), 1);
}

#[test]
fn a_search_gives_at_most_twenty() {
    let rig = Rig::new();
    rig.server.own_shares(|s| {
        for n in 0..30 {
            s.add_person(&format!("x{n:02}"), &format!("Crowd {n:02}"), &format!("crowd{n}@example.test"));
        }
    });
    let found = rig.core.share_people("crowd".into()).unwrap();
    assert_eq!(found.len(), 20);
    assert_eq!(found[0].name, "Crowd 00");
}

#[test]
fn a_server_that_does_not_let_apps_look_users_up_says_so() {
    let rig = Rig::new();
    rig.server.own_shares(|s| s.set_directory(false));
    let words = invalid(rig.core.share_people("second".into()).unwrap_err());
    assert!(words.contains("does not let apps look up other users"), "{words}");
    assert!(words.contains("already shared"), "{words}");
    // A blank text never reaches the server.
    assert!(rig.core.share_people(String::new()).unwrap().is_empty());
}

// ---- whom something is shared with ----

#[test]
fn shares_are_read_for_each_kind_with_names_and_levels() {
    let rig = Rig::new();
    let docs = rig.server.add_folder(ACCOUNT_ID, None, "Docs", None);
    rig.server.own_shares(|s| {
        s.grant("Mailbox", ARCHIVE_ID, "p2", &["mayReadItems"]);
        s.grant("Mailbox", ARCHIVE_ID, "p3", &["mayReadItems", "maySetSeen", "maySetKeywords"]);
        s.grant("Calendar", "cal1", "p2", &["mayReadFreeBusy", "mayReadItems", "mayWriteOwn"]);
        s.grant("AddressBook", "b", "p3", &["mayRead"]);
        s.grant("FileNode", &docs, "p2", &["mayRead", "mayModifyContent"]);
        // An entry with nothing on is no share.
        s.grant("Calendar", "cal1", "p3", &[]);
    });
    let both = [
        Share { person: person("p2", "Second Person", "second@example.test"), level: ShareLevel::Read },
        Share { person: person("p3", "Third Person", "third@example.test"), level: ShareLevel::Write },
    ];
    assert_eq!(rig.core.shares(ShareKind::Mailbox, ARCHIVE_ID.into()).unwrap(), both);
    let calendar = rig.core.shares(ShareKind::Calendar, "cal1".into()).unwrap();
    assert_eq!(
        calendar.iter().map(|s| (s.person.id.as_str(), s.level)).collect::<Vec<_>>(),
        [("p2", ShareLevel::Write)]
    );
    let book = rig.core.shares(ShareKind::AddressBook, "b".into()).unwrap();
    assert_eq!(book.iter().map(|s| (s.person.id.as_str(), s.level)).collect::<Vec<_>>(), [("p3", ShareLevel::Read)]);
    let folder = rig.core.shares(ShareKind::FileFolder, docs).unwrap();
    assert_eq!(folder.iter().map(|s| (s.person.id.as_str(), s.level)).collect::<Vec<_>>(), [("p2", ShareLevel::Write)]);
    assert!(rig.core.shares(ShareKind::Mailbox, INBOX_ID.into()).unwrap().is_empty());
}

#[test]
fn without_a_directory_a_share_is_shown_by_the_id_alone() {
    let rig = Rig::new();
    rig.server.own_shares(|s| {
        s.set_directory(false);
        s.grant("Calendar", "cal1", "p2", &["mayReadItems"]);
        s.grant("Calendar", "cal1", "p3", &["mayReadItems"]);
        // One that shared something with the user is named even then.
        s.add_sharer("p3");
    });
    let shares = rig.core.shares(ShareKind::Calendar, "cal1".into()).unwrap();
    assert_eq!(shares[0], Share { person: person("p2", "p2", ""), level: ShareLevel::Read });
    assert_eq!(shares[1].person, person("p3", "Third Person", "third@example.test"));
}

#[test]
fn an_id_that_is_not_the_users_own_is_refused() {
    let rig = Rig::new();
    let docs = rig.server.add_folder(ACCOUNT_ID, None, "Docs", None);
    let file = rig.server.add_file(ACCOUNT_ID, Some(&docs), "a.txt", b"x", "text/plain");
    for (kind, id, what) in [
        (ShareKind::Mailbox, "zz", "folder"),
        (ShareKind::Calendar, "zz", "calendar"),
        (ShareKind::AddressBook, "zz", "address book"),
        (ShareKind::FileFolder, "zz", "file folder"),
        (ShareKind::FileFolder, file.as_str(), "file folder"),
    ] {
        let words = invalid(rig.core.shares(kind, id.into()).unwrap_err());
        assert_eq!(words, format!("that is not one of your own {what}s"), "{kind:?} {id}");
        let words = invalid(rig.core.set_share(kind, id.into(), "p2".into(), Some(ShareLevel::Read)).unwrap_err());
        assert_eq!(words, format!("that is not one of your own {what}s"), "{kind:?} {id}");
    }
    // The id of one of someone else's folders is not the user's own either: `mb2` of the
    // shared account is the user's Archive here, and what is shared there is not touched.
    let rig = Rig::shared();
    rig.core.set_share(ShareKind::Mailbox, ARCHIVE_ID.into(), "p2".into(), Some(ShareLevel::Read)).unwrap();
    let there = rig.core.shared_mailboxes(OWNER.into()).unwrap();
    assert_eq!(there.len(), 2, "the owner's folders are as they were");
}

// ---- changing a share ----

#[test]
fn only_the_named_persons_entry_is_written() {
    let rig = Rig::new();
    rig.server.own_shares(|s| {
        s.grant("Mailbox", ARCHIVE_ID, "p2", &["mayReadItems"]);
        s.grant("Mailbox", ARCHIVE_ID, "p3", &["mayReadItems", "mayShare", "mayRename"]);
        s.grant("Mailbox", INBOX_ID, "p3", &["mayReadItems"]);
    });
    let after =
        rig.core.set_share(ShareKind::Mailbox, ARCHIVE_ID.into(), "p2".into(), Some(ShareLevel::Write)).unwrap();
    assert_eq!(
        after.iter().map(|s| (s.person.id.as_str(), s.level)).collect::<Vec<_>>(),
        [("p2", ShareLevel::Write), ("p3", ShareLevel::Read)]
    );
    // p3 has only rights that are no content: shown as Read, and kept as it is.
    rig.server.own_shares(|s| {
        // The patch names the one person's entry and nothing else.
        assert_eq!(s.patches.len(), 1);
        let (object, id, patch) = s.patches[0].clone();
        assert_eq!((object.as_str(), id.as_str()), ("Mailbox", ARCHIVE_ID));
        assert_eq!(patch.as_object().unwrap().keys().collect::<Vec<_>>(), ["shareWith/p2"]);
        // The other person kept what they had, the rights the app would never give included.
        assert_eq!(sorted(s.rights("Mailbox", ARCHIVE_ID, "p3")), ["mayReadItems", "mayRename", "mayShare"]);
        assert_eq!(s.rights("Mailbox", INBOX_ID, "p3"), ["mayReadItems"]);
    });
}

#[test]
fn a_level_changes_and_a_share_is_taken_back() {
    let rig = Rig::new();
    let set = |level| rig.core.set_share(ShareKind::Calendar, "cal1".into(), "p2".into(), level);
    assert_eq!(set(Some(ShareLevel::Read)).unwrap().len(), 1);
    assert_eq!(rig.level(ShareKind::Calendar, "cal1", "p2"), Some(ShareLevel::Read));
    set(Some(ShareLevel::Write)).unwrap();
    assert_eq!(rig.level(ShareKind::Calendar, "cal1", "p2"), Some(ShareLevel::Write));
    set(Some(ShareLevel::Read)).unwrap();
    assert_eq!(rig.level(ShareKind::Calendar, "cal1", "p2"), Some(ShareLevel::Read));
    let after = set(None).unwrap();
    assert!(after.is_empty(), "taken back");
    assert_eq!(rig.level(ShareKind::Calendar, "cal1", "p2"), None);
    // Taking back what is not shared is not an error.
    assert!(set(None).unwrap().is_empty());
    let patches: Vec<String> = rig.server.own_shares(|s| {
        s.patches.iter().map(|(_, _, p)| p.as_object().unwrap().keys().next().unwrap().clone()).collect()
    });
    assert_eq!(patches, ["shareWith/p2"; 5]);
}

#[test]
fn each_level_is_exactly_the_rights_of_that_level_for_the_kind() {
    let rig = Rig::new();
    let docs = rig.server.add_folder(ACCOUNT_ID, None, "Docs", None);
    let cases = [
        (
            ShareKind::Mailbox,
            "Mailbox",
            ARCHIVE_ID.to_string(),
            vec!["mayReadItems"],
            vec!["mayAddItems", "mayReadItems", "mayRemoveItems", "maySetKeywords", "maySetSeen"],
        ),
        (
            ShareKind::Calendar,
            "Calendar",
            "cal1".to_string(),
            vec!["mayReadFreeBusy", "mayReadItems"],
            vec!["mayRSVP", "mayReadFreeBusy", "mayReadItems", "mayWriteAll", "mayWriteOwn"],
        ),
        (ShareKind::AddressBook, "AddressBook", "b".to_string(), vec!["mayRead"], vec!["mayRead", "mayWrite"]),
        (
            ShareKind::FileFolder,
            "FileNode",
            docs,
            vec!["mayRead"],
            vec!["mayAddChildren", "mayModifyContent", "mayRead"],
        ),
    ];
    for (kind, object, id, read, write) in cases {
        rig.core.set_share(kind, id.clone(), "p2".into(), Some(ShareLevel::Read)).unwrap();
        assert_eq!(sorted(rig.server.own_shares(|s| s.rights(object, &id, "p2"))), read, "{kind:?} read");
        rig.core.set_share(kind, id.clone(), "p2".into(), Some(ShareLevel::Write)).unwrap();
        let rights = sorted(rig.server.own_shares(|s| s.rights(object, &id, "p2")));
        assert_eq!(rights, write, "{kind:?} write");
        for never in ["mayShare", "mayDelete", "mayRename", "maySetMetadata", "mayCreateChild", "maySubmit"] {
            assert!(!rights.iter().any(|r| r == never), "{kind:?} gives {never}");
        }
        rig.core.set_share(kind, id.clone(), "p2".into(), None).unwrap();
        assert!(rig.server.own_shares(|s| s.people_of(object, &id)).is_empty());
    }
}

#[test]
fn the_folders_that_must_stay_private_and_the_user_are_refused() {
    let rig = Rig::new();
    for (id, word) in [(DRAFTS_ID, "Drafts"), (TRASH_ID, "Trash"), (JUNK_ID, "Junk")] {
        let words = invalid(
            rig.core.set_share(ShareKind::Mailbox, id.into(), "p2".into(), Some(ShareLevel::Read)).unwrap_err(),
        );
        assert!(words.contains(word) && words.contains("cannot be shared"), "{words}");
    }
    assert!(rig.server.own_shares(|s| s.patches.is_empty()), "nothing was sent");
    // The Inbox can be shared; taking back is never refused.
    rig.core.set_share(ShareKind::Mailbox, INBOX_ID.into(), "p2".into(), Some(ShareLevel::Read)).unwrap();
    rig.server.own_shares(|s| s.grant("Mailbox", DRAFTS_ID, "p2", &["mayReadItems"]));
    assert!(rig.core.set_share(ShareKind::Mailbox, DRAFTS_ID.into(), "p2".into(), None).unwrap().is_empty());
    let words = invalid(
        rig.core.set_share(ShareKind::Calendar, "cal1".into(), ACCOUNT_ID.into(), Some(ShareLevel::Read)).unwrap_err(),
    );
    assert_eq!(words, "you cannot share with yourself");
    let words = invalid(
        rig.core.set_share(ShareKind::Calendar, "cal1".into(), " ".into(), Some(ShareLevel::Read)).unwrap_err(),
    );
    assert_eq!(words, "choose whom to share with");
}

#[test]
fn a_person_the_server_does_not_know_is_in_words() {
    let rig = Rig::new();
    let words = invalid(
        rig.core.set_share(ShareKind::AddressBook, "b".into(), "zz".into(), Some(ShareLevel::Read)).unwrap_err(),
    );
    assert_eq!(words, "that person is not a user of this server");
}

#[test]
fn a_person_id_with_a_slash_is_one_key() {
    let rig = Rig::new();
    rig.server.own_shares(|s| s.add_person("a/b", "Slash", "slash@example.test"));
    rig.core.set_share(ShareKind::Calendar, "cal1".into(), "a/b".into(), Some(ShareLevel::Read)).unwrap();
    assert_eq!(rig.level(ShareKind::Calendar, "cal1", "a/b"), Some(ShareLevel::Read));
}

// ---- writing events in a shared calendar ----

#[test]
fn an_event_is_made_changed_and_deleted_in_a_shared_calendar_and_nowhere_else() {
    let rig = Rig::shared();
    let own_before = rig.server.events().len();
    let id = rig.core.shared_save_event(OWNER.into(), None, input("Planning", "2026-11-18T09:00:00", "cal1")).unwrap();
    let made = rig.server.sharing(|s| s.calendar(OWNER).event(&id)).expect("the owner's calendar has it");
    assert_eq!(made["title"], "Planning");
    assert_eq!(rig.server.events().len(), own_before, "the user's own calendar is untouched");

    let mut changed = rig.core.shared_event(OWNER.into(), id.clone(), None).unwrap().input;
    changed.title = "Planning, moved".into();
    rig.core.shared_save_event(OWNER.into(), Some(id.clone()), changed).unwrap();
    let now = rig.server.sharing(|s| s.calendar(OWNER).event(&id)).unwrap();
    assert_eq!(now["title"], "Planning, moved");
    assert_eq!(now["start"], "2026-11-18T09:00:00");

    rig.core.shared_delete_event(OWNER.into(), id.clone(), None).unwrap();
    assert!(rig.server.sharing(|s| s.calendar(OWNER).event(&id)).is_none());
    assert_eq!(rig.server.events().len(), own_before);
}

#[test]
fn a_change_to_a_shared_event_leaves_what_the_app_does_not_own() {
    let rig = Rig::shared();
    let mut detail = rig.core.shared_event(OWNER.into(), "ev1".into(), None).unwrap();
    assert!(detail.may_write);
    detail.input.title = "Standup, renamed".into();
    rig.core.shared_save_event(OWNER.into(), Some("ev1".into()), detail.input).unwrap();
    let event = rig.server.sharing(|s| s.calendar(OWNER).event("ev1")).unwrap();
    assert_eq!(event["title"], "Standup, renamed");
    assert_eq!(event["x-keep"], "stays", "a property the app does not know stays");
    assert_eq!(event["recurrenceRules"][0]["frequency"], "weekly");
    assert_eq!(event["description"], "Daily");
}

#[test]
fn one_occurrence_of_a_shared_repeating_event_can_be_deleted() {
    let rig = Rig::shared();
    let start = 1_794_787_200 - 3 * 3600;
    let agenda = rig.core.shared_agenda(OWNER.into(), start, start + 21 * 86_400, ZONE.into()).unwrap();
    let before = agenda.entries.iter().filter(|e| e.event_id == "ev1").count();
    let second = agenda.entries.iter().find(|e| e.event_id == "ev1").unwrap().recurrence_id.clone();
    rig.core.shared_delete_event(OWNER.into(), "ev1".into(), second).unwrap();
    let after = rig.core.shared_agenda(OWNER.into(), start, start + 21 * 86_400, ZONE.into()).unwrap();
    assert_eq!(after.entries.iter().filter(|e| e.event_id == "ev1").count(), before - 1);
    assert!(rig.server.sharing(|s| s.calendar(OWNER).event("ev1")).is_some(), "the series stays");
}

#[test]
fn a_write_the_rights_do_not_allow_is_refused_in_words() {
    let rig = Rig::shared();
    // The event of the read-only calendar: the app knows it may not, and says so before asking.
    let mut detail = rig.core.shared_event(OWNER.into(), "ev2".into(), None).unwrap();
    assert!(!detail.may_write);
    detail.input.title = "Hijacked".into();
    let words = invalid(rig.core.shared_save_event(OWNER.into(), Some("ev2".into()), detail.input).unwrap_err());
    assert_eq!(words, "You cannot change the calendar 'Read only'.");
    let made = rig.core.shared_save_event(OWNER.into(), None, input("New", "2026-11-19T09:00:00", "cal2")).unwrap_err();
    assert_eq!(invalid(made), "You cannot change the calendar 'Read only'.");
    // A delete is not checked first: the server refuses and the words say what.
    let words = invalid(rig.core.shared_delete_event(OWNER.into(), "ev2".into(), None).unwrap_err());
    assert_eq!(words, "you are not allowed to delete events from this calendar");
    let event = rig.server.sharing(|s| s.calendar(OWNER).event("ev2")).unwrap();
    assert_eq!(event["title"], "Look only");
    assert!(!rig.server.sharing(|s| s.calendar(OWNER).events().iter().any(|e| e["title"] == "New")));
}

#[test]
fn the_servers_refusal_is_in_words_whatever_it_names() {
    let rig = Rig::shared();
    // The calendar looked writable when the app read it, and the rights were taken back before
    // the write: the server's own text names an id, the words say what could not be done.
    let mut detail = rig.core.shared_event(OWNER.into(), "ev1".into(), None).unwrap();
    rig.server.sharing(|s| {
        s.deny_writes.insert(OWNER.into());
    });
    detail.input.title = "Late".into();
    let words = invalid(rig.core.shared_save_event(OWNER.into(), Some("ev1".into()), detail.input).unwrap_err());
    assert_eq!(words, "you are not allowed to change events in this calendar");
    let made =
        rig.core.shared_save_event(OWNER.into(), None, input("Late", "2026-11-19T09:00:00", "cal1")).unwrap_err();
    assert_eq!(invalid(made), "you are not allowed to change events in this calendar");
    let words = invalid(rig.core.shared_delete_event(OWNER.into(), "ev1".into(), None).unwrap_err());
    assert_eq!(words, "you are not allowed to delete events from this calendar");
    let mut contact = rig.core.shared_contact(OWNER.into(), "c1".into()).unwrap().contact;
    contact.name = "Late".into();
    let words = invalid(rig.core.shared_save_contact(OWNER.into(), Some("c1".into()), contact.clone()).unwrap_err());
    assert_eq!(words, "you are not allowed to change contacts in this address book");
    let words = invalid(rig.core.shared_save_contact(OWNER.into(), None, a_contact("Late", "b")).unwrap_err());
    assert_eq!(words, "you are not allowed to change contacts in this address book");
    let words = invalid(rig.core.shared_delete_contact(OWNER.into(), "c1".into()).unwrap_err());
    assert_eq!(words, "you are not allowed to delete contacts from this address book");
    let event = rig.server.sharing(|s| s.calendar(OWNER).event("ev1")).unwrap();
    assert_eq!(event["title"], "Standup");
}

// ---- contacts in a shared address book ----

#[test]
fn a_contact_is_made_changed_and_deleted_in_a_shared_address_book() {
    let rig = Rig::shared();
    let own_before = rig.server.contact_cards().len();
    let mut contact = rig.core.shared_contact(OWNER.into(), "c1".into()).unwrap().contact;
    assert_eq!(contact.address_book_id, "b");
    contact.name = "Zed Zimmerman".into();
    rig.core.shared_save_contact(OWNER.into(), Some("c1".into()), contact.clone()).unwrap();
    let card = rig.server.sharing(|s| s.contacts(OWNER).card("c1")).unwrap();
    assert_eq!(card["name"]["full"], "Zed Zimmerman");
    assert_eq!(card["x-keep"], "stays", "what the app does not show stays on the card");

    let fresh = a_contact("New Person", "b");
    let id = rig.core.shared_save_contact(OWNER.into(), None, fresh).unwrap();
    assert_eq!(rig.server.sharing(|s| s.contacts(OWNER).card(&id)).unwrap()["name"]["full"], "New Person");
    rig.core.shared_delete_contact(OWNER.into(), id.clone()).unwrap();
    assert!(rig.server.sharing(|s| s.contacts(OWNER).card(&id)).is_none());
    assert_eq!(rig.server.contact_cards().len(), own_before, "the user's own contacts are untouched");
}

#[test]
fn a_contact_write_the_rights_do_not_allow_is_refused_in_words() {
    let rig = Rig::shared();
    let mut contact = rig.core.shared_contact(OWNER.into(), "c3".into()).unwrap();
    assert!(!contact.may_write);
    contact.contact.name = "Hijacked".into();
    let words = invalid(rig.core.shared_save_contact(OWNER.into(), Some("c3".into()), contact.contact).unwrap_err());
    assert_eq!(words, "The address book \"Read only\" is read-only.");
    let words = invalid(rig.core.shared_delete_contact(OWNER.into(), "c3".into()).unwrap_err());
    assert_eq!(words, "you are not allowed to delete contacts from this address book");
    assert_eq!(rig.server.sharing(|s| s.contacts(OWNER).card("c3")).unwrap()["name"]["full"], "Ro Reader");
    let fresh = a_contact("Late", "ro");
    assert!(rig.core.shared_save_contact(OWNER.into(), None, fresh).is_err());
    assert_eq!(rig.server.sharing(|s| s.contacts(OWNER).cards().len()), 3);
}

#[test]
fn the_address_books_of_an_account_come_with_their_rights() {
    let rig = Rig::shared();
    let books = rig.core.shared_address_books(OWNER.into()).unwrap();
    assert_eq!(books.iter().map(|b| (b.id.as_str(), b.may_write)).collect::<Vec<_>>(), [("b", true), ("ro", false)]);
}

#[test]
fn writing_into_an_account_that_is_not_shared_is_invalid() {
    let rig = Rig::shared();
    let nobody = "sh2";
    let e = rig.core.shared_save_event(nobody.into(), None, input("x", "2026-11-18T09:00:00", "cal1")).unwrap_err();
    assert_eq!(invalid(e), "this is no longer shared with you");
    assert!(rig.core.shared_delete_event(nobody.into(), "ev1".into(), None).is_err());
    assert!(rig.core.shared_address_books(nobody.into()).is_err());
    assert!(rig.core.shared_save_contact(nobody.into(), None, a_contact("", "b")).is_err());
    assert!(rig.core.shared_delete_contact(nobody.into(), "c1".into()).is_err());
    assert!(rig.core.new_draft_from_shared(nobody.into(), DraftKind::Reply { message_id: "e1".into() }).is_err());
    // The account was taken back after the connection was made.
    rig.server.sharing(|s| s.set_listed(OWNER, false));
    assert!(rig.core.shared_save_event(OWNER.into(), None, input("x", "2026-11-18T09:00:00", "cal1")).is_err());
}

// ---- answering and forwarding a shared message ----

#[test]
fn a_reply_to_a_shared_message_is_an_own_draft_sent_from_the_own_identity() {
    let rig = Rig::shared();
    rig.sync();
    let before = rig.rows_of(OWNER);
    let mut draft = rig.core.new_draft_from_shared(OWNER.into(), DraftKind::Reply { message_id: "e1".into() }).unwrap();
    assert_eq!(draft.subject, "Re: Shared one");
    assert_eq!(draft.to.iter().map(|a| a.email.as_str()).collect::<Vec<_>>(), ["owner@shared.test"]);
    assert!(draft.body.contains("Body of Shared one"), "the original is quoted");
    assert_eq!(draft.source_message_id, None, "the source is not a message of the user's account");
    assert!(!draft.is_forward);
    let identities = rig.core.identities().unwrap();
    assert_eq!(draft.identity_id, identities[0].id, "from the user's own address");
    // It is an ordinary own draft: in the cache, listed, and nothing of the shared message is.
    assert_eq!(rig.core.drafts().unwrap().len(), 1);
    assert_eq!(rig.rows_of(OWNER), before);

    draft.body = format!("Thanks!\n{}", draft.body);
    rig.core.send(draft, 0).unwrap();
    rig.sync();
    let sent = rig.server.submissions();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].identity_id, identities[0].id);
    assert_eq!(sent[0].recipients, ["owner@shared.test"]);
    let view = rig.server.view(&sent[0].email_id);
    assert_eq!(view.from.1, identities[0].email);
    assert_eq!(view.in_reply_to, ["m-e1@shared.test"]);
    assert_eq!(view.references, ["root-e1@shared.test", "m-e1@shared.test"]);
    // Nothing was marked answered: not the shared message, and not the user's own message that
    // has the same id.
    assert!(!rig.server.sharing(|s| s.keywords_of(OWNER, "e1")).unwrap().contains(&"$answered".to_string()));
    assert!(!rig.server.keywords_of("e1").contains(&"$answered".to_string()));
}

#[test]
fn reply_all_and_forward_are_answers_to_a_shared_message_too() {
    let rig = Rig::shared();
    rig.sync();
    let all = rig.core.new_draft_from_shared(OWNER.into(), DraftKind::ReplyAll { message_id: "e1".into() }).unwrap();
    assert_eq!(all.subject, "Re: Shared one");
    assert!(!all.is_forward);
    let forward = rig.core.new_draft_from_shared(OWNER.into(), DraftKind::Forward { message_id: "e1".into() }).unwrap();
    assert_eq!(forward.subject, "Fwd: Shared one");
    assert!(forward.is_forward && forward.to.is_empty());
    assert!(forward.body.contains("Body of Shared one"));
    assert_eq!(rig.core.drafts().unwrap().len(), 2);
}

#[test]
fn only_a_reply_or_a_forward_can_be_made_from_a_shared_message() {
    let rig = Rig::shared();
    for kind in [DraftKind::New, DraftKind::Existing { message_id: "e1".into() }] {
        assert!(matches!(rig.core.new_draft_from_shared(OWNER.into(), kind), Err(Error::Invalid { .. })));
    }
    // A message the user may not read is gone; no draft is made.
    let e = rig.core.new_draft_from_shared(OWNER.into(), DraftKind::Reply { message_id: "e3".into() }).unwrap_err();
    assert_eq!(invalid(e), "that message is no longer there");
    assert!(rig.core.drafts().unwrap().is_empty());
}

#[test]
fn a_forwarded_attachment_is_a_blob_of_the_users_own_account_and_is_sent() {
    let rig = Rig::shared();
    rig.server.sharing(|s| {
        s.add_message_file(OWNER, "e1", "plan.txt", "text/plain", b"the plan");
    });
    rig.sync();
    let draft = rig.core.new_draft_from_shared(OWNER.into(), DraftKind::Forward { message_id: "e1".into() }).unwrap();
    assert_eq!(draft.attachments.len(), 1);
    let file = &draft.attachments[0];
    assert_eq!((file.name.as_str(), file.mime_type.as_str(), file.size), ("plan.txt", "text/plain", 8));
    assert!(!file.blob_id.starts_with("sb-"), "not the blob of the shared account: {}", file.blob_id);
    // The copied file is pinned at once by a Drafts copy on the server.
    rig.sync();
    assert_eq!(rig.server.ids_in("drafts").len(), 1, "a forward with files has a server copy");

    let mut draft = draft;
    draft.to = vec![kweb_core::model::Address { name: String::new(), email: "bo@example.test".into() }];
    rig.core.send(draft, 0).unwrap();
    rig.sync();
    let sent = rig.server.submissions();
    assert_eq!(sent.len(), 1);
    let view = rig.server.view(&sent[0].email_id);
    assert_eq!(view.files, [("plan.txt".to_string(), "text/plain".to_string(), b"the plan".to_vec())]);
    // The shared message and its file were not touched.
    assert!(rig.server.sharing(|s| s.message_exists(OWNER, "e1")));
}

#[test]
fn a_forwarded_attachment_larger_than_the_server_takes_makes_no_draft() {
    let rig = Rig::shared();
    rig.server.sharing(|s| {
        s.add_message_file(OWNER, "e1", "big.bin", "application/octet-stream", &[7u8; 64]);
    });
    rig.server.set_upload_limit(16);
    let e = rig.core.new_draft_from_shared(OWNER.into(), DraftKind::Forward { message_id: "e1".into() }).unwrap_err();
    assert!(invalid(e).contains("larger than the server takes"));
    assert!(rig.core.drafts().unwrap().is_empty());
}

// ---- nothing is kept ----

#[test]
fn nothing_of_a_shared_account_reaches_the_cache() {
    let rig = Rig::shared();
    rig.sync();
    let before = rig.cache();
    let id = rig.core.shared_save_event(OWNER.into(), None, input("Planning", "2026-11-18T09:00:00", "cal1")).unwrap();
    rig.core
        .shared_save_event(OWNER.into(), Some(id.clone()), input("Planning 2", "2026-11-18T09:00:00", "cal1"))
        .unwrap();
    rig.core.shared_delete_event(OWNER.into(), id, None).unwrap();
    let card = rig.core.shared_save_contact(OWNER.into(), None, a_contact("Kept?", "b")).unwrap();
    rig.core.shared_delete_contact(OWNER.into(), card).unwrap();
    rig.core.shared_address_books(OWNER.into()).unwrap();
    rig.core.shared_contact(OWNER.into(), "c1".into()).unwrap();
    assert_eq!(rig.cache(), before, "not a row of the cache changed");
    assert_eq!(rig.rows_of(OWNER), 0);
    // Sharing one's own things keeps nothing either.
    rig.core.set_share(ShareKind::Calendar, "cal1".into(), "p2".into(), Some(ShareLevel::Read)).unwrap();
    rig.core.shares(ShareKind::Calendar, "cal1".into()).unwrap();
    assert_eq!(rig.cache(), before);
}

#[test]
fn a_reply_leaves_only_the_draft_in_the_cache() {
    let rig = Rig::shared();
    rig.sync();
    let before = rig.cache();
    rig.core.new_draft_from_shared(OWNER.into(), DraftKind::Reply { message_id: "e1".into() }).unwrap();
    let after = rig.cache();
    let changed: Vec<(&String, i64)> =
        after.iter().filter(|(t, n)| before.get(*t) != Some(n)).map(|(t, n)| (t, *n)).collect();
    assert_eq!(changed.iter().map(|(t, _)| t.as_str()).collect::<Vec<_>>(), ["drafts"], "{changed:?}");
    assert_eq!(rig.rows_of(OWNER), 0);
}
