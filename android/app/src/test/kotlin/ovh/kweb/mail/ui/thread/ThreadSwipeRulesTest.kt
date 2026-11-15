package ovh.kweb.mail.ui.thread

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import ovh.kweb.mail.ui.Route
import ovh.kweb.mail.ui.list.MailListState
import ovh.kweb.mail.ui.thread.ThreadSwipeRules.Source
import uniffi.kweb_core.Mailbox
import uniffi.kweb_core.MessageHeader
import uniffi.kweb_core.ThreadRow

class ThreadSwipeRulesTest {
    private fun box(role: String?) = Mailbox("m1", "Folder", role, null, 0u, 0u, 0u, false)

    private fun row(threadId: String) = ThreadRow(
        MessageHeader("msg-$threadId", threadId, listOf("m1"), "", "", emptyList(), "", "", 0, false, false, false, 0u),
        listOf("msg-$threadId"), 0u, false, false, emptyList(),
    )

    private fun state(
        vararg threads: String,
        role: String? = "inbox",
        limit: Int = 50,
        stopped: Boolean = false,
        paused: Boolean = false,
        loaded: Boolean = true,
    ) = MailListState(current = box(role), rows = threads.map(::row), limit = limit, loaded = loaded, olderStopped = stopped, olderPaused = paused)

    // ---- where the order comes from ----

    @Test fun onlyTheMailListAndTheSearchGiveAnOrder() {
        val thread = Route.Thread("t", null, null)
        assertEquals(Source.LIST, ThreadSwipeRules.sourceOf(listOf(Route.MailList, thread)))
        assertEquals(Source.SEARCH, ThreadSwipeRules.sourceOf(listOf(Route.MailList, Route.Search, thread)))
        assertNull(ThreadSwipeRules.sourceOf(listOf(Route.MailList, Route.Contact("c"), thread)))
        assertNull(ThreadSwipeRules.sourceOf(listOf(Route.MailList, Route.Label("l", "L"), thread)))
        assertNull(ThreadSwipeRules.sourceOf(listOf(Route.MailList, Route.AssistantChat, thread)))
        // The thread alone, or a thread that is not on top (a message being written over it): no step.
        assertNull(ThreadSwipeRules.sourceOf(listOf(thread)))
        assertNull(ThreadSwipeRules.sourceOf(listOf(Route.MailList, thread, Route.Compose("d"))))
    }

    @Test fun theListingIsTheRowsInOrderWithTheFolder() {
        val listing = ThreadSwipeRules.listing(state("a", "b", "c"))
        assertEquals(listOf("a", "b", "c"), listing.ids)
        assertEquals("m1", listing.folderId)
    }

    @Test fun inAListWithSubfoldersEachPageOpensWithItsOwnFolder() {
        val boxes = listOf(box("inbox"), Mailbox("sub", "Sub", null, "m1", 0u, 0u, 0u, false))
        fun inFolder(threadId: String, folder: String) = row(threadId).let {
            it.copy(newest = it.newest.copy(mailboxIds = listOf(folder)))
        }
        val s = MailListState(
            current = boxes[0], mailboxes = boxes, scope = listOf("m1", "sub"), loaded = true,
            rows = listOf(inFolder("a", "m1"), inFolder("b", "sub")),
        )
        val listing = ThreadSwipeRules.listing(s)
        assertEquals("m1", listing.folderOf("a"))
        assertEquals("sub", listing.folderOf("b"))
        // A list of one folder names no folders: every page opens with the folder.
        assertEquals("m1", ThreadSwipeRules.listing(state("a", "b")).folderOf("b"))
        assertNull(ThreadSwipeRules.searchListing(listOf("a")).folderOf("a"))
    }

    @Test fun aFolderThatOpensForEditingOrIsNotReadHasNoOrder() {
        assertEquals(ThreadSwipeRules.Listing.EMPTY, ThreadSwipeRules.listing(state("a", role = "drafts")))
        assertEquals(ThreadSwipeRules.Listing.EMPTY, ThreadSwipeRules.listing(state("a", loaded = false)))
        assertEquals(ThreadSwipeRules.Listing.EMPTY, ThreadSwipeRules.listing(MailListState()))
    }

    @Test fun theListCanGoOnWhileThereIsCacheOrOlderMailToFetch() {
        assertEquals(true, ThreadSwipeRules.listing(state("a", "b", limit = 50)).more)
        // Every row asked for is there: the cache may hold another page, even when the server has no older mail.
        assertEquals(true, ThreadSwipeRules.listing(state("a", "b", limit = 2, stopped = true)).more)
        assertEquals(false, ThreadSwipeRules.listing(state("a", "b", limit = 50, stopped = true)).more)
        assertEquals(false, ThreadSwipeRules.listing(state("a", "b", limit = 50, paused = true)).more)
    }

    @Test fun aSearchShowsEachConversationOnceAndCannotLoadMore() {
        val listing = ThreadSwipeRules.searchListing(listOf("a", "b", "a", "c"))
        assertEquals(listOf("a", "b", "c"), listing.ids)
        assertEquals(false, listing.more)
        assertNull(listing.folderId)
    }

