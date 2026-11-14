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

/// One mail address as a header carries it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, uniffi::Record)]
pub struct Address {
    /// Display name; empty when the header had none.
    pub name: String,
    pub email: String,
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
    /// The To header: what a row in Sent or Drafts shows in place of the sender.
    pub to: Vec<Address>,
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
    /// Set the `$flagged` keyword (the star).
    Flag {
        message_id: String,
    },
    Unflag {
        message_id: String,
    },
    /// Move to another mailbox, out of whatever it is in. Also how Archive and Delete are undone
    /// and how mail is marked as spam (the Junk mailbox).
    Move {
        message_id: String,
        mailbox_id: String,
    },
}

/// Which conversations of a mailbox a list shows (the chips above the list).
#[derive(Debug, Clone, Copy, PartialEq, uniffi::Enum)]
pub enum ListFilter {
    All,
    /// Conversations with unread mail in this mailbox.
    Unread,
    Flagged,
    /// Conversations with an attachment.
    Attachments,
}

/// One conversation as a row of a mailbox list. Everything in it is about the messages of the
/// thread that are in that mailbox and in the cache (mail of the thread in other mailboxes,
/// such as the replies in Sent, is not counted here).
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ThreadRow {
    /// The newest message: its subject, preview and time are the row's.
    pub newest: MessageHeader,
    /// Ids of the messages, oldest first: what a swipe on the row acts on.
    pub message_ids: Vec<String>,
    pub unread_count: u32,
    /// Any of them is flagged.
    pub is_flagged: bool,
    /// Any of them has an attachment.
    pub has_attachment: bool,
    /// The distinct senders, oldest first: display name, or the address when there is none.
    pub senders: Vec<String>,
}

/// A file carried by a message.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, uniffi::Record)]
pub struct Attachment {
    /// What [`crate::ffi::Core::download_blob`] takes.
    pub blob_id: String,
    /// File name; empty when the part had none.
    pub name: String,
    pub mime_type: String,
    pub size: u64,
    /// Content-ID without the angle brackets: the HTML refers to it as `cid:<content_id>`.
    pub content_id: Option<String>,
    /// The HTML body shows it itself (it refers to its Content-ID), so it is not listed as a file.
    pub is_inline: bool,
}

/// The parts of a message a list does not need: recipients, text and files. Kept in the cache
/// once fetched, so a message that was opened (or prefetched) can be read offline.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, uniffi::Record)]
pub struct MessageBody {
    pub message_id: String,
    /// The blob of the whole message as it was received (RFC 5322).
    pub blob_id: String,
    pub to: Vec<Address>,
    pub cc: Vec<Address>,
    pub bcc: Vec<Address>,
    pub reply_to: Vec<Address>,
    /// The Date header. `None` when the message has none.
    pub sent_at: Option<i64>,
    /// The HTML version exactly as sent. It is NOT cleaned: the host must show it with scripts
    /// off and must not load remote content unless the user allows it. `None` when the message
    /// has only plain text.
    pub html: Option<String>,
    /// The plain-text version. `None` when the message has only HTML.
    pub text: Option<String>,
    pub attachments: Vec<Attachment>,
    /// The text was longer than the core fetches; the end is missing.
    pub is_truncated: bool,
}

/// The answer to a search.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct SearchResult {
    /// Newest first.
    pub messages: Vec<MessageHeader>,
    /// The server could not be reached: these are only the matches among cached headers
    /// (sender, subject, preview), not a search of the whole mailbox.
    pub from_cache: bool,
}

/// An address the server lets the user send as (JMAP Identity).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, uniffi::Record)]
pub struct Identity {
    pub id: String,
    /// The display name that goes with the address; may be empty.
    pub name: String,
    pub email: String,
    /// Plain-text signature; empty when there is none.
    pub signature: String,
}

/// What a new draft starts from.
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum DraftKind {
    /// An empty message.
    New,
    /// An answer to the sender of a message.
    Reply { message_id: String },
    /// An answer to the sender and everyone else on a message, without the user's own address.
    ReplyAll { message_id: String },
    /// A message passed on, with its attachments.
    Forward { message_id: String },
    /// A draft that is in the Drafts mailbox on the server (written here or elsewhere).
    Existing { message_id: String },
}

/// A message being written. It lives in the cache from the first keystroke, so nothing typed
/// is lost offline or when the app is killed; the core keeps a copy in Drafts on the server.
/// The text is plain text: this version writes no HTML.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, uniffi::Record)]
pub struct Draft {
    /// Made by the core; stays the same for the life of the draft.
    pub id: String,
    /// Which [`Identity`] it is sent as.
    pub identity_id: String,
    pub to: Vec<Address>,
    pub cc: Vec<Address>,
    pub bcc: Vec<Address>,
    pub subject: String,
    /// The whole text, the quoted original of a reply included.
    pub body: String,
    pub attachments: Vec<Attachment>,
    /// The message this answers or passes on. Sending marks it answered or forwarded.
    pub source_message_id: Option<String>,
    /// It passes the source on (Forward); otherwise it answers it.
    pub is_forward: bool,
    /// The id of its copy in Drafts on the server; `None` until it has been saved there.
    pub server_id: Option<String>,
    /// Why the last try to send it failed; `None` when it was never sent or the try worked.
    pub send_error: Option<String>,
    pub updated_at: i64,
}

/// A message the user sent that the core still holds back, so the send can be undone.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct PendingSend {
    pub draft_id: String,
    pub subject: String,
    /// When it goes to the server.
    pub send_at: i64,
}

/// How much of one limit of the account is used (JMAP Quota).
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Quota {
    pub name: String,
    /// What is counted: "octets" (bytes) or "count" (objects).
    pub resource: String,
    pub used: u64,
    /// The limit; `None` when the server sets none.
    pub limit: Option<u64>,
}

/// A calendar or task reminder the server fired. Times stay as the server gives them: the
/// host has the time zone database and the user's locale, so it does the formatting.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Reminder {
    pub event_id: String,
    pub alert_id: String,
    /// A task (due time) and not an event (start time).
    pub is_task: bool,
    /// Empty when the details could not be fetched; the host then shows a generic text.
    pub title: String,
    pub location: String,
    /// When this occurrence starts (or the task is due), as a JSCalendar local date-time
    /// like `2026-11-12T10:00:00`. `None` when unknown.
    pub local_start: Option<String>,
    /// IANA zone `local_start` is in. `None` is a floating time: the same wall-clock time
    /// wherever the phone is.
    pub time_zone: Option<String>,
    pub all_day: bool,
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
