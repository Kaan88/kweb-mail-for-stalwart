package ovh.kweb.mail.ui.sending

/** What an intent of another app carries, as plain values (see [IncomingShareRules.parse]). */
data class RawShare(val action: String?, val text: String?, val subject: String?, val streams: List<String>)

/** What a new message is started from: [skipped] shared files were unusable and are not in [files]. */
data class SharedContent(val subject: String, val body: String, val files: List<String>, val skipped: Int)

/** Reads what other apps share into this one. No Android here, so it can be tested. */
object IncomingShareRules {
    const val ACTION_SEND = "android.intent.action.SEND"
    const val ACTION_SEND_MULTIPLE = "android.intent.action.SEND_MULTIPLE"

    fun isShare(action: String?): Boolean = action == ACTION_SEND || action == ACTION_SEND_MULTIPLE

    /**
     * The message to start, or null when [raw] is not a share or carries nothing usable. Only `content:` addresses
     * count: a `file:` address could name one of the app's own private files, and an address of the app's own
     * [ownAuthority] is the app handing itself a file. Repeats are taken once.
     */
    fun parse(raw: RawShare, ownAuthority: String): SharedContent? {
        if (!isShare(raw.action)) return null
        val files = LinkedHashSet<String>()
        var skipped = 0
        for (stream in raw.streams) {
            val uri = stream.trim()
            if (uri.isEmpty()) continue
            if (usable(uri, ownAuthority)) {
                if (!files.add(uri)) continue
            } else {
                skipped++
            }
        }
        val body = raw.text.orEmpty().replace("\r\n", "\n").replace('\r', '\n').trim()
        val subject = raw.subject.orEmpty().replace(Regex("\\s+"), " ").trim()
        if (body.isEmpty() && subject.isEmpty() && files.isEmpty()) return null
        return SharedContent(subject, body, files.toList(), skipped)
    }

    private fun usable(uri: String, ownAuthority: String): Boolean {
        if (!uri.startsWith("content://", ignoreCase = true)) return false
        val rest = uri.substring("content://".length)
        val authority = rest.takeWhile { it != '/' && it != '?' && it != '#' }.substringAfterLast('@')
        // The platform decodes the authority before it picks the provider, so an encoded one could name the app's own.
        if ('%' in authority) return false
        return authority.isNotEmpty() && !authority.equals(ownAuthority, ignoreCase = true)
    }

    /** The text of the new message: the shared text first, then what the draft started with (a signature). */
    fun body(shared: String, existing: String): String = when {
        shared.isEmpty() -> existing
        existing.isBlank() -> shared
        else -> shared + "\n\n" + existing
    }
}
