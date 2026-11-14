//! The UniFFI interface: the only part of the core the Kotlin side sees.
//!
//! Every function here blocks until it is done, so the host calls them off its main thread
//! (Kotlin: `Dispatchers.IO`). The exceptions are noted. A [`Listener`] callback must not call
//! a blocking function of [`Core`] from inside the callback.

use std::{
    collections::HashMap,
    fs,
    future::Future,
    path::Path,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex, MutexGuard,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use tokio::{net::TcpListener, runtime::Runtime};

use crate::{
    actions,
    auth::{self, loopback, OAuthClient, PendingSignIn, Tokens},
    compose, contacts,
    jmap::{self, is_unauthorized, Connection},
    model::{
        Account, Action, Address, AddressBook, Agenda, Attachment, AwayMessage, Calendar, ConnectionStatus, Contact,
        ContactDetail, ContactList, Draft, DraftKind, EventDetail, EventInput, Identity, ListFilter, Mailbox,
        MessageBody, MessageHeader, PendingSend, Quota, Reminder, SearchResult, ThreadRow,
    },
    notify::{self, OnScreen, Plan},
    push::{self, Deps, Listener, Timing},
    secrets::SecretStore,
    store::{DraftState, Store, Threading},
    sync, Error, Result,
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
    /// The connection the reading calls share, with the access token it was made with.
    reader: Option<(String, Arc<Connection>)>,
    /// The contact each form was last opened with, by contact id: what `save_contact` compares
    /// the saved form with, so that only what the user changed is written.
    contact_bases: HashMap<String, Contact>,
}

/// One per process. Owns the cache, the sign-in and the live connection.
#[derive(uniffi::Object)]
pub struct Core {
    rt: Runtime,
    store: Arc<Store>,
    secrets: Arc<dyn SecretStore>,
    /// Shared with the tasks that poke the live loop when a held-back message is due.
    inner: Arc<Mutex<Inner>>,
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
        Ok(Arc::new(Core { rt, store, secrets, inner: Arc::new(Mutex::new(Inner { session, ..Inner::default() })) }))
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
            inner.reader = None;
            inner.contact_bases.clear();
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

    // ---- M3: reading ----

    /// Several actions at once (a swipe on a conversation), in order. Stops at the first one
    /// that fails and returns its error; the ones before it stay applied and queued. Each is
    /// its own cache change, so a list read in between can show some of them.
    pub fn submit_all(&self, actions: Vec<Action>) -> Result<()> {
        let result = actions::submit_all(&self.store, &self.account_id()?, &actions);
        self.poke();
        result
    }

    /// One cached message header, e.g. to find the thread of a tapped notification.
    pub fn message(&self, message_id: String) -> Result<Option<MessageHeader>> {
        self.store.get_message(&self.account_id()?, &message_id)
    }

    /// The conversations of a mailbox from the cache, newest first. Only the part of the
    /// mailbox the cache holds without gaps is listed; [`Core::load_older`] extends it.
    pub fn threads(&self, mailbox_id: String, filter: ListFilter, limit: u32, offset: u32) -> Result<Vec<ThreadRow>> {
        self.store.list_threads(&self.account_id()?, &mailbox_id, filter, limit, offset)
    }

    /// Whether the server may have older mail in this mailbox than the cache lists. From the cache.
    pub fn has_older(&self, mailbox_id: String) -> Result<bool> {
        Ok(self.store.get_window(&self.account_id()?, &mailbox_id)?.is_some())
    }

    /// Fetches the next older page of a mailbox into the cache (network). Returns whether there
    /// may be still more. The listener's `cache_changed` is not called: the caller reloads.
    pub fn load_older(&self, mailbox_id: String) -> Result<bool> {
        let account_id = self.account_id()?;
        if self.store.get_window(&account_id, &mailbox_id)?.is_none() {
            return Ok(false);
        }
        let (store, account_id, mailbox_id) = (&self.store, &account_id, &mailbox_id);
        self.with_connection(|conn| async move { sync::load_older(&conn, store, account_id, mailbox_id).await })
    }

    /// Every cached message of a thread, in all mailboxes, oldest first.
    pub fn thread_messages(&self, thread_id: String) -> Result<Vec<MessageHeader>> {
        self.store.thread_messages(&self.account_id()?, &thread_id)
    }

    /// Asks the server which messages the thread has and caches the headers that are missing
    /// (network). Returns whether the cache changed.
    pub fn refresh_thread(&self, thread_id: String) -> Result<bool> {
        let account_id = self.account_id()?;
        let (store, account_id, thread_id) = (&self.store, &account_id, &thread_id);
        self.with_connection(|conn| async move { sync::refresh_thread(&conn, store, account_id, thread_id).await })
    }

    /// The body of a message: from the cache when it is there, else from the server (and then
    /// cached). Offline with nothing cached is `Error::Temporary`; a message the server does
    /// not have is `Error::Invalid`.
    pub fn message_body(&self, message_id: String) -> Result<MessageBody> {
        let account_id = self.account_id()?;
        if let Some(body) = self.store.get_body(&account_id, &message_id)? {
            return Ok(body);
        }
        let ids = [message_id];
        let ids = &ids;
        let fetched = self.with_connection(|conn| async move { conn.bodies(ids).await })?;
        let body = fetched
            .into_iter()
            .next()
            .ok_or_else(|| Error::invalid(format!("the server has no message '{}'", ids[0])))?;
        self.store.put_body(&account_id, &body)?;
        Ok(body)
    }

    /// Searches on the server (all mail, or one mailbox), newest first. When the server cannot
    /// be reached the cached headers are searched instead and `from_cache` is set. A blank
    /// query finds nothing and asks nothing.
    pub fn search(&self, query: String, mailbox_id: Option<String>, limit: u32) -> Result<SearchResult> {
        let account_id = self.account_id()?;
        let query = query.trim();
        if query.is_empty() {
            return Ok(SearchResult { messages: Vec::new(), from_cache: false });
        }
        let (store, id, mailbox) = (&self.store, &account_id, mailbox_id.as_deref());
        let found =
            self.with_connection(
                |conn| async move { sync::search(&conn, store, id, query, mailbox, limit as usize).await },
            );
        match found {
            Ok(messages) => Ok(SearchResult { messages, from_cache: false }),
            Err(e @ (Error::SignInNeeded { .. } | Error::Invalid { .. })) => Err(e),
            Err(_) => {
                let messages = self.store.search_messages(&account_id, query, mailbox, limit)?;
                Ok(SearchResult { messages, from_cache: true })
            }
        }
    }

    /// Downloads a blob (an attachment, an inline picture) to `dest_path` (network). The file
    /// appears only when complete: it is written next to it under a name of its own and
    /// renamed, so two downloads of the same file at once do not disturb each other.
    pub fn download_blob(&self, blob_id: String, dest_path: String) -> Result<()> {
        static SERIAL: AtomicU64 = AtomicU64::new(0);
        self.account_id()?;
        let part = format!("{dest_path}.{}-{}.part", std::process::id(), SERIAL.fetch_add(1, Ordering::Relaxed));
        let (blob_id, part_path) = (&blob_id, &part);
        let done = self
            .with_connection(|conn| async move {
                // Made anew for each try, so a retry after a 401 starts from an empty file.
                let mut file =
                    fs::File::create(part_path).map_err(|e| Error::store(format!("saving the file: {e}")))?;
                conn.download_to(blob_id, &mut file).await
            })
            .and_then(|()| fs::rename(&part, &dest_path).map_err(|e| Error::store(format!("saving the file: {e}"))));
        if done.is_err() {
            let _ = fs::remove_file(&part);
        }
        done
    }
}

// ---- M4: sending ----
#[uniffi::export]
impl Core {
    /// The addresses the user can send as, from the cache (a pass keeps them current). The
    /// first is the default.
    pub fn identities(&self) -> Result<Vec<Identity>> {
        self.store.identities_list(&self.account_id()?)
    }

    /// Starts a draft and stores it in the cache. A reply or forward is filled in from its
    /// source: recipients, subject ("Re: ", "Fwd: " added once), the original quoted in the
    /// text, and for a forward the original's attachments. The source's body is fetched when
    /// the cache does not have it (network, only then). `Existing` opens a server draft.
    pub fn new_draft(&self, kind: DraftKind) -> Result<Draft> {
        let account_id = self.account_id()?;
        let identities = self.store.identities_list(&account_id)?;
        let (id, at) = (compose::new_id()?, now());
        let draft = match kind {
            DraftKind::New => compose::blank(&identities, id, at),
            DraftKind::Reply { message_id } => {
                let (header, body) = self.source(&account_id, &message_id)?;
                compose::answer(false, (&header, &body), &identities, id, at)
            }
            DraftKind::ReplyAll { message_id } => {
                let (header, body) = self.source(&account_id, &message_id)?;
                compose::answer(true, (&header, &body), &identities, id, at)
            }
            DraftKind::Forward { message_id } => {
                let (header, body) = self.source(&account_id, &message_id)?;
                compose::forward((&header, &body), &identities, id, at)
            }
            DraftKind::Existing { message_id } => {
                return self.open_draft(&account_id, &message_id, &identities, id, at)
            }
        };
        self.store.draft_insert(&account_id, &draft, None, false)?;
        Ok(draft)
    }

    /// Stores the draft in the cache at once and has the next pass put it in Drafts on the
    /// server. Never needs the network. Returns the draft as stored. A draft that was thrown
    /// away, or that is being sent, is `Error::Invalid`.
    pub fn save_draft(&self, draft: Draft) -> Result<Draft> {
        check_draft_id(&draft.id)?;
        let saved = self.store.draft_save(&self.account_id()?, &draft, now())?;
        self.poke();
        Ok(saved)
    }

    /// The drafts in the cache that are not waiting to be sent, newest first.
    pub fn drafts(&self) -> Result<Vec<Draft>> {
        self.store.drafts_list(&self.account_id()?)
    }

    /// One draft from the cache.
    pub fn draft(&self, draft_id: String) -> Result<Option<Draft>> {
        Ok(self.store.draft_get(&self.account_id()?, &draft_id)?.map(|row| row.draft))
    }

    /// Throws a draft away, here at once and on the server with the next pass.
    pub fn discard_draft(&self, draft_id: String) -> Result<()> {
        self.store.draft_discard(&self.account_id()?, &draft_id)?;
        self.poke();
        Ok(())
    }

    /// Uploads a file and adds it to the draft (network). Returns the draft with it.
    /// A file larger than the server takes is `Error::Invalid`.
    pub fn add_attachment(
        &self,
        draft_id: String,
        file_path: String,
        name: String,
        mime_type: String,
    ) -> Result<Draft> {
        let account_id = self.account_id()?;
        match self.store.draft_get(&account_id, &draft_id)? {
            Some(row) if row.state == DraftState::Draft => {}
            _ => return Err(Error::invalid("no such draft is being written")),
        }
        let name = if name.is_empty() {
            Path::new(&file_path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
        } else {
            name
        };
        let mime_type = if mime_type.trim().is_empty() { "application/octet-stream".to_string() } else { mime_type };
        let (path, mime) = (&file_path, &mime_type);
        // No pass lock is held while the file goes up: it may take minutes.
        let uploaded = self.with_connection(|conn| async move {
            let (limit, size) = (conn.max_upload(), fs::metadata(path).map_err(read_error)?.len());
            if limit > 0 && size > limit {
                return Err(Error::invalid(format!("the file is larger than the server takes ({limit} bytes)")));
            }
            conn.upload(fs::read(path).map_err(read_error)?, mime).await
        })?;
        let file = Attachment {
            blob_id: uploaded.blob_id,
            name,
            mime_type,
            size: uploaded.size,
            content_id: None,
            is_inline: false,
        };
        let draft = self.store.draft_add_attachment(&account_id, &draft_id, file, now())?;
        self.poke();
        Ok(draft)
    }

    /// Hands the draft over for sending in `delay_secs` seconds, during which
    /// [`Core::undo_send`] takes it back. Works offline: it goes out with the first pass after
    /// that. A draft with no recipient or a malformed address is `Error::Invalid`. If the
    /// server refuses it later, it is a draft again with `send_error` set, and the listener's
    /// `cache_changed` is called.
    pub fn send(&self, draft: Draft, delay_secs: u32) -> Result<PendingSend> {
        let account_id = self.account_id()?;
        check_draft_id(&draft.id)?;
        compose::rules::check_recipients(&draft)?;
        if !self.store.identities_list(&account_id)?.iter().any(|i| i.id == draft.identity_id) {
            return Err(Error::invalid("choose an address to send from"));
        }
        let at = now();
        let pending = self.store.draft_send(&account_id, &draft, at + i64::from(delay_secs), at)?;
        self.poke_in(delay_secs);
        Ok(pending)
    }

    /// Takes a message back before it has gone to the server and returns it as a draft.
    /// Too late is `Error::Invalid`.
    pub fn undo_send(&self, draft_id: String) -> Result<Draft> {
        self.store.draft_undo_send(&self.account_id()?, &draft_id)
    }

    /// The messages that are still held back or waiting for a connection.
    pub fn pending_sends(&self) -> Result<Vec<PendingSend>> {
        self.store.pending_sends(&self.account_id()?)
    }

    /// Addresses that match what the user typed in a recipient field, best first: the address
    /// book (kept in the cache by a pass, when the server has one) and the people in cached
    /// mail. From the cache only.
    pub fn suggest_recipients(&self, text: String, limit: u32) -> Result<Vec<Address>> {
        compose::suggest(&self.store, &self.account_id()?, &text, limit)
    }

    /// How much of the account's storage is used (network).
    pub fn quota(&self) -> Result<Vec<Quota>> {
        self.with_connection(|conn| async move { conn.quotas().await })
    }
}

/// A draft id is made by the core: hex. Anything of that kind is taken.
fn check_draft_id(id: &str) -> Result<()> {
    let fine =
        !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if fine {
        Ok(())
    } else {
        Err(Error::invalid("not a draft id"))
    }
}

fn read_error(e: std::io::Error) -> Error {
    Error::invalid(format!("the file cannot be read: {e}"))
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
        inner.reader = None;
        self.spawn_watcher(&mut inner);
        Ok(account)
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        // Nothing is left half-done under this lock, so a panic elsewhere does not poison the state.
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The header and the body of a message to answer or pass on. The header must be in the
    /// cache; the body comes from the cache, else from the server.
    fn source(&self, account_id: &str, message_id: &str) -> Result<(MessageHeader, MessageBody)> {
        let header = self
            .store
            .get_message(account_id, message_id)?
            .ok_or_else(|| Error::invalid(format!("the message '{message_id}' is not in the cache")))?;
        Ok((header, self.message_body(message_id.to_string())?))
    }

    /// A draft that is in Drafts on the server, as a local draft. One that was opened before is
    /// the same local draft, not a second one.
    fn open_draft(
        &self,
        account_id: &str,
        message_id: &str,
        identities: &[Identity],
        id: String,
        at: i64,
    ) -> Result<Draft> {
        if let Some(known) = self.store.draft_by_server_id(account_id, message_id)? {
            return Ok(known);
        }
        let ids = [message_id.to_string()];
        let ids = &ids;
        let (headers, bodies, header_ids) = self.with_connection(|conn| async move {
            let (headers, _) = conn.messages(ids).await?;
            Ok((headers, conn.bodies(ids).await?, conn.header_ids(&ids[0]).await?))
        })?;
        let (Some(header), Some(body)) = (headers.first(), bodies.first()) else {
            return Err(Error::invalid(format!("the server has no draft '{message_id}'")));
        };
        let draft = compose::opened((header, body), identities, id, at);
        let threading = header_ids.map(|h| Threading { in_reply_to: h.in_reply_to, references: h.references });
        self.store.draft_insert(account_id, &draft, Some(&threading.unwrap_or_default()), false)?;
        Ok(draft)
    }

    /// Pokes the live loop in `delay_secs` seconds, so a message held back leaves on time.
    /// The loop is looked up when the time comes: it may have been restarted meanwhile.
    fn poke_in(&self, delay_secs: u32) {
        if delay_secs == 0 {
            return self.poke();
        }
        let inner = self.inner.clone();
        self.rt.spawn(async move {
            tokio::time::sleep(Duration::from_secs(u64::from(delay_secs))).await;
            if let Some(watcher) = &inner.lock().unwrap_or_else(|e| e.into_inner()).watcher {
                watcher.poke();
            }
        });
    }

    /// Runs `f` with a live connection. One connection is kept for as long as its access token
    /// is the current one. A 401 renews the token and runs `f` once more on a new connection;
    /// any other error is `f`'s own.
    fn with_connection<T, F, Fut>(&self, f: F) -> Result<T>
    where
        F: Fn(Arc<Connection>) -> Fut,
        Fut: Future<Output = Result<T>>,
    {
        let (tokens, server_url) = {
            let inner = self.lock();
            let session = inner.session.as_ref().ok_or_else(not_signed_in)?;
            (session.tokens.clone(), session.account.server_url.clone())
        };
        self.rt.block_on(async {
            let first = async { f(self.reader(&tokens, &server_url).await?).await }.await;
            match first {
                Err(e) if is_unauthorized(&e) => {
                    tokens.invalidate().await;
                    self.lock().reader = None;
                    f(self.reader(&tokens, &server_url).await?).await
                }
                other => other,
            }
        })
    }

    /// The shared connection, made again when the access token is not the one it was made with.
    async fn reader(&self, tokens: &Tokens, server_url: &str) -> Result<Arc<Connection>> {
        let token = tokens.access_token().await?;
        if let Some((used, conn)) = &self.lock().reader {
            if *used == token.token {
                return Ok(conn.clone());
            }
        }
        let conn = Arc::new(Connection::connect(server_url, &token.token).await?);
        self.lock().reader = Some((token.token, conn.clone()));
        Ok(conn)
    }

    /// Fetches every contact card and keeps them in the cache. `Some` is why the server could
    /// not be reached, which leaves the cache as it is; a listing that a save or a delete
    /// overtook is not kept and made once more.
    fn refresh_contacts(&self, account_id: &str) -> Result<Option<Error>> {
        for _ in 0..2 {
            let generation = self.store.contact_generation();
            match self.with_connection(|conn| async move { conn.contact_cards().await }) {
                Ok(cards) if self.store.contact_cards_replace(account_id, &cards, generation)? => return Ok(None),
                Ok(_) => {}
                Err(e @ (Error::SignInNeeded { .. } | Error::Invalid { .. })) => return Err(e),
                Err(e) => return Ok(Some(e)),
            }
        }
        Ok(None)
    }

    /// The server says the contact is gone: it leaves the cache and the suggestions. Returns `e`.
    fn forget_contact(&self, account_id: &str, contact_id: &str, e: Error) -> Error {
        self.lock().contact_bases.remove(contact_id);
        match self.store.contact_card_forget(account_id, contact_id) {
            Ok(()) => e,
            Err(store_error) => store_error,
        }
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

// ---- M5: calendar ----
#[uniffi::export]
impl Core {
    /// The user's calendars (network; the cached list when the server cannot be reached).
    pub fn calendars(&self) -> Result<Vec<Calendar>> {
        let account_id = self.account_id()?;
        match self.with_connection(|conn| async move { conn.calendars().await }) {
            Ok(list) => {
                self.store.calendars_replace(&account_id, &list)?;
                Ok(list)
            }
            Err(e @ (Error::SignInNeeded { .. } | Error::Invalid { .. })) => Err(e),
            Err(e) => match self.store.calendars_list(&account_id)? {
                // An empty cache says nothing: the list was never fetched, or the user has no calendar.
                list if list.is_empty() => Err(e),
                list => Ok(list),
            },
        }
    }

    /// Everything between `from` and `to` (Unix seconds, `to` exclusive) in the visible
    /// calendars, repeating events expanded, tasks included (a task with no due date has no
    /// place in time and is left out). `time_zone` is the IANA zone the phone is in: all-day
    /// entries and floating times are placed in it. The answer is kept in the cache; when the
    /// server cannot be reached the cache answers and `from_cache` is set (what it holds was
    /// placed in the zone that was asked for at the time). A span that ends before it starts, an
    /// unknown zone or a span longer than the server expands (a year) is `Error::Invalid`.
    pub fn agenda(&self, from: i64, to: i64, time_zone: String) -> Result<Agenda> {
        let account_id = self.account_id()?;
        let zone = jmap::check_span(from, to, &time_zone)?;
        match self.with_connection(|conn| async move { conn.agenda(from, to, zone).await }) {
            Ok((calendars, entries)) => {
                self.store.calendars_replace(&account_id, &calendars)?;
                self.store.agenda_replace(&account_id, from, to, &entries)?;
                Ok(Agenda { entries, from_cache: false })
            }
            Err(e @ (Error::SignInNeeded { .. } | Error::Invalid { .. })) => Err(e),
            Err(_) => {
                let mut entries = self.store.agenda_list(&account_id, from, to)?;
                jmap::sort_agenda(&mut entries, zone);
                Ok(Agenda { entries, from_cache: true })
            }
        }
    }

    /// One event or task in full (network). With `recurrence_id` (that of the [`Occurrence`]) it
    /// is that occurrence of a repeating event: the start (a task: due time), length, title,
    /// place, description and the user's answer as the agenda shows them, an override applied;
    /// `input.repeat` and the rest are the series'. An occurrence the series does not have, or
    /// that is excluded, is `Error::Invalid`.
    pub fn event(&self, event_id: String, recurrence_id: Option<String>) -> Result<EventDetail> {
        let (id, recurrence) = (&event_id, recurrence_id.as_deref());
        self.with_connection(|conn| async move { conn.event_detail(id, recurrence).await })
    }

    /// Creates an event or task (`event_id` is `None`) or changes one, the whole series when
    /// it repeats (network). Returns its id. A title that is blank, a start that is not a
    /// local date-time, an unknown zone, an all-day entry that is not whole days from
    /// midnight, a repeat rule that makes no sense or a calendar that is unknown or cannot be
    /// written is `Error::Invalid` with a message the user can read; a value the user did not change
    /// is not checked again. Moving the start of a repeating event moves its changed and deleted
    /// occurrences with it, and an event in several calendars stays in the others when its
    /// calendar is changed. An event someone else changed while it was being saved is read again
    /// once; a second clash is `Error::Temporary`. What the app cannot edit
    /// (participants, a complex repeat rule, other properties) is left as it is on the server.
    /// A task has no length; its `start` is its due time, empty for a task with no due date.
    pub fn save_event(&self, event_id: Option<String>, input: EventInput) -> Result<String> {
        let account_id = self.account_id()?;
        let (id, input) = (event_id.as_deref(), &input);
        let saved = self.with_connection(|conn| async move { conn.save_event(id, input).await })?;
        self.store.agenda_forget_event(&account_id, &saved)?;
        Ok(saved)
    }

    /// Deletes an event or task: the whole series, or with `recurrence_id` (the `recurrence_id`
    /// of the [`Occurrence`]) that one occurrence only (network). Nobody is told: an invitation
    /// declined with [`Core::respond_to_event`] is.
    pub fn delete_event(&self, event_id: String, recurrence_id: Option<String>) -> Result<()> {
        let account_id = self.account_id()?;
        let (id, recurrence) = (&event_id, recurrence_id.as_deref());
        self.with_connection(|conn| async move { conn.delete_event(id, recurrence).await })?;
        self.store.agenda_forget_event(&account_id, &event_id)
    }

    /// Answers an invitation: `status` is "accepted", "tentative" or "declined". The server
    /// tells the organizer (network). An event the user is not invited to, or another answer,
    /// is `Error::Invalid`.
    pub fn respond_to_event(&self, event_id: String, status: String) -> Result<()> {
        let account_id = self.account_id()?;
        let (id, status) = (&event_id, &status);
        self.with_connection(|conn| async move { conn.respond_to_event(id, status).await })?;
        self.store.agenda_forget_event(&account_id, &event_id)
    }

    /// Marks a task done or not done (network). An event is `Error::Invalid`.
    pub fn set_task_done(&self, event_id: String, done: bool) -> Result<()> {
        let account_id = self.account_id()?;
        let id = &event_id;
        self.with_connection(|conn| async move { conn.set_task_done(id, done).await })?;
        self.store.agenda_forget_event(&account_id, &event_id)
    }
}

// ---- M6: contacts and the away message ----
#[uniffi::export]
impl Core {
    /// The user's address books (network; the cached list when the server cannot be reached).
    pub fn address_books(&self) -> Result<Vec<AddressBook>> {
        let account_id = self.account_id()?;
        match self.with_connection(|conn| async move { conn.address_books().await }) {
            Ok(list) => {
                self.store.address_books_replace(&account_id, &list)?;
                Ok(list)
            }
            Err(e @ (Error::SignInNeeded { .. } | Error::Invalid { .. })) => Err(e),
            Err(e) => match self.store.address_books_list(&account_id)? {
                // An empty cache says nothing: the list was never fetched, or the user has none.
                list if list.is_empty() => Err(e),
                list => Ok(list),
            },
        }
    }

    /// The contacts whose name, organization, mail address or phone number contains `query`
    /// (all of them when it is blank), kept in the cache; from the cache with `from_cache` set
    /// when the server cannot be reached. A blank query lists the server's cards (the call to
    /// refresh the cache with); a search is answered from the cache once a listing is in it,
    /// as a listing downloads every card. Sorted by name without regard to case and accents as
    /// far as the core can tell (see `contacts::fold`: it does not cover other scripts or any
    /// language's own order of letters). Every word of the query must be found; a query of only
    /// digits and phone punctuation also finds a phone number by its digits. A cache that holds
    /// nothing is not an answer: that is the server's error.
    pub fn contacts(&self, query: String) -> Result<ContactList> {
        let account_id = self.account_id()?;
        let cached = !query.trim().is_empty() && self.store.contacts_listed(&account_id);
        let failure = if cached { None } else { self.refresh_contacts(&account_id)? };
        let index = self.store.contact_index(&account_id)?;
        match failure {
            Some(e) if index.is_empty() => Err(e),
            failure => Ok(ContactList { rows: contacts::select(index, &query), from_cache: failure.is_some() }),
        }
    }

    /// One contact in full (network; the cached card when the server cannot be reached). The
    /// contact is remembered as what the form was opened with (see `save_contact`).
    pub fn contact(&self, contact_id: String) -> Result<ContactDetail> {
        let account_id = self.account_id()?;
        let id = &contact_id;
        let fetched =
            self.with_connection(|conn| async move { Ok((conn.contact_card(id).await?, conn.address_books().await?)) });
        let (contact, books) = match fetched {
            Ok((entry, books)) => {
                self.store.contact_card_put(&account_id, &entry)?;
                self.store.address_books_replace(&account_id, &books)?;
                (entry.contact, books)
            }
            Err(e) if jmap::is_no_such_contact(&e) => return Err(self.forget_contact(&account_id, &contact_id, e)),
            Err(e @ (Error::SignInNeeded { .. } | Error::Invalid { .. })) => return Err(e),
            Err(e) => match self.store.contact_card_get(&account_id, &contact_id)? {
                Some(contact) => (contact, self.store.address_books_list(&account_id)?),
                None => return Err(e),
            },
        };
        self.lock().contact_bases.insert(contact_id.clone(), contact.clone());
        let may_write = books.iter().any(|b| b.id == contact.address_book_id && b.may_write);
        Ok(ContactDetail { id: contact_id, contact, may_write })
    }

    /// Creates a contact (`contact_id` is `None`) or changes one (network). Returns its id. A
    /// contact with neither a name nor a mail address, a malformed mail address or an unknown
    /// address book is `Error::Invalid`. What the app does not show stays on the card, and so
    /// does what was changed elsewhere since `contact` opened the form: of the saved contact
    /// only what differs from the one `contact` handed out last is written (a contact never
    /// opened is compared with the card as it is now).
    pub fn save_contact(&self, contact_id: Option<String>, contact: Contact) -> Result<String> {
        let account_id = self.account_id()?;
        let base = contact_id.as_deref().and_then(|id| self.lock().contact_bases.get(id).cloned());
        let (id, contact, base) = (contact_id.as_deref(), &contact, base.as_ref());
        let saved = self.with_connection(|conn| async move {
            let saved = conn.save_contact(id, contact, base).await?;
            // The write is done: a card that cannot be read back must not make it a failure.
            let fresh = conn.contact_card(&saved).await.ok();
            Ok((saved, fresh))
        });
        let (saved, fresh) = match (saved, &contact_id) {
            (Err(e), Some(id)) if jmap::is_no_such_contact(&e) => return Err(self.forget_contact(&account_id, id, e)),
            (saved, _) => saved?,
        };
        self.lock().contact_bases.remove(&saved);
        match fresh {
            Some(entry) => self.store.contact_card_put(&account_id, &entry)?,
            None => self.store.contact_card_forget(&account_id, &saved)?,
        }
        Ok(saved)
    }

    /// Deletes a contact (network).
    pub fn delete_contact(&self, contact_id: String) -> Result<()> {
        let account_id = self.account_id()?;
        let id = &contact_id;
        self.with_connection(|conn| async move { conn.delete_contact(id).await })?;
        self.store.contact_card_forget(&account_id, &contact_id)
    }

    /// The away message as the server has it (network).
    pub fn away_message(&self) -> Result<AwayMessage> {
        self.with_connection(|conn| async move { conn.away_message().await })
    }

    /// Switches the away message on or off and sets its dates and text (network). An end
    /// before the start, or an enabled message without text, is `Error::Invalid`. A subject and
    /// text that are as the server has them are not written, so formatting another client gave
    /// the message stays; a text that is written replaces the HTML version.
    pub fn set_away_message(&self, message: AwayMessage) -> Result<()> {
        let message = &message;
        self.with_connection(|conn| async move { conn.set_away_message(message).await })
    }
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
