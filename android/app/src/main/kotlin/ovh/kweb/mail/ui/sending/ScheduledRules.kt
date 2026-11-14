package ovh.kweb.mail.ui.sending

import uniffi.kweb_core.ScheduledSend

/** The rules of the list of held messages that need no Android. */
object ScheduledRules {
    /** Recipients shown in a row before "and n more". */
    const val SHOWN_RECIPIENTS = 2

    /** The first [SHOWN_RECIPIENTS] addresses and how many more there are. */
    data class Recipients(val shown: List<String>, val more: Int)

    fun recipients(to: List<String>): Recipients {
        val distinct = to.map { it.trim() }.filter { it.isNotEmpty() }.distinct()
        return Recipients(distinct.take(SHOWN_RECIPIENTS), (distinct.size - SHOWN_RECIPIENTS).coerceAtLeast(0))
    }

    /** Soonest first; the core sorts too, but the list must not depend on it. */
    fun ordered(items: List<ScheduledSend>): List<ScheduledSend> = items.sortedWith(compareBy({ it.sendAt }, { it.id }))
}
