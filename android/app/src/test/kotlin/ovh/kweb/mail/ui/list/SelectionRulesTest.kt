package ovh.kweb.mail.ui.list

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import ovh.kweb.mail.ui.list.SelectionRules.Tap
import uniffi.kweb_core.MessageHeader
import uniffi.kweb_core.ThreadRow

class SelectionRulesTest {
    private fun row(thread: String, unread: UInt = 0u, flagged: Boolean = false, messages: List<String> = emptyList()) = ThreadRow(
        MessageHeader("${thread}1", thread, listOf("m1"), "", "", emptyList(), "", "", 0, false, false, false, 0u),
        messages.ifEmpty { listOf("${thread}1") }, unread, flagged, false, emptyList(),
    )

    private val rows = listOf(row("a", 1u), row("b", 0u, true, listOf("b1", "b2")), row("c"))

    @Test fun aTapOpensUntilSomethingIsChosenAndThenChooses() {
        assertEquals(Tap.OPEN, SelectionRules.tap(false))
        assertEquals(Tap.TOGGLE, SelectionRules.tap(true))
    }

    @Test fun toggleChoosesAndLetsGo() {
        val one = SelectionRules.toggle(emptySet(), "a")
        assertEquals(setOf("a"), one)
        assertEquals(setOf("a", "b"), SelectionRules.toggle(one, "b"))
        // The last one let go ends the selection: the set is empty.
        assertEquals(emptySet<String>(), SelectionRules.toggle(one, "a"))
    }

    @Test fun rowsThatLeftTheListAreDropped() {
        assertEquals(setOf("a"), SelectionRules.prune(setOf("a", "gone"), listOf("a", "b")))
        assertEquals(emptySet<String>(), SelectionRules.prune(setOf("gone"), listOf("a")))
        assertEquals(emptySet<String>(), SelectionRules.prune(emptySet(), listOf("a")))
    }

    @Test fun selectAllIsEveryLoadedRow() {
        assertEquals(setOf("a", "b", "c"), SelectionRules.all(rows))
        assertTrue(SelectionRules.allSelected(rows, setOf("a", "b", "c")))
        assertFalse(SelectionRules.allSelected(rows, setOf("a", "b")))
        assertFalse(SelectionRules.allSelected(emptyList(), emptySet()))
    }

    @Test fun theChosenRowsComeInTheListsOrder() {
        assertEquals(listOf("a", "c"), SelectionRules.rowsOf(rows, setOf("c", "a")).map { it.newest.threadId })
        assertEquals(emptyList<ThreadRow>(), SelectionRules.rowsOf(rows, emptySet()))
    }

    @Test fun anActionGetsTheMessagesOfEveryChosenRowOnce() {
        val chosen = SelectionRules.rowsOf(rows, setOf("a", "b"))
        assertEquals(listOf("a1", "b1", "b2"), SelectionRules.messageIds(chosen))
        assertEquals(listOf("x", "y"), SelectionRules.messageIds(listOf(row("p", messages = listOf("x", "y")), row("q", messages = listOf("y")))))
    }

    @Test fun markReadWhileAnyIsUnread() {
        assertTrue(SelectionRules.marksRead(SelectionRules.rowsOf(rows, setOf("a", "c"))))
        assertFalse(SelectionRules.marksRead(SelectionRules.rowsOf(rows, setOf("b", "c"))))
    }

    @Test fun theStarGoesOnlyWhenEveryChosenRowHasOne() {
        assertTrue(SelectionRules.unstars(SelectionRules.rowsOf(rows, setOf("b"))))
        assertFalse(SelectionRules.unstars(SelectionRules.rowsOf(rows, setOf("a", "b"))))
        assertFalse(SelectionRules.unstars(emptyList()))
    }

    @Test fun archiveIsNotOfferedInTheArchive() {
        assertFalse(SelectionRules.archiveOffered("archive"))
        assertTrue(SelectionRules.archiveOffered("inbox"))
        assertTrue(SelectionRules.archiveOffered(null))
    }
}
