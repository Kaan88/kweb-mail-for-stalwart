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
     * The message the screen scrolls to when the thread is first opened, to stand at the top: the one the route names
     * when the thread has it, else the first message that is unread now (before opening marks anything read). Null
     * when nothing needs scrolling: no unread message, a thread of one, or the target is the first message anyway.
     * [messages] are oldest first.
     */
    fun scrollTarget(messages: List<MessageHeader>, focusId: String?): String? {
        if (messages.size < 2) return null
        val target = messages.firstOrNull { it.id == focusId } ?: messages.firstOrNull { it.isUnread }
        return target?.takeIf { it.id != messages.first().id }?.id
    }

    /**
     * One step of the scroll to [target] (a pixel offset of the content): where to scroll now, or null when it is done.
     * It is done when the screen is at or past [target], or when it is not where this scroll left it ([lastSet], null
     * before the first step): the content may still grow (the thread's mail loads) and each step goes as far as the
     * content allows, but a screen the user moved is theirs.
     */
    /** How long the first scroll follows the content as it loads, in milliseconds. */
    const val SCROLL_FOLLOW_MS = 3_000L

    fun scrollStep(target: Int, value: Int, lastSet: Int?): Int? {
        if (value >= target) return null
        if (lastSet != null && value != lastSet) return null
        return target
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

    /**
     * The message Reply, Reply all and Forward of the thread act on: the newest one, except that a
     * draft saved in the thread is not something to answer, so the newest message outside Drafts
     * counts (when every message is a draft, the newest of them). [messages] are oldest first.
     */
    fun replyTarget(messages: List<MessageHeader>, mailboxes: List<Mailbox>): MessageHeader? {
        val drafts = mailboxes.filter { it.role == "drafts" }.map { it.id }.toSet()
        return messages.lastOrNull { m -> m.mailboxIds.none { it in drafts } } ?: messages.lastOrNull()
    }

    /**
     * "Block sender" is offered for a message that has a sender address other than the user's own ([myAddress]).
     * The core refuses the user's own addresses (it knows the identities); this only spares the obvious one.
     */
    fun canBlockSender(header: MessageHeader, myAddress: String?): Boolean =
        header.fromEmail.isNotBlank() && !header.fromEmail.equals(myAddress, ignoreCase = true)

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
