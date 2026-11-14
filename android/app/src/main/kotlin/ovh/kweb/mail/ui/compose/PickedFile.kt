package ovh.kweb.mail.ui.compose

import android.content.Context
import android.net.Uri
import android.provider.OpenableColumns
import java.io.File
import java.io.IOException
import java.util.UUID
import ovh.kweb.mail.ui.thread.FileNames

/** A file the user picked, copied to the app's cache so the core can read it by path. */
class PickedFile(val file: File, val name: String, val mimeType: String) {
    /** Removes the copy and the folder made for it. */
    fun delete() {
        file.delete()
        file.parentFile?.delete()
    }

    companion object {
        /** The name the provider gives [uri], made safe for a path; "attachment" when it gives none. Blocks. */
        fun displayName(context: Context, uri: Uri): String = FileNames.safe(rawDisplayName(context, uri))

        /** The name the provider gives [uri], as it is ("" when it gives none). Blocks. */
        fun rawDisplayName(context: Context, uri: Uri): String {
            val queried = try {
                context.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use {
                    if (it.moveToFirst()) it.getString(0) else null
                }
            } catch (_: RuntimeException) {
                null
            }
            return queried ?: uri.lastPathSegment.orEmpty()
        }

        /** Copies the content of [uri] to a new file under [root] in the cache. Blocks: call it off the main thread. */
        fun copy(context: Context, uri: Uri, name: String, root: String = "outgoing"): PickedFile {
            val dir = File(context.cacheDir, "$root/${UUID.randomUUID()}")
            if (!dir.mkdirs()) throw IOException("no folder for the copy")
            val file = File(dir, name)
            try {
                val input = context.contentResolver.openInputStream(uri) ?: throw IOException("no input stream")
                input.use { source -> file.outputStream().use { source.copyTo(it) } }
            } catch (e: Exception) {
                file.delete()
                dir.delete()
                throw e
            }
            val mime = try {
                context.contentResolver.getType(uri)?.takeIf { it.isNotBlank() } ?: "application/octet-stream"
            } catch (e: Exception) {
                file.delete()
                dir.delete()
                throw e
            }
            return PickedFile(file, name, mime)
        }
    }
}
