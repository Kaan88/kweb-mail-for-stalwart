//! The loop itself: connect, hold the stream, run passes, back off, renew tokens.

use std::{
    convert::Infallible,
    sync::{Arc, Mutex},
    time::Duration,
};

use futures_util::StreamExt;
use tokio::{
    sync::Notify,
    time::{sleep, sleep_until, Instant},
};

use super::{now_secs, pass, Deps, Listener, Timing};
use crate::{
    jmap::{is_unauthorized, Connection, Event, EventStream},
    model::ConnectionStatus,
    notify::OnScreen,
    Error, Result,
};

/// Remembers the last status and only passes on changes. The listener is called with no lock
/// held, so it may call `Handle::status` from inside the callback.
pub(super) struct Reporter {
    listener: Arc<dyn Listener>,
    last: Mutex<Option<ConnectionStatus>>,
}

impl Reporter {
    pub(super) fn new(listener: Arc<dyn Listener>) -> Self {
        Reporter { listener, last: Mutex::new(None) }
    }

    pub(super) fn current(&self) -> ConnectionStatus {
        self.last.lock().unwrap().clone().unwrap_or(ConnectionStatus::Connecting)
    }

    fn set(&self, status: ConnectionStatus) {
        let changed = {
            let mut last = self.last.lock().unwrap();
            let changed = last.as_ref() != Some(&status);
            *last = Some(status.clone());
            changed
        };
        if changed {
            self.listener.status(status);
        }
    }
}

/// Retry pacing, reset once a session is up.
struct Health {
    timing: Timing,
    backoff: Duration,
    /// 401s in a row: a token the server keeps refusing must not be renewed in a tight loop.
    auth_failures: u32,
}

impl Health {
    fn new(timing: Timing) -> Self {
        Health { backoff: timing.first_backoff, auth_failures: 0, timing }
    }

    fn ok(&mut self) {
        self.backoff = self.timing.first_backoff;
        self.auth_failures = 0;
    }

    fn next_backoff(&mut self) -> Duration {
        let wait = self.backoff;
        self.backoff = (self.backoff * 2).min(self.timing.max_backoff);
        wait
    }
}

struct Runner {
    deps: Deps,
    wake: Arc<Notify>,
    reporter: Arc<Reporter>,
}

pub(super) async fn run(deps: Deps, wake: Arc<Notify>, reporter: Arc<Reporter>) {
    let mut health = Health::new(deps.timing.clone());
    let runner = Runner { deps, wake, reporter };
    loop {
        runner.reporter.set(ConnectionStatus::Connecting);
        let err = runner.session(&mut health).await;
        runner.after_failure(err, &mut health).await;
    }
}

enum Next {
    Item(Option<Result<Event>>),
    Rearm,
    Renew,
    Poke,
}

impl Runner {
    /// Runs until something fails; it never ends on its own.
    async fn session(&self, health: &mut Health) -> Error {
        match self.session_inner(health).await {
            Err(e) => e,
            Ok(never) => match never {},
        }
    }

    async fn session_inner(&self, health: &mut Health) -> Result<Infallible> {
        let mut token = self.deps.tokens.access_token().await?;
        let mut conn = Connection::connect(&self.deps.account.server_url, &token.token).await?;
        // A pass before the stream (it finds the Inbox and the mail already there) and one
        // after (whatever changed between the first pass and the stream being armed).
        self.pass(&conn).await?;
        let mut stream = conn.events().await?;
        self.reporter.set(ConnectionStatus::Live { last_event_at: now_secs() });
        health.ok();
        self.pass(&conn).await?;

        let mut rearm_at = Instant::now() + self.deps.timing.rearm_after;
        loop {
            let next = tokio::select! {
                item = stream.next() => Next::Item(item),
                _ = sleep_until(rearm_at) => Next::Rearm,
                _ = sleep_until(token.renew_at.into()) => Next::Renew,
                _ = self.wake.notified() => Next::Poke,
            };
            match next {
                Next::Item(None) => return Err(Error::temporary("push stream closed")),
                Next::Item(Some(Err(e))) => return Err(e),
                Next::Item(Some(Ok(event))) => {
                    rearm_at = Instant::now() + self.deps.timing.rearm_after;
                    if self.on_event(event) {
                        self.settle(&mut stream).await?;
                        self.pass(&conn).await?;
                    }
                }
                Next::Rearm => {
                    // The new stream is open before the old one is dropped, so nothing falls
                    // in between; the pass covers the moment of the swap.
                    stream = conn.events().await?;
                    rearm_at = Instant::now() + self.deps.timing.rearm_after;
                    self.pass(&conn).await?;
                }
                Next::Renew => {
                    token = self.deps.tokens.access_token().await?;
                    conn = Connection::connect(&self.deps.account.server_url, &token.token).await?;
                    stream = conn.events().await?;
                    rearm_at = Instant::now() + self.deps.timing.rearm_after;
                    self.pass(&conn).await?;
                }
                Next::Poke => self.pass(&conn).await?,
            }
        }
    }

