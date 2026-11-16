//! How much mail an account keeps on the phone: the size of the window a full fetch fills and
//! that [`prune`] cuts back to. The window is counted in messages (the newest ones of each
//! folder), not in time: the cache and "load older" are built on counts, and a time window would
//! have to guess how many messages that is.

use std::collections::{HashMap, HashSet};

use crate::{
    store::{Store, WindowRow},
    Result,
};

/// The key (in the account's key/value table) of the choice.
const KEY: &str = "sync_window";

/// The most a folder is asked for when everything is kept: a bound, so that a huge archive does
/// not fill the phone or one request.
pub const EVERYTHING_CAP: usize = 50_000;

/// How far back each folder is kept on the phone. The Inbox keeps more than the other folders.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, uniffi::Enum)]
pub enum SyncWindow {
    /// The newest 200 of the Inbox and 50 of every other folder.
    Light,
    /// The newest 500 of the Inbox and 100 of every other folder (the default).
    #[default]
    Standard,
    /// The newest 2,000 of the Inbox and 500 of every other folder.
    Large,
    /// Everything the server has, up to 50,000 per folder.
    Everything,
}

impl SyncWindow {
    /// How many of the newest messages the Inbox keeps.
    pub fn inbox(self) -> usize {
        match self {
            SyncWindow::Light => 200,
            SyncWindow::Standard => crate::sync::INBOX_WINDOW,
            SyncWindow::Large => 2_000,
            SyncWindow::Everything => EVERYTHING_CAP,
        }
    }

    /// How many of the newest messages every other folder keeps.
    pub fn other(self) -> usize {
        match self {
            SyncWindow::Light => 50,
            SyncWindow::Standard => crate::sync::OTHER_WINDOW,
            SyncWindow::Large => 500,
            SyncWindow::Everything => EVERYTHING_CAP,
        }
    }

    /// The count for a folder with this role.
    pub fn for_role(self, role: Option<&str>) -> usize {
        if role == Some("inbox") {
            self.inbox()
        } else {
            self.other()
        }
    }

    fn code(self) -> &'static str {
        match self {
            SyncWindow::Light => "light",
            SyncWindow::Standard => "standard",
            SyncWindow::Large => "large",
            SyncWindow::Everything => "everything",
        }
    }

    fn from_code(code: &str) -> Option<Self> {
        [SyncWindow::Light, SyncWindow::Standard, SyncWindow::Large, SyncWindow::Everything]
            .into_iter()
            .find(|w| w.code() == code)
    }
}

/// The account's choice (the default when none was made or the stored value is not known).
pub fn get(store: &Store, account_id: &str) -> Result<SyncWindow> {
    Ok(store.kv_get(account_id, KEY)?.as_deref().and_then(SyncWindow::from_code).unwrap_or_default())
}

/// Stores the choice. A bigger window makes the next sync fetch the older mail (the Email state
/// is dropped, so that pass is a full one); a smaller one is cut back by [`prune`], which the
/// caller runs. Returns whether the window grew.
pub fn set(store: &Store, account_id: &str, window: SyncWindow) -> Result<bool> {
    let before = get(store, account_id)?;
    store.kv_set(account_id, KEY, window.code())?;
    let grew = window.inbox() > before.inbox() || window.other() > before.other();
    if grew {
        store.clear_state(account_id, "Email")?;
    }
    Ok(grew)
}

/// Cuts every folder back to the newest messages of `window`. A message goes only when it is
/// outside the window of every folder it is in, and is read, not starred, not in Drafts and not
/// named by `keep` (what a queued action or a draft refers to). Its body goes with it. The
/// folder's window then ends at the oldest message it was cut to. Returns how many messages went.
pub fn prune(store: &Store, account_id: &str, window: SyncWindow, keep: &HashSet<String>) -> Result<usize> {
    let roles: HashMap<String, Option<String>> =
        store.list_mailboxes(account_id)?.into_iter().map(|m| (m.id, m.role)).collect();
    let role_of = |mailbox: &str| roles.get(mailbox).and_then(|r| r.as_deref());
    let rows = store.window_rows(account_id)?;
    // Folder by folder, newest first: whether a row is within the folder's count, and where the count ends.
    let mut inside: HashSet<&str> = HashSet::new();
    let mut outside: Vec<&WindowRow> = Vec::new();
    let mut ends: HashMap<&str, i64> = HashMap::new();
    let mut rank: HashMap<&str, usize> = HashMap::new();
    for row in &rows {
        let seen = rank.entry(row.mailbox_id.as_str()).or_insert(0);
        *seen += 1;
        if *seen <= window.for_role(role_of(&row.mailbox_id)) {
            inside.insert(row.id.as_str());
            ends.insert(row.mailbox_id.as_str(), row.received_at);
        } else {
            outside.push(row);
        }
    }
    let mut protected: HashSet<&str> = HashSet::new();
    for row in &rows {
        if row.is_unread || row.is_flagged || role_of(&row.mailbox_id) == Some("drafts") || keep.contains(&row.id) {
            protected.insert(row.id.as_str());
        }
    }
    let doomed: HashSet<&str> =
        outside.iter().map(|r| r.id.as_str()).filter(|id| !inside.contains(id) && !protected.contains(id)).collect();
    let doomed: Vec<String> = doomed.into_iter().map(String::from).collect();
    store.delete_messages(account_id, &doomed)?;
    // A folder that had more than its count now ends where its count ends (never further back
    // than it ended before).
    for (mailbox, end) in ends {
        if rank.get(mailbox).copied().unwrap_or(0) > window.for_role(role_of(mailbox)) {
            let before = store.get_window(account_id, mailbox)?;
            store.set_window(account_id, mailbox, before.map_or(end, |w| w.max(end)))?;
        }
    }
    store.prune_dismissed(account_id)?;
    Ok(doomed.len())
}
