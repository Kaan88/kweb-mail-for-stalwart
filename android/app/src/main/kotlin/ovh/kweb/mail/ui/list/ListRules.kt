package ovh.kweb.mail.ui.list

import ovh.kweb.mail.ui.compose.SendRules
import uniffi.kweb_core.Draft
import uniffi.kweb_core.PendingSend

/** Rules of the mail list that need no Android. */
object ListRules {
    /** A tap on a conversation in this folder opens it for editing and not on the thread screen. */
    fun opensCompose(role: String?): Boolean = role == "drafts"

    /** The draft the "could not be sent" bar opens: the newest one the server refused, if any. [drafts] are newest first. */
    fun failedDraft(drafts: List<Draft>): Draft? = drafts.firstOrNull { it.sendError != null }

    /** The message whose Undo is still on offer at [nowMillis]: the one that goes out last, if any is still held. */
    fun holding(pending: List<PendingSend>, nowMillis: Long): PendingSend? =
        pending.filter { SendRules.remainingMillis(it.sendAt, nowMillis) > 0 }.maxByOrNull { it.sendAt }

    /** What the quiet bar counts: the messages still held back or waiting for a connection. */
    fun waiting(pending: List<PendingSend>): Int = pending.size
}
