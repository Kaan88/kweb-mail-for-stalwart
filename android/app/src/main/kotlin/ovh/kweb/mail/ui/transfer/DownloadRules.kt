package ovh.kweb.mail.ui.transfer

import java.util.Locale
import ovh.kweb.mail.ui.thread.SizeFormat
import uniffi.kweb_core.Exception as CoreException

/** How far a download is: [total] is null when neither the server nor the list says how big the file is. */
data class DownloadProgress(val done: Long, val total: Long?)

/** What the screen says about a download that failed. */
sealed interface DownloadFailure {
    /** The server could not be reached at all: "you may be offline" is true. */
    data object Offline : DownloadFailure

    /** The core's own words for what went wrong (the server's answer, the disk). */
    data class Words(val text: String) : DownloadFailure

    data object SignIn : DownloadFailure

    /** Nothing useful is known. */
    data object Unknown : DownloadFailure
}

/** Pure decisions about downloads (what to show, how to word a failure). */
object DownloadRules {
    /** How the core words a download that could not even connect (`jmap::DOWNLOAD_OFFLINE`). */
    const val OFFLINE_DOWNLOAD = "download: could not reach the server"

    /** How the core words a failed connection to the server before any download (jmap-client's own text). */
    const val OFFLINE_SESSION = "Transport error"

    private const val DOWNLOAD_PREFIX = "download: "

    /** The bar's fill from 0 to 1, or null when it cannot be told (an indeterminate bar). */
    fun fraction(progress: DownloadProgress): Float? {
        val total = progress.total ?: return null
        if (total <= 0) return null
        return (progress.done.toDouble() / total).coerceIn(0.0, 1.0).toFloat()
    }

    /** "12.3 MB of 45.0 MB", or just "12.3 MB" when the size is not known. */
    fun line(progress: DownloadProgress, locale: Locale, of: (String, String) -> String): String {
        val done = SizeFormat.format(progress.done, locale)
        val total = progress.total?.takeIf { it > 0 } ?: return done
        return of(done, SizeFormat.format(total, locale))
    }

    /** The size to expect before the core has said: a positive size the list or the message gave. */
    fun expected(size: Long?): Long? = size?.takeIf { it > 0 }

    fun describe(error: Throwable): DownloadFailure = when {
        error is CoreException.SignInNeeded -> DownloadFailure.SignIn
        error is CoreException.Temporary && isOffline(error.msg) -> DownloadFailure.Offline
        error is CoreException.Temporary && error.msg.isNotBlank() -> DownloadFailure.Words(plain(error.msg))
        error is CoreException.Invalid && error.msg.isNotBlank() -> DownloadFailure.Words(plain(error.msg))
        error is CoreException.Store && error.msg.isNotBlank() -> DownloadFailure.Words(plain(error.msg))
        else -> DownloadFailure.Unknown
    }

    fun isOffline(message: String): Boolean = message.startsWith(OFFLINE_DOWNLOAD) || message.startsWith(OFFLINE_SESSION)

    /** The core's words without their "download: " lead, which the screen's own sentence replaces. */
    private fun plain(message: String): String = message.removePrefix(DOWNLOAD_PREFIX).trim()
}
