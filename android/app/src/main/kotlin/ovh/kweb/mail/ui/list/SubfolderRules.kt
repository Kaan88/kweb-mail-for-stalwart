package ovh.kweb.mail.ui.list

import ovh.kweb.mail.ui.folders.FolderRules
import uniffi.kweb_core.Mailbox
import uniffi.kweb_core.ThreadRow

/**
 * "Show with subfolders": the list of a folder that also holds the mail of every folder below it. Pure.
 * The choice is made from the folder's menu in the drawer and is not kept across restarts.
 */
object SubfolderRules {
    /** The folder has folders inside it, so the menu offers the entry. */
    fun offered(mailboxes: List<Mailbox>, folderId: String): Boolean = FolderRules.hasFolders(mailboxes, folderId)

    /**
     * The folders a list covers: [folderId] first, then (when [withSubfolders]) every folder below it, at any
     * depth, in the order of [mailboxes]. A folder that has none is just itself, whatever was asked.
     */
    fun scope(mailboxes: List<Mailbox>, folderId: String, withSubfolders: Boolean): List<String> {
        if (!withSubfolders) return listOf(folderId)
        val below = FolderRules.descendants(mailboxes, folderId)
        return listOf(folderId) + mailboxes.map { it.id }.filter { it in below && it != folderId }
    }

    /** Whether the list covers more than the folder itself. */
    fun isCombined(scope: List<String>): Boolean = scope.size > 1

    /**
     * The folder to name on a row, or null when the row needs none: the newest message of the row is in [parentId]
     * (the folder the list is for), or in none of the [scope] folders (then it is not what the list was built for).
     * Otherwise the first folder of the message that is in the scope, by the order of [mailboxes]. The name is
     * the folder's own ([Mailbox.name]; the caller may translate a role).
     */
    fun rowFolder(row: ThreadRow, parentId: String, mailboxes: List<Mailbox>, scope: List<String>): Mailbox? {
        val mine = row.newest.mailboxIds
        if (parentId in mine) return null
        val inScope = scope.toSet()
        return mailboxes.firstOrNull { it.id in mine && it.id in inScope }
    }

    /**
     * The folder a row's conversation is opened from: the one the row names, else the folder the list is for.
     * Archive, Delete and Move in the thread act on it.
     */
    fun openFrom(row: ThreadRow, parentId: String, mailboxes: List<Mailbox>, scope: List<String>): String =
        rowFolder(row, parentId, mailboxes, scope)?.id ?: parentId
}
