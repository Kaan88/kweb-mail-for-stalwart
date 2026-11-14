use super::*;
use crate::contacts::Entry;
use crate::model::{
    Action, Address, AddressBook, Attachment, Calendar, Contact, Labeled, ListFilter, Mailbox, MessageBody,
    MessageHeader, Occurrence, ThreadRow,
};

const A: &str = "acc1";

fn s() -> Store {
    let st = Store::open_in_memory().unwrap();
    st.save_account(&account(A)).unwrap();
    st
}

fn account(id: &str) -> Account {
    Account {
        id: id.into(),
        server_url: "https://mail.example.com".into(),
        username: "kaan".into(),
        signed_in_at: 1000,
    }
}

fn mbox(id: &str, name: &str, role: Option<&str>, sort: u32) -> Mailbox {
    Mailbox {
        id: id.into(),
        name: name.into(),
        role: role.map(String::from),
        parent_id: None,
        sort_order: sort,
        total_emails: 3,
        unread_emails: 1,
        notify: false,
    }
}

fn msg(id: &str, boxes: &[&str], at: i64, unread: bool) -> MessageHeader {
    MessageHeader {
        id: id.into(),
        thread_id: format!("t-{id}"),
        mailbox_ids: boxes.iter().map(|b| b.to_string()).collect(),
        from_name: "Ann".into(),
        from_email: "ann@example.com".into(),
        to: Vec::new(),
        subject: format!("subject {id}"),
        preview: "preview".into(),
        received_at: at,
        is_unread: unread,
        is_flagged: false,
        has_attachment: true,
        size: 5_000_000_000,
    }
}

fn ids(v: &[MessageHeader]) -> Vec<&str> {
    v.iter().map(|m| m.id.as_str()).collect()
}

fn strs(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

#[test]
fn store_is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Store>();
}

#[test]
fn account_save_load_update() {
    let st = Store::open_in_memory().unwrap();
    assert_eq!(st.load_account().unwrap(), None);
    st.save_account(&account(A)).unwrap();
    let mut a = account(A);
    a.username = "other".into();
    st.save_account(&a).unwrap();
    assert_eq!(st.load_account().unwrap(), Some(a));
}

#[test]
fn updating_account_keeps_its_data() {
    let st = s();
    st.kv_set(A, "k", "v").unwrap();
    st.save_account(&account(A)).unwrap();
    assert_eq!(st.kv_get(A, "k").unwrap().as_deref(), Some("v"));
}

#[test]
fn kv_roundtrip() {
    let st = s();
    assert_eq!(st.kv_get(A, "oauth").unwrap(), None);
    st.kv_set(A, "oauth", "one").unwrap();
    st.kv_set(A, "oauth", "two").unwrap();
    assert_eq!(st.kv_get(A, "oauth").unwrap().as_deref(), Some("two"));
    st.kv_delete(A, "oauth").unwrap();
    assert_eq!(st.kv_get(A, "oauth").unwrap(), None);
}

#[test]
fn state_roundtrip() {
    let st = s();
    assert_eq!(st.get_state(A, "Email").unwrap(), None);
    st.set_state(A, "Email", "s1").unwrap();
    st.set_state(A, "Mailbox", "m1").unwrap();
    st.set_state(A, "Email", "s2").unwrap();
    assert_eq!(st.get_state(A, "Email").unwrap().as_deref(), Some("s2"));
    st.clear_state(A, "Email").unwrap();
    assert_eq!(st.get_state(A, "Email").unwrap(), None);
    assert_eq!(st.get_state(A, "Mailbox").unwrap().as_deref(), Some("m1"));
}

#[test]
fn mailboxes_crud_and_order() {
    let st = s();
    st.upsert_mailboxes(
        A,
        &[mbox("b", "Zeta", None, 2), mbox("a", "Inbox", Some("inbox"), 1), mbox("c", "Alpha", None, 2)],
    )
    .unwrap();
    let names: Vec<_> = st.list_mailboxes(A).unwrap().into_iter().map(|m| m.name).collect();
    assert_eq!(names, ["Inbox", "Alpha", "Zeta"]);

    assert_eq!(st.mailbox_by_role(A, "inbox").unwrap().unwrap().id, "a");
    assert_eq!(st.mailbox_by_role(A, "trash").unwrap(), None);
    assert_eq!(st.get_mailbox(A, "b").unwrap().unwrap().name, "Zeta");

    let mut changed = mbox("b", "Renamed", None, 2);
    changed.parent_id = Some("a".into());
    changed.unread_emails = 9;
    st.upsert_mailboxes(A, &[changed.clone()]).unwrap();
    assert_eq!(st.get_mailbox(A, "b").unwrap(), Some(changed));

    st.delete_mailboxes(A, &strs(&["b", "c"])).unwrap();
    assert_eq!(st.list_mailboxes(A).unwrap().len(), 1);

    st.replace_mailboxes(A, &[mbox("x", "New", None, 0)]).unwrap();
    let all = st.list_mailboxes(A).unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].id, "x");
}

#[test]
fn messages_roundtrip_and_replace() {
    let st = s();
    let m = msg("m1", &["inbox"], 100, true);
    st.upsert_messages(A, std::slice::from_ref(&m)).unwrap();
    assert_eq!(st.get_message(A, "m1").unwrap(), Some(m));
    assert_eq!(st.get_message(A, "nope").unwrap(), None);

    st.replace_messages(A, &[msg("m2", &["inbox"], 200, true), msg("m3", &["inbox"], 300, true)]).unwrap();
    assert_eq!(st.get_message(A, "m1").unwrap(), None);
    assert_eq!(ids(&st.list_messages(A, "inbox", 10, 0).unwrap()), ["m3", "m2"]);
}

#[test]
fn list_is_newest_first_with_paging() {
    let st = s();
    let all: Vec<_> = (1..=5).map(|i| msg(&format!("m{i}"), &["inbox"], i * 10, true)).collect();
    st.upsert_messages(A, &all).unwrap();
    st.upsert_messages(A, &[msg("other", &["archive"], 999, true)]).unwrap();
    assert_eq!(ids(&st.list_messages(A, "inbox", 2, 0).unwrap()), ["m5", "m4"]);
    assert_eq!(ids(&st.list_messages(A, "inbox", 2, 2).unwrap()), ["m3", "m2"]);
    assert_eq!(ids(&st.list_messages(A, "inbox", 2, 4).unwrap()), ["m1"]);
}

#[test]
fn unread_count_and_partial_updates() {
    let st = s();
    st.upsert_messages(
        A,
        &[msg("a", &["inbox"], 1, true), msg("b", &["inbox"], 2, true), msg("c", &["inbox"], 3, false)],
    )
    .unwrap();
    assert_eq!(st.count_unread(A, "inbox").unwrap(), 2);

    assert!(st.set_unread(A, "a", false).unwrap());
    assert_eq!(st.count_unread(A, "inbox").unwrap(), 1);
    assert!(st.set_flagged(A, "a", true).unwrap());
    assert!(st.get_message(A, "a").unwrap().unwrap().is_flagged);
    assert!(!st.set_unread(A, "missing", true).unwrap());

    assert!(st.set_message_mailboxes(A, "a", &strs(&["archive"])).unwrap());
    assert_eq!(st.get_message(A, "a").unwrap().unwrap().mailbox_ids, ["archive"]);
    assert!(!st.set_message_mailboxes(A, "missing", &strs(&["archive"])).unwrap());
    assert_eq!(st.count_unread(A, "inbox").unwrap(), 1);
}

#[test]
fn message_in_two_mailboxes_and_membership_replaced_on_upsert() {
    let st = s();
    st.upsert_messages(A, &[msg("m", &["inbox", "label"], 1, true)]).unwrap();
    assert_eq!(ids(&st.list_messages(A, "inbox", 10, 0).unwrap()), ["m"]);
    assert_eq!(ids(&st.list_messages(A, "label", 10, 0).unwrap()), ["m"]);
    // Listing must not duplicate the message.
    assert_eq!(st.get_message(A, "m").unwrap().unwrap().mailbox_ids, ["inbox", "label"]);

    st.upsert_messages(A, &[msg("m", &["inbox"], 1, true)]).unwrap();
    assert_eq!(ids(&st.list_messages(A, "inbox", 10, 0).unwrap()), ["m"]);
    assert!(st.list_messages(A, "label", 10, 0).unwrap().is_empty());
}

#[test]
fn messages_without_mailbox_are_kept_and_delete_works() {
    let st = s();
    st.upsert_messages(A, &[msg("m", &[], 1, true), msg("n", &["inbox"], 2, true)]).unwrap();
    assert!(st.get_message(A, "m").unwrap().is_some());
    st.delete_messages(A, &strs(&["n", "m"])).unwrap();
    assert!(st.get_message(A, "m").unwrap().is_none());
    assert!(st.list_messages(A, "inbox", 10, 0).unwrap().is_empty());
}

