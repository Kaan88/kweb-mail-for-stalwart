package ovh.kweb.mail.ui.transfer

import ovh.kweb.mail.accounts.Accounts
import uniffi.kweb_core.Core
import uniffi.kweb_core.DownloadCancel
import uniffi.kweb_core.DownloadListener

/** The core's downloads with progress and stop, in terms of [DownloadStop]. Every call blocks: run it off the main thread. */
object CoreDownloads {
    /** A mail attachment. True when the file is complete at [dest]; false when [stop] ended it (nothing is left). */
    fun blob(core: Core, blobId: String, dest: String, stop: DownloadStop, onProgress: (Long, Long?) -> Unit): Boolean =
        run(core, TransferHub.Kind.Attachment, stop, onProgress) { cancel, listener -> core.downloadBlobTracked(blobId, dest, cancel, listener) }

    /** A file of the file space [account] (null: the user's own). */
    fun file(core: Core, account: String?, blobId: String, dest: String, stop: DownloadStop, onProgress: (Long, Long?) -> Unit): Boolean =
        run(core, TransferHub.Kind.File, stop, onProgress) { cancel, listener -> core.fileDownloadTracked(account, blobId, dest, cancel, listener) }

    private fun run(
        core: Core,
        kind: TransferHub.Kind,
        stop: DownloadStop,
        onProgress: (Long, Long?) -> Unit,
        call: (DownloadCancel, DownloadListener) -> Boolean,
    ): Boolean {
        // The download is entered with its account, so that the foreground service keeps the process alive while it runs.
        val account = Accounts.openSessions().firstOrNull { it.core === core }?.record?.id
        val entry = account?.let { TransferHub.begin(it, kind, stop) }
        var complete = false
        val cancel = DownloadCancel()
        // The listener is called by the core on this thread; whatever it does must not throw into the core.
        val listener = object : DownloadListener {
            override fun progress(done: ULong, total: ULong?) {
                try {
                    entry?.let { TransferHub.report(it, done.toLong(), total?.toLong()) }
                    onProgress(done.toLong(), total?.toLong())
                } catch (_: Exception) {
                }
            }
        }
        stop.attach { cancel.cancel() }
        try {
            complete = call(cancel, listener)
            return complete
        } finally {
            entry?.let { TransferHub.end(it, complete) }
            // No stop may reach the core object after this point.
            stop.attach(null)
            cancel.close()
        }
    }
}
