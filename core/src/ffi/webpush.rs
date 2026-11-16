//! Push through Google instead of the permanent connection: the contract between the core and
//! the Android side. `dev/notes/google-push-contract.md` has the sequences and the Android names;
//! `dev/notes/stalwart-fcm-experiment.md` has the shapes and timings seen on the emulator.
//!
//! The doc comment of each function is its specification. The work is in `core/src/webpush/`
//! (keys, RFC 8291, the state record) and `core/src/jmap/pushsub.rs` (PushSubscription/get and
//! /set); these functions take the account's session, the locks and a fresh connection, and
//! call into them.
//!
//! The road: the server sends its Web Push (RFC 8030, encrypted per RFC 8291, signed with its
//! own VAPID key) to `https://fcm.googleapis.com/fcm/send/<token>`; Play services hands the
//! encrypted bytes to the app; the app gives them to [`Core::web_push_receive`]. The core never
//! talks to Google and never takes a URL from the host: it builds the one above from the token.
//!
//! What is kept where (per account; one core is one account):
//! - secret store, [`SECRET_PRIVATE_KEY`]: the P-256 private key (32 bytes, base64url, no
//!   padding); [`SECRET_AUTH`]: the 16-byte auth secret (same encoding). Nowhere else, ever.
//! - key/value table of the account, [`STATE_KEY`]: one JSON record with the random id of the
//!   registration, the `deviceClientId`, the server key the registration was made for, the
//!   token, the subscription id, when it was created, verified and expires, when a message was
//!   last decrypted, a verification code that could not be written back yet, and the ids of
//!   subscriptions that could not be destroyed yet. `Store::delete_account` removes it with the
//!   rest of the account.
//!
//! What never appears in an error, a `Debug` text or a log: the private key, the auth secret,
//! the token, the push URL, the verification code and any part of a message, encrypted or not.
//! An error says what failed in plain words and nothing else.
//!
//! The notification rule is untouched: a message only causes the same pass as the 15-minute
//! check (`push::check_once`). Nothing shown comes from a message's own content.

use std::{sync::Arc, time::Duration};

use crate::{
    auth::Tokens,
    jmap::{is_unauthorized, Connection},
    model::{Account, Reminder},
    notify::{OnScreen, Plan},
    webpush::{self, message::Incoming, record, Ctx},
    Result,
};

use super::{not_signed_in, now, Core, Shade};

/// The longest the reminder of a calendar alert is waited for inside a push receiver (which
/// has 10 seconds in all); without it the reminder goes out with its ids alone.
const REMINDER_BUDGET: Duration = Duration::from_secs(6);
/// The longest a sign-out waits for the server to destroy the subscription.
pub(super) const SIGN_OUT_BUDGET: Duration = Duration::from_secs(6);

/// Secret store name of the P-256 private key of this account's subscription.
pub const SECRET_PRIVATE_KEY: &str = "web_push_private_key";
/// Secret store name of the 16-byte auth secret of this account's subscription.
pub const SECRET_AUTH: &str = "web_push_auth";
/// Key (in the account's key/value table) of the JSON record described in the module text.
pub const STATE_KEY: &str = "web_push";
/// The server sends one verification per account per minute and only for the newest
/// subscription: a second create inside this many seconds is refused here.
pub const VERIFY_GAP_SECS: i64 = 60;
/// The server keeps a subscription for at most seven days; a longer wish is cut to this.
pub const LIFETIME_SECS: i64 = 7 * 24 * 3600;
/// [`Core::web_push_renew`] extends the subscription once less than this is left.
pub const RENEW_BEFORE_SECS: i64 = 4 * 24 * 3600;
/// [`Core::web_push_renew`] looks at the server at least this often, even with nothing to extend.
/// Shorter than the 15 minutes between two background checks, so every one of them looks: a
/// subscription the server lost (removed there, a restored backup) means no push at all, and
/// nothing else would notice. On the emulator a subscription destroyed behind the app went
/// unnoticed while this was a day. The check has the server on the line anyway.
pub const LOOK_EVERY_SECS: i64 = 10 * 60;
/// The data types of the subscription: the ones the live connection listens to
/// (`Connection::events`). New mail does not come as one of them but as an EmailPush.
pub const TYPES: [&str; 5] = ["Email", "Mailbox", "CalendarAlert", "CalendarEventNotification", "ShareNotification"];
/// The `emailPush` properties asked for: the list the experiment was run with (part d), so the
/// shape is one the server is known to accept and deliver. Their content is not used in this
/// version: a message only says "mail came".
pub const EMAIL_PUSH_PROPERTIES: [&str; 8] =
    ["id", "threadId", "mailboxIds", "keywords", "receivedAt", "subject", "from", "preview"];
