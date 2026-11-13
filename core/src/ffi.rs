//! The UniFFI interface: the only part of the core the Kotlin side sees.
//!
//! Every function here blocks until it is done, so the host calls them off its main thread
//! (Kotlin: `Dispatchers.IO`). The exceptions are noted. A [`Listener`] callback must not call
//! a blocking function of [`Core`] from inside the callback.

use std::{
    path::Path,
    sync::{Arc, Mutex, MutexGuard},
    time::{SystemTime, UNIX_EPOCH},
};

use tokio::{net::TcpListener, runtime::Runtime};

use crate::{
    actions,
    auth::{self, loopback, OAuthClient, PendingSignIn, Tokens},
    jmap::Connection,
    model::{Account, Action, ConnectionStatus, Mailbox, MessageHeader, Reminder},
    notify::{self, OnScreen, Plan},
    push::{self, Deps, Listener, Timing},
    secrets::SecretStore,
    store::Store,
    Error, Result,
};

/// Where the serialized [`OAuthClient`] of an account is kept in the store.
const OAUTH_CLIENT_KEY: &str = "oauth_client";
/// Named on the page the browser shows after a sign-in.
const APP_NAME: &str = "kweb mail";

/// Proves the bridge in M0: the first Compose screen shows this string.
#[uniffi::export]
pub fn core_version() -> String {
    format!("kweb-core {}", env!("CARGO_PKG_VERSION"))
}

/// What the host knows about the notification shade when it asks for a check.
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum Shade {
    /// Nothing is known: trust what the core remembers.
    Unknown,
    /// The shade was emptied behind the back of the app (the phone restarted).
    Cleared,
    /// The Android ids of the notifications that are really there.
    Ids { ids: Vec<i32> },
}

struct Session {
    account: Account,
    tokens: Tokens,
}

#[derive(Default)]
struct Inner {
    /// A sign-in the browser has not come back from yet, with the socket it will come back
    /// to (none when it comes back through a redirect the host catches itself).
    pending: Option<(PendingSignIn, Option<TcpListener>)>,
    session: Option<Session>,
    listener: Option<Arc<dyn Listener>>,
    watcher: Option<push::Handle>,
}

/// One per process. Owns the cache, the sign-in and the live connection.
#[derive(uniffi::Object)]
pub struct Core {
    rt: Runtime,
    store: Arc<Store>,
    secrets: Arc<dyn SecretStore>,
    inner: Mutex<Inner>,
}

#[uniffi::export]
impl Core {
    /// Opens (or creates) the cache at `db_path` and picks up the signed-in account, if any.
    #[uniffi::constructor]
    pub fn new(db_path: String, secrets: Arc<dyn SecretStore>) -> Result<Arc<Self>> {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .thread_name("kweb-core")
            .build()
            .map_err(|e| Error::temporary(format!("starting the core: {e}")))?;
        let store = Arc::new(Store::open(Path::new(&db_path))?);
        let session = match store.load_account()? {
            Some(account) => stored_client(&store, &account.id)?
                .map(|client| Session { account, tokens: Tokens::new(client, secrets.clone()) }),
            None => None,
        };
        Ok(Arc::new(Core { rt, store, secrets, inner: Mutex::new(Inner { session, ..Inner::default() }) }))
    }

    /// The signed-in account. Does not block.
    pub fn account(&self) -> Option<Account> {
        self.lock().session.as_ref().map(|s| s.account.clone())
    }

    /// Starts a sign-in and returns the URL to open in the browser. The browser comes back to
    /// a socket on this device (`http://127.0.0.1:<port>/callback`); [`Core::finish_sign_in`]
    /// waits for it.
    pub fn begin_sign_in(&self, server_url: String) -> Result<String> {
        let previous = match self.store.load_account()? {
            Some(account) => stored_client(&self.store, &account.id)?,
            None => None,
        };
        let (pending, listener) = self.rt.block_on(async {
            let (listener, redirect_uri) = loopback::bind().await?;
            let pending = auth::begin_sign_in(&server_url, &redirect_uri, previous.as_ref()).await?;
            Ok::<_, Error>((pending, listener))
        })?;
        let url = pending.authorize_url.clone();
        self.lock().pending = Some((pending, Some(listener)));
        Ok(url)
    }

    /// Starts a sign-in whose answer the host catches itself and returns the URL to open in
    /// the browser. `redirect_uri` is a private-use scheme the host is registered for, such
    /// as `ovh.kweb.mail:/oauth/callback`. This is the way on Android: a socket in the app
    /// cannot be relied on while the browser is in front, because Android may freeze the
    /// process. Finish with [`Core::complete_sign_in`].
    pub fn begin_sign_in_with_redirect(&self, server_url: String, redirect_uri: String) -> Result<String> {
        let previous = match self.store.load_account()? {
            Some(account) => stored_client(&self.store, &account.id)?,
            None => None,
        };
        let pending = self.rt.block_on(auth::begin_sign_in(&server_url, &redirect_uri, previous.as_ref()))?;
        let url = pending.authorize_url.clone();
        self.lock().pending = Some((pending, None));
        Ok(url)
    }

