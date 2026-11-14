//! The address books and the contact cards, kept so the contact list and a contact can be shown
//! offline. The cards also feed the recipient suggestions (the `contacts` table), so a contact
//! the user just added or removed is seen there at once.

use std::sync::MutexGuard;

use rusqlite::{params, Transaction};

use super::Store;
use crate::{
    contacts::{Entry, Indexed},
    model::{AddressBook, Contact},
    Error, Result,
};

/// What the cache of cards knows about itself besides the rows.
#[derive(Default)]
pub(super) struct Cards {
    /// Counts the writes of single cards (`put`, `forget`). A listing that started before one
    /// is older than the cache and must not replace it.
    pub(super) generation: u64,
    /// The account whose cards the cache holds in full: a listing of them was kept by this
    /// process. Without it the cache may hold only the cards that were saved or opened.
    pub(super) listed: Option<String>,
}

fn json_error(e: serde_json::Error) -> Error {
    Error::store(e.to_string())
}

/// Makes the suggestions the mail addresses of the cards, in the order of the cards. A card
/// with no name of its own (the contact's name is then one of its addresses) suggests the
/// address alone.
fn rebuild_suggestions(tx: &Transaction, account_id: &str) -> Result<()> {
    tx.execute("DELETE FROM contacts WHERE account_id = ?1", [account_id])?;
    let mut cards = tx.prepare("SELECT contact_json FROM contact_cards WHERE account_id = ?1 ORDER BY position, id")?;
    let mut insert =
        tx.prepare("INSERT OR IGNORE INTO contacts (account_id, email, name, position) VALUES (?1, ?2, ?3, ?4)")?;
    let mut position = 0_i64;
    for json in cards.query_map([account_id], |r| r.get::<_, String>(0))? {
        // An unreadable row (written by another version of the app) is skipped.
        let Ok(contact) = serde_json::from_str::<Contact>(&json?) else { continue };
        let named = !contact.emails.iter().any(|e| e.value == contact.name);
        for email in &contact.emails {
            let name = if named { contact.name.as_str() } else { "" };
            insert.execute(params![account_id, email.value, name, position])?;
            position += 1;
        }
    }
    Ok(())
}

fn insert_card(tx: &Transaction, account_id: &str, position: i64, e: &Entry) -> Result<()> {
    tx.execute(
        "INSERT OR REPLACE INTO contact_cards
             (account_id, id, position, sort_key, search, digits, row_json, contact_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            account_id,
            e.id,
            position,
            e.sort_key,
            e.search,
            e.digits,
            serde_json::to_string(&e.row).map_err(json_error)?,
            serde_json::to_string(&e.contact).map_err(json_error)?
        ],
    )?;
    Ok(())
}

impl Store {
    pub(super) fn cards_state(&self) -> MutexGuard<'_, Cards> {
        self.cards.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Where the cache of cards is now; a listing that is to replace it asks before it starts.
    pub fn contact_generation(&self) -> u64 {
        self.cards_state().generation
    }

    /// Whether the cache holds all the account's cards as of a listing this process kept.
    pub fn contacts_listed(&self, account_id: &str) -> bool {
        self.cards_state().listed.as_deref() == Some(account_id)
    }

    /// The address books as the server last gave them, in its order.
    pub fn address_books_list(&self, account_id: &str) -> Result<Vec<AddressBook>> {
        self.read(|c| {
            let mut stmt = c.prepare("SELECT book FROM address_books WHERE account_id = ?1 ORDER BY position")?;
            let mut out = Vec::new();
            for json in stmt.query_map([account_id], |r| r.get::<_, String>(0))? {
                out.extend(serde_json::from_str(&json?).ok());
            }
            Ok(out)
        })
    }

    pub fn address_books_replace(&self, account_id: &str, books: &[AddressBook]) -> Result<()> {
        self.write(|c| {
            c.execute("DELETE FROM address_books WHERE account_id = ?1", [account_id])?;
            let mut insert = c.prepare("INSERT INTO address_books (account_id, position, book) VALUES (?1, ?2, ?3)")?;
            for (position, book) in books.iter().enumerate() {
                insert.execute(params![
                    account_id,
                    position as i64,
                    serde_json::to_string(book).map_err(json_error)?
                ])?;
            }
            Ok(())
        })
    }

    /// Replaces every cached card with `cards`, which are all the server has, in its order, if
    /// no card was stored or dropped since `generation` (see `contact_generation`): such a
    /// listing is older than a change the cache has. Returns whether it was kept.
    pub fn contact_cards_replace(&self, account_id: &str, cards: &[Entry], generation: u64) -> Result<bool> {
        self.write(|c| {
            let mut state = self.cards_state();
            if state.generation != generation {
                return Ok(false);
            }
            c.execute("DELETE FROM contact_cards WHERE account_id = ?1", [account_id])?;
            for (position, card) in cards.iter().enumerate() {
                insert_card(c, account_id, position as i64, card)?;
            }
            rebuild_suggestions(c, account_id)?;
            state.listed = Some(account_id.to_string());
            Ok(true)
        })
    }

    /// Stores one card the server has just given: a new one goes last, a known one keeps its place.
    pub fn contact_card_put(&self, account_id: &str, card: &Entry) -> Result<()> {
        self.write(|c| {
            let position = c.query_row(
                "SELECT COALESCE(
                     (SELECT position FROM contact_cards WHERE account_id = ?1 AND id = ?2),
                     (SELECT COALESCE(MAX(position) + 1, 0) FROM contact_cards WHERE account_id = ?1))",
                params![account_id, card.id],
                |r| r.get::<_, i64>(0),
            )?;
            insert_card(c, account_id, position, card)?;
            self.cards_state().generation += 1;
            rebuild_suggestions(c, account_id)
        })
    }

    /// Drops a card, so that one the user just deleted (or could not be read back) is not shown.
    pub fn contact_card_forget(&self, account_id: &str, id: &str) -> Result<()> {
        self.write(|c| {
            c.execute("DELETE FROM contact_cards WHERE account_id = ?1 AND id = ?2", [account_id, id])?;
            self.cards_state().generation += 1;
            rebuild_suggestions(c, account_id)
        })
    }

    /// The cached contact, if there is one.
    pub fn contact_card_get(&self, account_id: &str, id: &str) -> Result<Option<Contact>> {
        self.read(|c| {
            let mut stmt = c.prepare("SELECT contact_json FROM contact_cards WHERE account_id = ?1 AND id = ?2")?;
            let json = stmt.query_map([account_id, id], |r| r.get::<_, String>(0))?.next().transpose()?;
            Ok(json.and_then(|j| serde_json::from_str(&j).ok()))
        })
    }

    /// Every cached card as the list needs it, in the order of the list: by name, then by id.
    pub fn contact_index(&self, account_id: &str) -> Result<Vec<Indexed>> {
        self.read(|c| {
            let mut stmt = c.prepare(
                "SELECT row_json, search, digits FROM contact_cards WHERE account_id = ?1 ORDER BY sort_key, id",
            )?;
            let rows = stmt.query_map([account_id], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
            })?;
            let mut out = Vec::new();
            for row in rows {
                let (json, search, digits) = row?;
                if let Ok(row) = serde_json::from_str(&json) {
                    out.push(Indexed { row, search, digits });
                }
            }
            Ok(out)
        })
    }
}