#[test]
fn notifiable_filters() {
    let st = s();
    st.upsert_messages(
        A,
        &[
            msg("new1", &["inbox"], 500, true),
            msg("new2", &["inbox"], 600, true),
            msg("edge", &["inbox"], 400, true),
            msg("read", &["inbox"], 700, false),
            msg("old", &["inbox"], 399, true),
            msg("dismissed", &["inbox"], 800, true),
            msg("elsewhere", &["archive"], 900, true),
            msg("both", &["archive", "inbox"], 450, true),
        ],
    )
    .unwrap();
    st.dismissed_add(A, "dismissed").unwrap();
    assert_eq!(ids(&st.notifiable(A, "inbox", 400).unwrap()), ["new2", "new1", "both", "edge"]);
}

#[test]
fn shown_set() {
    let st = s();
    st.shown_add(A, "b", 20).unwrap();
    st.shown_add(A, "a", 10).unwrap();
    assert_eq!(st.shown_list(A).unwrap(), [("a".to_string(), 10), ("b".to_string(), 20)]);
    st.shown_add(A, "a", 30).unwrap();
    assert_eq!(st.shown_list(A).unwrap()[1], ("a".to_string(), 30));
    st.shown_remove(A, "a").unwrap();
    st.shown_remove(A, "unknown").unwrap();
    assert_eq!(st.shown_list(A).unwrap(), [("b".to_string(), 20)]);
}

#[test]
fn dismissing_removes_from_shown_and_prune_drops_stale_rows() {
    let st = s();
    st.upsert_messages(A, &[msg("unread", &["inbox"], 1, true), msg("read", &["inbox"], 2, false)]).unwrap();
    st.shown_add(A, "unread", 5).unwrap();
    for id in ["unread", "read", "gone"] {
        st.dismissed_add(A, id).unwrap();
    }
    assert!(st.shown_list(A).unwrap().is_empty());
    assert!(st.is_dismissed(A, "unread").unwrap());
    assert!(!st.is_dismissed(A, "other").unwrap());

    assert_eq!(st.prune_dismissed(A).unwrap(), 2);
    assert!(st.is_dismissed(A, "unread").unwrap());
    assert!(!st.is_dismissed(A, "read").unwrap());
    assert!(!st.is_dismissed(A, "gone").unwrap());
}

#[test]
fn outbox_lifecycle() {
    let st = s();
    let first = st.outbox_enqueue(A, &Action::MarkRead { message_id: "m1".into() }).unwrap();
    let second = st.outbox_enqueue(A, &Action::Archive { message_id: "m2".into() }).unwrap();
    st.outbox_enqueue(A, &Action::Delete { message_id: "m3".into() }).unwrap();
    st.outbox_enqueue(A, &Action::MarkUnread { message_id: "m4".into() }).unwrap();
    st.outbox_enqueue(A, &Action::Destroy { message_id: "m5".into() }).unwrap();
    assert!(second > first);

    let pending = st.outbox_pending(A).unwrap();
    assert_eq!(pending.len(), 5);
    assert_eq!(pending[0], (first, Action::MarkRead { message_id: "m1".into() }, 0));
    assert_eq!(pending[2].1, Action::Delete { message_id: "m3".into() });
    assert_eq!(pending[3].1, Action::MarkUnread { message_id: "m4".into() });
    assert_eq!(pending[4].1, Action::Destroy { message_id: "m5".into() });

    st.outbox_failed(first, "timeout").unwrap();
    st.outbox_failed(first, "502").unwrap();
    assert_eq!(st.outbox_pending(A).unwrap()[0].2, 2);
    assert_eq!(st.outbox_last_error(first).unwrap().as_deref(), Some("502"));

    st.outbox_done(first).unwrap();
    let pending = st.outbox_pending(A).unwrap();
    assert_eq!(pending.len(), 4);
    assert_eq!(pending[0].0, second);
}

#[test]
fn outbox_skips_unknown_kinds() {
    let st = s();
    st.lock().execute("INSERT INTO outbox (account_id, kind, message_id) VALUES (?1, 'future', 'x')", [A]).unwrap();
    st.outbox_enqueue(A, &Action::Delete { message_id: "m".into() }).unwrap();
    assert_eq!(st.outbox_pending(A).unwrap().len(), 1);
}

fn count(st: &Store, table: &str) -> i64 {
    st.lock().query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0)).unwrap()
}

#[test]
fn delete_account_leaves_nothing() {
    let st = s();
    st.save_account(&account("acc2")).unwrap();
    st.kv_set(A, "k", "v").unwrap();
    st.set_state(A, "Email", "s").unwrap();
    st.upsert_mailboxes(A, &[mbox("inbox", "Inbox", Some("inbox"), 0)]).unwrap();
    st.upsert_messages(A, &[msg("m", &["inbox", "x"], 1, true)]).unwrap();
    st.shown_add(A, "m", 1).unwrap();
    st.dismissed_add(A, "z").unwrap();
    st.outbox_enqueue(A, &Action::Delete { message_id: "m".into() }).unwrap();
    // Another account's data must survive.
    st.kv_set("acc2", "k", "v").unwrap();
    st.upsert_messages("acc2", &[msg("m", &["inbox"], 1, true)]).unwrap();

    st.delete_account(A).unwrap();

    for table in [
        "kv",
        "sync_state",
        "mailboxes",
        "messages",
        "message_mailboxes",
        "notifications_shown",
        "notifications_dismissed",
        "outbox",
    ] {
        let left: i64 = st
            .lock()
            .query_row(&format!("SELECT COUNT(*) FROM {table} WHERE account_id = ?1"), [A], |r| r.get(0))
            .unwrap();
        assert_eq!(left, 0, "{table} still has rows");
    }
    assert_eq!(count(&st, "accounts"), 1);
    assert_eq!(count(&st, "messages"), 1);
    assert_eq!(st.kv_get("acc2", "k").unwrap().as_deref(), Some("v"));
}

#[test]
fn accounts_do_not_see_each_other() {
    let st = s();
    st.save_account(&account("acc2")).unwrap();
    st.upsert_messages(A, &[msg("m", &["inbox"], 1, true)]).unwrap();
    assert!(st.list_messages("acc2", "inbox", 10, 0).unwrap().is_empty());
    assert!(st.get_message("acc2", "m").unwrap().is_none());
}

#[test]
fn failed_multi_row_write_rolls_back() {
    let st = s();
    st.upsert_messages(A, &[msg("keep", &["inbox"], 1, true)]).unwrap();
    // An unknown account breaks the foreign key on the second... first row; nothing may remain.
    let err = st.upsert_messages("nobody", &[msg("x", &["inbox"], 1, true)]);
    assert!(err.is_err());
    assert_eq!(count(&st, "messages"), 1);
}

#[test]
fn reopening_a_file_keeps_data_and_skips_migrations() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cache.db");
    {
        let st = Store::open(&path).unwrap();
        st.save_account(&account(A)).unwrap();
        st.upsert_messages(A, &[msg("m", &["inbox"], 1, true)]).unwrap();
        assert_eq!(st.schema_version().unwrap() as usize, schema::MIGRATIONS.len());
        let mode: String = st.lock().query_row("PRAGMA journal_mode", [], |r| r.get(0)).unwrap();
        assert_eq!(mode, "wal");
    }
    let st = Store::open(&path).unwrap();
    assert!(st.get_message(A, "m").unwrap().is_some());
    assert_eq!(st.load_account().unwrap().unwrap().id, A);
    // Nothing left to run on an up-to-date database.
    assert_eq!(schema::migrate(&mut st.lock(), schema::MIGRATIONS).unwrap(), 0);
}

#[test]
fn migrations_apply_only_new_steps() {
    let mut conn = Connection::open_in_memory().unwrap();
    let v1 = ["CREATE TABLE t (a TEXT);"];
    assert_eq!(schema::migrate(&mut conn, &v1).unwrap(), 1);
    conn.execute("INSERT INTO t VALUES ('x')", []).unwrap();

    let v2 = ["CREATE TABLE t (a TEXT);", "ALTER TABLE t ADD COLUMN b TEXT;"];
    assert_eq!(schema::migrate(&mut conn, &v2).unwrap(), 1);
    assert_eq!(schema::migrate(&mut conn, &v2).unwrap(), 0);
    let n: i64 = conn.query_row("SELECT COUNT(*) FROM t WHERE b IS NULL", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 1);

    // A database from a newer app is refused.
    assert!(schema::migrate(&mut conn, &v1).is_err());
}

// ---- M3: reading ----

fn in_thread(mut m: MessageHeader, thread: &str) -> MessageHeader {
    m.thread_id = thread.into();
    m
}

fn from(mut m: MessageHeader, name: &str, email: &str) -> MessageHeader {
    m.from_name = name.into();
    m.from_email = email.into();
    m
}

fn read(mut m: MessageHeader) -> MessageHeader {
    m.is_unread = false;
    m
}

fn plain(mut m: MessageHeader) -> MessageHeader {
    m.has_attachment = false;
    m
}

fn flagged(mut m: MessageHeader) -> MessageHeader {
    m.is_flagged = true;
    m
}

fn rows(st: &Store, mailbox: &str, filter: ListFilter, limit: u32, offset: u32) -> Vec<ThreadRow> {
    st.list_threads(A, mailbox, filter, limit, offset).unwrap()
}

fn newest_ids(rows: &[ThreadRow]) -> Vec<&str> {
    rows.iter().map(|r| r.newest.id.as_str()).collect()
}

