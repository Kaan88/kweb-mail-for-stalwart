//! SQLite cache: mailboxes, messages, bodies, state strings, outbox, shown notifications.
//!
//! Calls are short and synchronous; the connection sits behind a mutex so one `Arc<Store>`
//! can be shared by async tasks. Never hold the store across an `.await`.
//! Every table is keyed by account id. Times are Unix seconds.

mod bodies;
mod bookkeeping;
mod calendar;
mod contacts;
mod drafts;
mod mailboxes;
mod messages;
mod outbox;
mod schema;
#[cfg(test)]
mod tests;
mod threads;

pub use drafts::{DraftRow, DraftState, Threading, DIRECTORY_EVERY};
pub use outbox::Job;

use crate::model::Account;
use crate::Result;
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use std::path::Path;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

pub struct Store {
    conn: Mutex<Connection>,
    /// See [`Store::pass_lock`].
    pass: tokio::sync::Mutex<()>,
    /// When the identities and the address book were last fetched by this process.
    directory: Mutex<Option<Instant>>,
    /// See [`contacts::Cards`].
    cards: Mutex<contacts::Cards>,
}

impl Store {
    /// Held for the length of one pass (send queued actions, sync, run the notification
    /// rule). The live connection and the 15-minute check share one cache; if two passes
    /// ran at once, both would read "not shown yet" and the same mail would be announced
    /// twice, or announced to the one nobody is listening to.
    pub fn pass_lock(&self) -> &tokio::sync::Mutex<()> {
        &self.pass
    }

    pub fn open(path: &Path) -> Result<Store> {
        let conn = Connection::open(path)?;
        // WAL lets the UI read while sync writes. The pragma answers with a row.
        conn.query_row("PRAGMA journal_mode = WAL", [], |r| r.get::<_, String>(0))?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Store> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(mut conn: Connection) -> Result<Store> {
        // Without a timeout a second connection (the Android worker process) fails at once.
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        schema::migrate(&mut conn, schema::MIGRATIONS)?;
        Ok(Store {
            conn: Mutex::new(conn),
            pass: tokio::sync::Mutex::new(()),
            directory: Mutex::new(None),
            cards: Mutex::default(),
        })
    }

    /// The schema version of the open database.
    pub fn schema_version(&self) -> Result<u32> {
        self.read(|c| Ok(c.query_row("PRAGMA user_version", [], |r| r.get(0))?))
    }

    /// Runs `f` on the connection. A poisoned lock is still usable: SQLite state is
    /// consistent because writers use transactions.
    fn lock(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn read<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        f(&self.lock())
    }

    /// Runs `f` in one transaction; rolls back if it returns an error.
    fn write<T>(&self, f: impl FnOnce(&Transaction) -> Result<T>) -> Result<T> {
        let mut guard = self.lock();
        let tx = guard.transaction()?;
        let out = f(&tx)?;
        tx.commit()?;
        Ok(out)
    }

    // ---- account ----

    /// Inserts or updates; the rows that belong to the account are left alone.
    pub fn save_account(&self, account: &Account) -> Result<()> {
        self.write(|c| {
            c.execute(
                "INSERT INTO accounts (id, server_url, username, signed_in_at) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(id) DO UPDATE SET server_url = ?2, username = ?3, signed_in_at = ?4",
                params![account.id, account.server_url, account.username, account.signed_in_at],
            )?;
            Ok(())
        })
    }

    /// The current account. Phase 1 has one; with several, the oldest saved.
    pub fn load_account(&self) -> Result<Option<Account>> {
        self.read(|c| {
            Ok(c.query_row(
                "SELECT id, server_url, username, signed_in_at FROM accounts ORDER BY rowid LIMIT 1",
                [],
                |r| Ok(Account { id: r.get(0)?, server_url: r.get(1)?, username: r.get(2)?, signed_in_at: r.get(3)? }),
            )
            .optional()?)
        })
    }

    /// Sign-out: removes the account and, by cascade, everything cached for it.
    pub fn delete_account(&self, account_id: &str) -> Result<()> {
        self.write(|c| {
            c.execute("DELETE FROM accounts WHERE id = ?1", [account_id])?;
            let mut cards = self.cards_state();
            (cards.listed, cards.generation) = (None, cards.generation + 1);
            Ok(())
        })
    }

    // ---- key/value ----

    pub fn kv_get(&self, account_id: &str, key: &str) -> Result<Option<String>> {
        self.read(|c| {
            Ok(c.query_row("SELECT value FROM kv WHERE account_id = ?1 AND key = ?2", [account_id, key], |r| r.get(0))
                .optional()?)
        })
    }

    pub fn kv_set(&self, account_id: &str, key: &str, value: &str) -> Result<()> {
        self.write(|c| {
            c.execute(
                "INSERT INTO kv (account_id, key, value) VALUES (?1, ?2, ?3)
                 ON CONFLICT(account_id, key) DO UPDATE SET value = ?3",
                [account_id, key, value],
            )?;
            Ok(())
        })
    }

    pub fn kv_delete(&self, account_id: &str, key: &str) -> Result<()> {
        self.write(|c| {
            c.execute("DELETE FROM kv WHERE account_id = ?1 AND key = ?2", [account_id, key])?;
            Ok(())
        })
    }

    // ---- JMAP state strings ----

    pub fn get_state(&self, account_id: &str, data_type: &str) -> Result<Option<String>> {
        self.read(|c| {
            Ok(c.query_row(
                "SELECT state FROM sync_state WHERE account_id = ?1 AND data_type = ?2",
                [account_id, data_type],
                |r| r.get(0),
            )
            .optional()?)
        })
    }

    pub fn set_state(&self, account_id: &str, data_type: &str, state: &str) -> Result<()> {
        self.write(|c| {
            c.execute(
                "INSERT INTO sync_state (account_id, data_type, state) VALUES (?1, ?2, ?3)
                 ON CONFLICT(account_id, data_type) DO UPDATE SET state = ?3",
                [account_id, data_type, state],
            )?;
            Ok(())
        })
    }

    /// Forgets the state so the next sync is a full resync.
    pub fn clear_state(&self, account_id: &str, data_type: &str) -> Result<()> {
        self.write(|c| {
            c.execute("DELETE FROM sync_state WHERE account_id = ?1 AND data_type = ?2", [account_id, data_type])?;
            Ok(())
        })
    }
}
