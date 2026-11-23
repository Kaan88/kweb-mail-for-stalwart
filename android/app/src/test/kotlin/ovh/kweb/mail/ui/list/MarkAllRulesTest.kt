package ovh.kweb.mail.ui.list

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.kweb_core.Mailbox

class MarkAllRulesTest {
    private fun box(id: String, parent: String? = null, role: String? = null, unread: UInt = 0u) =
        Mailbox(id, id, role, parent, 0u, 0u, unread, false)

    private val boxes = listOf(
        box("inbox", role = "inbox", unread = 3u),
        box("work"),
        box("a", "work", unread = 2u),
        box("drafts", role = "drafts", unread = 1u),
        box("empty", unread = 0u),
    )

    @Test
    fun aFolderWithUnreadMailOffersIt() {
        assertTrue(MarkAllRules.offered(boxes, "inbox", false))
        assertFalse(MarkAllRules.offered(boxes, "empty", false))
    }

    @Test
    fun subfoldersCountOnlyWhenTheirChoiceIsOn() {
        assertFalse(MarkAllRules.offered(boxes, "work", false))
        assertTrue(MarkAllRules.offered(boxes, "work", true))
        assertEquals(listOf("work", "a"), MarkAllRules.scope(boxes, "work", true))
        assertEquals(listOf("work"), MarkAllRules.scope(boxes, "work", false))
    }

    @Test
    fun draftsAndUnknownFoldersNeverOfferIt() {
        assertFalse(MarkAllRules.offered(boxes, "drafts", false))
        assertFalse(MarkAllRules.offered(boxes, "nope", false))
    }
}
