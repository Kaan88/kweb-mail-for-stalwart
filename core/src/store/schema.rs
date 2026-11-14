//! Schema and migrations. The version lives in `PRAGMA user_version`.
//! To change the schema, append a new entry to `MIGRATIONS`; never edit an old one,
//! because installed databases have already run it.

use crate::Result;
use rusqlite::Connection;

pub(super) const MIGRATIONS: &[&str] = &[
    // 1: the first schema.
    "
    CREATE TABLE accounts (
        id           TEXT PRIMARY KEY,
        server_url   TEXT NOT NULL,
        username     TEXT NOT NULL,
        signed_in_at INTEGER NOT NULL
    );

    CREATE TABLE kv (
        account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
        key        TEXT NOT NULL,
        value      TEXT NOT NULL,
        PRIMARY KEY (account_id, key)
    );

    CREATE TABLE sync_state (
        account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
        data_type  TEXT NOT NULL,
        state      TEXT NOT NULL,
        PRIMARY KEY (account_id, data_type)
    );

    CREATE TABLE mailboxes (
        account_id    TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
        id            TEXT NOT NULL,
        name          TEXT NOT NULL,
        role          TEXT,
        parent_id     TEXT,
        sort_order    INTEGER NOT NULL,
        total_emails  INTEGER NOT NULL,
        unread_emails INTEGER NOT NULL,
        PRIMARY KEY (account_id, id)
    );

    CREATE TABLE messages (
        account_id     TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
        id             TEXT NOT NULL,
        thread_id      TEXT NOT NULL,
        from_name      TEXT NOT NULL,
        from_email     TEXT NOT NULL,
        subject        TEXT NOT NULL,
        preview        TEXT NOT NULL,
        received_at    INTEGER NOT NULL,
        is_unread      INTEGER NOT NULL,
        is_flagged     INTEGER NOT NULL,
        has_attachment INTEGER NOT NULL,
        size           INTEGER NOT NULL,
        PRIMARY KEY (account_id, id)
    );
    CREATE INDEX messages_by_date ON messages (account_id, received_at DESC);

    -- No foreign key to mailboxes: a message can be synced before its mailbox is.
    CREATE TABLE message_mailboxes (
        account_id TEXT NOT NULL,
        message_id TEXT NOT NULL,
        mailbox_id TEXT NOT NULL,
        PRIMARY KEY (account_id, message_id, mailbox_id),
        FOREIGN KEY (account_id, message_id) REFERENCES messages(account_id, id) ON DELETE CASCADE
    );
    CREATE INDEX message_mailboxes_by_mailbox ON message_mailboxes (account_id, mailbox_id, message_id);

    -- What the core last told Android to show.
    CREATE TABLE notifications_shown (
        account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
        message_id TEXT NOT NULL,
        posted_at  INTEGER NOT NULL,
        PRIMARY KEY (account_id, message_id)
    );

    -- Swiped away by the user. No foreign key to messages: the row may outlive the message
    -- until `prune_dismissed` runs.
    CREATE TABLE notifications_dismissed (
        account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
        message_id TEXT NOT NULL,
        PRIMARY KEY (account_id, message_id)
    );

    CREATE TABLE outbox (
        id         INTEGER PRIMARY KEY AUTOINCREMENT,
        account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
        kind       TEXT NOT NULL,
        message_id TEXT NOT NULL,
        attempts   INTEGER NOT NULL DEFAULT 0,
        last_error TEXT
    );
    ",
    // 2: M3 (reading). Recipients on messages, the target of a queued Move, cached bodies, and
    // how far back each mailbox is cached.
    "
    ALTER TABLE messages ADD COLUMN to_json TEXT NOT NULL DEFAULT '[]';
    ALTER TABLE outbox ADD COLUMN target TEXT;
    CREATE INDEX messages_by_thread ON messages (account_id, thread_id);

    -- A `MessageBody` as JSON. Goes away with its message.
    CREATE TABLE message_bodies (
        account_id TEXT NOT NULL,
        message_id TEXT NOT NULL,
        body       TEXT NOT NULL,
        PRIMARY KEY (account_id, message_id),
        FOREIGN KEY (account_id, message_id) REFERENCES messages(account_id, id) ON DELETE CASCADE
    );

    -- The cache holds the mailbox without gaps from now back to `oldest` (a received_at).
    -- No row: the whole mailbox is cached.
    CREATE TABLE mailbox_windows (
        account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
        mailbox_id TEXT NOT NULL,
        oldest     INTEGER NOT NULL,
        PRIMARY KEY (account_id, mailbox_id)
    );

    -- One full resync, which fills `to_json` and the windows.
    DELETE FROM sync_state WHERE data_type = 'Email';
    ",
];

/// Runs the migrations the database has not seen yet, each in its own transaction.
/// Returns how many ran.
pub(super) fn migrate(conn: &mut Connection, migrations: &[&str]) -> Result<usize> {
    let current = conn.query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))? as usize;
    if current > migrations.len() {
        // A database from a newer app version: better to refuse than to guess.
        return Err(crate::Error::store(format!(
            "database version {current} is newer than this app understands ({})",
            migrations.len()
        )));
    }
    for (i, sql) in migrations.iter().enumerate().skip(current) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        // PRAGMA does not accept bound parameters; the number is ours.
        tx.execute_batch(&format!("PRAGMA user_version = {}", i + 1))?;
        tx.commit()?;
    }
    Ok(migrations.len() - current)
}
