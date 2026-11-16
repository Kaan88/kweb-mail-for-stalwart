//! Reading settings the core decides on: whether remote pictures in a message may load.
//!
//! The rule: pictures load without a tap only in the user's own account, for a sender the user allowed or
//! when the user chose "always", and never for a message the core marked as blocked (S/MIME content). A
//! shared account ignores both the setting and the allow list. A decision is made when a message is opened
//! and is never kept anywhere a notification could read it.

use crate::compose::rules::address_key;

/// The account-wide choice for remote pictures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum RemotePictures {
    /// Ask for each message (the default).
    Off,
    /// Show them in every message of the user's own account.
    Always,
}

impl RemotePictures {
    pub(crate) fn code(self) -> &'static str {
        match self {
            RemotePictures::Off => "off",
            RemotePictures::Always => "always",
        }
    }

    pub(crate) fn from_code(code: Option<&str>) -> Self {
        if code == Some("always") {
            RemotePictures::Always
        } else {
            RemotePictures::Off
        }
    }
}

/// Whether remote pictures may load in a message with no tap. `shared` is a message of another user's
/// account, `blocked` a message whose content the core says must stay without remote content.
pub(crate) fn may_load(shared: bool, blocked: bool, mode: RemotePictures, sender_allowed: bool) -> bool {
    if shared || blocked {
        return false;
    }
    mode == RemotePictures::Always || sender_allowed
}

/// The key a sender is kept under: the From address in lower case.
pub(crate) fn sender_key(address: &str) -> String {
    address_key(address)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_and_blocked_never_load() {
        assert!(!may_load(true, false, RemotePictures::Always, true));
        assert!(!may_load(false, true, RemotePictures::Always, true));
        assert!(may_load(false, false, RemotePictures::Always, false));
        assert!(may_load(false, false, RemotePictures::Off, true));
        assert!(!may_load(false, false, RemotePictures::Off, false));
    }
}
