package ovh.kweb.mail.ui.list

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.kweb_core.Mailbox
import uniffi.kweb_core.MessageHeader
import uniffi.kweb_core.ThreadRow

class SubfolderRulesTest {
    private fun box(id: String, parent: String? = null, role: String? = null) = Mailbox(id, id, role, parent, 0u, 0u, 0u, false)

    // work > a > b, work > c, and an unrelated folder
    private val boxes = listOf(box("inbox", role = "inbox"), box("work"), box("a", "work"), box("b", "a"), box("c", "work"), box("other"))

    private fun row(vararg inBoxes: String) = ThreadRow(
        newest = MessageHeader("m", "t", inBoxes.toList(), "", "", emptyList(), "s", "p", 0L, false, false, false, 0uL),
        messageIds = listOf("m"),
        unreadCount = 0u,
        isFlagged = false,
        hasAttachment = false,
        senders = emptyList(),
    )

    @Test
    fun onlyAFolderWithFoldersInsideOffersTheEntry() {
        assertTrue(SubfolderRules.offered(boxes, "work"))
        assertTrue(SubfolderRules.offered(boxes, "a"))
        assertFalse(SubfolderRules.offered(boxes, "b"))
        assertFalse(SubfolderRules.offered(boxes, "inbox"))
    }

    @Test
    fun theScopeIsTheFolderThenEveryFolderBelowItAtAnyDepth() {
        assertEquals(listOf("work", "a", "b", "c"), SubfolderRules.scope(boxes, "work", true))
        assertEquals(listOf("a", "b"), SubfolderRules.scope(boxes, "a", true))
        assertEquals(listOf("work"), SubfolderRules.scope(boxes, "work", false))
        // A folder with none is itself, and a list of one is not a combined list.
        assertEquals(listOf("b"), SubfolderRules.scope(boxes, "b", true))
        assertFalse(SubfolderRules.isCombined(SubfolderRules.scope(boxes, "b", true)))
        assertTrue(SubfolderRules.isCombined(SubfolderRules.scope(boxes, "work", true)))
    }

    @Test
    fun aParentCycleCannotMakeTheScopeLoop() {
        val loop = listOf(box("x", "y"), box("y", "x"))
        assertEquals(listOf("x", "y"), SubfolderRules.scope(loop, "x", true).sorted())
    }

    @Test
    fun aRowNamesItsFolderOnlyWhenItIsNotInTheParent() {
        val scope = SubfolderRules.scope(boxes, "work", true)
        assertNull(SubfolderRules.rowFolder(row("work"), "work", boxes, scope))
        assertEquals("a", SubfolderRules.rowFolder(row("a"), "work", boxes, scope)?.id)
        assertEquals("b", SubfolderRules.rowFolder(row("b"), "work", boxes, scope)?.id)
        // In the parent and a subfolder: it is in the parent, no name.
        assertNull(SubfolderRules.rowFolder(row("a", "work"), "work", boxes, scope))
        // In a subfolder and in a folder outside the scope: the one in the scope.
        assertEquals("c", SubfolderRules.rowFolder(row("other", "c"), "work", boxes, scope)?.id)
        // In nothing of the scope: nothing to name.
        assertNull(SubfolderRules.rowFolder(row("other"), "work", boxes, scope))
    }

    @Test
    fun aConversationOpensFromItsOwnFolder() {
        val scope = SubfolderRules.scope(boxes, "work", true)
        assertEquals("work", SubfolderRules.openFrom(row("work"), "work", boxes, scope))
        assertEquals("b", SubfolderRules.openFrom(row("b"), "work", boxes, scope))
        assertEquals("work", SubfolderRules.openFrom(row("other"), "work", boxes, scope))
    }
}