fn body(id: &str) -> MessageBody {
    MessageBody {
        message_id: id.into(),
        blob_id: format!("blob-{id}"),
        to: vec![Address { name: "Bo".into(), email: "bo@example.com".into() }],
        cc: Vec::new(),
        bcc: Vec::new(),
        reply_to: Vec::new(),
        sent_at: Some(99),
        html: Some("<p>hi</p>".into()),
        text: Some("hi".into()),
        attachments: vec![Attachment {
            blob_id: "att".into(),
            name: "a.pdf".into(),
            mime_type: "application/pdf".into(),
            size: 10,
            content_id: None,
            is_inline: false,
        }],
        is_truncated: false,
    }
}

#[test]
fn migration_two_keeps_what_migration_one_stored() {
    let mut conn = Connection::open_in_memory().unwrap();
    conn.pragma_update(None, "foreign_keys", "ON").unwrap();
    assert_eq!(schema::migrate(&mut conn, &schema::MIGRATIONS[..1]).unwrap(), 1);
    conn.execute_batch(
        "INSERT INTO accounts VALUES ('acc1', 'https://x', 'kaan', 5);
         INSERT INTO messages VALUES ('acc1', 'm1', 't1', 'Ann', 'ann@x', 'Hi', 'p', 100, 1, 0, 0, 7);
         INSERT INTO message_mailboxes VALUES ('acc1', 'm1', 'inbox');
         INSERT INTO outbox (account_id, kind, message_id) VALUES ('acc1', 'archive', 'm1');
         INSERT INTO sync_state VALUES ('acc1', 'Email', 's1'), ('acc1', 'Mailbox', 's2');",
    )
    .unwrap();

    assert_eq!(schema::migrate(&mut conn, &schema::MIGRATIONS[..2]).unwrap(), 1);
    let st = Store::init(conn).unwrap();
    assert_eq!(st.schema_version().unwrap() as usize, schema::MIGRATIONS.len());
    let m = st.get_message(A, "m1").unwrap().unwrap();
    assert_eq!((m.subject.as_str(), m.mailbox_ids.clone(), m.to.clone()), ("Hi", strs(&["inbox"]), Vec::new()));
    let queued = st.outbox_pending(A).unwrap();
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].1, Action::Archive { message_id: "m1".into() });
    // One full resync is due for the Email data; the Mailbox state stays.
    assert_eq!(st.get_state(A, "Email").unwrap(), None);
    assert_eq!(st.get_state(A, "Mailbox").unwrap(), Some("s2".into()));
}

#[test]
fn recipients_are_stored_with_the_header() {
    let st = s();
    let mut m = msg("m1", &["inbox"], 1, true);
    m.to = vec![Address { name: "Bo".into(), email: "bo@example.com".into() }];
    st.upsert_messages(A, &[m.clone()]).unwrap();
    assert_eq!(st.get_message(A, "m1").unwrap().unwrap().to, m.to);
    assert_eq!(st.list_messages(A, "inbox", 10, 0).unwrap()[0].to, m.to);
}

#[test]
fn threads_group_a_mailboxes_messages_newest_thread_first() {
    let st = s();
    st.upsert_messages(
        A,
        &[
            in_thread(from(msg("a1", &["inbox"], 100, false), "Ann", "ann@x"), "T1"),
            in_thread(from(msg("a2", &["inbox"], 300, true), "", "bob@x"), "T1"),
            in_thread(from(msg("a3", &["inbox"], 200, true), "Ann", "ann@x"), "T1"),
            msg("b1", &["inbox"], 250, false),
            // Same thread, but another mailbox: not part of the Inbox row.
            in_thread(msg("a4", &["sent"], 400, true), "T1"),
        ],
    )
    .unwrap();
    let list = rows(&st, "inbox", ListFilter::All, 10, 0);
    assert_eq!(newest_ids(&list), ["a2", "b1"]);
    let row = &list[0];
    assert_eq!(row.newest.id, "a2");
    assert_eq!(row.newest.mailbox_ids, strs(&["inbox"]));
    assert_eq!(row.message_ids, strs(&["a1", "a3", "a2"]), "oldest first");
    assert_eq!(row.unread_count, 2);
    assert_eq!(row.senders, strs(&["Ann", "bob@x"]), "distinct, oldest first, the address stands in for a name");
    assert!(row.has_attachment && !row.is_flagged);
    assert_eq!(list[1].message_ids, strs(&["b1"]));
    assert_eq!(list[1].unread_count, 0);
}

#[test]
fn a_message_without_a_thread_id_is_its_own_thread() {
    let st = s();
    st.upsert_messages(
        A,
        &[in_thread(msg("x1", &["inbox"], 100, true), ""), in_thread(msg("x2", &["inbox"], 200, true), "")],
    )
    .unwrap();
    let list = rows(&st, "inbox", ListFilter::All, 10, 0);
    assert_eq!(newest_ids(&list), ["x2", "x1"]);
    assert_eq!(list[0].message_ids, strs(&["x2"]));
}

#[test]
fn a_sender_with_no_name_and_no_address_is_skipped() {
    let st = s();
    st.upsert_messages(A, &[from(msg("m1", &["inbox"], 100, true), "", "")]).unwrap();
    assert!(rows(&st, "inbox", ListFilter::All, 10, 0)[0].senders.is_empty());
}

#[test]
fn list_filters_apply_to_the_whole_thread() {
    let st = s();
    st.upsert_messages(
        A,
        &[
            // T1: unread only in an old message; flagged in the newest; no attachment.
            in_thread(plain(msg("a1", &["inbox"], 100, true)), "T1"),
            in_thread(flagged(plain(read(msg("a2", &["inbox"], 400, false)))), "T1"),
            // T2: all read, attachment, not flagged.
            read(msg("b1", &["inbox"], 300, false)),
            // T3: all read, plain, not flagged.
            plain(read(msg("c1", &["inbox"], 200, false))),
        ],
    )
    .unwrap();
    let ids = |f| newest_ids(&rows(&st, "inbox", f, 10, 0)).into_iter().map(String::from).collect::<Vec<_>>();
    assert_eq!(ids(ListFilter::All), strs(&["a2", "b1", "c1"]));
    assert_eq!(ids(ListFilter::Unread), strs(&["a2"]));
    assert_eq!(ids(ListFilter::Flagged), strs(&["a2"]));
    assert_eq!(ids(ListFilter::Attachments), strs(&["b1"]));
    let unread = rows(&st, "inbox", ListFilter::Unread, 10, 0);
    assert_eq!((unread[0].unread_count, unread[0].is_flagged, unread[0].has_attachment), (1, true, false));
}

#[test]
fn filters_come_before_paging() {
    let st = s();
    let all: Vec<_> = (0..6).map(|i| read(msg(&format!("m{i}"), &["inbox"], 100 + i, false))).collect();
    st.upsert_messages(A, &all).unwrap();
    // Only m1 and m4 are unread.
    st.set_unread(A, "m1", true).unwrap();
    st.set_unread(A, "m4", true).unwrap();
    assert_eq!(newest_ids(&rows(&st, "inbox", ListFilter::Unread, 1, 1)), ["m1"]);
    assert_eq!(newest_ids(&rows(&st, "inbox", ListFilter::Unread, 1, 0)), ["m4"]);
}

#[test]
fn the_window_decides_which_threads_are_listed_not_what_a_row_holds() {
    let st = s();
    st.upsert_messages(
        A,
        &[
            in_thread(msg("old", &["inbox"], 100, true), "T"),
            in_thread(msg("edge", &["inbox"], 200, true), "T"),
            in_thread(msg("new", &["inbox"], 300, true), "T"),
            msg("older", &["inbox"], 50, true),
        ],
    )
    .unwrap();
    assert_eq!(rows(&st, "inbox", ListFilter::All, 10, 0).len(), 2);
    st.set_window(A, "inbox", 200).unwrap();
    let list = rows(&st, "inbox", ListFilter::All, 10, 0);
    assert_eq!(newest_ids(&list), ["new"]);
    // The thread is listed because its newest mail is inside the window; the row then holds
    // every cached message of the thread here, so a swipe reaches the old one too.
    assert_eq!(list[0].message_ids, strs(&["old", "edge", "new"]));
    assert_eq!(list[0].unread_count, 3);
    // A thread whose newest mail is at the window's own second is still listed.
    st.set_window(A, "inbox", 300).unwrap();
    assert_eq!(newest_ids(&rows(&st, "inbox", ListFilter::All, 10, 0)), ["new"]);
    st.set_window(A, "inbox", 301).unwrap();
    assert!(rows(&st, "inbox", ListFilter::All, 10, 0).is_empty());
    st.clear_window(A, "inbox").unwrap();
    assert_eq!(rows(&st, "inbox", ListFilter::All, 10, 0).len(), 2);
    // Another mailbox's window does not matter.
    st.set_window(A, "sent", 1_000).unwrap();
    assert_eq!(newest_ids(&rows(&st, "inbox", ListFilter::All, 10, 0)), ["new", "older"]);
}

