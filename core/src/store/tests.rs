use super::*;
use crate::model::{Action, Mailbox, MessageHeader};

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
    }
}

fn msg(id: &str, boxes: &[&str], at: i64, unread: bool) -> MessageHeader {
    MessageHeader {
        id: id.into(),
        thread_id: format!("t-{id}"),
        mailbox_ids: boxes.iter().map(|b| b.to_string()).collect(),
        from_name: "Ann".into(),
        from_email: "ann@example.com".into(),
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