/// Only a push of this urgency wakes an idle phone (experiment, part e).
pub const EMAIL_PUSH_URGENCY: &str = "high";

/// What Play services needs for the registration intent.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct WebPushRegistration {
    /// The server's `applicationServerKey` as the session gives it: base64url of the 65-byte
    /// point, WITHOUT `=` padding (padding is stripped here if a server sends it; with padding
    /// Play services answers SERVICE_NOT_AVAILABLE). Goes into the extras `sender`,
    /// `subscription` and `X-subscription`.
    pub sender: String,
    /// `wp:https://<host of the account's server>/#<random id>`. The id is a version 4 UUID in
    /// text form, made once per account and kept in the state record; the host is the server
    /// URL's host without a port. Goes into the extras `subtype` and `X-subtype`, and comes back
    /// as the extra `subtype` of every message: the Android side keeps it to find the account.
    pub subtype: String,
}

/// Where this account's subscription stands. Made from the state record alone.
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum WebPushState {
    /// No subscription and none being made.
    Off,
    /// A subscription was created at `since` (Unix seconds) and the server's verification has
    /// not been received and written back yet. No message is sent to it until then.
    Verifying { since: i64 },
    /// Verified and in use until `expires` (Unix seconds). `last_message_at`: when a message
    /// for this account was last decrypted, for "last push N ago".
    Registered { expires: i64, last_message_at: Option<i64> },
    /// The server now has another key than the one the registration was made for. Nothing
    /// arrives any more: register again with [`Core::web_push_prepare`].
    KeyChanged,
    /// The server no longer has the subscription (expired, or removed there), or its time ran
    /// out here. Nothing arrives any more.
    Gone,
}

/// What a received message was, after the core has dealt with it.
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum WebPushMessage {
    /// The server's verification for the subscription that is waiting: the code was written
    /// back and the server accepted it. The state is `Registered` from now on.
    Verified,
    /// Mail came or something changed (an EmailPush or a StateChange naming this account): the
    /// pass was run, and this is its plan. The listener, if one is registered, got it as well,
    /// exactly as with [`Core::check_once`].
    Checked { plan: Plan },
    /// A calendar or task reminder fired on the server. The details were fetched; if that
    /// failed, the reminder has only its ids (the host shows a general text). It is NOT given
    /// to the listener: show it from here.
    Reminder { reminder: Reminder },
    /// Decrypted and understood, and nothing to do: a verification for a subscription that is
    /// not the waiting one, a StateChange that does not name this account, an unknown `@type`.
    Ignored,
    /// Not for this account's keys, cut short, not `aes128gcm`, more than one record, or the
    /// account has no keys. The host drops it. Nothing is said about why.
    Undecryptable,
}

/// Whether a pass is still owed for something the user did, and when.
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum WorkWaiting {
    Nothing,
    /// A pass is useful `in_ms` milliseconds from now; 0 means now.
    Due {
        in_ms: u64,
    },
}

#[uniffi::export]
impl Core {
    /// Whether the server of this account can push through Google: its session names an
    /// `applicationServerKey` under `urn:ietf:params:jmap:webpush-vapid` that is a 65-byte
    /// uncompressed P-256 point. A server without a key would send unsigned requests, which
    /// Google refuses.
    ///
    /// Reads the session anew from the server (never the one a kept connection remembers).
    /// Stores nothing.
    ///
    /// Refusals: not signed in is `SignInNeeded`; a server that cannot be reached is
    /// `Temporary`. A missing or malformed key is `Ok(false)`, not an error.
    pub fn web_push_supported(&self) -> Result<bool> {
        let (account, tokens) = self.push_session()?;
        self.rt.block_on(online(&tokens, &account.server_url, async |conn| Ok(conn.web_push_key().await?.is_some())))
    }