    // ---- the order ----

    @Test fun anOrderFromNothingIsTheList() =
        assertEquals(listOf("a", "b", "c"), ThreadSwipeRules.order(emptyList(), listOf("a", "b", "c"), emptySet()))

    @Test fun aConversationThatVanishedLeavesTheOrder() =
        assertEquals(listOf("a", "c"), ThreadSwipeRules.order(listOf("a", "b", "c"), listOf("a", "c"), emptySet()))

    @Test fun aVisitedConversationStaysEvenWhenReadingItTookItOutOfTheList() {
        // An Unread list: a and b were read on the way, c is the one shown now and d is the next one.
        val order = ThreadSwipeRules.order(listOf("a", "b", "c", "d"), listOf("d"), setOf("a", "b", "c"))
        assertEquals(listOf("a", "b", "c", "d"), order)
    }

    @Test fun newMailGoesInAfterTheOneBeforeItAndMoreLoadedMailAtTheEnd() {
        assertEquals(listOf("n", "a", "b", "c", "d"), ThreadSwipeRules.order(listOf("a", "b", "c"), listOf("n", "a", "b", "c", "d"), emptySet()))
        assertEquals(listOf("a", "x", "b"), ThreadSwipeRules.order(listOf("a", "b"), listOf("a", "x", "b"), emptySet()))
    }

    @Test fun theOrderDoesNotJumpWhenTheListReorders() =
        assertEquals(listOf("a", "b", "c"), ThreadSwipeRules.order(listOf("a", "b", "c"), listOf("c", "a", "b"), emptySet()))

    // ---- the pages ----

    @Test fun thePagesAreTheOrderWhenTheOpenedThreadIsInIt() =
        assertEquals(listOf("a", "b", "c"), ThreadSwipeRules.pages(listOf("a", "b", "c"), "b"))

    @Test fun aThreadThatIsNotInTheOrderIsASinglePage() {
        assertEquals(listOf("z"), ThreadSwipeRules.pages(listOf("a", "b"), "z"))
        assertEquals(listOf("z"), ThreadSwipeRules.pages(emptyList(), "z"))
    }

    @Test fun morePagesAreAskedForNearTheEndWhenTheListCanGoOn() {
        // Five pages: from page 2 on (two before the last).
        assertEquals(false, ThreadSwipeRules.wantsMore(1, 5, true))
        assertEquals(true, ThreadSwipeRules.wantsMore(2, 5, true))
        assertEquals(true, ThreadSwipeRules.wantsMore(4, 5, true))
        assertEquals(false, ThreadSwipeRules.wantsMore(4, 5, false))
        assertEquals(false, ThreadSwipeRules.wantsMore(-1, 5, true))
        assertEquals(false, ThreadSwipeRules.wantsMore(0, 0, true))
        // A short list asks at once.
        assertEquals(true, ThreadSwipeRules.wantsMore(0, 2, true))
    }

    @Test fun onlyThePagesAroundTheShownOneKeepTheirViewModels() {
        val pages = listOf("a", "b", "c", "d", "e", "f", "g")
        assertEquals(setOf("a", "b", "c"), ThreadSwipeRules.keep(pages, "a"))
        assertEquals(setOf("b", "c", "d", "e", "f"), ThreadSwipeRules.keep(pages, "d"))
        assertEquals(setOf("e", "f", "g"), ThreadSwipeRules.keep(pages, "g"))
        assertEquals(setOf("zzz"), ThreadSwipeRules.keep(pages, "zzz"))
    }

    @Test fun aNeighbourIsTheNextOrThePreviousPageAndNothingPastTheEnds() {
        assertEquals(2, ThreadSwipeRules.neighbour(1, 3, forward = true))
        assertEquals(0, ThreadSwipeRules.neighbour(1, 3, forward = false))
        assertNull(ThreadSwipeRules.neighbour(2, 3, forward = true))
        assertNull(ThreadSwipeRules.neighbour(0, 3, forward = false))
        assertNull(ThreadSwipeRules.neighbour(-1, 3, forward = true))
    }

    @Test fun aNotificationOverTheListIsNotPartOfTheList() {
        assertNull(ThreadSwipeRules.sourceOf(listOf(Route.MailList, Route.Thread("t", "m1", "msg"))))
        // From the search the message is named too, and the search is the order.
        assertEquals(Source.SEARCH, ThreadSwipeRules.sourceOf(listOf(Route.MailList, Route.Search, Route.Thread("t", null, "msg"))))
    }

    // ---- the memory ----

    @Test fun theMemoryKeepsTheOrderAndEveryVisitedConversation() {
        val memory = ThreadOrderMemory()
        memory.remember(listOf("a", "b"), "a")
        memory.remember(listOf("a", "b", "c"), "b")
        assertEquals(listOf("a", "b", "c"), memory.snapshot)
        assertEquals(setOf("a", "b"), memory.visited)
    }
}
