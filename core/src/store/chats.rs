//! Earlier chats of the assistant (M12 stage 6): the words of each conversation, kept per account on
//! the device. Only what the user typed and what the assistant answered is kept: no tool call, no
//! tool result, no mail text that the model looked up, no suggestion card. Chat text is the user's
//! data: it is never logged and never in an error.

use rusqlite::OptionalExtension;

use super::Store;
use crate::Result;

/// The most chats kept per account; the least recently used go first.
pub const MOST_CHATS: usize = 50;
/// The most characters of one chat's words; the oldest exchanges go first.
pub const MOST_CHARS: usize = 200_000;

/// One message of a kept chat.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StoredMessage {
    pub user: bool,
    pub text: String,
}

/// A kept chat without its words, for the list.
#[derive(Debug, Clone, PartialEq)]
pub struct ChatInfo {
    pub id: String,
    pub title: String,
    pub updated_at: i64,
    pub base_url: String,
    pub model: String,
    pub about_subject: Option<String>,
}

/// A kept chat with its words.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredChat {
    pub info: ChatInfo,
    pub messages: Vec<StoredMessage>,
}

/// What the `provider` column holds for every new row: the one protocol there is. Rows of the
/// Anthropic service that is gone may still say "anthropic"; the column is not read.
const PROTOCOL: &str = "openai";

fn chars(text: &str) -> usize {
    text.chars().count()
}

/// Drops the oldest exchanges (a user message and what followed it) until the words fit.
fn trim(messages: &mut Vec<StoredMessage>) {
    let mut total: usize = messages.iter().map(|m| chars(&m.text)).sum();
    while total > MOST_CHARS && messages.len() > 2 {
        let end = messages.iter().skip(1).position(|m| m.user).map_or(messages.len(), |p| p + 1);
        let end = end.min(messages.len() - 2);
        for m in messages.drain(..end.max(1)) {
            total -= chars(&m.text);
        }
    }
}

