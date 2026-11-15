package ovh.kweb.mail.ui.notifications

import org.junit.Assert.assertEquals
import org.junit.Test
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
}
