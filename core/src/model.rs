//! The data the core hands out. Plain records: they cross the UniFFI bridge as they are.
//! Times are Unix seconds, UTC.

/// The signed-in account. Phase 1 has one; every table is keyed by `id` so more can follow.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Account {
    /// The JMAP account id.
    pub id: String,
    /// `https://host[:port]` of the Stalwart server.
    pub server_url: String,
    pub username: String,
    /// Mail that arrived before this is never notified.
    pub signed_in_at: i64,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Mailbox {
    pub id: String,
    pub name: String,
    /// JMAP role in lower case: "inbox", "archive", "trash", "drafts", "sent", "junk".
    pub role: Option<String>,
    pub parent_id: Option<String>,
    pub sort_order: u32,
    pub total_emails: u32,
    pub unread_emails: u32,
}

/// What a message list row and a notification need. No body.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct MessageHeader {
    pub id: String,
    pub thread_id: String,
    pub mailbox_ids: Vec<String>,
    /// Display name of the first sender; empty when the server gave none.
    pub from_name: String,
    pub from_email: String,
    pub subject: String,
    pub preview: String,
    pub received_at: i64,
    /// No `$seen` keyword.
    pub is_unread: bool,
    /// `$flagged` keyword.
    pub is_flagged: bool,
    pub has_attachment: bool,
    pub size: u64,
}

/// Something the user did that the server has to hear about. Queued when offline.
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum Action {
    MarkRead {
        message_id: String,
    },
    MarkUnread {
        message_id: String,
    },
    Archive {
        message_id: String,
    },
    /// Move to Trash.
    Delete {
        message_id: String,
    },
    /// Remove for good. `Delete` on a message that is already in Trash becomes this.
    Destroy {
        message_id: String,
    },
}

/// The state of the live connection, for the "Connected" entry and the Delivery check screen.
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum ConnectionStatus {
    SignedOut,
    Connecting,
    /// `last_event_at`: when the server last sent anything on the push stream.
    Live {
        last_event_at: i64,
    },
    Offline {
        reason: String,
    },
    SignInNeeded,
}
