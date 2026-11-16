package ovh.kweb.mail.ui

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class HomeTabTest {
    @Test fun homeSitsOnTheMailListLikeTheAgenda() =
        assertEquals(listOf(Route.MailList, Route.Home), BackStack.switchTo(Route.Home))

    @Test fun backFromHomeGoesToTheMailList() =
        assertEquals(BackStack.initial, BackStack.pop(BackStack.switchTo(Route.Home)))

    @Test fun homeIsARootSoItShowsTheBar() {
        assertTrue(BackStack.isRoot(Route.Home))
        assertFalse(BackStack.isRoot(Route.HomeAttention))
    }

    @Test fun theStartIsHomeOnlyWhenItIsOn() {
        assertEquals(BackStack.initial, BackStack.start(false))
        assertEquals(listOf(Route.MailList, Route.Home), BackStack.start(true))
        assertEquals(listOf(Route.MailList), BackStack.initial)
    }

    @Test fun switchingHomeOffTakesItOutOfTheStack() {
        assertEquals(listOf(Route.MailList, Route.Settings), BackStack.withoutHome(listOf(Route.MailList, Route.Home, Route.Settings)))
        assertEquals(BackStack.initial, BackStack.withoutHome(listOf(Route.Home)))
    }

    @Test fun theBarHasFourTabsOrFiveWithHomeFirst() {
        assertEquals(listOf(Route.MailList, Route.Calendar, Route.Contacts, Route.Files), MainTabs.of(false).map { it.route })
        assertEquals(listOf(Route.Home, Route.MailList, Route.Calendar, Route.Contacts, Route.Files), MainTabs.of(true).map { it.route })
        assertEquals(MainTabs.all, MainTabs.of(false))
    }

    @Test fun anEditorForAMovedStartIsAnotherRouteThanThePlainOne() {
        val plain = Route.EditEvent("e1", null)
        val moved = Route.EditEvent("e1", "2026-11-19", startAt = "2026-11-19T15:00:00")
        assertTrue(plain != moved)
    }
}
