package ovh.kweb.mail.background

import uniffi.kweb_core.Mailbox

/** Which folder a tap on a mail notification opens the message from. Plain logic, tested on the JVM. */
internal object NotificationTarget {
    /**
     * The folder of [mailboxIds] to open the message from: the Inbox when the message is in it, else a folder that
     * notifies (that is why it was announced), else any folder the cache knows. Null when none is known.
     */
    fun folderFor(mailboxIds: List<String>, mailboxes: List<Mailbox>): Mailbox? {
        val known = mailboxIds.mapNotNull { id -> mailboxes.firstOrNull { it.id == id } }
        return known.firstOrNull { it.role == "inbox" } ?: known.firstOrNull { it.notify } ?: known.firstOrNull()
    }
}
