//! The keywords of cached messages: what the labels are made of.
//!
//! `$seen` and `$flagged` are not kept here; `is_unread` and `is_flagged` hold them.

use std::collections::HashMap;

use rusqlite::{params, Connection};

use super::messages::{query_headers, replace, upsert, COLUMNS};
use super::Store;
use crate::model::MessageHeader;
use crate::Result;

/// The keywords the table does not hold.
fn is_kept(keyword: &str) -> bool {
    keyword != "$seen" && keyword != "$flagged"
}

/// Replaces the keywords of every message in `msgs` by what `keywords` says (a message with no
/// entry has none).
fn write_keywords(
    c: &Connection,
    account_id: &str,
    msgs: &[MessageHeader],
    keywords: &HashMap<String, Vec<String>>,
) -> Result<()> {
    let mut clear = c.prepare_cached("DELETE FROM message_keywords WHERE account_id = ?1 AND message_id = ?2")?;
    let mut put = c.prepare_cached(
        "INSERT OR IGNORE INTO message_keywords (account_id, message_id, keyword) VALUES (?1, ?2, ?3)",
    )?;
    for m in msgs {
        clear.execute([account_id, &m.id])?;
        for keyword in keywords.get(&m.id).into_iter().flatten().filter(|k| is_kept(k)) {
            put.execute([account_id, &m.id, keyword])?;
        }
    }
    Ok(())
}

impl Store {
    /// Like [`Store::upsert_messages`], and the keywords of each message are replaced by what
    /// `keywords` (message id to keywords) says, in the same transaction.
    pub fn upsert_messages_with_keywords(
        &self,
        account_id: &str,
        msgs: &[MessageHeader],
        keywords: &HashMap<String, Vec<String>>,
    ) -> Result<()> {
        self.write(|c| {
            upsert(c, account_id, msgs)?;
            write_keywords(c, account_id, msgs, keywords)
        })
    }

    /// Like [`Store::replace_messages`], with the keywords as in
    /// [`Store::upsert_messages_with_keywords`].
    pub fn replace_messages_with_keywords(
        &self,
        account_id: &str,
        msgs: &[MessageHeader],
        keywords: &HashMap<String, Vec<String>>,
    ) -> Result<()> {
        self.write(|c| {
            replace(c, account_id, msgs)?;
            write_keywords(c, account_id, msgs, keywords)
        })
    }