    /// First step of switching on, and of registering again. Makes sure the account has a key
    /// pair, an auth secret and a registration id, reads the server's key anew, and returns what
    /// the registration intent needs. `None` means the server has no usable key (the reason
    /// `no_server_key`); nothing is made or stored then.
    ///
    /// Keys and auth secret are made once (from the system's random source) and loaded
    /// afterwards. They are made again only after [`Core::web_push_forget`]. The id inside the
    /// subtype is likewise kept, so the same account asks Play services for the same subtype and
    /// gets the same token back.
    ///
    /// When the server's key differs from the one in the state record, the record takes the new
    /// key and its token is cleared: the old token belongs to the old key. The subscription on
    /// the server is not touched here; the next [`Core::web_push_subscribe`] replaces it.
    ///
    /// Stored: the two secrets (secret store), the id and the server key (state record).
    ///
    /// Refusals: not signed in is `SignInNeeded`; server unreachable is `Temporary`; a secret
    /// store that cannot be read or written is `Store` ("the keys for push could not be kept"),
    /// and then nothing half-made is left behind: either both secrets are there or neither.
    pub fn web_push_prepare(&self) -> Result<Option<WebPushRegistration>> {
        let (account, tokens) = self.push_session()?;
        self.rt.block_on(async {
            let _one_at_a_time = self.store.webpush_lock().lock().await;
            let ctx = Ctx { store: &self.store, secrets: &*self.secrets, account: &account };
            online(&tokens, &account.server_url, async |conn| webpush::prepare(&ctx, conn).await).await
        })
    }

    /// Second step: Play services answered with `token`. Makes this account's subscription on
    /// the server point at `https://fcm.googleapis.com/fcm/send/<token>` and returns the state.
    ///
    /// 1. Reads the session anew. If the server's key is not the one of the last
    ///    [`Core::web_push_prepare`], refuses with `Invalid` ("the server's key changed:
    ///    register again"): the token was made for the other key.
    /// 2. Lists the account's subscriptions (`PushSubscription/get`: id, deviceClientId, expires).
    /// 3. If the token is the stored one and the stored subscription is in that list and was
    ///    verified: only extends it to [`LIFETIME_SECS`] from now and returns `Registered`. No
    ///    new verification is needed.
    /// 4. Otherwise replaces it. If the previous create was less than [`VERIFY_GAP_SECS`] ago,
    ///    refuses first with `Temporary` naming the seconds to wait, and changes nothing (the
    ///    server would not verify the new one). Then destroys every listed subscription whose
    ///    `deviceClientId` is this device's (the stored one and stale ones; the server allows 15
    ///    per account and never drops a dead one), and creates one with: `deviceClientId`, the
    ///    URL, `keys` (`p256dh`: the 65-byte public point, `auth`: the auth secret, both
    ///    base64url without padding), `types` = [`TYPES`], `expires` = now + [`LIFETIME_SECS`],
    ///    and `emailPush` = `{<account id>: {"properties": EMAIL_PUSH_PROPERTIES, "urgency":
    ///    "high"}}`. Returns `Verifying`.
    ///
    /// The server sends the verification once and never again. The host waits about 90 seconds
    /// for [`WebPushMessage::Verified`], then calls this again with the same token (step 4 runs,
    /// since the stored one is unverified), three times in all, and then gives up with
    /// [`Core::web_push_drop`].
    ///
    /// `deviceClientId` is `kweb-<the registration id>`: the same for every subscription this
    /// account ever makes on this install, and different on every other device.
    ///
    /// Stored (state record): token, subscription id, created at, expires; "verified" is cleared
    /// in step 4. The record is written after the server answered, so a failure leaves the old
    /// record; a create whose answer was lost leaves a subscription the next call destroys by
    /// its `deviceClientId`.
    ///
    /// Refusals: not signed in is `SignInNeeded`. No keys (prepare was not called, or forget was)
    /// is `Invalid`. A token that is empty, longer than 4096 characters or holds anything but
    /// letters, digits and `-_:.~` is `Invalid` ("the token from Google's push service is not
    /// usable"), without repeating it. Server unreachable is `Temporary`. The server refusing
    /// the create is `Invalid` with the server's own description, except that a refused URL or
    /// keys are named in fixed words (the description may repeat them). 15 subscriptions of other
    /// devices is `Invalid` ("this account has too many push subscriptions on other devices").
    pub fn web_push_subscribe(&self, token: String) -> Result<WebPushState> {
        let (account, tokens) = self.push_session()?;
        self.rt.block_on(async {
            let _one_at_a_time = self.store.webpush_lock().lock().await;
            let ctx = Ctx { store: &self.store, secrets: &*self.secrets, account: &account };
            let token = &token;
            online(&tokens, &account.server_url, async |conn| webpush::subscribe(&ctx, conn, token, now()).await).await
        })
    }

