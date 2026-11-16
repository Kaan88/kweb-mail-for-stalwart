//! Blob downloads: streamed to a file piece by piece, with progress and a way to stop.
//!
//! What can go wrong is told in the app's own words: a server that cannot be reached at all
//! ([`OFFLINE`]) is told from one that was reached and then stopped, refused or failed, because
//! the host says "you may be offline" only for the first.

use std::{
    io::Write,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

use tokio::sync::Notify;

use super::{errors, Connection};
use crate::{Error, Result};

/// The start of the message of a download that could not even connect. The host shows its
/// "you may be offline" wording for exactly this.
pub const OFFLINE: &str = "download: could not reach the server";
/// How often progress is reported at most (the host redraws on each report).
const REPORT_EVERY: Duration = Duration::from_millis(150);

/// A way to stop a download from another thread: [`DownloadWatch::cancel`] ends the transfer at
/// its next piece (or at once while it waits for the server).
#[derive(Default)]
pub struct DownloadWatch {
    cancelled: AtomicBool,
    wake: Notify,
}

impl DownloadWatch {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        // One permit is kept when nobody waits yet, so a cancel is never missed.
        self.wake.notify_one();
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    pub(crate) async fn cancelled(&self) {
        if !self.is_cancelled() {
            self.wake.notified().await;
        }
    }
}

/// How a transfer ended when it did not fail.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Outcome {
    Complete,
    /// Stopped on request; the file holds what had arrived and is the caller's to remove.
    Cancelled,
}

fn transport(e: &reqwest::Error) -> Error {
    if e.is_connect() {
        Error::temporary(format!("{OFFLINE} ({})", short(e)))
    } else if e.is_timeout() {
        Error::temporary("download: the server stopped sending")
    } else {
        Error::temporary(format!("download: the connection broke ({})", short(e)))
    }
}

/// The kind of failure without the address it was about.
fn short(e: &reqwest::Error) -> &'static str {
    if e.is_connect() {
        "connection refused or no route"
    } else if e.is_body() || e.is_decode() {
        "while receiving"
    } else if e.is_request() {
        "while asking"
    } else {
        "unknown"
    }
}

impl Connection {
    /// Like [`Connection::download_from`], reporting `(bytes so far, total)` to `progress` (the total
    /// when the server says it) at the start, now and then, and at the end, and giving up when
    /// `watch` is cancelled. A short transfer, where the server said how long it would be, is an error.
    pub async fn download_tracked(
        &self,
        account: &str,
        blob_id: &str,
        file: &mut dyn Write,
        watch: &DownloadWatch,
        progress: &mut dyn FnMut(u64, Option<u64>),
    ) -> Result<Outcome> {
        if watch.is_cancelled() {
            return Ok(Outcome::Cancelled);
        }
        let mut headers = self.client.headers().clone();
        headers.remove(reqwest::header::CONTENT_TYPE);
        let request = self.downloads.get(self.download_url(account, blob_id)).headers(headers).send();
        let mut response = tokio::select! {
            sent = request => sent.map_err(|e| transport(&e))?,
            () = watch.cancelled() => return Ok(Outcome::Cancelled),
        };
        match response.status().as_u16() {
            200..=299 => {}
            401 => return Err(errors::unauthorized("download")),
            404 => return Err(Error::invalid("the server does not have this file")),
            status => return Err(Error::temporary(format!("download: the server answered {status}"))),
        }
        let total = response.content_length();
        let mut done: u64 = 0;
        progress(0, total);
        let mut last = Instant::now();
        loop {
            let chunk = tokio::select! {
                chunk = response.chunk() => chunk.map_err(|e| transport(&e))?,
                () = watch.cancelled() => return Ok(Outcome::Cancelled),
            };
            let Some(chunk) = chunk else { break };
            file.write_all(&chunk).map_err(|e| Error::store(format!("saving the file: {e}")))?;
            done += chunk.len() as u64;
            if last.elapsed() >= REPORT_EVERY {
                progress(done, total);
                last = Instant::now();
            }
        }
        if let Some(total) = total.filter(|t| *t != done) {
            return Err(Error::temporary(format!("download: the server stopped after {done} of {total} bytes")));
        }
        progress(done, total);
        Ok(Outcome::Complete)
    }
}
