package ovh.kweb.mail.ui.accounts

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import ovh.kweb.mail.ui.Route
import ovh.kweb.mail.ui.admin.AdminScreen

class SwitchRulesTest {
    @Test
    fun theSameAccountJustOpens() {
        for (top in listOf(Route.MailList, Route.Compose("d"), Route.EditEvent(null, null), Route.Away)) {
            assertEquals(Incoming.OPEN, SwitchRules.onIncoming(top, sameAccount = true))
        }
    }

    @Test
    fun anotherAccountOnAPlainScreenSwitches() {
        for (top in listOf(Route.MailList, Route.Calendar, Route.Settings, Route.Thread("t", null, null), Route.Search, Route.DeliveryCheck)) {
            assertEquals(Incoming.SWITCH_THEN_OPEN, SwitchRules.onIncoming(top, sameAccount = false))
        }
    }

    @Test
    fun aMessageBeingWrittenIsSavedAsADraftAndTheAppSwitches() {
        assertEquals(Incoming.SWITCH_SAVING_DRAFT, SwitchRules.onIncoming(Route.Compose("d"), sameAccount = false))
    }

    @Test
    fun aFormIsNeverDroppedSilently() {
        val forms = listOf(
            Route.EditEvent("e", null), Route.EditContact(null), Route.Filters(), Route.Away, Route.Senders, Route.Assistant,
            Route.AddAccount, Route.SignInAgain, Route.Admin(AdminScreen.NewAccount), Route.Admin(AdminScreen.Account("u1")),
        )
        for (top in forms) assertEquals(top.toString(), Incoming.ASK, SwitchRules.onIncoming(top, sameAccount = false))
    }

    @Test
    fun onlyFormsAreForms() {
        assertTrue(SwitchRules.isForm(Route.Filters("m")))
        assertFalse(SwitchRules.isForm(Route.Folders()))
        assertFalse(SwitchRules.isForm(Route.Event("e", null)))
        assertFalse(SwitchRules.isForm(Route.Admin(AdminScreen.Queue)))
    }
}
