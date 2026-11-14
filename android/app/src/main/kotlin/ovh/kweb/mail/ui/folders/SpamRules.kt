package ovh.kweb.mail.ui.folders

import ovh.kweb.mail.ui.UiMessage
import uniffi.kweb_core.Mailbox
import uniffi.kweb_core.MessageHeader

/** Rules for "Report spam" and "Not spam" that need no Android. */
object SpamRules {
    /** Report spam is not offered for mail you wrote or have not sent yet. */
    private val QUIET_ROLES = setOf("drafts", "sent")

    /** Whether the menu offers it for a list in a folder with [role]: needs a Junk folder. */
    fun offered(role: String?, hasJunk: Boolean): Boolean = hasJunk && role !in QUIET_ROLES

    /**
     * Where Undo puts the mail back: the folder the report was made from. Null means the core's own undo
     * (`report_spam(false)`: back to the Inbox, marked not junk), which is right for a report made in the Inbox, and
     * for "Not spam" in Junk the undo is a report, so it is null too.
     */
    fun undoGoesTo(wasSpam: Boolean, fromId: String?, fromRole: String?): String? =
        if (wasSpam && fromId != null && fromRole != "inbox") fromId else null

    /** In the Junk folder the entry reads "Not spam" and does the opposite. */
    fun isNotSpamEntry(role: String?): Boolean = role == "junk"

    /**
     * For a thread: it is "Not spam" when it was opened from Junk, or (opened from a search, [mailboxId] null) when
     * every message in [targetIds] is in Junk.
     */
    fun threadIsNotSpam(messages: List<MessageHeader>, targetIds: List<String>, mailboxId: String?, mailboxes: List<Mailbox>): Boolean {
        val junk = mailboxes.firstOrNull { it.role == "junk" }?.id ?: return false
        if (mailboxId != null) return mailboxId == junk
        val targets = messages.filter { it.id in targetIds }
        return targets.isNotEmpty() && targets.all { junk in it.mailboxIds }
    }

    /** The thread menu offers it when there is a Junk folder and the thread was not opened from a folder it makes no sense for. */
    fun threadOffered(from: Mailbox?, hasJunk: Boolean): Boolean = hasJunk && from?.role !in QUIET_ROLES

    /** Words for a refusal of the core, or the app's own. */
    fun failure(error: Throwable): UiMessage = UiMessage.of(error, ovh.kweb.mail.R.string.spam_failed)
}
