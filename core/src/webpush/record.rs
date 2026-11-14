//! The state record of an account's subscription (kv `web_push`) and the rules that read it.

use serde::{Deserialize, Serialize};

use crate::{
    ffi::{WebPushState, LIFETIME_SECS, LOOK_EVERY_SECS, RENEW_BEFORE_SECS, STATE_KEY},
    store::Store,
    Error, Result,
};

/// The kv key of the time of the last decrypted message. A key of its own: messages arrive
/// while a subscribe or a renew is writing the record, and neither may overwrite the other.
pub const SEEN_KEY: &str = "web_push_seen";

/// What was found out about a subscription that stopped working.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Lost {
    /// The server's key is not the one the registration was made for.
    KeyChanged,
    /// The server does not list the subscription any more.
    Gone,
}

/// A verification code the server sent that could not be written back yet.
#[derive(Clone, Serialize, Deserialize)]
pub struct Pending {
    pub subscription_id: String,
    pub code: String,
}

/// A subscription that could not be destroyed yet: the server reuses ids, so the id alone does
/// not say whose it is by the time it is destroyed.
#[derive(Clone, Serialize, Deserialize)]
pub struct Leftover {
    pub id: String,
    /// The `deviceClientId` it was made with.
    pub device: String,
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Record {
    /// The random id inside the registration subtype and the `deviceClientId`. Empty: none yet.
    pub reg_id: String,
    /// The server's `applicationServerKey` the registration was made for (unpadded base64url).
    pub server_key: Option<String>,
    pub token: Option<String>,
    pub subscription_id: Option<String>,
    pub created_at: Option<i64>,
    pub verified_at: Option<i64>,
    pub expires: Option<i64>,
    /// When a subscription was last created on the server (survives a drop: the server sends
    /// one verification per account per minute).
    pub last_create_at: Option<i64>,
    /// When the server was last asked about the subscription.
    pub last_look: Option<i64>,
    pub pending_code: Option<Pending>,
    /// Subscriptions that could not be destroyed yet.
    pub leftovers: Vec<Leftover>,
    pub lost: Option<Lost>,
}

/// Token, code and ids stay out of any text made from a `Debug`.
impl std::fmt::Debug for Record {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Record { .. }")
    }
}

impl Record {
    pub fn device_client_id(&self) -> String {
        format!("kweb-{}", self.reg_id)
    }

    pub fn is_verified(&self) -> bool {
        self.subscription_id.is_some() && self.verified_at.is_some()
    }

    /// Forgets the subscription (not the registration, nor the leftovers).
    pub fn clear_subscription(&mut self) {
        self.token = None;
        self.subscription_id = None;
        self.created_at = None;
        self.verified_at = None;
        self.expires = None;
        self.last_look = None;
        self.pending_code = None;
        self.lost = None;
    }

    /// Where the subscription stands.
    pub fn state(&self, now: i64, last_message_at: Option<i64>) -> WebPushState {
        match self.lost {
            Some(Lost::KeyChanged) => return WebPushState::KeyChanged,
            Some(Lost::Gone) => return WebPushState::Gone,
            None => {}
        }
        if self.subscription_id.is_none() {
            return WebPushState::Off;
        }
        match (self.verified_at, self.expires) {
            (None, _) => WebPushState::Verifying { since: self.created_at.unwrap_or(now) },
            (Some(_), Some(expires)) if expires > now => WebPushState::Registered { expires, last_message_at },
            (Some(_), _) => WebPushState::Gone,
        }
    }

    /// Whether [`crate::webpush::renew`] has anything to ask the server.
    pub fn wants_server(&self, now: i64) -> bool {
        if self.pending_code.is_some() || !self.leftovers.is_empty() {
            return true;
        }
        if self.subscription_id.is_none() || self.lost.is_some() || !self.is_verified() {
            return false;
        }
        let left = self.expires.map_or(0, |e| e - now);
        left < RENEW_BEFORE_SECS || self.last_look.is_none_or(|l| now - l >= LOOK_EVERY_SECS)
    }