    /// The keywords of these messages, sorted; a message without any (or not cached) has no entry.
    pub fn message_keywords(&self, account_id: &str, ids: &[String]) -> Result<HashMap<String, Vec<String>>> {
        self.read(|c| {
            let mut stmt = c.prepare_cached(
                "SELECT keyword FROM message_keywords WHERE account_id = ?1 AND message_id = ?2 ORDER BY keyword",
            )?;
            let mut out = HashMap::new();
            for id in ids {
                let found: Vec<String> =
                    stmt.query_map([account_id, id], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
                if !found.is_empty() {
                    out.insert(id.clone(), found);
                }
            }
            Ok(out)
        })
    }

    /// Puts a keyword on a cached message or takes it off. False when the message is not cached.
    pub fn set_keyword(&self, account_id: &str, id: &str, keyword: &str, on: bool) -> Result<bool> {
        self.write(|c| {
            let sql = if on {
                "INSERT OR IGNORE INTO message_keywords (account_id, message_id, keyword)
                 SELECT ?1, ?2, ?3 WHERE EXISTS (SELECT 1 FROM messages WHERE account_id = ?1 AND id = ?2)"
            } else {
                "DELETE FROM message_keywords WHERE account_id = ?1 AND message_id = ?2 AND keyword = ?3"
            };
            c.execute(sql, params![account_id, id, keyword])?;
            Ok(c.query_row("SELECT COUNT(*) FROM messages WHERE account_id = ?1 AND id = ?2", [account_id, id], |r| {
                r.get::<_, u32>(0)
            })? > 0)
        })
    }

    /// Every keyword some cached message carries, sorted.
    pub fn known_keywords(&self, account_id: &str) -> Result<Vec<String>> {
        self.read(|c| {
            let mut stmt =
                c.prepare("SELECT DISTINCT keyword FROM message_keywords WHERE account_id = ?1 ORDER BY keyword")?;
            let rows = stmt.query_map([account_id], |r| r.get(0))?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    /// The cached messages that carry `keyword`, newest first.
    pub fn messages_with_keyword(&self, account_id: &str, keyword: &str, limit: u32) -> Result<Vec<MessageHeader>> {
        self.read(|c| {
            let sql = format!(
                "SELECT {COLUMNS} FROM messages m
                 JOIN message_keywords k ON k.account_id = m.account_id AND k.message_id = m.id
                 WHERE m.account_id = ?1 AND k.keyword = ?2
                 ORDER BY m.received_at DESC, m.id DESC LIMIT ?3"
            );
            query_headers(c, &sql, params![account_id, keyword, limit], account_id)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Account;
    use crate::store::schema;

    const A: &str = "acc1";

    fn store() -> Store {
        let st = Store::open_in_memory().unwrap();
        st.save_account(&Account {
            id: A.into(),
            server_url: "https://mail.example.com".into(),
            username: "kaan".into(),
            signed_in_at: 1000,
        })
        .unwrap();
        st
    }

    fn msg(id: &str, at: i64) -> MessageHeader {
        MessageHeader {
            id: id.into(),
            thread_id: format!("t-{id}"),
            mailbox_ids: vec!["inbox".into()],
            from_name: "Ann".into(),
            from_email: "ann@example.com".into(),
            to: Vec::new(),
            subject: format!("subject {id}"),
            preview: "preview".into(),
            received_at: at,
            is_unread: true,
            is_flagged: false,
            has_attachment: false,
            size: 1,
        }
    }

    fn kw(list: &[(&str, &[&str])]) -> HashMap<String, Vec<String>> {
        list.iter().map(|(id, k)| (id.to_string(), k.iter().map(|k| k.to_string()).collect())).collect()
    }

    fn strs(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn keywords_are_stored_with_the_message_and_replaced_each_time() {
        let st = store();
        let both = [msg("a", 1), msg("b", 2)];
        st.upsert_messages_with_keywords(A, &both, &kw(&[("a", &["work", "$seen", "$flagged", "home"]), ("b", &[])]))
            .unwrap();
        let got = st.message_keywords(A, &strs(&["a", "b", "nope"])).unwrap();
        // `$seen` and `$flagged` are what `is_unread` and `is_flagged` say.
        assert_eq!(got, kw(&[("a", &["home", "work"])]));
        st.upsert_messages_with_keywords(A, &both[..1], &kw(&[("a", &["work"])])).unwrap();
        assert_eq!(st.message_keywords(A, &strs(&["a"])).unwrap(), kw(&[("a", &["work"])]));
        // A message with no entry has none; the old way to store leaves them alone.
        st.upsert_messages(A, &both[1..]).unwrap();
        st.upsert_messages_with_keywords(A, &both[..1], &HashMap::new()).unwrap();
        assert!(st.message_keywords(A, &strs(&["a"])).unwrap().is_empty());
        st.upsert_messages_with_keywords(A, &both[..1], &kw(&[("a", &["x"])])).unwrap();
        st.upsert_messages(A, &both[..1]).unwrap();
        assert_eq!(st.message_keywords(A, &strs(&["a"])).unwrap(), kw(&[("a", &["x"])]));
    }

    #[test]
    fn a_full_resync_drops_the_keywords_of_what_is_gone() {
        let st = store();
        st.upsert_messages_with_keywords(A, &[msg("a", 1), msg("b", 2)], &kw(&[("a", &["x"]), ("b", &["y"])])).unwrap();
        st.replace_messages_with_keywords(A, &[msg("b", 2)], &kw(&[("b", &["z"])])).unwrap();
        assert!(st.message_keywords(A, &strs(&["a"])).unwrap().is_empty());
        assert_eq!(st.known_keywords(A).unwrap(), ["z"]);
        st.delete_messages(A, &strs(&["b"])).unwrap();
        assert!(st.known_keywords(A).unwrap().is_empty(), "they go with the message");
    }

    #[test]
    fn a_keyword_is_put_on_and_taken_off_a_cached_message_only() {
        let st = store();
        st.upsert_messages_with_keywords(A, &[msg("a", 1)], &kw(&[("a", &["x"])])).unwrap();
        assert!(st.set_keyword(A, "a", "y", true).unwrap());
        assert!(st.set_keyword(A, "a", "y", true).unwrap(), "twice is once");
        assert_eq!(st.message_keywords(A, &strs(&["a"])).unwrap(), kw(&[("a", &["x", "y"])]));
        assert!(st.set_keyword(A, "a", "x", false).unwrap());
        assert_eq!(st.message_keywords(A, &strs(&["a"])).unwrap(), kw(&[("a", &["y"])]));
        assert!(!st.set_keyword(A, "gone", "y", true).unwrap());
        assert!(st.message_keywords(A, &strs(&["gone"])).unwrap().is_empty());
    }

    #[test]
    fn the_messages_with_a_keyword_come_newest_first_within_the_limit() {
        let st = store();
        let all = [msg("a", 10), msg("b", 30), msg("c", 20), msg("d", 40)];
        st.upsert_messages_with_keywords(
            A,
            &all,
            &kw(&[("a", &["x"]), ("b", &["x"]), ("c", &["x", "y"]), ("d", &["y"])]),
        )
        .unwrap();
        let ids = |k: &str, limit| -> Vec<String> {
            st.messages_with_keyword(A, k, limit).unwrap().into_iter().map(|m| m.id).collect()
        };
        assert_eq!(ids("x", 10), ["b", "c", "a"]);
        assert_eq!(ids("x", 2), ["b", "c"]);
        assert_eq!(ids("y", 10), ["d", "c"]);
        assert!(ids("none", 10).is_empty());
        assert_eq!(st.known_keywords(A).unwrap(), ["x", "y"]);
    }

    #[test]
    fn migration_seven_adds_the_table_and_keeps_the_rest() {
        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        assert_eq!(schema::migrate(&mut conn, &schema::MIGRATIONS[..6]).unwrap(), 6);
        conn.execute_batch(
            "INSERT INTO accounts VALUES ('acc1', 'https://x', 'kaan', 5);
             INSERT INTO messages VALUES ('acc1', 'm1', 't1', 'Ann', 'ann@x', 'Hi', 'p', 100, 1, 0, 0, 7, '[]');
             INSERT INTO sync_state VALUES ('acc1', 'Email', 's1'), ('acc1', 'Mailbox', 's2');",
        )
        .unwrap();
        assert_eq!(schema::migrate(&mut conn, schema::MIGRATIONS).unwrap(), schema::MIGRATIONS.len() - 6);
        let st = Store::init(conn).unwrap();
        assert!(st.get_message(A, "m1").unwrap().is_some(), "the cache is kept");
        assert!(st.message_keywords(A, &strs(&["m1"])).unwrap().is_empty());
        assert_eq!(st.get_state(A, "Email").unwrap(), Some("s1".into()));
        assert_eq!(st.get_state(A, "Mailbox").unwrap(), Some("s2".into()));
    }
}
