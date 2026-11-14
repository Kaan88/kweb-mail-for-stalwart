//! The live figures: the server's own event stream (`GET /api/live/metrics`), read the way the
//! assistant reads its answers (`assistant/sse.rs`), and the names runs are stopped by.

use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex, MutexGuard},
    time::Duration,
};

use serde_json::Value;
use tokio::{sync::Notify, time::timeout};

use super::{LiveFigures, LiveListener};
use crate::{assistant::sse::SseParser, jmap::Connection, Error, Result};

/// The figures asked for, by the server's own names (those of its web dashboard).
const QUEUE: &str = "queue.count";
const USERS: &str = "user.count";
const MEMORY: &str = "server.memory";
const CONNECTIONS: &str = "delivery.active-connections";
/// Seconds between two frames.
const INTERVAL: u64 = 5;
/// How long the server may take to answer and send its first frame (it sends one at once).
const FIRST: Duration = Duration::from_secs(10);
/// How long it may say nothing: three frames missed.
const SILENCE: Duration = Duration::from_secs(15);
/// How many stops for runs that were not running are remembered.
const REMEMBERED: usize = 16;

#[derive(Default)]
struct Runs {
    running: HashMap<String, Arc<Notify>>,
    /// Runs that were told to stop before they began, newest last.
    stopped: VecDeque<String>,
}

/// The runs of `admin_live` and the way to stop each by name, as the assistant's `Slot` does
/// it for its one task. Several runs may exist at once here (one per name), and the names are
/// kept for the whole process, so the table is not a field of `Core`.
static RUNS: Mutex<Option<Runs>> = Mutex::new(None);

fn runs() -> MutexGuard<'static, Option<Runs>> {
    RUNS.lock().unwrap_or_else(|e| e.into_inner())
}

/// A run that holds its name; dropping it frees the name.
pub struct Run {
    name: String,
    pub cancel: Arc<Notify>,
}

impl Drop for Run {
    fn drop(&mut self) {
        if let Some(table) = runs().as_mut() {
            table.running.remove(&self.name);
        }
    }
}

/// Takes the name `run_id`. `Ok(None)` when a stop for it came before it began (the stop is
/// used up); a name that is already running is `Error::Invalid`.
pub fn begin(run_id: &str) -> Result<Option<Run>> {
    let mut guard = runs();
    let table = guard.get_or_insert_with(Runs::default);
    if let Some(at) = table.stopped.iter().position(|id| id == run_id) {
        table.stopped.remove(at);
        return Ok(None);
    }
    if table.running.contains_key(run_id) {
        return Err(Error::invalid("a live view with this name is already running"));
    }
    let cancel = Arc::new(Notify::new());
    table.running.insert(run_id.to_string(), cancel.clone());
    Ok(Some(Run { name: run_id.to_string(), cancel }))
}

/// Stops the run `run_id`; one that is not running is remembered for when it begins. Another
/// run is never touched.
pub fn cancel(run_id: &str) {
    let mut guard = runs();
    let table = guard.get_or_insert_with(Runs::default);
    match table.running.get(run_id) {
        Some(cancel) => cancel.notify_one(),
        None => {
            if !table.stopped.iter().any(|id| id == run_id) {
                if table.stopped.len() == REMEMBERED {
                    table.stopped.pop_front();
                }
                table.stopped.push_back(run_id.to_string());
            }
        }
    }
}

/// The figures one frame carries: a JSON list of `{metric, @type, count}`. `None` for anything
/// else (the stream also carries pings). A figure the frame lacks is `None` in the result.
pub fn figures_from(data: &str) -> Option<LiveFigures> {
    let frame: Value = serde_json::from_str(data).ok()?;
    let rows = frame.as_array()?;
    let mut figures = LiveFigures { queue: None, connections: None, users: None, memory_bytes: None };
    for row in rows {
        let count = row["count"].as_u64().or_else(|| row["count"].as_f64().filter(|c| *c >= 0.0).map(|c| c as u64));
        match row["metric"].as_str() {
            Some(QUEUE) => figures.queue = count,
            Some(USERS) => figures.users = count,
            Some(MEMORY) => figures.memory_bytes = count,
            Some(CONNECTIONS) => figures.connections = count,
            _ => {}
        }
    }
    Some(figures)
}

/// Follows the stream until it ends, breaks or goes quiet; the caller stops it by dropping
/// this future. Never `Ok`: a stream that ends by itself is `Error::Temporary`.
pub async fn follow(conn: &Connection, listener: &dyn LiveListener) -> Result<()> {
    let url = format!(
        "{}/api/live/metrics?metrics={QUEUE},{USERS},{MEMORY},{CONNECTIONS}&interval={INTERVAL}",
        conn.server_origin()?
    );
    // No redirect is followed: the key would go along to wherever the server points.
    let client = reqwest::Client::builder()
        .connect_timeout(FIRST)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| Error::temporary("could not set up the connection"))?;
    let mut key = conn.key_header()?;
    key.set_sensitive(true);
    let request = client
        .get(url)
        .header(reqwest::header::AUTHORIZATION, key)
        .header(reqwest::header::ACCEPT, "text/event-stream");
    let mut response = match timeout(FIRST, request.send()).await {
        Err(_) => return Err(Error::temporary("the server did not answer in time")),
        Ok(Err(e)) if e.is_timeout() => return Err(Error::temporary("the server did not answer in time")),
        Ok(Err(_)) => return Err(Error::temporary("could not reach the server")),
        Ok(Ok(response)) => response,
    };
    match response.status().as_u16() {
        200..=299 => {}
        401 => return Err(crate::ffi::admin_link::key_refused()),
        403 => return Err(Error::invalid("this key may not follow the live figures")),
        404 => return Err(Error::invalid("this server has no live figures")),
        code => return Err(Error::temporary(format!("the server answered {code} for the live figures"))),
    }
    let mut parser = SseParser::default();
    let mut limit = FIRST;
    loop {
        let chunk = match timeout(limit, response.chunk()).await {
            Err(_) => return Err(Error::temporary("the server stopped sending live figures")),
            Ok(Err(_)) => return Err(Error::temporary("the live connection broke")),
            Ok(Ok(None)) => return Err(Error::temporary("the live connection ended")),
            Ok(Ok(Some(chunk))) => chunk,
        };
        limit = SILENCE;
        for data in parser.feed(&chunk)? {
            if let Some(figures) = figures_from(&data) {
                listener.figures(figures);
            }
        }
    }
}