    async fn pass(&self, conn: &Connection) -> Result<()> {
        pass(&self.deps, conn, OnScreen::Unknown).await.map(drop)
    }

    /// Handles one event. True when a pass is due.
    fn on_event(&self, event: Event) -> bool {
        self.reporter.set(ConnectionStatus::Live { last_event_at: now_secs() });
        match event {
            Event::StateChange { .. } => true,
            Event::CalendarAlert { event_id, alert_id, recurrence_id } => {
                self.deps.listener.calendar_alert(event_id, alert_id, recurrence_id);
                false
            }
        }
    }

    /// Lets a burst of state changes settle, so it costs one pass. The window is fixed from
    /// the first event: a steady trickle must not keep the pass from ever running.
    async fn settle(&self, stream: &mut EventStream) -> Result<()> {
        let until = Instant::now() + self.deps.timing.settle;
        loop {
            tokio::select! {
                item = stream.next() => match item {
                    None => return Err(Error::temporary("push stream closed")),
                    Some(Err(e)) => return Err(e),
                    Some(Ok(event)) => {
                        self.on_event(event);
                    }
                },
                _ = sleep_until(until) => return Ok(()),
            }
        }
    }

    async fn after_failure(&self, err: Error, health: &mut Health) {
        if matches!(err, Error::SignInNeeded { .. }) {
            return self.wait_for_sign_in(health).await;
        }
        if is_unauthorized(&err) {
            return self.after_refused_token(health).await;
        }
        self.reporter.set(ConnectionStatus::Offline { reason: err.to_string() });
        let wait = health.next_backoff();
        self.wait(wait, health).await;
    }

    /// The server refused the access token: drop it so the next turn renews it. The first
    /// refusal renews quickly; repeats wait like any failure; too many mean the grant is bad.
    async fn after_refused_token(&self, health: &mut Health) {
        health.auth_failures += 1;
        self.deps.tokens.invalidate().await;
        if health.auth_failures >= health.timing.max_auth_failures {
            return self.wait_for_sign_in(health).await;
        }
        let wait = if health.auth_failures == 1 { health.timing.first_auth_retry } else { health.next_backoff() };
        self.wait(wait, health).await;
    }

    /// Waits until the host pokes (the user signed in again, or the app came to the foreground).
    async fn wait_for_sign_in(&self, health: &mut Health) {
        self.reporter.set(ConnectionStatus::SignInNeeded);
        self.wake.notified().await;
        health.ok();
    }

    /// A poke ends the wait and starts over with the short backoff.
    async fn wait(&self, wait: Duration, health: &mut Health) {
        tokio::select! {
            _ = sleep(wait) => {}
            _ = self.wake.notified() => health.ok(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_doubles_up_to_the_cap_and_resets() {
        let timing = Timing {
            first_backoff: Duration::from_secs(5),
            max_backoff: Duration::from_secs(300),
            ..Timing::default()
        };
        let mut h = Health::new(timing);
        let waits: Vec<u64> = (0..8).map(|_| h.next_backoff().as_secs()).collect();
        assert_eq!(waits, [5, 10, 20, 40, 80, 160, 300, 300]);
        h.auth_failures = 3;
        h.ok();
        assert_eq!(h.next_backoff().as_secs(), 5);
        assert_eq!(h.auth_failures, 0);
    }

    #[test]
    fn production_timings() {
        let t = Timing::default();
        assert_eq!(t.first_backoff, Duration::from_secs(5));
        assert_eq!(t.max_backoff, Duration::from_secs(300));
        assert_eq!(t.rearm_after, Duration::from_secs(300));
        assert_eq!(t.settle, Duration::from_millis(300));
        assert_eq!(t.max_auth_failures, 4);
    }
}