    /// A message arrived from Play services: `body` is the `rawData` extra as it came, `shade`
    /// what the host knows of the notification shade (as for [`Core::check_once`]). Decrypts it
    /// with this account's keys (RFC 8291, `aes128gcm`, one record of at most 4096 bytes) and
    /// deals with it:
    ///
    /// - `PushVerification` whose `pushSubscriptionId` is the stored, unverified subscription:
    ///   writes the code back (`PushSubscription/set` update `verificationCode`), marks the
    ///   record verified, returns `Verified`. If the write cannot be made now, the code is kept
    ///   in the state record, the call fails with `Temporary`, and the next
    ///   [`Core::web_push_renew`] writes it. A code the server says does not match is dropped
    ///   and is `Invalid`.
    /// - `EmailPush` for this account, or `StateChange` whose `changed` names this account:
    ///   runs the pass ([`Core::check_once`] with `shade`: queued actions, sync, the rule, drafts
    ///   and messages to send) and returns `Checked` with its plan. Nothing of the EmailPush is
    ///   used but the fact that it came. The pass is run here whether or not the live connection
    ///   is running: the process may have been woken for this message alone.
    /// - `CalendarAlert` for this account: fetches the reminder as the live connection does and
    ///   returns `Reminder` (with the ids alone if the fetch fails; never an error for that).
    /// - anything else that decrypts: `Ignored`. What does not decrypt: `Undecryptable`.
    ///
    /// Every message that decrypts sets "last message at" in the state record.
    ///
    /// The receiver has 10 seconds in all. This call does not watch the clock: the host runs it
    /// on a thread, waits about 8 seconds, and if it has not returned (or returned an error)
    /// hands over to the expedited check and finishes. The call goes on meanwhile and is safe to
    /// overlap with that check: passes take turns (the store's pass lock), and each hands its
    /// plan to the listener ([`Core::listen`]) while it holds that lock, so the host shows the
    /// plans in the order they were made and does not apply the returned one again.
    ///
    /// Errors mean "understood, but the work did not get done": `Temporary` (server unreachable)
    /// and `SignInNeeded` from the pass or the write-back, `Store` from the cache. No error for a
    /// message that is not ours.
    pub fn web_push_receive(&self, body: Vec<u8>, shade: Shade) -> Result<WebPushMessage> {
        let (account, tokens) = self.push_session()?;
        let ctx = Ctx { store: &self.store, secrets: &*self.secrets, account: &account };
        let Some(incoming) = webpush::decrypt(&ctx, &body)? else { return Ok(WebPushMessage::Undecryptable) };
        // Failing to note the time must not lose the message.
        let _ = record::touch_message(&self.store, &account.id, now());
        match incoming {
            Incoming::Verification { subscription_id, code } => self.rt.block_on(async {
                // The server sends this within a moment of the create: wait for a subscribe
                // that is still writing its record.
                let _one_at_a_time = self.store.webpush_lock().lock().await;
                if !webpush::waiting_for(&ctx, &subscription_id)? {
                    return Ok(WebPushMessage::Ignored);
                }
                let (id, code) = (&subscription_id, &code);
                let answer =
                    online(&tokens, &account.server_url, async |conn| conn.push_subscription_verify(id, code).await)
                        .await;
                webpush::verified(&ctx, id, code, answer, now())
            }),
            Incoming::Mail | Incoming::Changed => Ok(WebPushMessage::Checked { plan: self.check_pass_only(shade)? }),
            Incoming::CalendarAlert { event_id, alert_id, recurrence_id } => {
                let found = self.with_connection_within(Some(REMINDER_BUDGET), |conn| {
                    let (event_id, alert_id, recurrence_id) =
                        (event_id.clone(), alert_id.clone(), recurrence_id.clone());
                    async move { conn.reminder(&event_id, &alert_id, recurrence_id.as_deref()).await }
                });
                let reminder = found.unwrap_or_else(|_| bare_reminder(event_id, alert_id, recurrence_id));
                Ok(WebPushMessage::Reminder { reminder })
            }
            Incoming::Other => Ok(WebPushMessage::Ignored),
        }
    }

