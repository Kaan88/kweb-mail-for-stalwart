package ovh.kweb.mail.ui.sending

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class IncomingShareRulesTest {
    private val own = "ovh.kweb.mail.files"
    private fun raw(
        action: String? = IncomingShareRules.ACTION_SEND,
        text: String? = null,
        subject: String? = null,
        streams: List<String> = emptyList(),
    ) = RawShare(action, text, subject, streams)

    @Test fun textGoesInTheBodyAndASubjectInTheSubject() {
        val c = IncomingShareRules.parse(raw(text = "https://example.org/a\n", subject = "A page"), own)!!
        assertEquals("https://example.org/a", c.body)
        assertEquals("A page", c.subject)
        assertEquals(emptyList<String>(), c.files)
    }

    @Test fun otherActionsAreNotShares() {
        assertNull(IncomingShareRules.parse(raw(action = "android.intent.action.VIEW", text = "x"), own))
        assertNull(IncomingShareRules.parse(raw(action = null, text = "x"), own))
    }

    @Test fun nothingToShareIsNull() {
        assertNull(IncomingShareRules.parse(raw(text = "  \n ", subject = " "), own))
        assertNull(IncomingShareRules.parse(raw(streams = listOf("file:///data/data/x/db")), own))
    }

    @Test fun onlyContentAddressesAreFiles() {
        val c = IncomingShareRules.parse(
            raw(text = "hi", streams = listOf("content://media/external/images/1", "file:///data/data/ovh.kweb.mail/files/secret", "http://example.org/x.png", "")),
            own,
        )!!
        assertEquals(listOf("content://media/external/images/1"), c.files)
        assertEquals(2, c.skipped)
    }

    @Test fun theAppsOwnProviderIsRefused() {
        val c = IncomingShareRules.parse(
            raw(action = IncomingShareRules.ACTION_SEND_MULTIPLE, streams = listOf("content://ovh.kweb.mail.files/attachments/a.pdf", "content://com.android.providers/1")),
            own,
        )!!
        assertEquals(listOf("content://com.android.providers/1"), c.files)
        assertEquals(1, c.skipped)
    }

    @Test fun anAuthorityIsFoundThroughUserInfoAndQuery() {
        assertEquals(1, IncomingShareRules.parse(raw(text = "x", streams = listOf("content://0@ovh.kweb.mail.files/a")), own)!!.skipped)
        assertEquals(1, IncomingShareRules.parse(raw(text = "x", streams = listOf("CONTENT://OVH.KWEB.MAIL.FILES?x=1")), own)!!.skipped)
    }

    @Test fun anEncodedAuthorityCannotHideTheAppsOwnProvider() {
        val c = IncomingShareRules.parse(raw(text = "x", streams = listOf("content://ovh.kweb.mail%2Efiles/attachments/a", "content://%6Fvh.kweb.mail.files/a")), own)!!
        assertEquals(emptyList<String>(), c.files)
        assertEquals(2, c.skipped)
    }

    @Test fun repeatedFilesCountOnce() {
        val c = IncomingShareRules.parse(raw(action = IncomingShareRules.ACTION_SEND_MULTIPLE, streams = listOf("content://a/1", " content://a/1", "content://a/2")), own)!!
        assertEquals(listOf("content://a/1", "content://a/2"), c.files)
        assertEquals(0, c.skipped)
    }

    @Test fun lineEndingsAndSubjectWhitespaceAreTidied() {
        val c = IncomingShareRules.parse(raw(text = "a\r\nb\rc", subject = "Hello \n  world"), own)!!
        assertEquals("a\nb\nc", c.body)
        assertEquals("Hello world", c.subject)
    }

    @Test fun sharedTextComesBeforeASignature() {
        assertEquals("text\n\n-- \nme", IncomingShareRules.body("text", "-- \nme"))
        assertEquals("text", IncomingShareRules.body("text", "\n\n"))
        assertEquals("-- \nme", IncomingShareRules.body("", "-- \nme"))
    }
}
