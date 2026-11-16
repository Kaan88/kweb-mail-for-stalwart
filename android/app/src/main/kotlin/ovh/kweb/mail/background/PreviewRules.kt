package ovh.kweb.mail.background

/**
 * How much of a mail a notification says, per account: the user's choice for the lock screen and the shade
 * alike. [key] is what is stored and never changes.
 */
enum class PreviewChoice(val key: String) {
    /** Sender, subject and the first lines of the text. What the app always did. */
    Full("full"),

    /** Sender and subject, no text. */
    SubjectOnly("subject"),

    /** Only that a mail arrived: no sender and no subject. */
    Nothing("nothing"),
    ;

    companion object {
        /** The choice a stored text stands for; nothing stored, or a key from a later version, is [Full]. */
        fun parse(stored: String?): PreviewChoice = entries.firstOrNull { it.key == stored } ?: Full
    }
}

/** What a notification shows: [title], one line [text] (empty for none) and the expanded [big] text (empty for none). */
data class ShownText(val title: String, val text: String, val big: String)

/** The texts of a mail notification for a [PreviewChoice], as a plain function so it can be tested on the JVM. */
object PreviewRules {
    /**
     * [from] is the sender's name or address, [subject] and [preview] as the core gives them, [generic] the text
     * for "a mail arrived" ("New mail").
     */
    fun shown(choice: PreviewChoice, from: String, subject: String, preview: String, generic: String): ShownText = when (choice) {
        PreviewChoice.Full -> ShownText(from, subject, preview.ifBlank { subject })
        PreviewChoice.SubjectOnly -> ShownText(from, subject, subject)
        PreviewChoice.Nothing -> ShownText(generic, "", "")
    }

    /**
     * Whether the notification carries a public version for the lock screen. With the full text there is none, so
     * the system's own "contents hidden" setting decides as before; a reduced choice is the user's word for what the
     * lock screen may show, so it is given as the public version too.
     */
    fun hasPublicVersion(choice: PreviewChoice): Boolean = choice != PreviewChoice.Full
}
