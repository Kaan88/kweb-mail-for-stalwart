//! The long-running loop that keeps the phone's notifications correct.
//!
//! [`spawn`] holds a JMAP EventSource stream open and, whenever something may have changed
//! (the session came up, the server pushed a state change, the stream was re-armed, the token
//! was renewed, the host called [`Handle::poke`]), runs one *pass*: send queued actions, sync
//! the cache, run the notification rule and tell the [`Listener`] what to post and cancel.
//!
//! [`check_once`] is the same pass without a stream, for the 15-minute job, the boot receiver
//! and "the app came to the foreground".
//!
//! Ported from the desktop watcher in Postern: backoff, token renewal, re-arming a quiet
//! stream and the catch-up passes are the same; only the work done in a pass differs.

mod run;

use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use tokio::{sync::Notify, task::JoinHandle};

use crate::{
    actions,
    auth::Tokens,
    jmap::{is_unauthorized, Connection},
    model::{Account, ConnectionStatus, Reminder},
    notify::{self, OnScreen, Plan},
    store::Store,
    sync, Error, Result,
};

/// Implemented by the host (Android service, cli). Called from the loop's task; must return quickly.
#[uniffi::export(with_foreign)]
pub trait Listener: Send + Sync {
    fn status(&self, status: ConnectionStatus);
    /// Post and cancel these. Never empty.
    fn notifications(&self, plan: notify::Plan);
    /// The cache changed: lists on screen should reload.
    fn cache_changed(&self);
    /// The server fired a calendar or task reminder: show it now.
    fn reminder(&self, reminder: Reminder);
}

/// Timings, so tests can run in milliseconds. `Default` gives the production values.
#[derive(Debug, Clone)]
pub struct Timing {
    /// First wait after a failure; doubles each time, reset once a session is up.
    pub first_backoff: Duration,
    pub max_backoff: Duration,
    /// A stream that has been quiet this long is replaced: jmap-client does not surface the
    /// server's pings, so a connection that died silently cannot be told from a calm one.
    pub rearm_after: Duration,
    /// After a state change, wait this long for the rest of the burst (a delivery touches
    /// Email and Mailbox) so that it costs one pass.
    pub settle: Duration,
    /// Wait before renewing after the first 401 in a row; later ones back off.
    pub first_auth_retry: Duration,
    /// After this many 401s in a row, each with a freshly renewed token, ask for a sign-in.
    pub max_auth_failures: u32,
}

impl Default for Timing {
    fn default() -> Self {
        Timing {
            first_backoff: Duration::from_secs(5),
            max_backoff: Duration::from_secs(300),
            rearm_after: Duration::from_secs(300),
            settle: Duration::from_millis(300),
            first_auth_retry: Duration::from_secs(2),
            max_auth_failures: 4,
        }
    }
}

#[derive(Clone)]
pub struct Deps {
    pub store: Arc<Store>,
    pub tokens: Tokens,
    pub account: Account,
    pub listener: Arc<dyn Listener>,
    pub timing: Timing,
}

/// One pass without a stream: token, connect, send queued actions, sync, run the rule, report
/// the plan to the listener (if it is not empty) and return it. A 401 is retried once with a
/// renewed token.
pub async fn check_once(deps: &Deps, on_screen: OnScreen<'_>) -> Result<Plan> {
    match attempt(deps, on_screen).await {
        Err(e) if is_unauthorized(&e) => {
            deps.tokens.invalidate().await;
            attempt(deps, on_screen).await
        }
        other => other,
    }
}

async fn attempt(deps: &Deps, on_screen: OnScreen<'_>) -> Result<Plan> {
    let token = deps.tokens.access_token().await?;
    let conn = Connection::connect(&deps.account.server_url, &token.token).await?;
    pass(deps, &conn, on_screen).await
}

/// The work of one pass, shared by the loop and [`check_once`].
///
/// A queued action that cannot be sent right now does not stop the sync and the rule: the
/// user still has to see new mail. A 401 does stop it (the caller renews the token), and so
/// does a lost grant.
async fn pass(deps: &Deps, conn: &Connection, on_screen: OnScreen<'_>) -> Result<Plan> {
    let _one_at_a_time = deps.store.pass_lock().lock().await;
    let id = &deps.account.id;
    let sent = match actions::flush(conn, &deps.store, id).await {
        Ok(n) => n,
        Err(e @ Error::SignInNeeded { .. }) => return Err(e),
        Err(e) if is_unauthorized(&e) => return Err(e),
        Err(_) => 0,
    };
    let outcome = sync::sync(conn, &deps.store, id).await?;
    if outcome.changed || sent > 0 {
        deps.listener.cache_changed();
    }
    let plan = notify::run(&deps.store, &deps.account, on_screen, now_secs())?;
    if !plan.is_empty() {
        deps.listener.notifications(plan.clone());
    }
    Ok(plan)
}

fn now_secs() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Starts the loop on the current tokio runtime.
pub fn spawn(deps: Deps) -> Handle {
    let wake = Arc::new(Notify::new());
    let reporter = Arc::new(run::Reporter::new(deps.listener.clone()));
    let task = tokio::spawn(run::run(deps, wake.clone(), reporter.clone()));
    Handle { wake, reporter, task }
}

/// The running loop. Dropping it ends the loop too.
pub struct Handle {
    wake: Arc<Notify>,
    reporter: Arc<run::Reporter>,
    task: JoinHandle<()>,
}

impl Handle {
    /// Run a pass now (an action was queued, the app came to the foreground). Also ends a
    /// back-off wait, and asks again after a "sign-in needed".
    pub fn poke(&self) {
        // One permit is kept, so pokes during a pass or a wait coalesce into one more pass.
        self.wake.notify_one();
    }

    /// Stop the loop and wait until it has ended.
    pub async fn stop(mut self) {
        self.task.abort();
        let _ = (&mut self.task).await;
    }

    /// The last status reported to the listener.
    pub fn status(&self) -> ConnectionStatus {
        self.reporter.current()
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        self.task.abort();
    }
}