    /// Finishes a sign-in started with [`Core::begin_sign_in_with_redirect`]. `callback_url`
    /// is the full URL the browser was sent back to. A URL that does not belong to the
    /// sign-in in progress is refused and the sign-in stays open.
    pub fn complete_sign_in(&self, callback_url: String) -> Result<Account> {
        let pending = {
            let mut inner = self.lock();
            match inner.pending.take() {
                Some((pending, _)) if belongs_to(&callback_url, pending.state()) => pending,
                other => {
                    inner.pending = other;
                    return Err(Error::invalid("this is not the answer to the sign-in in progress"));
                }
            }
        };
        self.signed_in(pending, callback_url)
    }

    /// Waits (up to five minutes) for the browser to come back, finishes the sign-in and
    /// returns the account. The live connection starts if a listener was given to
    /// [`Core::start`] before.
    pub fn finish_sign_in(&self) -> Result<Account> {
        let (pending, listener) = {
            let mut inner = self.lock();
            match inner.pending.take() {
                Some((pending, Some(listener))) => (pending, listener),
                other => {
                    inner.pending = other;
                    return Err(Error::invalid("no sign-in is waiting for the browser on this device"));
                }
            }
        };
        let state = pending.state().to_string();
        let callback = self.rt.block_on(async {
            let wait = loopback::wait_for_callback(listener, &state, APP_NAME);
            tokio::time::timeout(auth::SIGN_IN_TIMEOUT, wait)
                .await
                .map_err(|_| Error::temporary("no answer from the browser within 5 minutes"))?
        })?;
        self.signed_in(pending, callback)
    }
    /// Revokes the grant on the server (best effort) and wipes the cache.
    pub fn sign_out(&self) -> Result<()> {
        self.stop();
        let session = {
            let mut inner = self.lock();
            inner.pending = None;
            inner.session.take()
        };
        if let Some(session) = session {
            self.rt.block_on(session.tokens.sign_out());
            self.store.delete_account(&session.account.id)?;
        }
        Ok(())
    }

    /// Starts the live connection and reports to `listener` from now on. Without an account
    /// it only remembers the listener; the connection starts once a sign-in finishes.
    /// Does not block.
    pub fn start(&self, listener: Arc<dyn Listener>) {
        let mut inner = self.lock();
        inner.listener = Some(listener);
        if inner.watcher.is_none() {
            self.spawn_watcher(&mut inner);
        }
    }

    /// Ends the live connection. The listener is kept for the next [`Core::start`] or sign-in.
    pub fn stop(&self) {
        let watcher = self.lock().watcher.take();
        if let Some(watcher) = watcher {
            self.rt.block_on(watcher.stop());
        }
    }

    /// Asks the live connection for a pass now (the app came to the foreground). Does not block.
    pub fn poke(&self) {
        if let Some(watcher) = &self.lock().watcher {
            watcher.poke();
        }
    }

    /// The state of the live connection. Does not block.
    pub fn status(&self) -> ConnectionStatus {
        let inner = self.lock();
        match (&inner.session, &inner.watcher) {
            (None, _) => ConnectionStatus::SignedOut,
            (Some(_), Some(watcher)) => watcher.status(),
            (Some(_), None) => ConnectionStatus::Offline { reason: "not started".into() },
        }
    }

    /// One pass without the live connection: for the 15-minute job and the boot receiver.
    /// Returns what to post and cancel; the listener, if any, is told as well.
    pub fn check_once(&self, shade: Shade) -> Result<Plan> {
        let deps = {
            let inner = self.lock();
            let session = inner.session.as_ref().ok_or_else(not_signed_in)?;
            self.deps(session, inner.listener.clone().unwrap_or_else(|| Arc::new(Silent)))
        };
        let on_screen = match &shade {
            Shade::Unknown => OnScreen::Unknown,
            Shade::Cleared => OnScreen::Cleared,
            Shade::Ids { ids } => OnScreen::Ids(ids),
        };
        self.rt.block_on(push::check_once(&deps, on_screen))
    }

    /// Mailboxes from the cache.
    pub fn mailboxes(&self) -> Result<Vec<Mailbox>> {
        self.store.list_mailboxes(&self.account_id()?)
    }

    /// Message headers of a mailbox from the cache, newest first.
    pub fn messages(&self, mailbox_id: String, limit: u32, offset: u32) -> Result<Vec<MessageHeader>> {
        self.store.list_messages(&self.account_id()?, &mailbox_id, limit, offset)
    }