    /// Housekeeping, cheap enough for every run of the 15-minute check: it goes to the server
    /// only when there is something to do (which the periodic check always has: the look), and
    /// otherwise answers from the state record (one-off checks in between).
    ///
    /// It goes to the server when: a verification code is waiting to be written back; a
    /// subscription could not be destroyed earlier; less than [`RENEW_BEFORE_SECS`] is left; or
    /// the last look was more than [`LOOK_EVERY_SECS`] ago. There it, in this order: destroys
    /// what was left over; writes a kept code; reads the session anew and returns `KeyChanged`
    /// if the server's key is not the stored one; looks the subscription up and returns `Gone`
    /// if the server does not list it; extends it to now + [`LIFETIME_SECS`] when less than
    /// [`RENEW_BEFORE_SECS`] is left.
    ///
    /// Without going to the server: `Off` stays `Off`; `Verifying` stays (the host's own clock
    /// decides when to try again); a subscription whose time ran out is `Gone`.
    ///
    /// `KeyChanged` and `Gone` are remembered in the state record, so [`Core::web_push_state`]
    /// says the same until the next prepare and subscribe, or drop.
    ///
    /// Refusals: not signed in is `SignInNeeded`; server unreachable is `Temporary` and changes
    /// nothing (the next run tries again, and four days of them are left before the end).
    pub fn web_push_renew(&self) -> Result<WebPushState> {
        let (account, tokens) = self.push_session()?;
        self.rt.block_on(async {
            let _one_at_a_time = self.store.webpush_lock().lock().await;
            let ctx = Ctx { store: &self.store, secrets: &*self.secrets, account: &account };
            if let Some(state) = webpush::renew_locally(&ctx, now())? {
                return Ok(state);
            }
            online(&tokens, &account.server_url, async |conn| webpush::renew(&ctx, conn, now()).await).await
        })
    }

    /// First step of switching off, of signing out and of removing the account: destroys this
    /// account's subscription on the server, and every other one with this device's
    /// `deviceClientId`. The keys stay, so a message already on its way still decrypts; call
    /// [`Core::web_push_forget`] after Play services was told.
    ///
    /// With no subscription and nothing left over it does nothing and succeeds.
    ///
    /// When the server cannot be reached the call fails with `Temporary`, and the subscription
    /// id is put on the left-over list of the state record, which the next
    /// [`Core::web_push_renew`] or [`Core::web_push_subscribe`] works off. The host goes on
    /// with switching off regardless (a token Play services no longer knows makes the
    /// subscription harmless; the server stops trying when it expires). On a sign-out there is
    /// no later: the subscription is then left to expire.
    ///
    /// State afterwards: `Off` (also after a failure: nothing is expected to arrive any more).
    ///
    /// Refusals: not signed in is `SignInNeeded`.
    pub fn web_push_drop(&self) -> Result<()> {
        self.web_push_drop_within(None)
    }

    /// Last step of switching off: deletes the key pair and the auth secret from the secret
    /// store and everything in the state record except the left-over list. The server is not
    /// contacted. Does not block on the network. Safe to call twice and without an account.
    ///
    /// `Core::sign_out` does the same on its own (one line there), before it revokes the grant.
    ///
    /// Refusals: a secret store that cannot delete is `Store`; the state record is cleared
    /// regardless, so the keys can no longer be used through the core.
    pub fn web_push_forget(&self) -> Result<()> {
        let account = self.lock().session.as_ref().map(|s| s.account.id.clone());
        webpush::forget(account.as_deref().map(|id| (&*self.store, id)), &*self.secrets)
    }

    /// Where the subscription stands, for the Settings and Delivery check screens. From the
    /// state record alone: does not block and never fails (an unreadable record is `Off`).
    pub fn web_push_state(&self) -> WebPushState {
        match self.account() {
            Some(account) => Ctx { store: &self.store, secrets: &*self.secrets, account: &account }.state(now()),
            None => WebPushState::Off,
        }
    }

