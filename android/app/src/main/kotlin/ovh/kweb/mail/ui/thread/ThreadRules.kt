package ovh.kweb.mail.ui.thread

import uniffi.kweb_core.Address
import uniffi.kweb_core.Mailbox
import uniffi.kweb_core.MessageHeader

/** The rules of the thread screen that need no Android. */
object ThreadRules {
    private val SPECIAL_ROLES = setOf("trash", "drafts", "sent")

    /**
     * The messages that start expanded: the one the route names when the thread has it, else every
     * unread message and the last one. [messages] are oldest first.
     */
    fun initiallyExpanded(messages: List<MessageHeader>, focusId: String?): Set<String> {
        if (focusId != null && messages.any { it.id == focusId }) return setOf(focusId)
        return buildSet {
            messages.filter { it.isUnread }.forEach { add(it.id) }
            messages.lastOrNull()?.let { add(it.id) }
        }
    }

    /**
     * The thread has nothing left to show: the cache was read, the server was asked (and
     * answered) and no message is there. Before that, "no messages" only means "not here yet".
     */
    fun isGone(loaded: Boolean, refreshed: Boolean, count: Int): Boolean = loaded && refreshed && count == 0

    /**
     * The messages Archive, Delete, Move and Spam act on. Opened from a folder: those in it.
     * Opened from a search ([mailboxId] null): those that are not in Trash, Drafts or Sent, since
     * a search result is no sign the user meant a draft or a sent copy; but when every message
     * is in such a folder, all of them.
     */
    fun targets(messages: List<MessageHeader>, mailboxId: String?, mailboxes: List<Mailbox>): List<String> {
        if (mailboxId != null) return messages.filter { mailboxId in it.mailboxIds }.map { it.id }
        val special = mailboxes.filter { it.role in SPECIAL_ROLES }.map { it.id }.toSet()
        val others = messages.filter { m -> m.mailboxIds.none { it in special } }
        return (others.ifEmpty { messages }).map { it.id }
    }

    /** Delete on any of these removes mail for good: one of them is in Trash. */
    fun deletesForever(messages: List<MessageHeader>, targetIds: List<String>, mailboxes: List<Mailbox>): Boolean {
        val trash = mailboxes.firstOrNull { it.role == "trash" }?.id ?: return false
        return messages.any { it.id in targetIds && trash in it.mailboxIds }
    }

    /**
     * The names of the line under the sender, "to me, Ayşe": [me] (the account's address, when it
     * is one) comes out as [meLabel] and first, others by first name, at most [limit] of them and
     * then "+n". Null when there are no recipients.
     */
    fun recipientNames(to: List<Address>, cc: List<Address>, me: String?, meLabel: String, limit: Int = 3): String? {
        val all = (to + cc).distinctBy { it.email.lowercase() }
        val isMe = { a: Address -> me != null && a.email.equals(me, ignoreCase = true) }
        val names = all.filter(isMe).map { meLabel } + all.filterNot(isMe).map { firstName(it) }
        if (names.isEmpty()) return null
        val shown = names.take(limit).joinToString(", ")
        return if (names.size > limit) "$shown +${names.size - limit}" else shown
    }

    /** "Name <address>", or the address alone. */
    fun fullAddress(a: Address): String = if (a.name.isBlank()) a.email else "${a.name.trim()} <${a.email}>"

    private fun firstName(a: Address): String {
        val label = a.name.trim().ifEmpty { a.email.trim() }
        return label.substringBefore(' ')
    }
}

/** File sizes as short text. */
object SizeFormat {
    /** "812 B", "1.5 KB", "23.4 MB": powers of 1000, one decimal from the kilobyte up. */
    fun format(bytes: Long, locale: java.util.Locale): String {
        if (bytes < 1000) return "${maxOf(bytes, 0)} B"
        val units = listOf("KB", "MB", "GB", "TB")
        var value = bytes / 1000.0
        var unit = 0
        while (value >= 999.95 && unit < units.lastIndex) {
            value /= 1000
            unit++
        }
        return String.format(locale, "%.1f %s", value, units[unit])
    }
}
