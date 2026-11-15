package ovh.kweb.mail.ui.thread

import ovh.kweb.mail.ui.Route
import ovh.kweb.mail.ui.list.ListRules
import ovh.kweb.mail.ui.list.MailListState
import ovh.kweb.mail.ui.list.SubfolderRules

/**
 * The conversations a thread screen can page through: the order of the list it was opened from (folder, chip, search results),
 * and when the pager asks that list for more. No Android here: the screen only asks and acts on the answer.
 */
object ThreadSwipeRules {
    /** The list under the thread that gives the order. */
    enum class Source { LIST, SEARCH }

    /** The conversation ids of the list now (each once, in the list's order) and what is known about more of them. */
    data class Listing(
        val ids: List<String>,
        /** Another page can be read or fetched: the list can go on past its last loaded row. */
        val more: Boolean = false,
        /** The folder the conversations are in: what a thread opened from here acts on (null for a search). */
        val folderId: String? = null,
        /**
         * In a list that covers a folder and its subfolders: the folder each conversation (by id) is shown in, which is what
         * its page acts on. A conversation not named here is in [folderId].
         */
        val folders: Map<String, String> = emptyMap(),
    ) {
        /** The folder the page of [threadId] opens with. */
        fun folderOf(threadId: String): String? = folders[threadId] ?: folderId

        companion object {
            val EMPTY = Listing(emptyList())
        }
    }

    /** How many pages from the last one the pager starts to ask for more. */
    const val NEAR_END = 2

    /** Pages farther than this from the shown one lose their view models (the ones next to it are kept, with room to spare). */
    const val KEEP_RADIUS = 2

    /**
     * Which list gives the order: the route under the thread on [stack]. Only the mail list and the search do; a thread opened
     * from a contact, a label, the chat, a notification or the like has no order (a single page). A thread opened from the mail
     * list names no message; one that does was opened by a notification over the list, and the list is not where it came from.
     */
    fun sourceOf(stack: List<Route>): Source? {
        val thread = stack.lastOrNull() as? Route.Thread ?: return null
        return when (stack.getOrNull(stack.size - 2)) {
            Route.MailList -> if (thread.messageId == null) Source.LIST else null
            Route.Search -> Source.SEARCH
            else -> null
        }
    }

    /** The conversations of the mail list now. Nothing for a folder whose rows open for editing (Drafts) or before it is read. */
    fun listing(state: MailListState): Listing {
        val current = state.current ?: return Listing.EMPTY
        if (!state.loaded || ListRules.opensCompose(current.role)) return Listing.EMPTY
        // The same test as the list's own loading of more: more of the cache, or older mail the server may still have.
        val more = state.rows.size >= state.limit || !(state.olderStopped || state.olderPaused)
        val folders = if (SubfolderRules.isCombined(state.scope)) {
            state.rows.associate { it.newest.threadId to SubfolderRules.openFrom(it, current.id, state.mailboxes, state.scope) }
        } else {
            emptyMap()
        }
        return Listing(state.rows.map { it.newest.threadId }.distinct(), more, current.id, folders)
    }

    /** The conversations of a search, each once: the results are messages, and several can be of one thread. A search cannot load more. */
    fun searchListing(threadIds: List<String>): Listing = Listing(threadIds.distinct())

    /**
     * The order to walk: [previous] (what was walked before) brought up to date with [live]. Conversations that are not in
     * [live] any more are dropped, except those in [keep] (the ones already visited: reading one takes it out of an Unread
     * list, and stepping back to it must still work). A conversation new in [live] goes in after the one before it there.
     */
    fun order(previous: List<String>, live: List<String>, keep: Set<String>): List<String> {
        val liveSet = live.toHashSet()
        val result = previous.filter { it in liveSet || it in keep }.distinct().toMutableList()
        var after = -1
        for (id in live) {
            val at = result.indexOf(id)
            if (at >= 0) {
                after = at
            } else {
                result.add(after + 1, id)
                after += 1
            }
        }
        return result
    }

    /**
     * The pages of the pager: [shown] when the thread opened is part of it, else just that one conversation (a list that is not
     * loaded yet, a folder that opens for editing, a thread that is no longer in the list).
     */
    fun pages(shown: List<String>, current: String): List<String> = if (current in shown) shown else listOf(current)

    /** The pager is on page [index] of [count] and the list can go on: time to ask it for more. */
    fun wantsMore(index: Int, count: Int, more: Boolean): Boolean = more && index >= 0 && count > 0 && index >= count - 1 - NEAR_END

    /** The conversations whose view models stay while [current] is the shown one: those within [KEEP_RADIUS] pages of it. */
    fun keep(pages: List<String>, current: String): Set<String> {
        val at = pages.indexOf(current)
        if (at < 0) return setOf(current)
        return pages.subList(maxOf(0, at - KEEP_RADIUS), minOf(pages.size, at + KEEP_RADIUS + 1)).toSet()
    }

    /** The page to step to from [index] ([forward]: the next one, else the one before), or null at either end. */
    fun neighbour(index: Int, count: Int, forward: Boolean): Int? = (if (forward) index + 1 else index - 1).takeIf { index >= 0 && it in 0 until count }
}

/**
 * What was walked since a conversation was opened from a list: the order, and the conversations visited. It lives in the pager's
 * view model, so as long as the thread is on the back stack, and is only written from effects, never while composing.
 */
class ThreadOrderMemory {
    var snapshot: List<String> = emptyList()
        private set
    var visited: Set<String> = emptySet()
        private set

    fun remember(order: List<String>, current: String) {
        snapshot = order
        visited = visited + current
    }
}