    /// Applies an action to the cache at once and sends it to the server as soon as possible.
    /// Works offline and when the live connection is not running (the next check sends it).
    pub fn submit(&self, action: Action) -> Result<()> {
        actions::submit(&self.store, &self.account_id()?, &action)?;
        self.poke();
        Ok(())
    }

    /// The user swiped a mail notification away: it is not shown again.
    pub fn notification_swiped(&self, message_id: String) -> Result<()> {
        notify::swiped(&self.store, &self.account_id()?, &message_id)
    }
}

impl Core {
    /// Exchanges the code, finds the account and starts the live connection if a listener
    /// is waiting. Shared by both ways of coming back from the browser.
    fn signed_in(&self, pending: PendingSignIn, callback: String) -> Result<Account> {
        let (client, tokens, account) = self.rt.block_on(async {
            let signed = auth::complete_sign_in(pending, &callback, self.secrets.clone()).await?;
            let token = signed.tokens.access_token().await?;
            let conn = Connection::connect(&signed.client.server_url, &token.token).await?;
            let username = if signed.username.is_empty() { conn.username() } else { signed.username };
            let account = Account {
                id: conn.account_id(),
                server_url: signed.client.server_url.clone(),
                username,
                signed_in_at: now(),
            };
            Ok::<_, Error>((signed.client, signed.tokens, account))
        })?;

        self.stop();
        let account = self.remember(account, &client)?;
        let mut inner = self.lock();
        inner.session = Some(Session { account: account.clone(), tokens });
        self.spawn_watcher(&mut inner);
        Ok(account)
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        // Nothing is left half-done under this lock, so a panic elsewhere does not poison the state.
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn account_id(&self) -> Result<String> {
        self.lock().session.as_ref().map(|s| s.account.id.clone()).ok_or_else(not_signed_in)
    }

    fn deps(&self, session: &Session, listener: Arc<dyn Listener>) -> Deps {
        Deps {
            store: self.store.clone(),
            tokens: session.tokens.clone(),
            account: session.account.clone(),
            listener,
            timing: Timing::default(),
        }
    }

    fn spawn_watcher(&self, inner: &mut Inner) {
        if let (Some(session), Some(listener)) = (&inner.session, &inner.listener) {
            let _runtime = self.rt.enter();
            inner.watcher = Some(push::spawn(self.deps(session, listener.clone())));
        }
    }

    /// Saves the account and its OAuth client. Signing in again to the same account keeps its
    /// cache and its sign-in time, so mail that is already on screen stays; another account
    /// replaces the old one (phase 1 has one account).
    fn remember(&self, mut account: Account, client: &OAuthClient) -> Result<Account> {
        if let Some(old) = self.store.load_account()? {
            if old.id == account.id {
                account.signed_in_at = old.signed_in_at;
            } else {
                self.store.delete_account(&old.id)?;
            }
        }
        self.store.save_account(&account)?;
        let json = serde_json::to_string(client).map_err(|e| Error::store(e.to_string()))?;
        self.store.kv_set(&account.id, OAUTH_CLIENT_KEY, &json)?;
        Ok(account)
    }
}

/// Whether `callback_url` carries the `state` of the pending sign-in.
fn belongs_to(callback_url: &str, state: &str) -> bool {
    url::Url::parse(callback_url).is_ok_and(|url| url.query_pairs().any(|(k, v)| k == "state" && v == state))
}

fn stored_client(store: &Store, account_id: &str) -> Result<Option<OAuthClient>> {
    // An unreadable entry is treated as none: the user signs in again.
    Ok(store.kv_get(account_id, OAUTH_CLIENT_KEY)?.and_then(|json| serde_json::from_str(&json).ok()))
}

fn not_signed_in() -> Error {
    Error::sign_in_needed("not signed in")
}

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// For a check that runs with no listener registered: the caller gets the plan back anyway.
struct Silent;

impl Listener for Silent {
    fn status(&self, _: ConnectionStatus) {}
    fn notifications(&self, _: Plan) {}
    fn cache_changed(&self) {}
    fn reminder(&self, _: Reminder) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::MemorySecrets;

    #[test]
    fn version_names_the_crate() {
        assert!(core_version().starts_with("kweb-core "));
    }

    #[test]
    fn a_new_core_is_signed_out() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cache.db").to_string_lossy().into_owned();
        let core = Core::new(path, Arc::new(MemorySecrets::default())).unwrap();
        assert_eq!(core.account(), None);
        assert_eq!(core.status(), ConnectionStatus::SignedOut);
        assert!(matches!(core.mailboxes(), Err(Error::SignInNeeded { .. })));
        assert!(matches!(core.check_once(Shade::Unknown), Err(Error::SignInNeeded { .. })));
        assert!(matches!(core.finish_sign_in(), Err(Error::Invalid { .. })));
        core.sign_out().unwrap();
    }
}
