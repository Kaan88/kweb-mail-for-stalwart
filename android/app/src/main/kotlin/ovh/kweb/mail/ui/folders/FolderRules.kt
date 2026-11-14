package ovh.kweb.mail.ui.folders

import uniffi.kweb_core.Mailbox

/** What the bell of a folder does. */
enum class Bell {
    /** The Inbox: always notifies, cannot be switched. */
    FIXED_ON,

    /** Drafts, Sent, Trash and Junk: never notify. */
    NONE,

    /** The user's switch. */
    SWITCH,
}

/** What deleting a folder needs to say first. */
sealed interface DeletePlan {
    /** Folders inside it: they must go first. */
    data object HasFolders : DeletePlan

    /** Empty: delete after asking. */
    data object Empty : DeletePlan

    /** Holds [count] messages: they go to the Trash. */
    data class WithMail(val count: Int) : DeletePlan
}

/** The entries of a folder's menu. */
enum class FolderMenuItem {
    NOTIFY_ON,
    NOTIFY_OFF,
    RENAME,
    MOVE,
    NEW_INSIDE,
    DELETE,
}

/** Rules of the folder screens that need no Android. The core checks all of this again; these say what to offer. */
object FolderRules {
    /** Folders that may not hold other folders, nor be a place to move a folder to. */
    private val CLOSED_ROLES = setOf("drafts", "trash", "junk")
    private val NO_BELL_ROLES = setOf("drafts", "sent", "trash", "junk")

    /** A folder the user made: only these can be renamed, moved and deleted. */
    fun isUserFolder(mailbox: Mailbox): Boolean = mailbox.role == null

    fun bell(mailbox: Mailbox): Bell = when {
        mailbox.role == "inbox" -> Bell.FIXED_ON
        mailbox.role in NO_BELL_ROLES -> Bell.NONE
        else -> Bell.SWITCH
    }

    /** The drawer's small bell: on a folder that notifies, never on the Inbox (it always does). */
    fun showsDrawerBell(mailbox: Mailbox): Boolean = mailbox.role != "inbox" && mailbox.notify

    /** A new folder may be made inside this one. */
    fun canHoldFolders(mailbox: Mailbox): Boolean = mailbox.role !in CLOSED_ROLES

    /**
     * What the menu of [folder] offers. A folder with a role offers only New folder inside (and, in the drawer, its
     * bell, when [withBell] and it has a switch); a folder the user made offers all four.
     */
    fun menu(folder: Mailbox, withBell: Boolean): List<FolderMenuItem> = buildList {
        if (withBell && bell(folder) == Bell.SWITCH) add(if (folder.notify) FolderMenuItem.NOTIFY_OFF else FolderMenuItem.NOTIFY_ON)
        if (isUserFolder(folder)) {
            add(FolderMenuItem.RENAME)
            add(FolderMenuItem.MOVE)
        }
        if (canHoldFolders(folder)) add(FolderMenuItem.NEW_INSIDE)
        if (isUserFolder(folder)) add(FolderMenuItem.DELETE)
    }

    /** What the folder screen opens for a menu entry; null for the bell, which needs no dialog. */
    fun action(item: FolderMenuItem): FolderAction? = when (item) {
        FolderMenuItem.NOTIFY_ON, FolderMenuItem.NOTIFY_OFF -> null
        FolderMenuItem.RENAME -> FolderAction.RENAME
        FolderMenuItem.MOVE -> FolderAction.MOVE
        FolderMenuItem.NEW_INSIDE -> FolderAction.NEW_INSIDE
        FolderMenuItem.DELETE -> FolderAction.DELETE
    }

    fun hasFolders(mailboxes: List<Mailbox>, id: String): Boolean = mailboxes.any { it.parentId == id }

    /** The ids of every folder inside [id], at any depth. */
    fun descendants(mailboxes: List<Mailbox>, id: String): Set<String> {
        val byParent = mailboxes.groupBy { it.parentId }
        val out = mutableSetOf<String>()
        val queue = ArrayDeque(listOf(id))
        while (queue.isNotEmpty()) {
            for (child in byParent[queue.removeFirst()].orEmpty()) if (out.add(child.id)) queue += child.id
        }
        return out
    }

    /**
     * The folders [id] may be moved under: not itself, not a folder inside it, not Drafts, Trash or Junk, not
     * the folder it is in already. The top level is not in the list (the picker adds it).
     */
    fun moveTargets(mailboxes: List<Mailbox>, id: String): List<Mailbox> {
        val moving = mailboxes.firstOrNull { it.id == id } ?: return emptyList()
        val excluded = descendants(mailboxes, id) + id
        return mailboxes.filter { it.id !in excluded && it.role !in CLOSED_ROLES && it.id != moving.parentId }
    }

    /** The picker offers the top level unless the folder is there already. */
    fun canMoveToTopLevel(mailboxes: List<Mailbox>, id: String): Boolean =
        mailboxes.firstOrNull { it.id == id }?.parentId?.let { parent -> mailboxes.any { it.id == parent } } ?: false

    /** Null when the folder is gone. */
    fun deletePlan(mailboxes: List<Mailbox>, id: String): DeletePlan? {
        val folder = mailboxes.firstOrNull { it.id == id } ?: return null
        return when {
            hasFolders(mailboxes, id) -> DeletePlan.HasFolders
            folder.totalEmails > 0u -> DeletePlan.WithMail(folder.totalEmails.toInt())
            else -> DeletePlan.Empty
        }
    }

    /** The name field is worth sending: something other than blanks. The core says what else is wrong. */
    fun nameCanBeSent(text: String): Boolean = text.isNotBlank()
}
