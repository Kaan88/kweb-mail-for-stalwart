//! Admin design, step 2: admin access with an API key, and the admin features every Stalwart
//! server has (the mail queue, accounts, blocked addresses, logs, maintenance).
//!
//! Every function here works on the server the account is signed in to, with the admin key of
//! that account (see `admin_link.rs`). Nothing is cached. What the key may not do is
//! `Error::Invalid` with words for the user.

use std::future::Future;

use super::{
    admin_link::{is_plausible_key, key_blocked, key_refused},
    not_signed_in, Core,
};
use crate::{
    jmap::{is_forbidden, is_unauthorized, Connection},
    Error, Result,
};

/// What the account's admin key opens.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct AdminAccess {
    /// A key is stored and the server takes it.
    pub connected: bool,
    /// A key is stored but the server refused it (run out or deleted).
    pub key_refused: bool,
    /// "enterprise", "community", "oss" or empty.
    pub edition: String,
    pub may_queue: bool,
    pub may_accounts: bool,
    pub may_blocked: bool,
    pub may_logs: bool,
    pub may_maintenance: bool,
    /// The Enterprise areas: each true only on a licensed server where the key may read it.
    pub may_dashboard: bool,
    pub may_history: bool,
    pub may_recovery: bool,
    pub may_alerts: bool,
    pub may_tenants: bool,
}