#[test]
fn a_window_goes_with_its_mailbox() {
    let st = s();
    let boxes = [mbox("inbox", "Inbox", Some("inbox"), 1), mbox("work", "Work", None, 2)];
    st.replace_mailboxes(A, &boxes).unwrap();
    st.set_window(A, "inbox", 10).unwrap();
    st.set_window(A, "work", 20).unwrap();
    st.delete_mailboxes(A, &strs(&["work"])).unwrap();
    assert_eq!((st.get_window(A, "inbox").unwrap(), st.get_window(A, "work").unwrap()), (Some(10), None));
    st.set_window(A, "gone", 30).unwrap();
    st.replace_mailboxes(A, &boxes[..1]).unwrap();
    assert_eq!((st.get_window(A, "inbox").unwrap(), st.get_window(A, "gone").unwrap()), (Some(10), None));
}

#[test]
fn windows_are_per_mailbox_and_replaced() {
    let st = s();
    assert_eq!(st.get_window(A, "inbox").unwrap(), None);
    st.set_window(A, "inbox", 10).unwrap();
    st.set_window(A, "inbox", 5).unwrap();
    st.set_window(A, "sent", 7).unwrap();
    assert_eq!(st.get_window(A, "inbox").unwrap(), Some(5));
    st.clear_window(A, "inbox").unwrap();
    assert_eq!((st.get_window(A, "inbox").unwrap(), st.get_window(A, "sent").unwrap()), (None, Some(7)));
    st.delete_account(A).unwrap();
    assert_eq!(st.get_window(A, "sent").unwrap(), None);
}

#[test]
fn paging_threads_is_stable_when_times_tie() {
    let st = s();
    let all: Vec<_> = ["a", "b", "c", "d", "e"].iter().map(|id| msg(id, &["inbox"], 100, true)).collect();
    st.upsert_messages(A, &all).unwrap();
    let mut paged = Vec::new();
    for offset in 0..5 {
        paged.extend(newest_ids(&rows(&st, "inbox", ListFilter::All, 1, offset)).into_iter().map(String::from));
    }
    assert_eq!(paged, strs(&["e", "d", "c", "b", "a"]));
    assert_eq!(rows(&st, "inbox", ListFilter::All, 10, 5).len(), 0);
}

#[test]
fn a_thread_listed_in_two_mailboxes_shows_only_what_is_in_each() {
    let st = s();
    st.upsert_messages(
        A,
        &[
            in_thread(msg("a1", &["inbox", "archive"], 100, true), "T"),
            in_thread(msg("a2", &["archive"], 200, true), "T"),
        ],
    )
    .unwrap();
    assert_eq!(rows(&st, "inbox", ListFilter::All, 10, 0)[0].message_ids, strs(&["a1"]));
    assert_eq!(rows(&st, "archive", ListFilter::All, 10, 0)[0].message_ids, strs(&["a1", "a2"]));
}

#[test]
fn thread_messages_span_mailboxes_and_run_oldest_first() {
    let st = s();
    st.upsert_messages(
        A,
        &[
            in_thread(msg("a2", &["sent"], 200, true), "T"),
            in_thread(msg("a1", &["inbox"], 100, true), "T"),
            in_thread(msg("a3", &["inbox", "archive"], 200, true), "T"),
            in_thread(msg("other", &["inbox"], 150, true), "U"),
            in_thread(msg("none", &["inbox"], 150, true), ""),
        ],
    )
    .unwrap();
    assert_eq!(ids(&st.thread_messages(A, "T").unwrap()), ["a1", "a2", "a3"]);
    assert_eq!(st.thread_messages(A, "T").unwrap()[2].mailbox_ids, strs(&["archive", "inbox"]));
    assert!(st.thread_messages(A, "").unwrap().is_empty());
    assert!(st.thread_messages(A, "missing").unwrap().is_empty());
}

#[test]
fn search_matches_sender_subject_and_preview_without_regard_to_case() {
    let st = s();
    let mut by_subject = msg("m1", &["inbox"], 100, true);
    by_subject.subject = "Quarterly Report".into();
    let mut by_name = from(msg("m2", &["archive"], 300, true), "Renate Report", "r@x");
    by_name.subject = "zzz".into();
    let mut by_email = from(msg("m3", &["inbox"], 200, true), "", "report@x");
    by_email.subject = "zzz".into();
    let mut by_preview = msg("m4", &["inbox"], 50, true);
    by_preview.subject = "zzz".into();
    by_preview.preview = "see the REPORT".into();
    let mut none = msg("m5", &["inbox"], 400, true);
    none.subject = "zzz".into();
    st.upsert_messages(A, &[by_subject, by_name, by_email, by_preview, none]).unwrap();

    assert_eq!(ids(&st.search_messages(A, "report", None, 10).unwrap()), ["m2", "m3", "m1", "m4"], "newest first");
    assert_eq!(ids(&st.search_messages(A, "REPORT", Some("inbox"), 10).unwrap()), ["m3", "m1", "m4"]);
    assert_eq!(ids(&st.search_messages(A, "report", None, 2).unwrap()), ["m2", "m3"]);
    assert!(st.search_messages(A, "nothing like this", None, 10).unwrap().is_empty());
}

