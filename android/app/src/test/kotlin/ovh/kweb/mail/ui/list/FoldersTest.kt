package ovh.kweb.mail.ui.list

import org.junit.Assert.assertEquals
import org.junit.Test
import uniffi.kweb_core.Mailbox

class FoldersTest {
    private fun box(id: String, name: String = id, role: String? = null, parent: String? = null, order: UInt = 0u) =
        Mailbox(id, name, role, parent, order, 0u, 0u)

    private fun listed(vararg boxes: Mailbox) = Folders.ordered(boxes.toList()).map { it.mailbox.id to it.depth }

    @Test fun systemFoldersComeFirstInTheirOwnOrder() = assertEquals(
        listOf("inbox" to 0, "drafts" to 0, "sent" to 0, "archive" to 0, "junk" to 0, "trash" to 0, "z" to 0),
        listed(
            box("z"),
            box("trash", role = "trash"),
            box("junk", role = "junk"),
            box("archive", role = "archive"),
            box("sent", role = "sent"),
            box("drafts", role = "drafts"),
            box("inbox", role = "inbox"),
        ),
    )

    @Test fun otherFoldersFormATreeBySortOrderThenName() = assertEquals(
        listOf("inbox" to 0, "b" to 0, "b1" to 1, "b11" to 2, "b2" to 1, "a" to 0, "c" to 0),
        listed(
            box("a", order = 5u),
            box("c", name = "Cee", order = 5u),
            box("b", order = 1u),
            box("b2", parent = "b", order = 2u),
            box("b1", parent = "b", order = 1u),
            box("b11", parent = "b1"),
            box("inbox", role = "inbox"),
        ),
    )

    @Test fun aFolderUnderASystemFolderIsListedBelowIt() = assertEquals(
        listOf("inbox" to 0, "work" to 1, "sent" to 0),
        listed(box("sent", role = "sent"), box("work", parent = "inbox"), box("inbox", role = "inbox")),
    )

    @Test fun aFolderWhoseParentIsMissingIsTopLevel() = assertEquals(listOf("x" to 0), listed(box("x", parent = "gone")))

    @Test fun aParentCycleLosesNothing() =
        assertEquals(setOf("a", "b"), Folders.ordered(listOf(box("a", parent = "b"), box("b", parent = "a"))).map { it.mailbox.id }.toSet())

    @Test fun namesAreComparedIgnoringCase() = assertEquals(listOf("b" to 0, "a" to 0), listed(box("a", name = "Zed"), box("b", name = "alpha")))

    @Test fun findsAFolderByRole() = assertEquals("t", Folders.withRole(listOf(box("i", role = "inbox"), box("t", role = "trash")), "trash")?.id)
}
