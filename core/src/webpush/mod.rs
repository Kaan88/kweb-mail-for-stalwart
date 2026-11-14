//! Push through Google: the parts that do not need the `Core` object. `ffi/webpush.rs` is the
//! interface and its doc comments are the specification; this module is what it calls.
//!
//! - [`ece`]: RFC 8291 decryption.
//! - [`keys`]: the key pair and the auth secret, in the secret store.
//! - [`record`]: the state record in the account's key/value table.
//! - [`message`]: what a decrypted message is.
//!
//! The functions here take what they need (the cache, the secret store, a connection that was
//! made with a session read just now) and never hold a lock across the network: the caller
//! holds [`Store::webpush_lock`] for any call that changes the record.

pub mod ece;
pub mod keys;
pub mod message;
pub mod record;

use crate::{
    ffi::{WebPushMessage, WebPushRegistration, WebPushState, EMAIL_PUSH_PROPERTIES, EMAIL_PUSH_URGENCY, TYPES},
    ffi::{RENEW_BEFORE_SECS, VERIFY_GAP_SECS},
    jmap::{Connection, NewSubscription, Verification},
    model::Account,
    secrets::SecretStore,
    store::Store,
    Error, Result,
};
use record::{Leftover, Lost, Pending, Record};

/// Where Google's push service takes a Web Push message for a token.
const GOOGLE_PUSH: &str = "https://fcm.googleapis.com/fcm/send/";
/// The longest token that is taken.
const MAX_TOKEN: usize = 4096;

/// What the calls need besides a connection.
pub struct Ctx<'a> {
    pub store: &'a Store,
    pub secrets: &'a dyn SecretStore,
    pub account: &'a Account,
}

impl Ctx<'_> {
    fn load(&self) -> Result<Record> {
        record::load(self.store, &self.account.id)
    }

    fn save(&self, record: &Record) -> Result<()> {
        record::save(self.store, &self.account.id, record)
    }

    /// Where the subscription stands, from the record alone.
    pub fn state(&self, now: i64) -> WebPushState {
        match self.load() {
            Ok(record) => record.state(now, record::last_message(self.store, &self.account.id)),
            Err(_) => WebPushState::Off,
        }
    }
}

fn host_of(server_url: &str) -> String {
    url::Url::parse(server_url).ok().and_then(|u| u.host_str().map(str::to_string)).unwrap_or_default()
}

/// `wp:https://<host>/#<id>`.
fn subtype(account: &Account, reg_id: &str) -> String {
    format!("wp:https://{}/#{reg_id}", host_of(&account.server_url))
}

/// The listed subscriptions that carry this device's id.
fn listed_of_device(listed: &[crate::jmap::SubscriptionInfo], device: &str) -> Vec<String> {
    listed.iter().filter(|s| s.device_client_id.as_deref() == Some(device)).map(|s| s.id.clone()).collect()
}

/// The left-overs that the server still lists with the `deviceClientId` they were made with.
/// The server reuses ids: a left-over whose id is free, or belongs to another device now, is
/// not ours to destroy and is simply gone.
fn listed_leftovers(leftovers: &[Leftover], listed: &[crate::jmap::SubscriptionInfo]) -> Vec<String> {
    leftovers
        .iter()
        .filter(|l| listed.iter().any(|s| s.id == l.id && s.device_client_id.as_deref() == Some(l.device.as_str())))
        .map(|l| l.id.clone())
        .collect()
}

/// After a destroy: only the left-overs that were attempted and not destroyed stay.
fn keep_undestroyed(record: &mut Record, attempted: &[String], destroyed: &[String]) {
    record.leftovers.retain(|l| attempted.contains(&l.id) && !destroyed.contains(&l.id));
}

fn check_token(token: &str) -> Result<()> {
    let usable = !token.is_empty()
        && token.len() <= MAX_TOKEN
        && token.chars().all(|c| c.is_ascii_alphanumeric() || "-_:.~".contains(c));
    if usable {
        Ok(())
    } else {
        Err(Error::invalid("the token from Google's push service is not usable"))
    }
}

/// First step of switching on: keys, registration id, the server's key.
pub async fn prepare(ctx: &Ctx<'_>, conn: &Connection) -> Result<Option<WebPushRegistration>> {
    let Some(server_key) = conn.web_push_key().await? else { return Ok(None) };
    if keys::load(ctx.secrets)?.is_none() {
        keys::create(ctx.secrets)?;
    }
    let mut record = ctx.load()?;
    if record.reg_id.is_empty() {
        record.reg_id = keys::uuid_v4()?;
    }
    if record.server_key.as_deref() != Some(server_key.as_str()) {
        record.server_key = Some(server_key.clone());
        record.token = None;
    }
    ctx.save(&record)?;
    Ok(Some(WebPushRegistration { sender: server_key, subtype: subtype(ctx.account, &record.reg_id) }))
}

