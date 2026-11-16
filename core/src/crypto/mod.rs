//! M9: reading signed and encrypted mail (OpenPGP and S/MIME).
//!
//! Off by default and independent of the server: everything here works on the raw message
//! fetched over plain JMAP. What the user sees is a [`types::CryptoView`] made by
//! [`analyze::analyze`]; keys live in the keyring (public material, database) and, for the
//! user's own, in the host's secret store.

pub mod analyze;
pub mod mime;
pub mod openpgp;
pub mod smime;
pub mod types;

use std::{collections::HashMap, sync::Mutex};

/// What the core keeps in memory for the feature: nothing that is a secret and nothing that is
/// mail content, only which messages were found to be signed or encrypted.
#[derive(Default)]
pub struct State {
    kinds: Mutex<HashMap<String, types::CryptoKind>>,
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

    pub fn forget(&self) {
        self.kinds.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }
}

#[cfg(test)]
mod tests;
