package ovh.kweb.mail.ui.notifications

import org.junit.Assert.assertEquals
import org.junit.Test
import ovh.kweb.mail.accounts.AccountRecord
import uniffi.kweb_core.AddressChoice
import uniffi.kweb_core.AddressMode

class NotificationsRulesTest {
    private fun choice(address: String, mode: AddressMode = AddressMode.NOTIFY) = AddressChoice("Me", address, mode)

    @Test
    fun theDialogOffersTheDefaultFirstAndEveryMode() {
        assertEquals(AddressMode.NOTIFY, NotificationsRules.modes.first())
        assertEquals(AddressMode.entries.toSet(), NotificationsRules.modes.toSet())
    }

    @Test
    fun aChoiceReplacesOnlyItsAddressWithoutRegardToCase() {
        val list = listOf(choice("Me@x.org"), choice("work@x.org"))
        val changed = NotificationsRules.with(list, "me@X.org", AddressMode.OFF)
        assertEquals(listOf(AddressMode.OFF, AddressMode.NOTIFY), changed.map { it.mode })
        assertEquals(list.map { it.address }, changed.map { it.address })
    }

    private val record = AccountRecord("bbbb", legacy = false, username = "b", serverUrl = "https://x", jmapAccountId = "j", addedAt = 2)

    @Test
    fun anAddressNotifiesInItsOwnLoudChannelAndQuietInItsOwnQuietOneAndOffInNone() {
        val list = listOf(
            choice("Me@x.org"),
            choice("work@x.org", AddressMode.SILENT),
            choice("shop@x.org", AddressMode.OFF),
        )
        assertEquals(
            setOf("mail.a.bbbb.me@x.org", "mail_quiet.a.bbbb.work@x.org"),
            NotificationsRules.wantedChannels(record, list),
        )
    }

    @Test
    fun theKeyOfAnAddressIsLowerCaseLikeTheCoresAndTheChannelIdCarriesTheAccount() {
        assertEquals("me@x.org", NotificationsRules.key(" Me@X.org "))
        val other = record.copy(id = "cccc")
        assertEquals(
            false,
            NotificationsRules.wantedChannels(record, listOf(choice("me@x.org"))).any { it in NotificationsRules.wantedChannels(other, listOf(choice("me@x.org"))) },
        )
    }
}