/// Second step: the subscription on the server points at Google's address for `token`.
pub async fn subscribe(ctx: &Ctx<'_>, conn: &Connection, token: &str, now: i64) -> Result<WebPushState> {
    check_token(token)?;
    let not_prepared = || Error::invalid("push through Google is not prepared: there are no keys");
    let keys = keys::load(ctx.secrets)?.ok_or_else(not_prepared)?;
    let mut record = ctx.load()?;
    if record.reg_id.is_empty() {
        return Err(not_prepared());
    }
    let current = conn.web_push_key().await?;
    if current.is_none() || current != record.server_key {
        return Err(Error::invalid("the server's key changed: register again"));
    }
    let listed = conn.push_subscriptions(None).await?;
    let device = record.device_client_id();
    // Only a subscription with this install's `deviceClientId` is ours: ids are reused.
    let stored_is_listed = record
        .subscription_id
        .as_ref()
        .is_some_and(|id| listed.iter().any(|s| &s.id == id && s.device_client_id.as_deref() == Some(device.as_str())));

    // The same token, already verified and still there: only extend it.
    if record.token.as_deref() == Some(token) && record.is_verified() && stored_is_listed {
        let id = record.subscription_id.clone().unwrap_or_default();
        let expires = Record::new_expiry(now);
        if conn.push_subscription_extend(&id, expires).await? {
            record.expires = Some(expires);
            record.last_look = Some(now);
            record.lost = None;
            ctx.save(&record)?;
            return Ok(ctx.state(now));
        }
    }

    // Otherwise it is replaced; the server verifies one new subscription per minute.
    if let Some(at) = record.last_create_at {
        let wait = at + VERIFY_GAP_SECS - now;
        if wait > 0 {
            return Err(Error::temporary(format!(
                "push through Google was set up a moment ago: try again in {wait} seconds"
            )));
        }
    }
    let mut doomed = listed_of_device(&listed, &device);
    doomed.extend(listed_leftovers(&record.leftovers, &listed));
    doomed.sort();
    doomed.dedup();
    let destroyed = conn.push_subscriptions_destroy(&doomed).await?;
    keep_undestroyed(&mut record, &doomed, &destroyed);
    record.clear_subscription();
    ctx.save(&record)?;

    let url = format!("{GOOGLE_PUSH}{token}");
    let (p256dh, auth) = (keys.p256dh(), keys.auth_text());
    let wish = Record::new_expiry(now);
    let new = NewSubscription {
        device_client_id: &device,
        url: &url,
        p256dh: &p256dh,
        auth: &auth,
        types: &TYPES,
        expires: wish,
        account_id: &ctx.account.id,
        properties: &EMAIL_PUSH_PROPERTIES,
        urgency: EMAIL_PUSH_URGENCY,
    };
    let (id, expires) = conn.push_subscription_create(&new).await?;
    record.token = Some(token.to_string());
    record.subscription_id = Some(id);
    record.created_at = Some(now);
    record.last_create_at = Some(now);
    record.expires = Some(expires.unwrap_or(wish).min(wish));
    record.last_look = Some(now);
    ctx.save(&record)?;
    Ok(WebPushState::Verifying { since: now })
}

/// Decrypts a message with this account's keys. `None`: not for these keys (or no keys).
pub fn decrypt(ctx: &Ctx<'_>, body: &[u8]) -> Result<Option<message::Incoming>> {
    let Some(keys) = keys::load(ctx.secrets)? else { return Ok(None) };
    Ok(ece::decrypt(body, keys.secret(), keys.auth()).map(|plain| message::parse(&plain, &ctx.account.id)))
}

/// Whether a verification from the server is the one that is waited for.
pub fn waiting_for(ctx: &Ctx<'_>, subscription_id: &str) -> Result<bool> {
    let record = ctx.load()?;
    Ok(record.subscription_id.as_deref() == Some(subscription_id) && record.verified_at.is_none())
}

/// What the server answered to a code that was written back, or why it could not be written.
pub fn verified(ctx: &Ctx<'_>, id: &str, code: &str, answer: Result<Verification>, now: i64) -> Result<WebPushMessage> {
    let mut record = ctx.load()?;
    let matches = record.subscription_id.as_deref() == Some(id) && record.verified_at.is_none();
    if !matches {
        return Ok(WebPushMessage::Ignored);
    }
    match answer {
        Ok(Verification::Accepted) => {
            record.verified_at = Some(now);
            record.pending_code = None;
            record.lost = None;
            ctx.save(&record)?;
            Ok(WebPushMessage::Verified)
        }
        Ok(Verification::Refused) => {
            record.pending_code = None;
            ctx.save(&record)?;
            Err(Error::invalid("the server did not accept the verification code"))
        }
        Ok(Verification::Gone) => {
            record.pending_code = None;
            record.lost = Some(Lost::Gone);
            ctx.save(&record)?;
            Err(Error::invalid("the server no longer has the push subscription"))
        }
        Err(e) => {
            // Kept, so the next housekeeping writes it: the server sends it only once.
            record.pending_code = Some(Pending { subscription_id: id.to_string(), code: code.to_string() });
            ctx.save(&record)?;
            Err(e)
        }
    }
}

