package ovh.kweb.mail.ui.shared

import java.time.LocalDate
import java.time.ZoneId
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.UiMessage
import ovh.kweb.mail.ui.calendar.AgendaWindow
import ovh.kweb.mail.ui.thread.FileNames
import uniffi.kweb_core.Exception as CoreException
import uniffi.kweb_core.MessageHeader
import uniffi.kweb_core.SharedAccount
import uniffi.kweb_core.SharedAction
import uniffi.kweb_core.SharedMailbox

/** A shared folder with how deep it is nested among the folders that are shared. */
data class SharedFolderEntry(val mailbox: SharedMailbox, val depth: Int)

/** The kinds of change a folder's rights can allow. Flagging goes with marking: the server gives both rights or neither. */
enum class SharedActionKind { MARK, MOVE, DELETE }

/** The pure rules of the "Shared with me" area. */
object SharedRules {
    /** Messages in a page of a folder. */
    const val PAGE = 50

    /** The most the core gives in one call. */
    const val MAX_PAGE = 100

    // ---- folders and rights

    /**
     * The folders as a tree by `parentId`, siblings by name. A folder whose parent is not in [mailboxes] (the
     * parent is not shared) counts as a top-level one. Folders in a parent cycle are not lost.
     */
    fun tree(mailboxes: List<SharedMailbox>): List<SharedFolderEntry> {
        val ids = mailboxes.map { it.id }.toSet()
        val byParent = mailboxes.groupBy { it.parentId?.takeIf { parent -> parent in ids && parent != it.id } }
        val out = mutableListOf<SharedFolderEntry>()
        val seen = mutableSetOf<String>()

        fun add(mailbox: SharedMailbox, depth: Int) {
            if (!seen.add(mailbox.id)) return
            out += SharedFolderEntry(mailbox, depth)
            byParent[mailbox.id].orEmpty().sortedWith(SIBLINGS).forEach { add(it, depth + 1) }
        }

        byParent[null].orEmpty().sortedWith(SIBLINGS).forEach { add(it, 0) }
        mailboxes.filter { it.id !in seen }.sortedWith(SIBLINGS).forEach { add(it, 0) }
        return out
    }

    private val SIBLINGS = compareBy<SharedMailbox>({ it.name.lowercase() }, { it.id })

    /** The folders a message of [current] may be moved to: the ones that take messages, not [current] itself, as a tree of their own. */
    fun moveTargets(folders: List<SharedMailbox>, current: SharedMailbox?): List<SharedFolderEntry> =
        tree(folders.filter { it.mayAdd && it.id != current?.id })

    /** Whether [kind] is offered for messages of [folder] (null while the folder is not known: nothing is). */
    fun allows(kind: SharedActionKind, folder: SharedMailbox?, folders: List<SharedMailbox>): Boolean = when {
        folder == null -> false
        kind == SharedActionKind.MARK -> folder.mayMark
        kind == SharedActionKind.DELETE -> folder.mayRemove
        else -> folder.mayRemove && moveTargets(folders, folder).isNotEmpty()
    }

    /** The kind of change [action] is, for checking it against the folder's rights. */
    fun kindOf(action: SharedAction): SharedActionKind = when (action) {
        SharedAction.MarkRead, SharedAction.MarkUnread, SharedAction.Flag, SharedAction.Unflag -> SharedActionKind.MARK
        is SharedAction.Move -> SharedActionKind.MOVE
        SharedAction.Delete -> SharedActionKind.DELETE
    }

    /** What the read/unread entry of the menu does to [header]. */
    fun toggleRead(header: MessageHeader): SharedAction = if (header.isUnread) SharedAction.MarkRead else SharedAction.MarkUnread

    /** What the star entry of the menu does to [header]. */
    fun toggleFlag(header: MessageHeader): SharedAction = if (header.isFlagged) SharedAction.Unflag else SharedAction.Flag

    // ---- errors

