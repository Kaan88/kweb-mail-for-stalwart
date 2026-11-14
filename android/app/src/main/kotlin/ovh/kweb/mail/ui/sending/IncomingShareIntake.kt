package ovh.kweb.mail.ui.sending

import android.app.Application
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.util.Log
import kotlinx.coroutines.CancellationException
import ovh.kweb.mail.background.Background
import uniffi.kweb_core.Core
import ovh.kweb.mail.ui.compose.PickedFile
import uniffi.kweb_core.Draft
import uniffi.kweb_core.DraftKind
import uniffi.kweb_core.Exception as CoreException

/** What starting a message from a share asks of the core. Every call blocks. The tests use a fake. */
interface ShareBackend {
    fun newDraft(): Draft
    fun saveDraft(draft: Draft): Draft
    fun addAttachment(draftId: String, path: String, name: String, mimeType: String): Draft
    fun discardDraft(draftId: String)
}

/** Copies a shared file out of its content address into the app's cache; blocks. */
interface ShareFiles {
    fun copy(uri: String): PickedFile
}

class CoreShareBackend(private val core: Core) : ShareBackend {
    override fun newDraft() = core.newDraft(DraftKind.New)
    override fun saveDraft(draft: Draft) = core.saveDraft(draft)
    override fun addAttachment(draftId: String, path: String, name: String, mimeType: String) =
        core.addAttachment(draftId, path, name, mimeType)

    override fun discardDraft(draftId: String) = core.discardDraft(draftId)
}

/** The same path an attached file takes: the cache's "outgoing" folder, one folder per copy, removed once the core has it. */
class AndroidShareFiles(private val context: Context) : ShareFiles {
    override fun copy(uri: String): PickedFile {
        val parsed = Uri.parse(uri)
        return PickedFile.copy(context, parsed, PickedFile.displayName(context, parsed))
    }
}

/** How starting a message from a share ended. */
sealed interface ShareOutcome {
    /** The draft [draftId] is stored with what could be put in it; [left] files did not make it. */
    data class Ready(val draftId: String, val attached: Int, val left: Int) : ShareOutcome

    /** No draft was made (or it could not take the text): nothing is left behind. */
    data object Failed : ShareOutcome
}

/**
 * Starts a new draft from a share: the text in the body, a shared subject in the subject, each file copied
 * out of its content address at once (the permission to read it is only held for a while) and attached through
 * the core's own path. Copies are removed whatever happens. Blocks: call off the main thread.
 */
class IncomingShareIntake(private val backend: ShareBackend, private val files: ShareFiles) {
    fun run(content: SharedContent): ShareOutcome {
        val copies = ArrayList<PickedFile>()
        var unreadable = content.skipped
        try {
            for (uri in content.files) {
                try {
                    copies += files.copy(uri)
                } catch (e: CancellationException) {
                    throw e
                } catch (e: Exception) {
                    unreadable++
                }
            }
            val started = try {
                backend.newDraft()
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                return ShareOutcome.Failed
            }
            val withText = started.copy(subject = content.subject.ifEmpty { started.subject }, body = IncomingShareRules.body(content.body, started.body))
            if (withText != started) {
                try {
                    backend.saveDraft(withText)
                } catch (e: CancellationException) {
                    throw e
                } catch (e: Exception) {
                    runCatching { backend.discardDraft(started.id) }
                    return ShareOutcome.Failed
                }
            }
            var attached = 0
            var left = unreadable
            for (copy in copies) {
                try {
                    backend.addAttachment(started.id, copy.file.path, copy.name, copy.mimeType)
                    attached++
                } catch (e: CancellationException) {
                    throw e
                } catch (e: CoreException.Invalid) {
                    left++
                } catch (e: Exception) {
                    left++
                }
            }
            return ShareOutcome.Ready(started.id, attached, left)
        } finally {
            copies.forEach { runCatching { it.delete() } }
        }
    }
}

/** Reads the parts of a share out of an intent. */
object IntentShare {
    /** The parts of [intent] when it is a share from another app, else null. Streams are read from the extra and from the clip. */
    fun read(intent: Intent): RawShare? {
        val action = intent.action
        if (!IncomingShareRules.isShare(action)) return null
        val streams = ArrayList<String>()
        try {
            @Suppress("DEPRECATION")
            if (action == IncomingShareRules.ACTION_SEND) {
                val one = if (Build.VERSION.SDK_INT >= 33) intent.getParcelableExtra(Intent.EXTRA_STREAM, Uri::class.java) else intent.getParcelableExtra<Uri>(Intent.EXTRA_STREAM)
                if (one != null) streams += one.toString()
            } else {
                val many = if (Build.VERSION.SDK_INT >= 33) {
                    intent.getParcelableArrayListExtra(Intent.EXTRA_STREAM, Uri::class.java)
                } else {
                    intent.getParcelableArrayListExtra<Uri>(Intent.EXTRA_STREAM)
                }
                many?.forEach { streams += it.toString() }
            }
            val clip = intent.clipData
            if (clip != null) for (i in 0 until clip.itemCount) clip.getItemAt(i).uri?.let { streams += it.toString() }
        } catch (e: RuntimeException) {
            // A bundle that cannot be read (a foreign class) is a share without files.
            Log.w(Background.TAG, "reading the files of a share failed: ${e.javaClass.simpleName}")
        }
        val text = try {
            intent.getCharSequenceExtra(Intent.EXTRA_TEXT)?.toString()
        } catch (e: RuntimeException) {
            null
        }
        val subject = try {
            intent.getStringExtra(Intent.EXTRA_SUBJECT)
        } catch (e: RuntimeException) {
            null
        }
        return RawShare(action, text, subject, streams)
    }

    /** The authority of the app's own file provider (declared in the manifest as `${applicationId}.files`). */
    fun ownAuthority(app: Application) = "${app.packageName}.files"
}