/// Housekeeping that needs no server. `Some` is the answer; `None` means the server has to be asked.
pub fn renew_locally(ctx: &Ctx<'_>, now: i64) -> Result<Option<WebPushState>> {
    let record = ctx.load()?;
    Ok((!record.wants_server(now)).then(|| ctx.state(now)))
}

/// Housekeeping with the server (see `Core::web_push_renew`).
pub async fn renew(ctx: &Ctx<'_>, conn: &Connection, now: i64) -> Result<WebPushState> {
    let mut record = ctx.load()?;
    // 1. What an earlier drop left behind.
    if !record.leftovers.is_empty() {
        let listed = conn.push_subscriptions(None).await?;
        let doomed = listed_leftovers(&record.leftovers, &listed);
        let destroyed = conn.push_subscriptions_destroy(&doomed).await?;
        keep_undestroyed(&mut record, &doomed, &destroyed);
        ctx.save(&record)?;
    }
    // 2. A code that could not be written back.
    if let Some(pending) = record.pending_code.clone() {
        let answer = conn.push_subscription_verify(&pending.subscription_id, &pending.code).await;
        match verified(ctx, &pending.subscription_id, &pending.code, answer, now) {
            Ok(_) | Err(Error::Invalid { .. }) => {}
            Err(e) => return Err(e),
        }
        record = ctx.load()?;
    }
    let Some(id) = record.subscription_id.clone() else { return Ok(ctx.state(now)) };
    if record.lost.is_some() {
        return Ok(ctx.state(now));
    }
    // 3. Is it still the server's key?
    let current = conn.web_push_key().await?;
    if current.is_none() || current != record.server_key {
        record.lost = Some(Lost::KeyChanged);
        ctx.save(&record)?;
        return Ok(WebPushState::KeyChanged);
    }
    // 4. Does the server still have it?
    let listed = conn.push_subscriptions(Some(std::slice::from_ref(&id))).await?;
    let device = record.device_client_id();
    if !listed.iter().any(|s| s.id == id && s.device_client_id.as_deref() == Some(device.as_str())) {
        record.lost = Some(Lost::Gone);
        ctx.save(&record)?;
        return Ok(WebPushState::Gone);
    }
    // 5. Extend it when its time is running out.
    if record.is_verified() && record.expires.is_none_or(|e| e - now < RENEW_BEFORE_SECS) {
        let expires = Record::new_expiry(now);
        if !conn.push_subscription_extend(&id, expires).await? {
            record.lost = Some(Lost::Gone);
            ctx.save(&record)?;
            return Ok(WebPushState::Gone);
        }
        record.expires = Some(expires);
    }
    record.last_look = Some(now);
    ctx.save(&record)?;
    Ok(ctx.state(now))
}

/// The local half of switching off: the subscription is forgotten here (its id goes on the
/// left-over list). `false`: there was nothing, and the server need not be asked.
pub fn drop_locally(ctx: &Ctx<'_>) -> Result<bool> {
    let mut record = ctx.load()?;
    if record.reg_id.is_empty() && record.leftovers.is_empty() && record.subscription_id.is_none() {
        return Ok(false);
    }
    if let Some(id) = record.subscription_id.clone() {
        if !record.leftovers.iter().any(|l| l.id == id) {
            record.leftovers.push(Leftover { id, device: record.device_client_id() });
        }
    }
    record.clear_subscription();
    ctx.save(&record)?;
    Ok(true)
}

/// The server half of switching off: destroys the left-over subscriptions and every other
/// one with this device's `deviceClientId`.
pub async fn drop_on_server(ctx: &Ctx<'_>, conn: &Connection) -> Result<()> {
    let mut record = ctx.load()?;
    let mut doomed = Vec::new();
    if !record.reg_id.is_empty() || !record.leftovers.is_empty() {
        let listed = conn.push_subscriptions(None).await?;
        if !record.reg_id.is_empty() {
            doomed.extend(listed_of_device(&listed, &record.device_client_id()));
        }
        doomed.extend(listed_leftovers(&record.leftovers, &listed));
    }
    doomed.sort();
    doomed.dedup();
    let destroyed = conn.push_subscriptions_destroy(&doomed).await?;
    keep_undestroyed(&mut record, &doomed, &destroyed);
    ctx.save(&record)?;
    if record.leftovers.is_empty() {
        Ok(())
    } else {
        Err(Error::temporary("the server did not destroy every push subscription"))
    }
}

/// The last step: the keys and the record go, but for the left-over list.
pub fn forget(store: Option<(&Store, &str)>, secrets: &dyn SecretStore) -> Result<()> {
    let keys = keys::forget(secrets);
    if let Some((store, account_id)) = store {
        let old = record::load(store, account_id)?;
        let fresh = Record { leftovers: old.leftovers, ..Record::default() };
        record::save(store, account_id, &fresh)?;
        store.kv_delete(account_id, record::SEEN_KEY)?;
    }
    keys
}
