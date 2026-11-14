package ovh.kweb.mail.ui.admin

import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import ovh.kweb.mail.R

/** The lists move between two reads: a row that is already on screen must not come twice (a repeated key closes a LazyColumn). */
class PagedLoaderTest {
    private val rig = Rig()

    @After fun tearDown() = rig.stop()

    @Test fun aRowThatMovedIntoTheNextPageIsNotAddedTwice() {
        var rows = (1..120).map { "r$it" }
        val loader = PagedLoader<String>(rig.scope, rig.io, 50u, R.string.admin_load_failed, { it }) { offset, limit, _ ->
            Page(rows.drop(offset.toInt()).take(limit.toInt()), rows.size)
        }
        loader.reload(quiet = false)
        waitFor("page 1") { loader.state.value.phase == ListPhase.READY }
        // A new message sorts first: every row moves down by one, so r50 is the first row of the next page too.
        rows = listOf("new") + rows
        loader.loadMore()
        waitFor("page 2") { loader.state.value.items.size == 99 && !loader.state.value.loadingMore }
        val items = loader.state.value.items
        assertEquals(items.distinct(), items)
        assertTrue(loader.state.value.more)
        loader.loadMore()
        waitFor("page 3") { loader.state.value.items.size == 120 && !loader.state.value.loadingMore }
        assertEquals(loader.state.value.items.distinct(), loader.state.value.items)
    }

    @Test fun aPageOfOnlyRepeatsDoesNotEndPagingOrLoopOnTheSameOffset() {
        val rows = (1..60).map { "r$it" }
        val offsets = mutableListOf<UInt>()
        val loader = PagedLoader<String>(rig.scope, rig.io, 50u, R.string.admin_load_failed, { it }) { offset, limit, _ ->
            offsets += offset
            // The server keeps giving the first rows again for a later page.
            Page(if (offset == 0u) rows.take(50) else rows.take(10), 200)
        }
        loader.reload(quiet = false)
        waitFor("page 1") { loader.state.value.phase == ListPhase.READY }
        loader.loadMore()
        waitFor("page 2") { offsets.size == 2 && !loader.state.value.loadingMore }
        assertEquals(50, loader.state.value.items.size)
        loader.loadMore()
        waitFor("page 3") { offsets.size == 3 && !loader.state.value.loadingMore }
        assertEquals(listOf(0u, 50u, 60u), offsets)
        assertTrue("paging goes on", loader.state.value.more)
    }
}
