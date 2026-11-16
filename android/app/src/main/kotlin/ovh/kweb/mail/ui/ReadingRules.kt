package ovh.kweb.mail.ui

import ovh.kweb.mail.R

/** The reading choices of Settings: what each value is called, kept as and does. No Android here but string ids. */
object ReadingRules {
    const val DEFAULT_PREVIEW_LINES = 1
    const val MAX_PREVIEW_LINES = 3

    fun clampPreviewLines(n: Int) = n.coerceIn(0, MAX_PREVIEW_LINES)

    /** When an opened message is marked read. */
    enum class MarkRead(val key: String, val label: Int, val delayMillis: Long?) {
        ON_OPEN("open", R.string.reading_mark_open, 0L),
        AFTER_DELAY("delay", R.string.reading_mark_delay, 2_000L),
        NEVER("never", R.string.reading_mark_never, null);

        companion object {
            fun of(key: String?) = entries.firstOrNull { it.key == key } ?: ON_OPEN
        }
    }

    /** Where Archive or Delete in the thread screen goes. */
    enum class AfterAction(val key: String, val label: Int) {
        LIST("list", R.string.reading_after_list),
        NEXT("next", R.string.reading_after_next),
        PREVIOUS("previous", R.string.reading_after_previous);

        companion object {
            fun of(key: String?) = entries.firstOrNull { it.key == key } ?: LIST
        }
    }

    /** The size of a message's text: the WebView's text zoom in percent, and the factor for plain text. */
    enum class TextSize(val key: String, val label: Int, val percent: Int) {
        SMALL("small", R.string.reading_size_small, 85),
        DEFAULT("default", R.string.reading_size_default, 100),
        LARGE("large", R.string.reading_size_large, 125),
        LARGER("larger", R.string.reading_size_larger, 150);

        val factor: Float get() = percent / 100f

        companion object {
            fun of(key: String?) = entries.firstOrNull { it.key == key } ?: DEFAULT
        }
    }

    /**
     * The page to go to when the page at [index] of [count] was archived or deleted: [AfterAction.NEXT] the page after it, else
     * the one before when it was the last; [AfterAction.PREVIOUS] the one before, else the one after; null: back to the list.
     */
    fun pageAfter(choice: AfterAction, index: Int, count: Int): Int? {
        if (count <= 1 || index !in 0 until count) return null
        val next = (index + 1).takeIf { it < count }
        val previous = (index - 1).takeIf { it >= 0 }
        return when (choice) {
            AfterAction.LIST -> null
            AfterAction.NEXT -> next ?: previous
            AfterAction.PREVIOUS -> previous ?: next
        }
    }
}