impl Store {
    /// Adds an exchange (what the user wrote and the answer) to chat `id`, making the chat when it is
    /// new (`title` then names it, `about_subject` is the conversation it was opened about). The
    /// service and model are those of the latest exchange; `now` is when it happened.
    #[allow(clippy::too_many_arguments)]
    pub fn chat_append(
        &self,
        account_id: &str,
        id: &str,
        title: &str,
        now: i64,
        base_url: &str,
        model: &str,
        about_subject: Option<&str>,
        user_text: &str,
        answer: &str,
    ) -> Result<()> {
        self.write(|c| {
            let old: Option<(String, i64)> = c
                .query_row(
                    "SELECT messages, created_at FROM ai_chats WHERE account_id = ?1 AND id = ?2",
                    [account_id, id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            let created = old.as_ref().map_or(now, |(_, at)| *at);
            let mut messages: Vec<StoredMessage> =
                old.and_then(|(json, _)| serde_json::from_str(&json).ok()).unwrap_or_default();
            messages.push(StoredMessage { user: true, text: user_text.to_string() });
            messages.push(StoredMessage { user: false, text: answer.to_string() });
            trim(&mut messages);
            let json = serde_json::to_string(&messages).unwrap_or_else(|_| "[]".into());
            c.execute(
                "INSERT INTO ai_chats
                   (account_id, id, title, created_at, updated_at, provider, base_url, model, about_subject, messages)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                 ON CONFLICT (account_id, id) DO UPDATE SET
                   updated_at = excluded.updated_at, provider = excluded.provider, base_url = excluded.base_url,
                   model = excluded.model, messages = excluded.messages",
                rusqlite::params![
                    account_id,
                    id,
                    title,
                    created,
                    now,
                    PROTOCOL,
                    base_url,
                    model,
                    about_subject,
                    json
                ],
            )?;
            c.execute(
                "DELETE FROM ai_chats WHERE account_id = ?1 AND id IN (
                   SELECT id FROM ai_chats WHERE account_id = ?1 ORDER BY updated_at DESC, rowid DESC LIMIT -1 OFFSET ?2)",
                rusqlite::params![account_id, MOST_CHATS as i64],
            )?;
            Ok(())
        })
    }

    /// The kept chats of the account, the most recently used first.
    pub fn chat_list(&self, account_id: &str) -> Result<Vec<ChatInfo>> {
        self.read(|c| {
            let mut stmt = c.prepare(
                "SELECT id, title, updated_at, provider, base_url, model, about_subject FROM ai_chats
                 WHERE account_id = ?1 ORDER BY updated_at DESC, rowid DESC",
            )?;
            let rows = stmt.query_map([account_id], |r| {
                Ok(ChatInfo {
                    id: r.get(0)?,
                    title: r.get(1)?,
                    updated_at: r.get(2)?,
                    base_url: r.get(4)?,
                    model: r.get(5)?,
                    about_subject: r.get(6)?,
                })
            })?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    pub fn chat_load(&self, account_id: &str, id: &str) -> Result<Option<StoredChat>> {
        self.read(|c| {
            Ok(c.query_row(
                "SELECT id, title, updated_at, provider, base_url, model, about_subject, messages FROM ai_chats
                 WHERE account_id = ?1 AND id = ?2",
                [account_id, id],
                |r| {
                    let json: String = r.get(7)?;
                    Ok(StoredChat {
                        info: ChatInfo {
                            id: r.get(0)?,
                            title: r.get(1)?,
                            updated_at: r.get(2)?,
                            base_url: r.get(4)?,
                            model: r.get(5)?,
                            about_subject: r.get(6)?,
                        },
                        messages: serde_json::from_str(&json).unwrap_or_default(),
                    })
                },
            )
            .optional()?)
        })
    }

    /// Removes one kept chat; whether there was one.
    pub fn chat_delete(&self, account_id: &str, id: &str) -> Result<bool> {
        self.write(|c| Ok(c.execute("DELETE FROM ai_chats WHERE account_id = ?1 AND id = ?2", [account_id, id])? > 0))
    }

    /// Removes every kept chat of the account; how many there were.
    pub fn chat_delete_all(&self, account_id: &str) -> Result<usize> {
        self.write(|c| Ok(c.execute("DELETE FROM ai_chats WHERE account_id = ?1", [account_id])?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> Store {
        let store = Store::open_in_memory().unwrap();
        store
            .save_account(&crate::model::Account {
                id: "a".into(),
                server_url: "https://x".into(),
                username: "u".into(),
                signed_in_at: 1,
            })
            .unwrap();
        store
    }

    fn put(s: &Store, id: &str, now: i64, user: &str, answer: &str) {
        s.chat_append("a", id, "Title", now, "https://api.example/v1", "m", None, user, answer).unwrap();
    }

    #[test]
    fn a_chat_is_kept_with_its_words_and_grows() {
        let s = store();
        put(&s, "c1", 10, "hello", "hi");
        put(&s, "c1", 20, "again", "yes");
        let chat = s.chat_load("a", "c1").unwrap().unwrap();
        assert_eq!(chat.messages.len(), 4);
        assert_eq!(chat.messages[2], StoredMessage { user: true, text: "again".into() });
        assert_eq!(chat.info.updated_at, 20);
        // The title and the creation time are those of the first exchange.
        assert_eq!(s.chat_list("a").unwrap().len(), 1);
        assert!(s.chat_load("a", "other").unwrap().is_none());
    }

    #[test]
    fn the_list_is_newest_first_deleting_one_or_all_and_the_most_kept() {
        let s = store();
        for i in 0..(MOST_CHATS as i64 + 5) {
            put(&s, &format!("c{i}"), 100 + i, "q", "a");
        }
        let list = s.chat_list("a").unwrap();
        assert_eq!(list.len(), MOST_CHATS);
        assert_eq!(list[0].id, format!("c{}", MOST_CHATS + 4));
        assert!(s.chat_load("a", "c0").unwrap().is_none(), "the oldest went");
        assert!(s.chat_delete("a", &list[0].id).unwrap());
        assert!(!s.chat_delete("a", &list[0].id).unwrap());
        assert_eq!(s.chat_delete_all("a").unwrap(), MOST_CHATS - 1);
        assert!(s.chat_list("a").unwrap().is_empty());
    }

    #[test]
    fn a_long_chat_loses_its_oldest_exchanges_first() {
        let s = store();
        let big = "x".repeat(MOST_CHARS / 3);
        for i in 0..4 {
            put(&s, "c", i, &format!("{i}{big}"), "ok");
        }
        let chat = s.chat_load("a", "c").unwrap().unwrap();
        assert!(chat.messages.iter().map(|m| chars(&m.text)).sum::<usize>() <= MOST_CHARS);
        assert!(chat.messages[0].user && !chat.messages.iter().any(|m| m.text.starts_with('0')));
        assert!(chat.messages.last().is_some_and(|m| !m.user), "ends with an answer");
    }

    #[test]
    fn chats_go_with_the_account_and_are_not_shared_between_accounts() {
        let s = store();
        put(&s, "c1", 1, "q", "a");
        assert!(s.chat_list("b").unwrap().is_empty());
        s.delete_account("a").unwrap();
        assert!(s.chat_list("a").unwrap().is_empty());
    }
}
