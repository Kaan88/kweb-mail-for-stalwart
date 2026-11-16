//! The attention settings of one account and the findings the user swiped away. Both live in the
//! `kv` table of the cache (no schema change).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{store::Store, Result};

const SETTINGS_KEY: &str = "attention.settings";
const DISMISSED_KEY: &str = "attention.dismissed";
/// A swipe is remembered this long; a finding that old that still holds comes back.
const DISMISS_KEEP_SECS: i64 = 60 * 24 * 3600;

/// Which mail the mail rules read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, uniffi::Enum)]
pub enum MailScope {
    /// The Inbox only.
    #[default]
    Inbox,
    /// The Inbox and every folder the user switched notifications on for.
    InboxAndNotified,
}

/// One switch per Home tile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
pub struct TileSwitches {
    pub unread: bool,
    pub next_event: bool,
    pub due_today: bool,
    pub flagged: bool,
    pub recent_files: bool,
    /// Only while there is a problem.
    pub delivery: bool,
    /// Only while an away message is on.
    pub away: bool,
}

impl Default for TileSwitches {
    fn default() -> Self {
        TileSwitches {
            unread: true,
            next_event: true,
            due_today: true,
            flagged: true,
            recent_files: true,
            delivery: true,
            away: true,
        }
    }
}

/// Home and attention, per account. Attention works with Home off.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
pub struct AttentionSettings {
    /// The Home tab (the first tab). Off at first.
    #[serde(default)]
    pub home: bool,
    #[serde(default)]
    pub tiles: TileSwitches,
    #[serde(default = "yes")]
    pub calendar: bool,
    #[serde(default = "yes")]
    pub tasks: bool,
    #[serde(default = "yes")]
    pub mail: bool,
    #[serde(default)]
    pub mail_scope: MailScope,
    /// Show the dialog that names what will be sent before a run that sends mail to the service.
    #[serde(default = "yes")]
    pub ask_each_run: bool,
    /// Under a result: what was sent and the tokens the service reported.
    #[serde(default = "yes")]
    pub show_cost: bool,
}

fn yes() -> bool {
    true
}

impl Default for AttentionSettings {
    fn default() -> Self {
        AttentionSettings {
            home: false,
            tiles: TileSwitches::default(),
            calendar: true,
            tasks: true,
            mail: true,
            mail_scope: MailScope::Inbox,
            ask_each_run: true,
            show_cost: true,
        }
    }
}

pub(crate) fn load_settings(store: &Store, account_id: &str) -> Result<AttentionSettings> {
    Ok(store
        .kv_get(account_id, SETTINGS_KEY)?
        // An unreadable value (another version) is the defaults.
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default())
}

pub(crate) fn save_settings(store: &Store, account_id: &str, s: &AttentionSettings) -> Result<()> {
    store.kv_set(account_id, SETTINGS_KEY, &serde_json::to_string(s).map_err(|e| crate::Error::store(e.to_string()))?)
}

/// The ids of swiped findings and when each was swiped.
#[derive(Debug, Default, Clone)]
pub(crate) struct Dismissed(BTreeMap<String, i64>);

impl Dismissed {
    pub fn contains(&self, id: &str) -> bool {
        self.0.contains_key(id)
    }

    pub fn add(&mut self, id: &str, now: i64) {
        self.0.insert(id.to_string(), now);
        self.0.retain(|_, at| now - *at < DISMISS_KEEP_SECS);
    }
}

pub(crate) fn load_dismissed(store: &Store, account_id: &str) -> Result<Dismissed> {
    Ok(Dismissed(
        store.kv_get(account_id, DISMISSED_KEY)?.and_then(|json| serde_json::from_str(&json).ok()).unwrap_or_default(),
    ))
}

pub(crate) fn save_dismissed(store: &Store, account_id: &str, d: &Dismissed) -> Result<()> {
    store.kv_set(
        account_id,
        DISMISSED_KEY,
        &serde_json::to_string(&d.0).map_err(|e| crate::Error::store(e.to_string()))?,
    )
}
