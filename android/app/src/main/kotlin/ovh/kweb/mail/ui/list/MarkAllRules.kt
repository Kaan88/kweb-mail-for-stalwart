package ovh.kweb.mail.ui.list

import uniffi.kweb_core.Mailbox

/**
 * "Mark all as read" in the drawer's folder menu. Pure. It belongs to a folder, not to a list: it takes the folders
 * the list of that folder covers (the folder alone, or with its subfolders when that choice is on). It is not offered
 * in search results nor in the Unread and Flagged views, which are not folders.
 */
object MarkAllRules {
    /** The folders the entry would mark: the same as the folder's list covers. */
    fun scope(mailboxes: List<Mailbox>, folderId: String, withSubfolders: Boolean): List<String> =
        SubfolderRules.scope(mailboxes, folderId, withSubfolders)

    /** Offered when the server counts unread mail in any folder of the scope, and not for Drafts (there is nothing to read). */
    fun offered(mailboxes: List<Mailbox>, folderId: String, withSubfolders: Boolean): Boolean {
        val folder = mailboxes.firstOrNull { it.id == folderId } ?: return false
        if (folder.role == "drafts") return false
        val inScope = scope(mailboxes, folderId, withSubfolders).toSet()
        return mailboxes.any { it.id in inScope && it.unreadEmails > 0u }
    }
}
