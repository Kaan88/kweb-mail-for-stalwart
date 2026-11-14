package ovh.kweb.mail.ui.calendar

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.kweb_core.Participant

class EventRulesTest {
    private fun person(name: String, email: String = "", organizer: Boolean = false, me: Boolean = false, status: String = "accepted") =
        Participant(name, email, status, organizer, me)

    @Test fun anInvitationHasMeAmongTheGuestsAndNotAsOrganizer() {
        val guests = listOf(person("Org", organizer = true), person("Me", me = true))
        assertTrue(EventRules.isInvitation(guests, "needs-action"))
    }

    @Test fun myOwnEventIsNoInvitation() {
        assertFalse(EventRules.isInvitation(listOf(person("Me", organizer = true, me = true)), "accepted"))
    }

    @Test fun anEventWithoutMeIsNoInvitation() {
        assertFalse(EventRules.isInvitation(listOf(person("Org", organizer = true), person("Other")), null))
        assertFalse(EventRules.isInvitation(listOf(person("Org", organizer = true), person("Me", me = true)), null))
    }

    @Test fun anEntryThatDoesNotRepeatIsDeletedAsItIs() = assertEquals(EventRules.DeleteAsk.PLAIN, EventRules.deleteAsk(false, null))

    @Test fun anOccurrenceOfARepeatingEventAsksOneOrAll() =
        assertEquals(EventRules.DeleteAsk.ONE_OR_ALL, EventRules.deleteAsk(true, "2026-11-12T10:00:00"))

    @Test fun aRepeatingEventWithoutAnOccurrenceGoesWholeButAfterWarning() =
        assertEquals(EventRules.DeleteAsk.WHOLE_SERIES, EventRules.deleteAsk(true, null))

    @Test fun aRecurrenceIdOnAnEntryThatDoesNotRepeatChangesNothing() = assertEquals(EventRules.DeleteAsk.PLAIN, EventRules.deleteAsk(false, "x"))

    @Test fun guestsAreOrganizerThenMeThenByName() {
        val sorted = EventRules.sorted(listOf(person("Zeynep"), person("ayşe"), person("Me", me = true), person("Org", organizer = true)))
        assertEquals(listOf("Org", "Me", "ayşe", "Zeynep"), sorted.map { it.name })
    }

    @Test fun aGuestWithoutANameIsLabelledByTheAddress() {
        assertEquals("a@example.org", EventRules.label(person("  ", "a@example.org")))
        assertEquals("Ayşe", EventRules.label(person(" Ayşe ", "a@example.org")))
    }
}
