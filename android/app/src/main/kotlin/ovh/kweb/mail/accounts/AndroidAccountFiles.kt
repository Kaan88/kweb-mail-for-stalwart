package ovh.kweb.mail.accounts

import android.content.Context
import java.io.File

/** [AccountFiles] on the app's own directories. */
class AndroidAccountFiles(context: Context) : AccountFiles {
    private val app = context.applicationContext

    override fun legacyCacheExists(): Boolean = File(app.filesDir, AccountPaths.cache("", true)).exists()

    override fun clearBlobCaches() {
        File(app.cacheDir, "blobs").deleteRecursively()
        File(app.cacheDir, "attachments").deleteRecursively()
    }

    override fun deleteStorage(entry: DiscardEntry) {
        val (id, legacy) = entry
        if (legacy) {
            File(app.filesDir, AccountPaths.cache(id, true)).delete()
            AccountPaths.cacheSideFiles(id, true).forEach { File(app.filesDir, it).delete() }
        } else {
            AccountPaths.directory(id, false)?.let { File(app.filesDir, it).deleteRecursively() }
        }
        app.deleteSharedPreferences(AccountPaths.secretsPrefs(id, legacy))
        // The old file of the switches stays: the old app's sign-out kept them, and the next account added takes them over.
        if (!legacy) app.deleteSharedPreferences(AccountPaths.settingsPrefs(id, false))
        File(app.cacheDir, AccountPaths.blobs(id)).deleteRecursively()
        File(app.cacheDir, AccountPaths.attachments(id)).deleteRecursively()
    }
}
