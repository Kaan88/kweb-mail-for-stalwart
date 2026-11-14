package ovh.kweb.mail.ui.files

/**
 * The steps of an upload of several files, one after the other. [index] is the file being sent. Stopping lets that
 * one finish and sends none after it.
 */
data class UploadQueue<T>(val items: List<T>, val index: Int = 0, val stopped: Boolean = false) {
    val total: Int get() = items.size

    /** The file to send now; null when all are done or the rest was stopped. */
    val current: T? get() = if (stopped) null else items.getOrNull(index)

    /** "2 of 5" counts from 1 and never goes past the total. */
    val position: Int get() = if (total == 0) 0 else minOf(index + 1, total)

    fun advance(): UploadQueue<T> = copy(index = index + 1)

    fun stop(): UploadQueue<T> = copy(stopped = true)
}

enum class UploadOutcome { Uploaded, Skipped, Failed }

/** What happened to the files of one upload. */
data class UploadTally(val uploaded: Int = 0, val skipped: Int = 0, val failed: Int = 0) {
    fun record(outcome: UploadOutcome): UploadTally = when (outcome) {
        UploadOutcome.Uploaded -> copy(uploaded = uploaded + 1)
        UploadOutcome.Skipped -> copy(skipped = skipped + 1)
        UploadOutcome.Failed -> copy(failed = failed + 1)
    }

    val handled: Int get() = uploaded + skipped + failed

    /** Files that were never tried because the rest was stopped. */
    fun notSent(total: Int): Int = (total - handled).coerceAtLeast(0)
}