    /// Decision 4: what the user did must not wait because no live connection is running. The
    /// host asks this when it is about to stop the live connection of an account in Google mode
    /// (the app left the screen), and again at the end of every background check, and schedules
    /// a check for then (see the note for how).
    ///
    /// From the cache alone, no network:
    /// - `Due { in_ms: 0 }` when any of these exists: a queued action; a draft that is not on
    ///   the server in its latest form; a message waiting to be handed to the server; a message
    ///   held here whose time has come.
    /// - `Due { in_ms }` with the time to the earliest of: the send time of a message held here
    ///   (it leaves with the first pass after that); three seconds after the send time of a
    ///   message the server holds for the Undo window (the pass only brings the row up to date);
    ///   a send-later the server holds is NOT counted (the server sends it on its own, and the
    ///   next push or check updates the row).
    /// - `Nothing` otherwise, and without an account.
    ///
    /// The sending rule is not touched: this only says when to run the same pass.
    ///
    /// Refusals: `Store` when the cache cannot be read.
    pub fn work_waiting(&self) -> Result<WorkWaiting> {
        let Ok(account_id) = self.account_id() else { return Ok(WorkWaiting::Nothing) };
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        Ok(match self.store.work_waiting(&account_id, now_ms)? {
            Some(in_ms) => WorkWaiting::Due { in_ms },
            None => WorkWaiting::Nothing,
        })
    }
}

impl Core {
    /// The account and its tokens, cloned out of the session so no lock is held meanwhile.
    fn push_session(&self) -> Result<(Account, Tokens)> {
        let inner = self.lock();
        let session = inner.session.as_ref().ok_or_else(not_signed_in)?;
        Ok((session.account.clone(), session.tokens.clone()))
    }

    /// The pass of [`Core::check_once`] without the directory and the body prefetch after it:
    /// the receiver of a push has seconds, not a quarter of a minute.
    fn check_pass_only(&self, shade: Shade) -> Result<Plan> {
        let deps = {
            let inner = self.lock();
            let session = inner.session.as_ref().ok_or_else(not_signed_in)?;
            self.deps(session, inner.listener.clone().unwrap_or_else(|| Arc::new(super::Silent)))
        };
        let on_screen = match &shade {
            Shade::Unknown => OnScreen::Unknown,
            Shade::Cleared => OnScreen::Cleared,
            Shade::Ids { ids } => OnScreen::Ids(ids),
        };
        self.rt.block_on(crate::push::check_pass_only(&deps, on_screen))
    }

    /// [`Core::web_push_drop`], given up as `Temporary` after `budget` (a sign-out must not
    /// wait for a server that does not answer; what is left is on the left-over list).
    pub(super) fn web_push_drop_within(&self, budget: Option<Duration>) -> Result<()> {
        let (account, tokens) = self.push_session()?;
        let run = async {
            let _one_at_a_time = self.store.webpush_lock().lock().await;
            let ctx = Ctx { store: &self.store, secrets: &*self.secrets, account: &account };
            if !webpush::drop_locally(&ctx)? {
                return Ok(());
            }
            online(&tokens, &account.server_url, async |conn| webpush::drop_on_server(&ctx, conn).await).await
        };
        self.rt.block_on(async {
            match budget {
                Some(budget) => tokio::time::timeout(budget, run)
                    .await
                    .unwrap_or_else(|_| Err(crate::Error::temporary("the server did not answer in time"))),
                None => run.await,
            }
        })
    }
}

/// A reminder that only knows which alert fired.
fn bare_reminder(event_id: String, alert_id: String, recurrence_id: Option<String>) -> Reminder {
    Reminder {
        event_id,
        alert_id,
        recurrence_id,
        is_task: false,
        title: String::new(),
        location: String::new(),
        local_start: None,
        time_zone: None,
        all_day: false,
    }
}

/// Runs `f` on a connection made with a session read just now. A 401 renews the token and runs
/// it once more on a new connection.
async fn online<T>(tokens: &Tokens, server_url: &str, f: impl AsyncFn(&Connection) -> Result<T>) -> Result<T> {
    let mut renewed = false;
    loop {
        let token = tokens.access_token().await?;
        let result = match Connection::connect(server_url, &token.token).await {
            Ok(conn) => f(&conn).await,
            Err(e) => Err(e),
        };
        match result {
            Err(e) if !renewed && is_unauthorized(&e) => {
                renewed = true;
                tokens.invalidate().await;
            }
            other => return other,
        }
    }
}
