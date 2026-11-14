package ovh.kweb.mail.ui.thread

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.kweb_core.MessageHeader

class BlockSenderRulesTest {
    private fun from(email: String) = MessageHeader("1", "t", listOf("in"), "", email, emptyList(), "s", "p", 0L, false, false, false, 0uL)

    @Test fun aMessageFromSomeoneElseCanBeBlocked() {
        assertTrue(ThreadRules.canBlockSender(from("spam@x.org"), "me@x.org"))
        assertTrue(ThreadRules.canBlockSender(from("spam@x.org"), null))
    }

    @Test fun theUsersOwnAddressIsNotOfferedWhateverTheCase() {
        assertFalse(ThreadRules.canBlockSender(from("Me@X.org"), "me@x.org"))
    }

    @Test fun aMessageWithoutASenderAddressCannotBeBlocked() {
        assertFalse(ThreadRules.canBlockSender(from(""), "me@x.org"))
        assertFalse(ThreadRules.canBlockSender(from("  "), null))
    }
}