/// The figures on the admin home screen. A figure the key may not read is `None`.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct AdminSummary {
    pub queue_waiting: Option<u32>,
    pub accounts: Option<u32>,
    pub blocked: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct QueuedRecipient {
    pub address: String,
    /// "waiting", "delivered", "failed for now" or "failed": plain words made by the core.
    pub status: String,
    /// What the other server said, shortened; empty when nothing.
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct QueuedMessage {
    pub id: String,
    pub from: String,
    pub recipients: Vec<QueuedRecipient>,
    pub size: u64,
    /// Unix seconds.
    pub queued_at: Option<i64>,
    pub next_try_at: Option<i64>,
    pub tries: u32,
    /// The last error in plain words; empty when there was none.
    pub error: String,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct QueuePage {
    pub messages: Vec<QueuedMessage>,
    pub total: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ServerAccountKind {
    User,
    Group,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ServerAccount {
    pub id: String,
    /// The login name (the part before the @).
    pub name: String,
    pub email: String,
    pub description: String,
    pub kind: ServerAccountKind,
    pub is_admin: bool,
    /// Signing in is switched off.
    pub is_suspended: bool,
    /// The storage limit in bytes; `None` for no limit.
    pub quota: Option<u64>,
    /// Bytes in use, when the server says.
    pub used: Option<u64>,
    pub aliases: Vec<String>,
    /// Names of the groups it is in.
    pub groups: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ServerAccountPage {
    pub accounts: Vec<ServerAccount>,
    pub total: u32,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ServerDomain {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct BlockedAddress {
    pub id: String,
    /// The network address that is blocked.
    pub address: String,
    /// Why, in the server's words; may be empty.
    pub reason: String,
    pub blocked_at: Option<i64>,
    pub expires_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct LogLine {
    /// Unix seconds; `None` when the line has no time the core could read.
    pub at: Option<i64>,
    /// "error", "warning", "info", "debug" or "trace"; empty when unknown.
    pub level: String,
    pub text: String,
}

/// Something an administrator starts on the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum Maintenance {
    ReloadSettings,
    ReloadCertificates,
    PauseQueue,
    ResumeQueue,
}

/// What an account without an admin key has.
fn no_access() -> AdminAccess {
    AdminAccess {
        connected: false,
        key_refused: false,
        edition: String::new(),
        may_queue: false,
        may_accounts: false,
        may_blocked: false,
        may_logs: false,
        may_maintenance: false,
        may_dashboard: false,
        may_history: false,
        may_recovery: false,
        may_alerts: false,
        may_tenants: false,
    }
}

impl Core {
    /// Runs `f` with the stored admin key. A key the server turns down mid-way is
    /// `Error::Invalid` saying so, as at the connection.
    fn admin_run<T, F, Fut>(&self, f: F) -> Result<T>
    where
        F: FnOnce(Connection) -> Fut,
        Fut: Future<Output = Result<T>>,
    {
        self.admin(f).map_err(|e| if is_unauthorized(&e) { key_refused() } else { e })
    }

    /// The JMAP account id the app is signed in with.
    fn signed_in_account(&self) -> Result<String> {
        Ok(self.lock().session.as_ref().ok_or_else(not_signed_in)?.account.id.clone())
    }
}

#[uniffi::export]
impl Core {
    /// What the account's admin key opens. Asks the server when a key is stored. No key is
    /// `connected: false` with everything else false, never an error.
    pub fn admin_access(&self) -> Result<AdminAccess> {
        let Some(key) = self.admin_key()? else { return Ok(no_access()) };
        let refused = || AdminAccess { key_refused: true, ..no_access() };
        if self.admin_key_refused_before() {
            return Ok(refused());
        }
        match self.admin_with_key(&key, |conn| async move { conn.admin_info().await }) {
            Ok(info) => Ok(info.access()),
            Err(e) if e == key_refused() || is_unauthorized(&e) => {
                // A key the server turned down will not be taken later: not asked again.
                self.mark_admin_key_refused();
                Ok(refused())
            }
            // 403: another address, or the account is switched off. That can pass, so the key is kept
            // and tried again next time, but the host is told to offer a new key or to forget this one.
            Err(e) if e == key_blocked() || is_forbidden(&e) => Ok(refused()),
            Err(e) => Err(e),
        }
    }

    /// Takes an API key the administrator pasted: checks it against the server and stores it
    /// in the secret store when the server takes it and it belongs to the signed-in account.
    /// A key the server refuses, a key of another account, or a key that opens nothing
    /// administrative is `Error::Invalid` saying which, and nothing is stored. The key is
    /// never logged, cached or put into an error.
    pub fn admin_connect(&self, api_key: String) -> Result<AdminAccess> {
        let key = api_key.trim().to_string();
        if key.is_empty() {
            return Err(Error::invalid("paste the admin key first"));
        }
        if !is_plausible_key(&key) {
            return Err(Error::invalid("this is not an API key"));
        }
        let username = {
            let inner = self.lock();
            inner.session.as_ref().ok_or_else(not_signed_in)?.account.username.clone()
        };
        let (owner, info) = self
            .admin_with_key(&key, |conn| async move { Ok((conn.username(), conn.admin_info().await?)) })
            .map_err(|e| if is_unauthorized(&e) { key_refused() } else { e })?;
        if !owner.eq_ignore_ascii_case(&username) {
            return Err(Error::invalid("this key belongs to another account than the one signed in here"));
        }
        if !info.opens_something() {
            return Err(Error::invalid(
                "this key opens no admin area: make it with the permissions for the queue, accounts, blocked addresses, logs or maintenance",
            ));
        }
        self.store_admin_key(key)?;
        Ok(info.access())
    }

    /// Forgets the admin key on this device. The key itself stays valid on the server.
    pub fn admin_disconnect(&self) -> Result<()> {
        self.drop_admin_key()
    }

    pub fn admin_summary(&self) -> Result<AdminSummary> {
        self.admin_run(|conn| async move { conn.admin_summary().await })
    }

    /// Messages waiting to be delivered, the next to be tried first: `limit` (at most 100)
    /// from `offset`.
    pub fn admin_queue(&self, offset: u32, limit: u32) -> Result<QueuePage> {
        self.admin_run(move |conn| async move { conn.queue_page(offset as usize, limit as usize).await })
    }

    pub fn admin_queued_message(&self, id: String) -> Result<QueuedMessage> {
        self.admin_run(move |conn| async move { conn.queue_message(&id).await })
    }

    /// Has the server try a waiting message again now.
    pub fn admin_queue_retry(&self, id: String) -> Result<()> {
        self.admin_run(move |conn| async move { conn.queue_retry(&id).await })
    }

    /// Takes a message out of the queue: it is not delivered. One that already left or is
    /// gone is not an error.
    pub fn admin_queue_cancel(&self, id: String) -> Result<()> {
        self.admin_run(move |conn| async move { conn.queue_cancel(&id).await })
    }

    /// The server's accounts and groups whose name, address or description has `text` (blank:
    /// all), by name: `limit` (at most 100) from `offset`.
    pub fn admin_accounts(&self, text: String, offset: u32, limit: u32) -> Result<ServerAccountPage> {
        self.admin_run(move |conn| async move { conn.account_page(&text, offset as usize, limit as usize).await })
    }

    pub fn admin_account(&self, id: String) -> Result<ServerAccount> {
        self.admin_run(move |conn| async move { conn.account_one(&id).await })
    }

    /// The domains an account can be made in.
    pub fn admin_domains(&self) -> Result<Vec<ServerDomain>> {
        self.admin_run(|conn| async move { conn.domain_list().await })
    }

    /// Makes a user. `name` is the part before the @ (letters, digits, dot, hyphen,
    /// underscore: checked here, the server takes almost anything); a name that is taken, a
    /// password under 8 characters or an unknown domain is `Error::Invalid`.
    pub fn admin_create_account(
        &self,
        name: String,
        domain_id: String,
        description: String,
        password: String,
    ) -> Result<ServerAccount> {
        self.admin_run(move |conn| async move { conn.account_create(&name, &domain_id, &description, &password).await })
    }

    /// Changes an account's description and storage limit (`None`: no limit).
    pub fn admin_update_account(&self, id: String, description: String, quota: Option<u64>) -> Result<ServerAccount> {
        self.admin_run(move |conn| async move { conn.account_update(&id, &description, quota).await })
    }

    /// Gives an account a new password. The password is never logged or put into an error.
    pub fn admin_reset_password(&self, id: String, password: String) -> Result<()> {
        self.admin_run(move |conn| async move { conn.account_password(&id, &password).await })
    }

    /// Switches signing in off or on for an account. The signed-in account itself and the
    /// account the admin key belongs to cannot be suspended (`Error::Invalid`).
    pub fn admin_suspend_account(&self, id: String, suspended: bool) -> Result<()> {
        let mine = self.signed_in_account()?;
        self.admin_run(move |conn| async move {
            if suspended {
                refuse_own_account(&id, &mine, &conn, "suspended")?;
            }
            conn.account_suspend(&id, suspended).await
        })
    }

    /// Deletes an account with its mail. The signed-in account itself cannot be deleted; a
    /// group that still has members is `Error::Invalid` saying so.
    pub fn admin_delete_account(&self, id: String) -> Result<()> {
        let mine = self.signed_in_account()?;
        self.admin_run(move |conn| async move {
            refuse_own_account(&id, &mine, &conn, "deleted")?;
            conn.account_delete(&id).await
        })
    }

    /// Network addresses the server blocked (after failed sign-ins and the like), newest first.
    pub fn admin_blocked(&self, offset: u32, limit: u32) -> Result<Vec<BlockedAddress>> {
        self.admin_run(move |conn| async move { conn.blocked_list(offset as usize, limit as usize).await })
    }

    pub fn admin_unblock(&self, id: String) -> Result<()> {
        self.admin_run(move |conn| async move { conn.unblock(&id).await })
    }

    /// The newest lines of the server's log that have `text` (blank: the newest lines), at
    /// most `limit` (at most 200), newest first.
    pub fn admin_logs(&self, text: String, limit: u32) -> Result<Vec<LogLine>> {
        self.admin_run(move |conn| async move { conn.log_lines(&text, limit as usize).await })
    }

    /// Starts a maintenance action. The host asks first.
    pub fn admin_maintenance(&self, what: Maintenance) -> Result<()> {
        self.admin_run(move |conn| async move { conn.run_maintenance(what).await })
    }
}

/// The signed-in account and the account of the admin key are not touched by these calls: a
/// suspended or deleted one would lock the administrator out.
fn refuse_own_account(id: &str, signed_in: &str, conn: &Connection, done: &str) -> Result<()> {
    if id == signed_in {
        return Err(Error::invalid(format!("the account you are signed in with cannot be {done}")));
    }
    if id == conn.account_id() {
        return Err(Error::invalid(format!("the account this admin key belongs to cannot be {done}")));
    }
    Ok(())
}
