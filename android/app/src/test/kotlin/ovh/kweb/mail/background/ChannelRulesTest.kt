package ovh.kweb.mail.background

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import ovh.kweb.mail.accounts.AccountRecord

class ChannelRulesTest {
    private val legacy = AccountRecord("aaaa", legacy = true, username = "a", serverUrl = "https://x", jmapAccountId = "j", addedAt = 1)
    private val other = AccountRecord("bbbb", legacy = false, username = "b", serverUrl = "https://x", jmapAccountId = "j", addedAt = 2)

    @Test
    fun theLegacyAccountKeepsTheChannelTheOldVersionMade() {
        // Pinned: a channel cannot be changed once made, and the user's sound choice lives in it.
        assertEquals("mail", ChannelRules.mail(legacy))
        assertEquals("mail", ChannelRules.forMail(legacy, quiet = false))
        assertNull(ChannelRules.group(legacy))
    }

    @Test
    fun everyOtherAccountHasChannelsAndAGroupOfItsOwn() {
        assertEquals("mail.bbbb", ChannelRules.mail(other))
        assertEquals("mail_quiet.bbbb", ChannelRules.forMail(other, quiet = true))
        assertEquals("account.bbbb", ChannelRules.group(other))
        assertNotEquals(ChannelRules.mail(legacy), ChannelRules.mail(other))
        assertNotEquals(ChannelRules.quiet(legacy), ChannelRules.quiet(other))
    }

    @Test
    fun quietAndLoudAreDifferentChannelsAndNeitherIsTheDevicesOwn() {
        for (r in listOf(legacy, other)) {
            assertNotEquals(ChannelRules.mail(r), ChannelRules.quiet(r))
            assertTrue(ChannelRules.isMailChannelOf(r, ChannelRules.mail(r)))
            assertTrue(ChannelRules.isMailChannelOf(r, ChannelRules.quiet(r)))
            assertFalse(ChannelRules.isMailChannelOf(r, Channels.REMINDERS))
            assertFalse(ChannelRules.isMailChannelOf(r, Channels.CONNECTION))
        }
        assertFalse(ChannelRules.isMailChannelOf(other, ChannelRules.mail(legacy)))
    }

    @Test
    fun onlyTheLegacyChannelNeedsTheAccountInItsName() {
        assertEquals("Mail", ChannelRules.name("Mail", legacy, null))
        assertEquals("Mail · a", ChannelRules.name("Mail", legacy, "a"))
        assertEquals("Mail", ChannelRules.name("Mail", other, "b"))
    }
}
