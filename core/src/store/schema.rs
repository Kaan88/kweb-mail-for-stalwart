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
    // 3: M4 (sending). Drafts, the sender identities and the address book.
    "
    -- `draft` is a `Draft` as JSON. `server_id` and `send_error` are columns because they are
    -- looked up and changed on their own; the columns win over what the JSON says.
    -- `state`: 'draft', 'sending' (held back until `send_at`), 'submitting' (handed to the
    -- server, outcome not known yet). `dirty`: the server copy is not current. `rev` counts the
    -- user's edits, so a pass that saved an older version does not mark the newer one saved.
    -- `threading`: the In-Reply-To and References ids as JSON; NULL until they are looked up.
    CREATE TABLE drafts (
        account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
        id         TEXT NOT NULL,
        draft      TEXT NOT NULL,
        server_id  TEXT,
        send_error TEXT,
        updated_at INTEGER NOT NULL,
        state      TEXT NOT NULL DEFAULT 'draft',
        send_at    INTEGER,
        dirty      INTEGER NOT NULL DEFAULT 0,
        discarded  INTEGER NOT NULL DEFAULT 0,
        rev        INTEGER NOT NULL DEFAULT 0,
        threading  TEXT,
        PRIMARY KEY (account_id, id)
    );
    CREATE INDEX drafts_by_server_id ON drafts (account_id, server_id);

    -- Sender identities in the server's order (the first is the default). An `Identity` as JSON.
    CREATE TABLE identities (
        account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
        position   INTEGER NOT NULL,
        identity   TEXT NOT NULL,
        PRIMARY KEY (account_id, position)
    );

    -- The address book, one row per address, in the server's order.
    CREATE TABLE contacts (
        account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
        email      TEXT NOT NULL COLLATE NOCASE,
        name       TEXT NOT NULL,
        position   INTEGER NOT NULL,
        PRIMARY KEY (account_id, email)
    );
    ",
    // 4: M5 (calendar). The calendar list and the agenda entries, so both can be shown offline.
    "
    -- The calendars in the server's order. A `Calendar` as JSON.
    CREATE TABLE calendars (
        account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
        position   INTEGER NOT NULL,
        calendar   TEXT NOT NULL,
        PRIMARY KEY (account_id, position)
    );

    -- What the agenda last showed: one row per event or occurrence (`recurrence_id` is '' for
    -- an event that does not repeat), the `Occurrence` as JSON. `starts_at` and `ends_at` are
    -- columns so a span can be answered without reading every row.
    CREATE TABLE agenda_entries (
        account_id    TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
        event_id      TEXT NOT NULL,
        recurrence_id TEXT NOT NULL,
        starts_at     INTEGER NOT NULL,
        ends_at       INTEGER NOT NULL,
        occurrence    TEXT NOT NULL,
        PRIMARY KEY (account_id, event_id, recurrence_id)
    );
    CREATE INDEX agenda_entries_by_start ON agenda_entries (account_id, starts_at);
    ",
    // 5: M6 (contacts). The address books and the contact cards, so the list and a contact can be
    // shown offline. `contacts` (the recipient suggestions) stays and is rebuilt from the cards.
    "
    -- The address books in the server's order. An `AddressBook` as JSON.
    CREATE TABLE address_books (
        account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
        position   INTEGER NOT NULL,
        book       TEXT NOT NULL,
        PRIMARY KEY (account_id, position)
    );

    -- One row per card: the `ContactRow` and the `Contact` as JSON, and what the list needs
    -- without reading them: the order (`sort_key`, folded name) and what a search looks at.
    -- `position` is the server's order, which the suggestions keep.
    CREATE TABLE contact_cards (
        account_id   TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
        id           TEXT NOT NULL,
        position     INTEGER NOT NULL,
        sort_key     TEXT NOT NULL,
        search       TEXT NOT NULL,
        digits       TEXT NOT NULL,
        row_json     TEXT NOT NULL,
        contact_json TEXT NOT NULL,
        PRIMARY KEY (account_id, id)
    );
    CREATE INDEX contact_cards_by_name ON contact_cards (account_id, sort_key, id);
    ",
    // 6: M10 (folders). Which folders notify, and since when. Local only, never sent to the
    // server. It is a table of its own so that a refresh of the mailbox list (which replaces the
    // rows of `mailboxes`) keeps it; it goes with its mailbox.
    "
    CREATE TABLE mailbox_notify (
        account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
        mailbox_id TEXT NOT NULL,
        since      INTEGER NOT NULL,
        PRIMARY KEY (account_id, mailbox_id)
    );
    ",
    // 7: M10 (labels). The keywords of each message, so the labels of a message can be shown
    // offline. `$seen` and `$flagged` are not here: `is_unread` and `is_flagged` hold them.
    // Mail cached before this has no rows; the sync does one full fetch to fill them (it keeps a
    // flag for that in `kv`, so the Email state is left as it was).
    "
    CREATE TABLE message_keywords (
        account_id TEXT NOT NULL,
        message_id TEXT NOT NULL,
        keyword    TEXT NOT NULL,
        PRIMARY KEY (account_id, message_id, keyword),
        FOREIGN KEY (account_id, message_id) REFERENCES messages(account_id, id) ON DELETE CASCADE
    );
    CREATE INDEX message_keywords_by_keyword ON message_keywords (account_id, keyword, message_id);
    ",
    // 8: notification choices per receiving address (device only). A row only where the user chose
    // something other than the default (notify): 0 off, 1 silent. Additive: nothing is fetched again.
    "
    CREATE TABLE address_notify (
        account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
        address    TEXT NOT NULL,
        mode       INTEGER NOT NULL,
        PRIMARY KEY (account_id, address)
    );
    ",
    // 9: M9 (OpenPGP and S/MIME). The keyring: public keys and certificates (never a private part;
    // those live in the host's secret store) and the fingerprint first seen for each address.
    // Additive: nothing is fetched again. `material` is an OpenPGP transferable public key or a DER
    // certificate. `has_secret` and `needs_passphrase` only say that the private part exists in the
    // secret store and whether it is locked.
    "
    CREATE TABLE crypto_keys (
        account_id       TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
        id               TEXT NOT NULL,
        kind             TEXT NOT NULL,
        label            TEXT NOT NULL,
        emails           TEXT NOT NULL,
        fingerprint      TEXT NOT NULL,
        origin           TEXT NOT NULL,
        origin_detail    TEXT NOT NULL DEFAULT '',
        added_at         INTEGER NOT NULL,
        material         BLOB NOT NULL,
        has_secret       INTEGER NOT NULL DEFAULT 0,
        needs_passphrase INTEGER NOT NULL DEFAULT 0,
        expires_at       INTEGER,
        PRIMARY KEY (account_id, id)
    );

    CREATE TABLE crypto_first_seen (
        account_id  TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
        address     TEXT NOT NULL,
        fingerprint TEXT NOT NULL,
        seen_at     INTEGER NOT NULL,
        PRIMARY KEY (account_id, address)
    );
    ",
    // 10: M9 stage 3. The finished bytes of a message that is signed or encrypted, made once when the user
    // sends it (after the checks, before anything goes to the server) and used for every retry, so a retry
    // uploads the same message and never makes a second, different one. Null for everything else.
    "
    ALTER TABLE drafts ADD COLUMN raw BLOB;
    ",
    // 11: M9 stage 4. The user's word that a key was checked by hand.
    "
    ALTER TABLE crypto_keys ADD COLUMN verified INTEGER NOT NULL DEFAULT 0;
    ",
    // 12: M11 stage 1. The addresses a message was also delivered to (Cc, and the Delivered-To and
    // X-Original-To headers the server wrote), as JSON; null until they were fetched (nothing is fetched
    // again for a message that has them). And the route a mail notification was posted on (the receiving
    // address and whether it was quiet), so a notification in the wrong channel is posted again.
    "
    ALTER TABLE messages ADD COLUMN recv_json TEXT;
    ALTER TABLE notifications_shown ADD COLUMN route TEXT NOT NULL DEFAULT '';
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
