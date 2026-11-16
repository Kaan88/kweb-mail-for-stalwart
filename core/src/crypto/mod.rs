//! M9: reading signed and encrypted mail (OpenPGP and S/MIME).
//!
//! Off by default and independent of the server: everything here works on the raw message
//! fetched over plain JMAP. What the user sees is a [`types::CryptoView`] made by
//! [`analyze::analyze`]; keys live in the keyring (public material, database) and, for the
//! user's own, in the host's secret store.
//!
//! What is kept in memory, and for how long: which messages are signed or encrypted (kinds only), and
//! the files of the last few messages that were opened (decrypted files, wiped when replaced, on
//! switching the feature off, on removing a key and on sign-out). Decrypted text is never kept: it
//! is made again each time the message is opened, and never written to the cache or a log.

pub mod analyze;
pub mod mime;
pub mod openpgp;
pub mod smime;
pub mod types;

use std::{collections::HashMap, sync::Mutex};

use analyze::HeldPart;

/// How many messages' decrypted files are held at once.
const HELD_MESSAGES: usize = 8;

#[derive(Default)]
pub struct State {
    kinds: Mutex<HashMap<String, types::CryptoKind>>,
    /// Files of opened messages, oldest first. Each part wipes itself when dropped.
    held: Mutex<Vec<(String, Vec<HeldPart>)>>,
}

impl State {
    pub fn kind(&self, message_id: &str) -> Option<types::CryptoKind> {
        self.kinds.lock().unwrap_or_else(|e| e.into_inner()).get(message_id).copied()
    }

    pub fn set_kind(&self, message_id: &str, kind: types::CryptoKind) {
        let mut kinds = self.kinds.lock().unwrap_or_else(|e| e.into_inner());
        if kinds.len() > 500 {
            kinds.clear();
        }
        kinds.insert(message_id.to_string(), kind);
    }

    /// Keeps the files of a message just opened (replacing its earlier ones; none: forgets them).
    pub fn set_held(&self, message_id: &str, parts: Vec<HeldPart>) {
        let mut held = self.held.lock().unwrap_or_else(|e| e.into_inner());
        held.retain(|(id, _)| id != message_id);
        if !parts.is_empty() {
            if held.len() >= HELD_MESSAGES {
                held.remove(0);
            }
            held.push((message_id.to_string(), parts));
        }
    }

    /// The bytes of one held file, handed to `f` and not copied out.
    pub fn with_part<T>(&self, message_id: &str, index: usize, f: impl FnOnce(&HeldPart) -> T) -> Option<T> {
        let held = self.held.lock().unwrap_or_else(|e| e.into_inner());
        held.iter().find(|(id, _)| id == message_id).and_then(|(_, parts)| parts.get(index)).map(f)
    }

    /// Forgets everything: the kinds and every held file.
    pub fn forget(&self) {
        self.kinds.lock().unwrap_or_else(|e| e.into_inner()).clear();
        self.held.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_decrypt;
