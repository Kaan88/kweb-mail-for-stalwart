package ovh.kweb.mail.ui.thread

import android.content.Context
import java.io.File
import uniffi.kweb_core.Core

/**
 * Downloaded blobs under the app's cache directory, which Android may empty when space is short.
 * Names come from a hash of the blob id, never from the mail.
 *
 * - `blobs/<sha256 of blob id>`: pictures that HTML mail shows inline.
 * - `attachments/<sha256 of blob id>/<safe file name>`: attachments, the only place the
 *   `FileProvider` hands to other apps.
 */
class BlobFiles(context: Context) {
    private val cache = context.cacheDir

    fun inlineFile(blobId: String) = File(cache, "blobs/${FileNames.sha256Hex(blobId)}")

    fun attachmentFile(blobId: String, name: String) =
        File(cache, "attachments/${FileNames.sha256Hex(blobId)}/${FileNames.safe(name)}")

    /** [file] with the blob in it: as it is when it is there, else downloaded first. Blocks; call it off the main thread. */
    fun ensure(core: Core, blobId: String, file: File): File {
        if (!file.isFile) {
            file.parentFile?.mkdirs()
            core.downloadBlob(blobId, file.absolutePath)
        }
        return file
    }

    companion object {
        /** Deletes every downloaded blob and attachment (on sign-out). Blocks; call it off the main thread. */
        fun clear(context: Context) {
            File(context.cacheDir, "blobs").deleteRecursively()
            File(context.cacheDir, "attachments").deleteRecursively()
        }
    }
}
