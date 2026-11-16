//! Downloads with progress and a way to stop them (the host's attachment and file rows).
//!
//! The plain [`Core::download_blob`] and [`Core::file_download`] stay; these two report how far a
//! transfer is and can be cancelled from another thread. The listener is called on the thread that
//! called the function, at most about every 150 ms.

use std::sync::Arc;

use super::Core;
use crate::{jmap::DownloadWatch, Result};

/// Told how far a download is. Must return quickly and must not throw.
#[uniffi::export(with_foreign)]
pub trait DownloadListener: Send + Sync {
    /// `done` bytes have arrived of `total`, when the server said how many there are.
    fn progress(&self, done: u64, total: Option<u64>);
}

/// Stops one download: make one, pass it to the download, and call [`DownloadCancel::cancel`] from
/// any thread. One of these serves one download.
#[derive(uniffi::Object, Default)]
pub struct DownloadCancel {
    watch: DownloadWatch,
}

#[uniffi::export]
impl DownloadCancel {
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// The download ends at once (it may take a moment while a piece is being written). Cancelling
    /// after it ended does nothing.
    pub fn cancel(&self) {
        self.watch.cancel();
    }

    pub fn is_cancelled(&self) -> bool {
        self.watch.is_cancelled()
    }
}

impl DownloadCancel {
    /// Resolves once cancelled (for the core's own async code).
    pub(crate) async fn cancelled(&self) {
        self.watch.cancelled().await
    }
}

#[uniffi::export]
impl Core {
    /// As [`Core::download_blob`], reporting progress to `listener` and stoppable with `cancel`.
    /// `true` when the file is complete at `dest_path`; `false` when it was cancelled: then the
    /// part file is gone and nothing is at `dest_path` (a file already there stays as it was).
    pub fn download_blob_tracked(
        &self,
        blob_id: String,
        dest_path: String,
        cancel: Arc<DownloadCancel>,
        listener: Option<Arc<dyn DownloadListener>>,
    ) -> Result<bool> {
        self.download_to_path_tracked(None, &blob_id, &dest_path, &cancel.watch, listener.as_deref())
    }

    /// As [`Core::file_download`], with progress and cancel as [`Core::download_blob_tracked`].
    pub fn file_download_tracked(
        &self,
        account: Option<String>,
        blob_id: String,
        dest_path: String,
        cancel: Arc<DownloadCancel>,
        listener: Option<Arc<dyn DownloadListener>>,
    ) -> Result<bool> {
        self.download_to_path_tracked(
            Some(account.as_deref()),
            &blob_id,
            &dest_path,
            &cancel.watch,
            listener.as_deref(),
        )
    }
}
