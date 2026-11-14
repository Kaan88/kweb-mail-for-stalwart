package ovh.kweb.mail.ui.files

import android.content.Context
import android.provider.DocumentsContract
import android.provider.OpenableColumns
import android.util.Log
import androidx.core.net.toUri
import java.io.File
import java.io.IOException
import ovh.kweb.mail.background.Background
import ovh.kweb.mail.ui.compose.PickedFile
import ovh.kweb.mail.ui.thread.BlobFiles
import ovh.kweb.mail.ui.thread.FileNames
import uniffi.kweb_core.FileEntry

class AndroidFilesPlatform(context: Context) : FilesPlatform {
    private val context = context.applicationContext
    private val blobs = BlobFiles(this.context)

    private companion object {
        /** Copies for upload, apart from the mail composer's own "outgoing", so a sweep never touches a draft's files. */
        const val UPLOAD_ROOT = "outgoing-files"
    }

    override fun cacheFile(account: String?, entry: FileEntry): File =
        blobs.attachmentFile("file:${account.orEmpty()}:${entry.blobId.orEmpty()}", entry.name)

    override fun describe(item: String): UploadInfo {
        val uri = item.toUri()
        val size = try {
            context.contentResolver.query(uri, arrayOf(OpenableColumns.SIZE), null, null, null)?.use {
                if (it.moveToFirst() && !it.isNull(0)) it.getLong(0) else null
            }
        } catch (_: RuntimeException) {
            null
        }
        return UploadInfo(FileRules.serverName(PickedFile.rawDisplayName(context, uri)), size)
    }

    override fun copyForUpload(item: String, name: String): PickedUpload {
        // [name] is the name on the server; the copy's own file name only has to be safe for a path.
        val picked = PickedFile.copy(context, item.toUri(), FileNames.safe(name), UPLOAD_ROOT)
        return PickedUpload(picked.file.absolutePath, picked.mimeType, picked.file.length(), picked::delete)
    }

    override fun sweepLeftovers() {
        File(context.cacheDir, UPLOAD_ROOT).deleteRecursively()
    }

    override fun copyTo(source: File, target: String) {
        val out = context.contentResolver.openOutputStream(target.toUri()) ?: throw IOException("no output stream")
        out.use { sink -> source.inputStream().use { it.copyTo(sink) } }
    }

    override fun discard(target: String) {
        try {
            DocumentsContract.deleteDocument(context.contentResolver, target.toUri())
        } catch (e: Exception) {
            Log.w(Background.TAG, "removing the empty document failed: ${e.javaClass.simpleName}")
        }
    }
}