    /**
     * A load of a whole account's folders, folder pages, agenda or contacts that the core refuses (`Invalid`)
     * says the share is gone (or the account is not listed any more). The screens then go back to the shared
     * screen and read it again. A single message, event or contact that is refused is only that one gone.
     */
    fun isGone(error: Throwable): Boolean = error is CoreException.Invalid

    /** What to tell the user about [error]: the core's words when it has some, else a plain line. */
    fun message(error: Throwable): UiMessage = when {
        error is CoreException.Invalid && error.msg.isNotBlank() -> UiMessage.Text(error.msg)
        error is CoreException.Temporary && error.msg.isNotBlank() -> UiMessage.Text(error.msg)
        error is CoreException.SignInNeeded -> UiMessage.Res(R.string.shared_sign_in_needed)
        else -> UiMessage.Res(R.string.shared_failed)
    }

    // ---- accounts

    /** The kinds an account shares, in the order the screen lists them. */
    enum class Kind { MAIL, CALENDAR, CONTACTS }

    fun kinds(account: SharedAccount): List<Kind> = buildList {
        if (account.hasMail) add(Kind.MAIL)
        if (account.hasCalendars) add(Kind.CALENDAR)
        if (account.hasContacts) add(Kind.CONTACTS)
    }

    /** Accounts of which nothing is left to open are not listed. */
    fun listed(accounts: List<SharedAccount>): List<SharedAccount> = accounts.filter { kinds(it).isNotEmpty() }

    // ---- paging

    fun hasMore(loaded: Int, total: Int): Boolean = loaded < total

    /** How many messages to read when the folder is read again: the ones already shown, at least a page. Read in calls of at most [MAX_PAGE]. */
    fun reloadCount(loaded: Int): Int = maxOf(loaded, PAGE)

    /** [more] after [shown], without a message that is in both (the folder changed between the two calls). */
    fun append(shown: List<MessageHeader>, more: List<MessageHeader>): List<MessageHeader> {
        val ids = shown.mapTo(HashSet()) { it.id }
        return shown + more.filter { ids.add(it.id) }
    }

    // ---- keys

    /** A key for a list row, unique across accounts: ids are only unique within one. */
    fun key(account: String, id: String): String = "${account.length}:$account/$id"

    /** The name of a downloaded blob of [account] in the cache (see [FileNames.blobKey]). */
    fun blobKey(account: String, blobId: String): String = FileNames.blobKey(account, blobId)

    // ---- agenda

    /** The first span of a shared calendar: today and the next [DAYS] days. */
    fun firstSpan(today: LocalDate) = AgendaWindow.Window(today, today.plusDays(DAYS))

    /** [window] reaching [DAYS] days further, up to what the core is asked for at most. */
    fun extend(window: AgendaWindow.Window): AgendaWindow.Window {
        val limit = window.from.plusDays(AgendaWindow.MAX_DAYS)
        return AgendaWindow.Window(window.from, minOf(window.to.plusDays(DAYS), limit))
    }

    fun canExtend(window: AgendaWindow.Window): Boolean = window.days < AgendaWindow.MAX_DAYS

    /**
     * The time zone name to give the core, which refuses a name that is not an IANA one (and that is an `Invalid`,
     * which must not be mistaken for a lost share). A device zone such as `GMT+03:00` becomes `Etc/GMT-3`
     * (whole hours) or `UTC`.
     */
    fun coreZone(zone: ZoneId): String {
        val rules = zone.rules
        if (!rules.isFixedOffset) return zone.id
        if (zone.id == "UTC") return "UTC"
        val seconds = rules.getOffset(java.time.Instant.EPOCH).totalSeconds
        val hours = seconds / 3600
        return if (seconds % 3600 == 0 && hours in -12..14 && hours != 0) "Etc/GMT${if (hours > 0) "-" else "+"}${kotlin.math.abs(hours)}" else "UTC"
    }

    const val DAYS = 30L
}
