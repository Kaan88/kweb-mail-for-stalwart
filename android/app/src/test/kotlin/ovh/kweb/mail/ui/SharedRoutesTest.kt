package ovh.kweb.mail.ui

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Test

class SharedRoutesTest {
    private val base = BackStack.initial + Route.Shared + Route.SharedCalendar("acc", "o@x.org")

    @Test fun savingASharedEventDropsTheSharedEventScreenToo() {
        val stack = base + Route.SharedEventView("acc", "e1", "X") + Route.EditEvent("e1", null, "acc")
        assertEquals(base, BackStack.afterEventSaved(stack))
    }

    @Test fun aNewSharedEventLeavesTheSharedAgendaOnTop() {
        val stack = base + Route.EditEvent(null, "2026-11-20", "acc")
        assertEquals(base, BackStack.afterEventSaved(stack))
    }

    @Test fun savingASharedContactLeavesTheContactOrTheListOnTop() {
        val list = BackStack.initial + Route.Shared + Route.SharedContacts("acc", "o@x.org")
        assertEquals(list, BackStack.afterContactSaved(list + Route.EditContact(null, "acc")))
        val view = list + Route.SharedContactView("acc", "c1")
        assertEquals(view, BackStack.afterContactSaved(view + Route.EditContact("c1", "acc")))
    }

    @Test fun theSameEntryInTheOwnAndInASharedAccountAreDifferentRoutes() {
        assertNotEquals(Route.EditEvent("e1", null), Route.EditEvent("e1", null, "acc"))
        assertNotEquals(Route.EditContact("c1"), Route.EditContact("c1", "acc"))
    }
}
