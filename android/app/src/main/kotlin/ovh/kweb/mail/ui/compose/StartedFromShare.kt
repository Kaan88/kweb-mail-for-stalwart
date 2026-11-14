package ovh.kweb.mail.ui.compose

import java.util.concurrent.ConcurrentHashMap

/**
 * Drafts started from a share of another app. They hold text the user brought in, so leaving the screen
 * untouched keeps them instead of throwing a "blank" message away. Process-wide, like [Outbox].
 */
object StartedFromShare {
    private val ids = ConcurrentHashMap.newKeySet<String>()

    fun mark(draftId: String) {
        ids.add(draftId)
    }

    fun has(draftId: String): Boolean = draftId in ids

    /** The discard rule of [ComposeRules.discardsOnLeave], except for a draft that came from a share. */
    fun discards(draftId: String, opened: uniffi.kweb_core.Draft, now: uniffi.kweb_core.Draft, prefill: uniffi.kweb_core.Address?): Boolean =
        !has(draftId) && ComposeRules.discardsOnLeave(opened, now, prefill)
}
