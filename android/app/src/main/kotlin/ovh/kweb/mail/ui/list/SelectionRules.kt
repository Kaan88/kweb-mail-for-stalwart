package ovh.kweb.mail.ui.list

import uniffi.kweb_core.ThreadRow

/**
 * Selecting conversations in the mail list (a long press or a tap on the avatar starts it, a tap on any row goes on with it, and
 * it ends when the last row is let go). A conversation is chosen by its thread id; the actions act on the messages of the chosen
 * rows. No Android here.
 */
object SelectionRules {
    /** What a tap on a row does. */
    enum class Tap { OPEN, TOGGLE }

    /** While rows are chosen a tap anywhere on a row chooses or lets go of it; otherwise it opens the conversation. */
    fun tap(selecting: Boolean): Tap = if (selecting) Tap.TOGGLE else Tap.OPEN

    /**
     * Set by the list while conversation view is off: its rows are single messages, several of which can be of one thread, so a
     * row is known by its message id then. Conversation view (the default) knows a row by its thread id.
     */
    @Volatile
    var byMessage: Boolean = false

    fun key(row: ThreadRow): String = if (byMessage) row.newest.id else row.newest.threadId

    /** [id] chosen if it was not, let go of if it was. */
    fun toggle(selected: Set<String>, id: String): Set<String> = if (id in selected) selected - id else selected + id

    /** What stays chosen when the list is [liveIds] now: the rows that went away (synced away, acted on elsewhere) are dropped. */
    fun prune(selected: Set<String>, liveIds: Collection<String>): Set<String> {
        if (selected.isEmpty()) return selected
        val live = liveIds.toHashSet()
        return selected.filterTo(LinkedHashSet()) { it in live }
    }

    /** Every loaded row. */
    fun all(rows: List<ThreadRow>): Set<String> = rows.mapTo(LinkedHashSet()) { key(it) }

    fun allSelected(rows: List<ThreadRow>, selected: Set<String>): Boolean = rows.isNotEmpty() && rows.all { key(it) in selected }

    /** The chosen rows, in the list's order. */
    fun rowsOf(rows: List<ThreadRow>, selected: Set<String>): List<ThreadRow> = if (selected.isEmpty()) emptyList() else rows.filter { key(it) in selected }

    /** The messages of the chosen rows, each once: what an action is given. */
    fun messageIds(chosen: List<ThreadRow>): List<String> = chosen.flatMap { it.messageIds }.distinct()

    /** The button reads "Mark read" while any chosen conversation has unread mail, otherwise "Mark unread". */
    fun marksRead(chosen: List<ThreadRow>): Boolean = chosen.any { it.unreadCount > 0u }

    /** The star is taken off only when every chosen conversation has one; otherwise all get one. */
    fun unstars(chosen: List<ThreadRow>): Boolean = chosen.isNotEmpty() && chosen.all { it.isFlagged }

    /** Archive is offered everywhere but in the Archive itself. */
    fun archiveOffered(role: String?): Boolean = role != "archive"
}
