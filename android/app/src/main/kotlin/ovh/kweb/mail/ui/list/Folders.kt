package ovh.kweb.mail.ui.list

import uniffi.kweb_core.Mailbox

/** A folder of the drawer with how deep it is nested. */
data class FolderEntry(val mailbox: Mailbox, val depth: Int)

object Folders {
    /** The folders with a role come first, in this order. */
    val ROLE_ORDER = listOf("inbox", "drafts", "sent", "archive", "junk", "trash")

    /**
     * The folders as the drawer lists them: those with a role first, in [ROLE_ORDER], then the
     * rest as a tree by `parentId`, siblings by sort order and then name. A folder under a role
     * folder is listed right below it. A folder whose parent is missing counts as a top-level one.
     */
    fun ordered(mailboxes: List<Mailbox>): List<FolderEntry> {
        val ids = mailboxes.map { it.id }.toSet()
        val byParent = mailboxes.groupBy { it.parentId?.takeIf { parent -> parent in ids } }
        val roleFolders = mailboxes.filter { it.role in ROLE_ORDER }
            .sortedWith(compareBy<Mailbox> { ROLE_ORDER.indexOf(it.role) }.then(SIBLINGS))
        val out = mutableListOf<FolderEntry>()
        val seen = mutableSetOf<String>()

        fun add(mailbox: Mailbox, depth: Int) {
            if (!seen.add(mailbox.id)) return
            out += FolderEntry(mailbox, depth)
            byParent[mailbox.id].orEmpty().filter { it.role !in ROLE_ORDER }.sortedWith(SIBLINGS).forEach { add(it, depth + 1) }
        }

        roleFolders.forEach { add(it, 0) }
        byParent[null].orEmpty().filter { it.role !in ROLE_ORDER }.sortedWith(SIBLINGS).forEach { add(it, 0) }
        // Folders in a parent cycle are not reachable from any top-level one; do not lose them.
        mailboxes.filter { it.id !in seen }.sortedWith(SIBLINGS).forEach { add(it, 0) }
        return out
    }

    fun withRole(mailboxes: List<Mailbox>, role: String): Mailbox? = mailboxes.firstOrNull { it.role == role }

    private val SIBLINGS = compareBy<Mailbox>({ it.sortOrder }, { it.name.lowercase() })
}
