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

    @Test
    fun everyAddressHasItsOwnChannelsAndTheIdCarriesTheAccountAndTheAddress() {
        assertEquals("mail.a.bbbb.me@x.org", ChannelRules.address(other, "me@x.org"))
        assertEquals("mail_quiet.a.bbbb.me@x.org", ChannelRules.addressQuiet(other, "me@x.org"))
        assertEquals("mail.a.aaaa.me@x.org", ChannelRules.forMail(legacy, quiet = false, address = "me@x.org"))
        assertEquals("mail_quiet.a.aaaa.me@x.org", ChannelRules.forMail(legacy, quiet = true, address = "me@x.org"))
        // The same address of two accounts is two channels; two addresses of one account too.
        assertNotEquals(ChannelRules.address(legacy, "me@x.org"), ChannelRules.address(other, "me@x.org"))
        assertNotEquals(ChannelRules.address(other, "me@x.org"), ChannelRules.address(other, "work@x.org"))
        assertNotEquals(ChannelRules.address(other, "me@x.org"), ChannelRules.addressQuiet(other, "me@x.org"))
    }

    @Test
    fun mailThatCameToNoAddressKeepsTheAccountsOwnChannels() {
        assertEquals("mail", ChannelRules.forMail(legacy, quiet = false, address = null))
        assertEquals("mail.bbbb", ChannelRules.forMail(other, quiet = false, address = null))
        assertEquals("mail_quiet.bbbb", ChannelRules.forMail(other, quiet = true))
    }

    @Test
    fun anAddressChannelIsRecognisedByItsOwnAccountOnly() {
        val id = ChannelRules.address(other, "me@x.org")
        assertEquals("me@x.org", ChannelRules.addressOf(other, id))
        assertEquals("me@x.org", ChannelRules.addressOf(other, ChannelRules.addressQuiet(other, "me@x.org")))
        assertNull(ChannelRules.addressOf(legacy, id))
        assertNull(ChannelRules.addressOf(other, ChannelRules.mail(other)))
        assertNull(ChannelRules.addressOf(other, Channels.REMINDERS))
        assertTrue(ChannelRules.isMailChannelOf(other, id))
        assertFalse(ChannelRules.isMailChannelOf(legacy, id))
    }

    @Test
    fun anAddressThatIsOffOrGoneHasNoWantedChannel() {
        val wanted = ChannelRules.wantedAddressChannels(other, notify = listOf("me@x.org"), quiet = listOf("work@x.org"))
        assertEquals(setOf("mail.a.bbbb.me@x.org", "mail_quiet.a.bbbb.work@x.org"), wanted)
        assertFalse(ChannelRules.address(other, "shop@x.org") in wanted)
        assertFalse(ChannelRules.address(other, "work@x.org") in wanted)
    }
}
