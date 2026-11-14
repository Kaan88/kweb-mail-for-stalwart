package ovh.kweb.mail.background

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import uniffi.kweb_core.Mailbox

class NotificationTargetTest {
    private fun box(id: String, role: String? = null, notify: Boolean = false) = Mailbox(id, id, role, null, 0u, 0u, 0u, notify)

    private val boxes = listOf(box("in", "inbox", true), box("news", notify = true), box("quiet"), box("ar", "archive"))

    @Test fun theInboxWinsWhenTheMessageIsInIt() {
        assertEquals("in", NotificationTarget.folderFor(listOf("news", "in"), boxes)?.id)
    }

    @Test fun aFolderThatNotifiesIsTheOneOpenedFrom() {
        assertEquals("news", NotificationTarget.folderFor(listOf("quiet", "news"), boxes)?.id)
    }

    @Test fun otherwiseTheFirstFolderTheCacheKnows() {
        assertEquals("quiet", NotificationTarget.folderFor(listOf("gone", "quiet", "ar"), boxes)?.id)
    }

    @Test fun nothingKnownMeansNoFolder() {
        assertNull(NotificationTarget.folderFor(listOf("gone"), boxes))
        assertNull(NotificationTarget.folderFor(emptyList(), boxes))
    }
}