    /// The time a subscription made now runs until at the longest.
    pub fn new_expiry(now: i64) -> i64 {
        now + LIFETIME_SECS
    }
}

fn json_error(_: serde_json::Error) -> Error {
    Error::store("the push record could not be written")
}

/// The record; an account without one has the empty record. An unreadable record is the empty one.
pub fn load(store: &Store, account_id: &str) -> Result<Record> {
    let text = store.kv_get(account_id, STATE_KEY)?;
    Ok(text.and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default())
}

/// Keeps the record; the empty record (nothing but the defaults) deletes the entry.
pub fn save(store: &Store, account_id: &str, record: &Record) -> Result<()> {
    let empty = record.reg_id.is_empty() && record.leftovers.is_empty() && record.subscription_id.is_none();
    if empty {
        return store.kv_delete(account_id, STATE_KEY);
    }
    let text = serde_json::to_string(record).map_err(json_error)?;
    store.kv_set(account_id, STATE_KEY, &text)
}

/// When a message was last decrypted.
pub fn last_message(store: &Store, account_id: &str) -> Option<i64> {
    store.kv_get(account_id, SEEN_KEY).ok().flatten().and_then(|t| t.parse().ok())
}

pub fn touch_message(store: &Store, account_id: &str, now: i64) -> Result<()> {
    store.kv_set(account_id, SEEN_KEY, &now.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registered(now: i64) -> Record {
        Record {
            reg_id: "r".into(),
            subscription_id: Some("s".into()),
            created_at: Some(now - 100),
            verified_at: Some(now - 99),
            expires: Some(now + LIFETIME_SECS),
            last_look: Some(now - 10),
            ..Record::default()
        }
    }

    #[test]
    fn state_follows_the_record() {
        let now = 1_000_000;
        assert_eq!(Record::default().state(now, None), WebPushState::Off);
        let mut r = registered(now);
        assert_eq!(
            r.state(now, Some(5)),
            WebPushState::Registered { expires: now + LIFETIME_SECS, last_message_at: Some(5) }
        );
        r.expires = Some(now);
        assert_eq!(r.state(now, None), WebPushState::Gone);
        r.verified_at = None;
        assert_eq!(r.state(now, None), WebPushState::Verifying { since: now - 100 });
        r.lost = Some(Lost::KeyChanged);
        assert_eq!(r.state(now, None), WebPushState::KeyChanged);
        r.lost = Some(Lost::Gone);
        assert_eq!(r.state(now, None), WebPushState::Gone);
    }

    #[test]
    fn the_server_is_asked_only_when_there_is_a_reason() {
        let now = 1_000_000;
        let mut r = registered(now);
        assert!(!r.wants_server(now));
        r.last_look = Some(now - LOOK_EVERY_SECS);
        assert!(r.wants_server(now));
        // Every 15-minute check looks; a one-off check a moment after one does not.
        r.last_look = Some(now - 15 * 60);
        assert!(r.wants_server(now));
        r.last_look = Some(now - 60);
        assert!(!r.wants_server(now));
        r.last_look = Some(now);
        r.expires = Some(now + RENEW_BEFORE_SECS - 1);
        assert!(r.wants_server(now));
        r.expires = Some(now + LIFETIME_SECS);
        r.leftovers.push(Leftover { id: "x".into(), device: "kweb-y".into() });
        assert!(r.wants_server(now));
        r.leftovers.clear();
        r.pending_code = Some(Pending { subscription_id: "s".into(), code: "c".into() });
        assert!(r.wants_server(now));
        r.pending_code = None;
        r.verified_at = None;
        assert!(!r.wants_server(now), "an unverified subscription is the host's to retry");
        assert!(!Record::default().wants_server(now));
    }

    #[test]
    fn debug_shows_nothing() {
        let mut r = registered(5);
        r.token = Some("secret-token".into());
        assert!(!format!("{r:?}").contains("secret"));
    }
}
