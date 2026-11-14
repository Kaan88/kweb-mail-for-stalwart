package ovh.kweb.mail.ui.thread

import android.content.Context
import java.io.File
import ovh.kweb.mail.accounts.AccountPaths
import uniffi.kweb_core.Core

/**
 * Downloaded blobs under the app's cache directory, which Android may empty when space is short.
 * Names come from a hash of the blob id, never from the mail. A blob of another user's account (shared
 * with the user) is keyed by the account too, since blob ids are only unique within one account.
 *
 * - `blobs/<account id>/<sha256 of blob id>`: pictures that HTML mail shows inline.
 * - `attachments/<account id>/<sha256 of blob id>/<safe file name>`: attachments, the only place the
 *   `FileProvider` hands to other apps.
 *
 * Each account has its own directory (blob ids repeat between accounts).
 */
class BlobFiles(context: Context, accountId: String) {
    private val cache = context.cacheDir
    private val blobs = AccountPaths.blobs(accountId)
    private val attachments = AccountPaths.attachments(accountId)

    fun inlineFile(blobId: String, account: String? = null) = File(cache, "$blobs/${FileNames.sha256Hex(FileNames.blobKey(account, blobId))}")

    fun attachmentFile(blobId: String, name: String, account: String? = null) =
        File(cache, "$attachments/${FileNames.sha256Hex(FileNames.blobKey(account, blobId))}/${FileNames.safe(name)}")

    /**
     * [file] with the blob in it: as it is when it is there, else downloaded first. [account] is the owner's
     * account id for a blob of a shared account, null for the user's own. Blocks; call it off the main thread.
     */
    fun ensure(core: Core, blobId: String, file: File, account: String? = null): File {
        if (!file.isFile) {
            file.parentFile?.mkdirs()
            if (account == null) core.downloadBlob(blobId, file.absolutePath) else core.fileDownload(account, blobId, file.absolutePath)
        }
        return file
    }

    companion object {
        /** Deletes every downloaded blob and attachment of one account (on removal). Blocks; call it off the main thread. */
        fun clear(context: Context, accountId: String) {
            File(context.cacheDir, AccountPaths.blobs(accountId)).deleteRecursively()
            File(context.cacheDir, AccountPaths.attachments(accountId)).deleteRecursively()
        }
    }
}