#[test]
fn search_treats_like_wildcards_as_plain_characters() {
    let st = s();
    let named = |id: &str, subject: &str| {
        let mut m = msg(id, &["inbox"], 100, true);
        m.subject = subject.into();
        m.preview = String::new();
        m
    };
    st.upsert_messages(
        A,
        &[
            named("pct", "100% sure"),
            named("und", "a_b"),
            named("any", "axb"),
            named("bs", r"c:\dir"),
            named("x", "100 sure"),
        ],
    )
    .unwrap();
    let hits =
        |q: &str| ids(&st.search_messages(A, q, None, 10).unwrap()).into_iter().map(String::from).collect::<Vec<_>>();
    assert_eq!(hits("%"), strs(&["pct"]));
    assert_eq!(hits("_"), strs(&["und"]));
    assert_eq!(hits("a_b"), strs(&["und"]));
    assert_eq!(hits(r"\"), strs(&["bs"]));
    assert_eq!(hits(r"\d"), strs(&["bs"]));
    assert_eq!(hits("%%"), Vec::<String>::new());
}

#[test]
fn a_body_round_trips_and_is_replaced() {
    let st = s();
    st.upsert_messages(A, &[msg("m1", &["inbox"], 1, true)]).unwrap();
    assert_eq!(st.get_body(A, "m1").unwrap(), None);
    assert!(st.put_body(A, &body("m1")).unwrap());
    assert_eq!(st.get_body(A, "m1").unwrap(), Some(body("m1")));
    let mut newer = body("m1");
    newer.text = Some("changed".into());
    assert!(st.put_body(A, &newer).unwrap());
    assert_eq!(st.get_body(A, "m1").unwrap(), Some(newer));
}

#[test]
fn a_body_for_a_message_that_is_not_cached_is_dropped_quietly() {
    let st = s();
    assert!(!st.put_body(A, &body("ghost")).unwrap());
    assert_eq!(st.get_body(A, "ghost").unwrap(), None);
}

#[test]
fn an_unreadable_body_row_counts_as_no_body() {
    let st = s();
    st.upsert_messages(A, &[msg("m1", &["inbox"], 1, true)]).unwrap();
    st.lock().execute("INSERT INTO message_bodies VALUES (?1, 'm1', 'not json')", [A]).unwrap();
    assert_eq!(st.get_body(A, "m1").unwrap(), None);
    assert!(st.put_body(A, &body("m1")).unwrap());
    assert_eq!(st.get_body(A, "m1").unwrap(), Some(body("m1")));
}

#[test]
fn a_body_goes_with_its_message_and_survives_a_resync_that_keeps_it() {
    let st = s();
    st.upsert_messages(A, &[msg("m1", &["inbox"], 1, true), msg("m2", &["inbox"], 2, true)]).unwrap();
    st.put_body(A, &body("m1")).unwrap();
    st.put_body(A, &body("m2")).unwrap();
    st.delete_messages(A, &strs(&["m1"])).unwrap();
    assert_eq!(st.get_body(A, "m1").unwrap(), None);
    assert!(st.get_body(A, "m2").unwrap().is_some());

    // A resync that lists m2 again keeps its body; one that does not, drops it.
    st.replace_messages(A, &[msg("m2", &["inbox"], 2, false), msg("m3", &["inbox"], 3, true)]).unwrap();
    assert!(st.get_body(A, "m2").unwrap().is_some());
    assert!(!st.get_message(A, "m2").unwrap().unwrap().is_unread, "the header is the new one");
    st.replace_messages(A, &[msg("m3", &["inbox"], 3, true)]).unwrap();
    assert_eq!(st.get_body(A, "m2").unwrap(), None);
    assert!(st.get_message(A, "m2").unwrap().is_none());
}

#[test]
fn signing_out_removes_the_bodies() {
    let st = s();
    st.upsert_messages(A, &[msg("m1", &["inbox"], 1, true)]).unwrap();
    st.put_body(A, &body("m1")).unwrap();
    st.delete_account(A).unwrap();
    let n: i64 = st.lock().query_row("SELECT COUNT(*) FROM message_bodies", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 0);
}

#[test]
fn ids_without_body_look_only_at_the_newest_messages() {
    let st = s();
    let all: Vec<_> = (1..=5).map(|i| msg(&format!("m{i}"), &["inbox"], i * 10, true)).collect();
    st.upsert_messages(A, &all).unwrap();
    st.upsert_messages(A, &[msg("elsewhere", &["archive"], 1_000, true)]).unwrap();
    st.put_body(A, &body("m4")).unwrap();
    assert_eq!(st.ids_without_body(A, "inbox", 3).unwrap(), strs(&["m5", "m3"]));
    assert_eq!(st.ids_without_body(A, "inbox", 10).unwrap(), strs(&["m5", "m3", "m2", "m1"]));
    assert!(st.ids_without_body(A, "inbox", 0).unwrap().is_empty());
}

#[test]
fn a_full_resync_keeps_what_it_lists_and_drops_the_rest() {
    let st = s();
    st.upsert_messages(A, &[msg("keep", &["inbox"], 1, true), msg("gone", &["inbox"], 2, true)]).unwrap();
    st.replace_messages(A, &[msg("keep", &["archive"], 1, true), msg("new", &["inbox"], 3, true)]).unwrap();
    assert!(st.get_message(A, "gone").unwrap().is_none());
    assert_eq!(st.get_message(A, "keep").unwrap().unwrap().mailbox_ids, strs(&["archive"]));
    assert!(st.get_message(A, "new").unwrap().is_some());
}

#[test]
fn flag_unflag_and_move_survive_the_queue() {
    let st = s();
    let actions = [
        Action::Flag { message_id: "m1".into() },
        Action::Unflag { message_id: "m1".into() },
        Action::Move { message_id: "m2".into(), mailbox_id: "junk".into() },
    ];
    for a in &actions {
        st.outbox_enqueue(A, a).unwrap();
    }
    let got: Vec<Action> = st.outbox_pending(A).unwrap().into_iter().map(|(_, a, _)| a).collect();
    assert_eq!(got, actions);
}

#[test]
fn a_queued_move_without_a_target_is_skipped_like_an_unknown_kind() {
    let st = s();
    st.lock().execute("INSERT INTO outbox (account_id, kind, message_id) VALUES (?1, 'move', 'm1')", [A]).unwrap();
    st.lock()
        .execute("INSERT INTO outbox (account_id, kind, message_id) VALUES (?1, 'from_the_future', 'm1')", [A])
        .unwrap();
    st.outbox_enqueue(A, &Action::Flag { message_id: "m1".into() }).unwrap();
    let got: Vec<Action> = st.outbox_pending(A).unwrap().into_iter().map(|(_, a, _)| a).collect();
    assert_eq!(got, [Action::Flag { message_id: "m1".into() }]);
}

// ---- M4: sending ----

use crate::model::{Draft, Identity, PendingSend};
use crate::Error;

fn draft(id: &str) -> Draft {
    Draft {
        id: id.into(),
        identity_id: "i1".into(),
        to: vec![Address { name: "Bo".into(), email: "bo@x.org".into() }],
        cc: Vec::new(),
        bcc: Vec::new(),
        subject: format!("subject {id}"),
        body: "text".into(),
        attachments: Vec::new(),
        source_message_id: None,
        is_forward: false,
        server_id: None,
        send_error: None,
        updated_at: 1,
    }
}

fn row(st: &Store, id: &str) -> DraftRow {
    st.draft_get(A, id).unwrap().expect("a draft")
}

#[test]
fn migration_three_keeps_what_the_earlier_ones_stored() {
    let mut conn = Connection::open_in_memory().unwrap();
    conn.pragma_update(None, "foreign_keys", "ON").unwrap();
    assert_eq!(schema::migrate(&mut conn, &schema::MIGRATIONS[..2]).unwrap(), 2);
    conn.execute_batch(
        "INSERT INTO accounts VALUES ('acc1', 'https://x', 'kaan', 5);
         INSERT INTO messages (account_id, id, thread_id, from_name, from_email, subject, preview, received_at,
             is_unread, is_flagged, has_attachment, size, to_json)
             VALUES ('acc1', 'm1', 't1', 'Ann', 'ann@x', 'Hi', 'p', 100, 1, 0, 0, 7, '[]');
         INSERT INTO message_mailboxes VALUES ('acc1', 'm1', 'inbox');
         INSERT INTO outbox (account_id, kind, message_id) VALUES ('acc1', 'archive', 'm1');
         INSERT INTO sync_state VALUES ('acc1', 'Email', 's1');",
    )
    .unwrap();

    assert_eq!(schema::migrate(&mut conn, &schema::MIGRATIONS[..3]).unwrap(), 1);
    let st = Store::init(conn).unwrap();
    assert_eq!(st.get_message(A, "m1").unwrap().unwrap().subject, "Hi");
    assert_eq!(st.outbox_pending(A).unwrap().len(), 1);
    assert_eq!(st.get_state(A, "Email").unwrap(), Some("s1".into()));
    // The new tables are there, empty and usable.
    assert!(st.drafts_list(A).unwrap().is_empty());
    st.draft_insert(A, &draft("d1"), None, false).unwrap();
    assert!(st.identities_list(A).unwrap().is_empty() && st.contacts_list(A).unwrap().is_empty());
}

#[test]
fn a_stored_draft_comes_back_as_it_was_written() {
    let st = s();
    let mut d = draft("d1");
    d.server_id = Some("srv1".into());
    st.draft_insert(A, &d, Some(&Threading { in_reply_to: vec!["a@x".into()], references: vec!["a@x".into()] }), false)
        .unwrap();
    let r = row(&st, "d1");
    assert_eq!(r.draft, d);
    assert_eq!((r.state, r.dirty, r.discarded, r.rev, r.send_at), (DraftState::Draft, false, false, 0, None));
    assert_eq!(r.threading.unwrap().in_reply_to, vec!["a@x".to_string()]);
    assert_eq!(st.draft_by_server_id(A, "srv1").unwrap().unwrap().id, "d1");
    assert_eq!(st.draft_by_server_id(A, "other").unwrap(), None);
    assert_eq!(st.draft_get(A, "nope").unwrap(), None);
    // The same id twice is an error, not a replacement.
    assert!(st.draft_insert(A, &d, None, false).is_err());
}

#[test]
fn saving_marks_the_draft_for_the_server_and_keeps_what_the_pass_owns() {
    let st = s();
    let mut d = draft("d1");
    d.server_id = Some("srv1".into());
    st.draft_insert(A, &d, None, false).unwrap();

    // The caller's copy is older than the pass: its server id is stale.
    let mut edited = draft("d1");
    edited.body = "new text".into();
    edited.server_id = Some("stale".into());
    edited.send_error = Some("made up".into());
    let saved = save_new(&st, &edited, 50).unwrap();
    assert_eq!((saved.body.as_str(), saved.updated_at), ("new text", 50));
    assert_eq!((saved.server_id.as_deref(), saved.send_error), (Some("srv1"), None));
    let r = row(&st, "d1");
    assert_eq!((r.dirty, r.rev), (true, 1));

    // An unknown id is a new draft.
    save_new(&st, &draft("fresh"), 60).unwrap();
    assert_eq!(row(&st, "fresh").draft.updated_at, 60);
    // Newest first.
    let listed: Vec<String> = st.drafts_list(A).unwrap().into_iter().map(|d| d.id).collect();
    assert_eq!(listed, ["fresh", "d1"]);
}

#[test]
fn a_server_copy_counts_as_saved_only_if_the_draft_was_not_edited_meanwhile() {
    let st = s();
    save_new(&st, &draft("d1"), 10).unwrap();
    let seen = row(&st, "d1");
    assert_eq!(st.drafts_to_save(A).unwrap().len(), 1);

    // An edit arrives while the pass talks to the server.
    save_new(&st, &draft("d1"), 11).unwrap();
    st.draft_saved(A, "d1", "srv1", seen.rev).unwrap();
    let r = row(&st, "d1");
    assert_eq!((r.draft.server_id.as_deref(), r.dirty), (Some("srv1"), true), "the copy is kept, the row stays due");

    st.draft_saved(A, "d1", "srv2", r.rev).unwrap();
    let r = row(&st, "d1");
    assert_eq!((r.draft.server_id.as_deref(), r.dirty), (Some("srv2"), false));
    assert!(st.drafts_to_save(A).unwrap().is_empty());

    // A copy the server will not take is let go of until the next edit.
    save_new(&st, &draft("d1"), 12).unwrap();
    let r = row(&st, "d1");
    st.draft_give_up(A, "d1", r.rev - 1).unwrap();
    assert!(row(&st, "d1").dirty, "an older version's give-up does not count");
    st.draft_give_up(A, "d1", r.rev).unwrap();
    assert!(!row(&st, "d1").dirty);
}

#[test]
fn sending_holds_the_message_back_and_a_second_send_changes_nothing() {
    let st = s();
    let first = send_new(&st, &draft("d1"), 100, 90).unwrap();
    assert_eq!(first, PendingSend { draft_id: "d1".into(), subject: "subject d1".into(), send_at: 100 });
    assert!(st.drafts_list(A).unwrap().is_empty());
    assert_eq!(st.pending_sends(A).unwrap(), vec![first.clone()]);
    assert_eq!(send_new(&st, &draft("d1"), 500, 95).unwrap(), first, "a second tap does not move the time");

    assert!(st.drafts_to_send(A, 99).unwrap().is_empty());
    assert_eq!(st.drafts_to_send(A, 100).unwrap().len(), 1);
    // A message that is being sent cannot be edited.
    assert!(matches!(save_new(&st, &draft("d1"), 96), Err(Error::Invalid { .. })));
    assert!(st.drafts_to_save(A).unwrap().is_empty(), "not saved to Drafts while held back");
}

/// A send of a draft as the tests write it: the row is born first if it is not there, as
/// `new_draft` does for the app (a user write never makes a row).
fn send_new(st: &Store, d: &Draft, send_at: i64, now: i64) -> crate::Result<crate::model::PendingSend> {
    let _ = st.draft_insert(A, d, None, true);
    st.draft_send(A, d, send_at, now)
}

/// The same for an edit.
fn save_new(st: &Store, d: &Draft, now: i64) -> crate::Result<Draft> {
    let _ = st.draft_insert(A, d, None, true);
    st.draft_save(A, d, now)
}

/// Makes the server copy current, as a pass does, and starts the submission of that version.
fn begin(st: &Store, id: &str) -> bool {
    let Some(r) = st.draft_get(A, id).unwrap() else { return false };
    if r.state == DraftState::Sending {
        st.draft_saved(A, id, "srv", r.rev).unwrap();
    }
    st.draft_begin_submit(A, id, r.rev, "srv").unwrap()
}

#[test]
fn a_user_write_never_brings_back_a_draft_that_is_gone() {
    let st = s();
    // Never born here, or sent and removed by a pass: the same to the store.
    assert!(matches!(st.draft_save(A, &draft("gone"), 5), Err(Error::Invalid { .. })));
    assert!(matches!(st.draft_send(A, &draft("gone"), 100, 90), Err(Error::Invalid { .. })));
    send_new(&st, &draft("d1"), 100, 90).unwrap();
    st.draft_delete(A, "d1").unwrap();
    // A second tap on Send with the screen's old copy must not send it again.
    assert!(matches!(st.draft_send(A, &draft("d1"), 100, 95), Err(Error::Invalid { .. })));
    assert!(st.pending_sends(A).unwrap().is_empty());
}

#[test]
fn only_the_version_that_was_copied_is_submitted() {
    let st = s();
    send_new(&st, &draft("d1"), 100, 90).unwrap();
    let read = st.draft_get(A, "d1").unwrap().unwrap();
    st.draft_saved(A, "d1", "srv", read.rev).unwrap();
    // Meanwhile the user takes it back, changes it and sends again.
    let back = st.draft_undo_send(A, "d1").unwrap();
    st.draft_send(A, &back, 200, 95).unwrap();
    assert!(!st.draft_begin_submit(A, "d1", read.rev, "srv").unwrap(), "the old copy is not sent");
    assert_eq!(st.draft_get(A, "d1").unwrap().unwrap().state, DraftState::Sending);
}

#[test]
fn a_send_can_be_taken_back_until_it_is_started() {
    let st = s();
    send_new(&st, &draft("d1"), 100, 90).unwrap();
    let back = st.draft_undo_send(A, "d1").unwrap();
    assert_eq!(back.id, "d1");
    let r = row(&st, "d1");
    assert_eq!((r.state, r.send_at), (DraftState::Draft, None));
    assert!(st.pending_sends(A).unwrap().is_empty());
    assert_eq!(st.drafts_list(A).unwrap().len(), 1);
    // Not being sent: nothing to take back.
    assert!(matches!(st.draft_undo_send(A, "d1"), Err(Error::Invalid { .. })));

    send_new(&st, &draft("d1"), 100, 91).unwrap();
    assert!(begin(&st, "d1"));
    assert!(!begin(&st, "d1"), "started once");
    assert!(matches!(st.draft_undo_send(A, "d1"), Err(Error::Invalid { .. })), "too late");
    assert_eq!(st.pending_sends(A).unwrap().len(), 1, "still listed while the server decides");
    assert_eq!(st.drafts_to_send(A, 0).unwrap().len(), 1, "settled by the next pass even before its time");
    assert!(matches!(st.draft_undo_send(A, "nope"), Err(Error::Invalid { .. })));
}

#[test]
fn a_refused_message_is_a_draft_again_with_the_reason_until_it_is_sent_again() {
    let st = s();
    send_new(&st, &draft("d1"), 100, 90).unwrap();
    begin(&st, "d1");
    st.draft_refused(A, "d1", "no such domain").unwrap();
    let r = row(&st, "d1");
    assert_eq!((r.state, r.send_at, r.draft.send_error.as_deref()), (DraftState::Draft, None, Some("no such domain")));
    assert!(r.dirty);
    assert_eq!(st.drafts_list(A).unwrap().len(), 1);

    // Editing keeps the reason on show; sending again clears it.
    let saved = save_new(&st, &draft("d1"), 120).unwrap();
    assert_eq!(saved.send_error.as_deref(), Some("no such domain"));
    send_new(&st, &draft("d1"), 200, 130).unwrap();
    assert_eq!(row(&st, "d1").draft.send_error, None);
}

#[test]
fn a_lost_server_copy_starts_the_send_over() {
    let st = s();
    let mut d = draft("d1");
    d.server_id = Some("srv".into());
    st.draft_insert(A, &d, None, false).unwrap();
    send_new(&st, &d, 100, 90).unwrap();
    begin(&st, "d1");
    st.draft_copy_lost(A, "d1").unwrap();
    let r = row(&st, "d1");
    assert_eq!((r.state, r.dirty, r.draft.server_id), (DraftState::Sending, true, None));
    // A draft that is only being written keeps its state.
    save_new(&st, &draft("d2"), 5).unwrap();
    st.draft_copy_lost(A, "d2").unwrap();
    assert_eq!(row(&st, "d2").state, DraftState::Draft);
}

#[test]
fn a_discarded_draft_is_gone_from_the_lists_and_waits_for_the_pass() {
    let st = s();
    let mut d = draft("d1");
    d.server_id = Some("srv1".into());
    st.draft_insert(A, &d, None, false).unwrap();
    save_new(&st, &draft("d2"), 5).unwrap();
    st.draft_discard(A, "d1").unwrap();
    assert_eq!(st.drafts_list(A).unwrap().len(), 1);
    assert_eq!(st.draft_get(A, "d1").unwrap(), None);
    assert_eq!(st.draft_by_server_id(A, "srv1").unwrap(), None);
    assert!(st.drafts_to_save(A).unwrap().iter().all(|r| r.draft.id != "d1"));
    assert_eq!(st.drafts_discarded(A).unwrap(), vec![("d1".to_string(), Some("srv1".to_string()))]);
    // A late autosave cannot bring it back.
    assert!(matches!(save_new(&st, &d, 9), Err(Error::Invalid { .. })));
    st.draft_delete(A, "d1").unwrap();
    assert!(st.drafts_discarded(A).unwrap().is_empty());

    // Discarding what is not there is fine; discarding a send under way is not.
    st.draft_discard(A, "nope").unwrap();
    send_new(&st, &draft("d3"), 100, 90).unwrap();
    begin(&st, "d3");
    assert!(matches!(st.draft_discard(A, "d3"), Err(Error::Invalid { .. })));
    // A held-back message can be thrown away.
    send_new(&st, &draft("d4"), 100, 90).unwrap();
    st.draft_discard(A, "d4").unwrap();
    assert_eq!(st.pending_sends(A).unwrap().len(), 1);
}

#[test]
fn a_file_is_added_to_the_draft_being_written() {
    let st = s();
    save_new(&st, &draft("d1"), 10).unwrap();
    let file = Attachment {
        blob_id: "b1".into(),
        name: "a.pdf".into(),
        mime_type: "application/pdf".into(),
        size: 3,
        content_id: None,
        is_inline: false,
    };
    let d = st.draft_add_attachment(A, "d1", file.clone(), 20).unwrap();
    assert_eq!((d.attachments, d.updated_at), (vec![file.clone()], 20));
    assert_eq!(row(&st, "d1").rev, 2);
    assert!(matches!(st.draft_add_attachment(A, "nope", file.clone(), 20), Err(Error::Invalid { .. })));
    send_new(&st, &draft("d1"), 100, 90).unwrap();
    assert!(matches!(st.draft_add_attachment(A, "d1", file, 21), Err(Error::Invalid { .. })));
}

#[test]
fn identities_keep_the_servers_order_and_are_replaced_whole() {
    let st = s();
    let id = |n: &str| Identity { id: n.into(), name: n.into(), email: format!("{n}@x.org"), signature: "sig".into() };
    st.identities_replace(A, &[id("b"), id("a")]).unwrap();
    let got: Vec<String> = st.identities_list(A).unwrap().into_iter().map(|i| i.id).collect();
    assert_eq!(got, ["b", "a"]);
    st.identities_replace(A, &[id("c")]).unwrap();
    assert_eq!(st.identities_list(A).unwrap(), vec![id("c")]);
}

fn card(id: &str, name: &str, emails: &[&str]) -> Entry {
    let contact = Contact {
        address_book_id: "b".into(),
        name: name.into(),
        given_name: String::new(),
        family_name: String::new(),
        organization: String::new(),
        emails: emails.iter().map(|e| Labeled { label: String::new(), value: e.to_string() }).collect(),
        phones: vec![Labeled { label: String::new(), value: "+49 30 1234".into() }],
        addresses: Vec::new(),
        notes: String::new(),
    };
    Entry::new(id.into(), contact)
}

fn suggested(st: &Store) -> Vec<(String, String)> {
    st.contacts_list(A).unwrap().into_iter().map(|a| (a.name, a.email)).collect()
}

/// A listing that started just now and is kept.
fn replace(st: &Store, cards: &[Entry]) {
    assert!(st.contact_cards_replace(A, cards, st.contact_generation()).unwrap());
}

fn listed(st: &Store) -> Vec<String> {
    st.contact_index(A).unwrap().into_iter().map(|i| i.row.id).collect()
}

#[test]
fn the_cards_are_listed_by_name_and_replaced_whole() {
    let st = s();
    replace(&st, &[card("c1", "Özge", &["o@x.org"]), card("c2", "Ann", &[]), card("c3", "ann", &[])]);
    // By folded name, then id: "ann" and "Ann" tie, "Özge" sorts as an o.
    assert_eq!(listed(&st), ["c2", "c3", "c1"]);
    let first = st.contact_index(A).unwrap().remove(0);
    assert_eq!((first.row.name.as_str(), first.digits.as_str()), ("Ann", "49301234"));
    assert_eq!(st.contact_card_get(A, "c1").unwrap().unwrap().emails[0].value, "o@x.org");
    assert!(st.contact_card_get(A, "zz").unwrap().is_none());
    replace(&st, &[card("c9", "Zed", &[])]);
    assert_eq!(listed(&st), ["c9"]);
}

#[test]
fn a_listing_that_a_write_overtook_is_not_kept() {
    let st = s();
    replace(&st, &[card("c1", "Zed", &[])]);
    assert!(st.contacts_listed(A) && !st.contacts_listed("other"));
    // A listing starts, the user saves a card, the listing ends: it is older than the cache.
    let started = st.contact_generation();
    st.contact_card_put(A, &card("c2", "New", &[])).unwrap();
    assert!(!st.contact_cards_replace(A, &[card("c1", "Zed", &[])], started).unwrap());
    assert_eq!(listed(&st), ["c2", "c1"]);
    // The same for a card that was deleted.
    let started = st.contact_generation();
    st.contact_card_forget(A, "c1").unwrap();
    assert!(!st.contact_cards_replace(A, &[card("c1", "Zed", &[])], started).unwrap());
    assert_eq!(listed(&st), ["c2"]);
    // Sign-out: nothing is listed any more, and a listing that was on its way is dropped.
    let started = st.contact_generation();
    st.delete_account(A).unwrap();
    assert!(!st.contacts_listed(A));
    assert!(!st.contact_cards_replace(A, &[], started).unwrap());
}

#[test]
fn one_card_is_put_in_place_or_last_and_forgotten() {
    let st = s();
    replace(&st, &[card("c1", "Zed", &["zed@x.org"]), card("c2", "Ann", &["ann@x.org"])]);
    st.contact_card_put(A, &card("c3", "Bo", &["bo@x.org"])).unwrap();
    st.contact_card_put(A, &card("c1", "Zed Z", &["zed@x.org", "z2@x.org"])).unwrap();
    assert_eq!(listed(&st), ["c2", "c3", "c1"]);
    // The suggestions follow the server's order, which c1 kept.
    assert_eq!(
        suggested(&st),
        [("Zed Z", "zed@x.org"), ("Zed Z", "z2@x.org"), ("Ann", "ann@x.org"), ("Bo", "bo@x.org")]
            .map(|(n, e)| (n.to_string(), e.to_string()))
    );
    st.contact_card_forget(A, "c1").unwrap();
    st.contact_card_forget(A, "nope").unwrap();
    assert_eq!(listed(&st), ["c2", "c3"]);
    assert_eq!(suggested(&st).len(), 2);
}

#[test]
fn suggestions_keep_an_address_once_whatever_its_case_and_a_nameless_card_suggests_the_address() {
    let st = s();
    // A card with no name is named by its first address: it suggests the address alone.
    replace(
        &st,
        &[
            card("c1", "Zed", &["zed@x.org"]),
            card("c2", "Ann Again", &["ZED@x.org"]),
            card("c3", "n@x.org", &["n@x.org"]),
        ],
    );
    assert_eq!(suggested(&st), [("Zed", "zed@x.org"), ("", "n@x.org")].map(|(n, e)| (n.to_string(), e.to_string())));
    replace(&st, &[]);
    assert!(st.contacts_list(A).unwrap().is_empty());
}

#[test]
fn address_books_keep_the_servers_order_and_are_replaced_whole() {
    let st = s();
    let book =
        |id: &str| AddressBook { id: id.into(), name: id.to_uppercase(), is_default: id == "b", may_write: true };
    st.address_books_replace(A, &[book("z"), book("b")]).unwrap();
    assert_eq!(st.address_books_list(A).unwrap(), [book("z"), book("b")]);
    st.address_books_replace(A, &[book("q")]).unwrap();
    assert_eq!(st.address_books_list(A).unwrap(), [book("q")]);
}

#[test]
fn people_in_mail_are_senders_then_recipients_newest_message_first() {
    let st = s();
    let mut old = msg("old", &["inbox"], 10, false);
    old.from_email = "old@x.org".into();
    let mut new = msg("new", &["sent"], 20, false);
    new.from_name = "Me".into();
    new.from_email = "me@x.org".into();
    new.to = vec![Address { name: "Bo".into(), email: "bo@x.org".into() }];
    st.upsert_messages(A, &[old, new]).unwrap();
    let got: Vec<String> = st.people_in_mail(A, 10).unwrap().into_iter().map(|a| a.email).collect();
    assert_eq!(got, ["me@x.org", "bo@x.org", "old@x.org"]);
    assert_eq!(st.people_in_mail(A, 1).unwrap().len(), 2, "the scan is limited by messages");
}

#[test]
fn sign_out_takes_the_drafts_and_the_directory_with_it() {
    let st = s();
    save_new(&st, &draft("d1"), 1).unwrap();
    st.identities_replace(
        A,
        &[Identity { id: "i".into(), name: String::new(), email: "a@x".into(), signature: String::new() }],
    )
    .unwrap();
    replace(&st, &[card("c1", "C", &["c@x"])]);
    st.address_books_replace(A, &[AddressBook { id: "b".into(), name: "B".into(), is_default: true, may_write: true }])
        .unwrap();
    st.delete_account(A).unwrap();
    for table in ["drafts", "identities", "contacts", "contact_cards", "address_books"] {
        assert_eq!(count(&st, table), 0, "{table}");
    }
}

#[test]
fn the_directory_is_due_until_a_refresh_worked_and_then_for_an_hour() {
    let st = s();
    let start = Instant::now();
    assert!(st.directory_due(start));
    st.directory_done(start);
    assert!(!st.directory_due(start + Duration::from_secs(60)));
    assert!(st.directory_due(start + DIRECTORY_EVERY));
}

// ---- M5: calendars and the agenda ----

fn calendar(id: &str, name: &str) -> Calendar {
    Calendar {
        id: id.into(),
        name: name.into(),
        color: Some("#3a7bd5".into()),
        is_default: id == "c1",
        is_visible: true,
        may_write: true,
    }
}

fn entry(event: &str, recurrence: Option<&str>, starts_at: i64, ends_at: i64) -> Occurrence {
    Occurrence {
        event_id: event.into(),
        recurrence_id: recurrence.map(String::from),
        calendar_id: "c1".into(),
        title: format!("title {event}"),
        location: String::new(),
        starts_at,
        ends_at,
        all_day: false,
        is_task: false,
        is_done: false,
        is_recurring: recurrence.is_some(),
        my_status: None,
    }
}

fn starts(entries: &[Occurrence]) -> Vec<(&str, Option<&str>)> {
    let mut found: Vec<_> = entries.iter().map(|e| (e.event_id.as_str(), e.recurrence_id.as_deref())).collect();
    found.sort();
    found
}

#[test]
fn migration_four_keeps_what_the_earlier_ones_stored() {
    let mut conn = Connection::open_in_memory().unwrap();
    conn.pragma_update(None, "foreign_keys", "ON").unwrap();
    assert_eq!(schema::migrate(&mut conn, &schema::MIGRATIONS[..3]).unwrap(), 3);
    conn.execute_batch(
        "INSERT INTO accounts VALUES ('acc1', 'https://x', 'kaan', 5);
         INSERT INTO messages (account_id, id, thread_id, from_name, from_email, subject, preview, received_at,
             is_unread, is_flagged, has_attachment, size, to_json)
             VALUES ('acc1', 'm1', 't1', 'Ann', 'ann@x', 'Hi', 'p', 100, 1, 0, 0, 7, '[]');
         INSERT INTO identities VALUES ('acc1', 0, '{\"id\":\"i1\",\"name\":\"K\",\"email\":\"k@x\",\"signature\":\"\"}');
         INSERT INTO contacts VALUES ('acc1', 'ann@x', 'Ann', 0);
         INSERT INTO sync_state VALUES ('acc1', 'Email', 's1');",
    )
    .unwrap();

    assert_eq!(schema::migrate(&mut conn, &schema::MIGRATIONS[..4]).unwrap(), 1);
    let st = Store::init(conn).unwrap();
    assert_eq!(st.schema_version().unwrap() as usize, schema::MIGRATIONS.len());
    assert_eq!(st.get_message(A, "m1").unwrap().unwrap().subject, "Hi");
    assert_eq!(st.identities_list(A).unwrap()[0].email, "k@x");
    assert_eq!(st.contacts_list(A).unwrap().len(), 1);
    assert_eq!(st.get_state(A, "Email").unwrap(), Some("s1".into()));
    // The new tables are there, empty and usable.
    assert!(st.calendars_list(A).unwrap().is_empty() && st.agenda_list(A, 0, i64::MAX).unwrap().is_empty());
    st.calendars_replace(A, &[calendar("c1", "Personal")]).unwrap();
    st.agenda_replace(A, 0, 100, &[entry("e1", None, 10, 20)]).unwrap();
    assert_eq!((st.calendars_list(A).unwrap().len(), st.agenda_list(A, 0, 100).unwrap().len()), (1, 1));
}

#[test]
fn migration_five_keeps_what_a_version_four_database_stored() {
    let mut conn = Connection::open_in_memory().unwrap();
    conn.pragma_update(None, "foreign_keys", "ON").unwrap();
    assert_eq!(schema::migrate(&mut conn, &schema::MIGRATIONS[..4]).unwrap(), 4);
    conn.execute_batch(
        "INSERT INTO accounts VALUES ('acc1', 'https://x', 'kaan', 5);
         INSERT INTO contacts VALUES ('acc1', 'ann@x', 'Ann', 0);
         INSERT INTO calendars VALUES ('acc1', 0, '{\"id\":\"c1\",\"name\":\"Personal\",\"color\":null,\"is_default\":true,\"is_visible\":true,\"may_write\":true}');
         INSERT INTO sync_state VALUES ('acc1', 'Email', 's1');",
    )
    .unwrap();

    assert_eq!(schema::migrate(&mut conn, schema::MIGRATIONS).unwrap(), schema::MIGRATIONS.len() - 4);
    let st = Store::init(conn).unwrap();
    assert_eq!(st.schema_version().unwrap() as usize, schema::MIGRATIONS.len());
    assert_eq!(st.contacts_list(A).unwrap(), [Address { name: "Ann".into(), email: "ann@x".into() }]);
    assert_eq!(st.calendars_list(A).unwrap()[0].name, "Personal");
    assert_eq!(st.get_state(A, "Email").unwrap(), Some("s1".into()));
    // The new tables are there, empty and usable; the suggestions are rebuilt from the cards.
    assert!(st.address_books_list(A).unwrap().is_empty() && st.contact_index(A).unwrap().is_empty());
    replace(&st, &[card("c1", "Bo", &["bo@x"])]);
    assert_eq!(suggested(&st), [("Bo".to_string(), "bo@x".to_string())]);
}

#[test]
fn calendars_come_back_as_stored_and_are_replaced_whole() {
    let st = s();
    assert!(st.calendars_list(A).unwrap().is_empty());
    let first = [calendar("c2", "Work"), calendar("c1", "Personal")];
    st.calendars_replace(A, &first).unwrap();
    assert_eq!(st.calendars_list(A).unwrap(), first, "the server's order is kept");
    st.calendars_replace(A, &[calendar("c3", "New")]).unwrap();
    assert_eq!(st.calendars_list(A).unwrap(), [calendar("c3", "New")]);
    st.calendars_replace(A, &[]).unwrap();
    assert!(st.calendars_list(A).unwrap().is_empty());
    // Another account has its own list.
    st.save_account(&account("acc2")).unwrap();
    st.calendars_replace("acc2", &[calendar("c9", "Other")]).unwrap();
    st.calendars_replace(A, &[calendar("c1", "Personal")]).unwrap();
    assert_eq!(st.calendars_list("acc2").unwrap()[0].id, "c9");
}

#[test]
fn the_agenda_cache_answers_a_span_with_what_overlaps_it() {
    let st = s();
    let all = [
        entry("before", None, 10, 20),
        entry("starts-before", None, 90, 150),
        entry("inside", None, 120, 130),
        entry("point-at-start", None, 100, 100),
        entry("point-at-end", None, 200, 200),
        entry("ends-at-start", None, 50, 100),
        entry("series", Some("2026-11-16T08:00:00"), 150, 160),
        entry("series", Some("2026-11-17T08:00:00"), 250, 260),
    ];
    st.agenda_replace(A, 0, 1000, &all).unwrap();
    let found = st.agenda_list(A, 100, 200).unwrap();
    assert_eq!(
        starts(&found),
        [("inside", None), ("point-at-start", None), ("series", Some("2026-11-16T08:00:00")), ("starts-before", None)]
    );
    assert_eq!(found.iter().find(|e| e.event_id == "inside").unwrap(), &all[2], "as it was written");
}

#[test]
fn replacing_a_span_drops_what_the_server_no_longer_has_there() {
    let st = s();
    st.agenda_replace(
        A,
        0,
        1000,
        &[entry("a", None, 100, 200), entry("b", None, 300, 400), entry("c", None, 800, 900)],
    )
    .unwrap();
    // The server now has a changed `a`, no `b`, and a new `d`; `c` is outside the span asked for.
    st.agenda_replace(A, 0, 500, &[entry("a", None, 110, 210), entry("d", None, 450, 460)]).unwrap();
    let found = st.agenda_list(A, 0, 1000).unwrap();
    assert_eq!(starts(&found), [("a", None), ("c", None), ("d", None)]);
    assert_eq!(found.iter().find(|e| e.event_id == "a").unwrap().starts_at, 110);
    // An entry that began before the span and is still cached is replaced, not duplicated.
    st.agenda_replace(A, 400, 600, &[entry("d", None, 450, 460)]).unwrap();
    assert_eq!(st.agenda_list(A, 0, 1000).unwrap().len(), 3);
}

#[test]
fn an_event_is_forgotten_with_all_its_occurrences() {
    let st = s();
    st.agenda_replace(
        A,
        0,
        1000,
        &[entry("series", Some("r1"), 100, 110), entry("series", Some("r2"), 200, 210), entry("plain", None, 300, 310)],
    )
    .unwrap();
    st.agenda_forget_event(A, "series").unwrap();
    assert_eq!(starts(&st.agenda_list(A, 0, 1000).unwrap()), [("plain", None)]);
    st.agenda_forget_event(A, "never-cached").unwrap();
    assert_eq!(st.agenda_list(A, 0, 1000).unwrap().len(), 1);
}

#[test]
fn signing_out_takes_the_calendar_cache_with_it() {
    let st = s();
    st.calendars_replace(A, &[calendar("c1", "Personal")]).unwrap();
    st.agenda_replace(A, 0, 100, &[entry("e1", None, 10, 20)]).unwrap();
    st.delete_account(A).unwrap();
    st.save_account(&account(A)).unwrap();
    assert!(st.calendars_list(A).unwrap().is_empty());
    assert!(st.agenda_list(A, 0, 100).unwrap().is_empty());
}

#[test]
fn migration_six_keeps_a_version_five_database_and_adds_the_notify_table() {
    let mut conn = Connection::open_in_memory().unwrap();
    conn.pragma_update(None, "foreign_keys", "ON").unwrap();
    assert_eq!(schema::migrate(&mut conn, &schema::MIGRATIONS[..5]).unwrap(), 5);
    conn.execute_batch(
        "INSERT INTO accounts VALUES ('acc1', 'https://x', 'kaan', 5);
         INSERT INTO mailboxes VALUES ('acc1', 'in', 'Inbox', 'inbox', NULL, 1, 0, 0);
         INSERT INTO mailboxes VALUES ('acc1', 'w', 'Work', NULL, NULL, 2, 0, 0);
         INSERT INTO sync_state VALUES ('acc1', 'Email', 's1');",
    )
    .unwrap();

    assert_eq!(schema::migrate(&mut conn, schema::MIGRATIONS).unwrap(), schema::MIGRATIONS.len() - 5);
    let st = Store::init(conn).unwrap();
    assert_eq!(st.schema_version().unwrap() as usize, schema::MIGRATIONS.len());
    assert_eq!(st.get_state(A, "Email").unwrap(), Some("s1".into()));
    // Nothing notifies yet but the Inbox, and the choice can be made.
    let before: Vec<(String, bool)> = st.list_mailboxes(A).unwrap().into_iter().map(|m| (m.id, m.notify)).collect();
    assert_eq!(before, [("in".to_string(), true), ("w".to_string(), false)]);
    assert!(st.set_mailbox_notify(A, "w", true, 7).unwrap());
    assert_eq!(st.mailbox_notify_since(A, "w").unwrap(), Some(7));
    // It goes with the account.
    st.delete_account(A).unwrap();
    assert_eq!(st.mailbox_notify_since(A, "w").unwrap(), None);
}
