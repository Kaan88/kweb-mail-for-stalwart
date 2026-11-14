package ovh.kweb.mail.ui

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Test

class BackStackTest {
    private val thread = Route.Thread("t1", "inbox", null)

    @Test fun startsAtTheMailList() = assertEquals(listOf(Route.MailList), BackStack.initial)

    @Test fun pushAddsOnTop() = assertEquals(listOf(Route.MailList, Route.Search), BackStack.push(BackStack.initial, Route.Search))

    @Test fun theSameRouteTwiceCountsOnce() {
        val once = BackStack.push(BackStack.initial, thread)
        assertEquals(once, BackStack.push(once, thread))
    }

    @Test fun anotherThreadIsAnotherRoute() {
        val stack = BackStack.push(BackStack.push(BackStack.initial, thread), Route.Thread("t2", "inbox", null))
        assertEquals(3, stack.size)
    }

    @Test fun popRemovesTheTop() = assertEquals(BackStack.initial, BackStack.pop(listOf(Route.MailList, Route.Search)))

    @Test fun theRootIsNeverPopped() = assertEquals(BackStack.initial, BackStack.pop(BackStack.initial))

    @Test fun aNotificationOpensTheMessageOverTheList() = assertEquals(
        listOf(Route.MailList, Route.Thread("t9", "inbox", "m9")),
        BackStack.forNotification("t9", "inbox", "m9"),
    )

    @Test fun aNotificationReplyOpensTheDraftOverTheList() =
        assertEquals(listOf(Route.MailList, Route.Compose("d1")), BackStack.forCompose("d1"))

    @Test fun composeOverAThreadGoesBackToTheThread() {
        val stack = BackStack.push(BackStack.push(BackStack.initial, thread), Route.Compose("d1"))
        assertEquals(listOf(Route.MailList, thread), BackStack.pop(stack))
    }

    @Test fun theSameDraftTwiceCountsOnce() {
        val once = BackStack.push(BackStack.initial, Route.Compose("d1"))
        assertEquals(once, BackStack.push(once, Route.Compose("d1")))
    }

    @Test fun sendingReturnsToTheList() = assertEquals(BackStack.initial, BackStack.afterSend())

    @Test fun settingsIsAScreenOverTheList() =
        assertEquals(listOf(Route.MailList, Route.Settings), BackStack.push(BackStack.initial, Route.Settings))

    @Test fun theAgendaSitsOnTheMailList() =
        assertEquals(listOf(Route.MailList, Route.Calendar), BackStack.switchTo(Route.Calendar))

    @Test fun mailIsTheBottomOfTheStack() =
        assertEquals(BackStack.initial, BackStack.switchTo(Route.MailList))

    @Test fun backFromTheAgendaGoesToTheMailListAndThenOut() {
        val stack = BackStack.switchTo(Route.Calendar)
        assertEquals(BackStack.initial, BackStack.pop(stack))
        assertEquals(BackStack.initial, BackStack.pop(BackStack.pop(stack)))
    }

    @Test fun anEventOverTheAgendaGoesBackToTheAgenda() {
        val event = Route.Event("e1", null)
        val stack = BackStack.push(BackStack.switchTo(Route.Calendar), event)
        assertEquals(listOf(Route.MailList, Route.Calendar, event), stack)
        assertEquals(BackStack.switchTo(Route.Calendar), BackStack.pop(stack))
    }

    @Test fun theFormOverAnEventGoesBackToTheEvent() {
        val event = Route.Event("e1", "2026-11-12T10:00:00")
        val stack = BackStack.push(BackStack.push(BackStack.switchTo(Route.Calendar), event), Route.EditEvent("e1", null))
        assertEquals(event, BackStack.pop(stack).last())
    }

    @Test fun theSameOccurrenceTwiceCountsOnce() {
        val once = BackStack.push(BackStack.switchTo(Route.Calendar), Route.Event("e1", "r1"))
        assertEquals(once, BackStack.push(once, Route.Event("e1", "r1")))
    }

    @Test fun twoOccurrencesOfOneEventAreTwoRoutes() =
        assertNotEquals(Route.Event("e1", "r1"), Route.Event("e1", "r2"))

    @Test fun aReminderOpensTheEventOverTheAgenda() =
        assertEquals(listOf(Route.MailList, Route.Calendar, Route.Event("e7", null)), BackStack.forReminder("e7", null))

    @Test fun backFromAReminderGoesToTheAgenda() =
        assertEquals(BackStack.switchTo(Route.Calendar), BackStack.pop(BackStack.forReminder("e7", null)))

    @Test fun sendingStillReturnsToTheMailList() =
        assertEquals(BackStack.initial, BackStack.afterSend())

    @Test fun aReminderOfARepeatingEventOpensThatOccurrence() =
        assertEquals(Route.Event("e7", "2026-11-19T10:00:00"), BackStack.forReminder("e7", "2026-11-19T10:00:00").last())

    @Test fun aTargetReplacesTheStackWhileAScreenIsOnTop() {
        val stack = listOf(Route.MailList, Route.Calendar, Route.Event("e1", null))
        assertEquals(BackStack.forReminder("e7", null), BackStack.showOver(stack, BackStack.forReminder("e7", null)))
    }

    @Test fun aTargetGoesOverAFormSoBackReturnsToIt() {
        val form = Route.EditEvent("e1", null)
        val stack = listOf(Route.MailList, Route.Calendar, form)
        val shown = BackStack.showOver(stack, BackStack.forReminder("e7", null))
        assertEquals(stack + Route.Event("e7", null), shown)
        assertEquals(stack, BackStack.pop(shown))
    }

    @Test fun aTargetGoesOverAMessageBeingWritten() {
        val stack = listOf(Route.MailList, Route.Compose("d1"))
        assertEquals(stack + Route.Thread("t", null, "m"), BackStack.showOver(stack, BackStack.forNotification("t", null, "m")))
    }

    @Test fun aTargetWithNothingToShowLeavesAFormAlone() {
        val stack = listOf(Route.MailList, Route.Compose("d1"))
        assertEquals(stack, BackStack.showOver(stack, BackStack.initial))
    }

    @Test fun savingAnEntryDropsTheFormAndTheEventScreen() {
        val stack = listOf(Route.MailList, Route.Calendar, Route.Event("e1", "r1"), Route.EditEvent("e1", null))
        assertEquals(listOf(Route.MailList, Route.Calendar), BackStack.afterEventSaved(stack))
    }

    @Test fun savingANewEntryGoesBackToWhereItWasStarted() =
        assertEquals(BackStack.switchTo(Route.Calendar), BackStack.afterEventSaved(BackStack.switchTo(Route.Calendar) + Route.EditEvent(null, "2026-11-12")))

    @Test fun theContactsSitOnTheMailList() =
        assertEquals(listOf(Route.MailList, Route.Contacts), BackStack.switchTo(Route.Contacts))

    @Test fun backFromTheContactsGoesToTheMailList() =
        assertEquals(BackStack.initial, BackStack.pop(BackStack.switchTo(Route.Contacts)))

    @Test fun switchingBetweenTheRootsReplacesTheStack() {
        assertEquals(listOf(Route.MailList, Route.Calendar), BackStack.switchTo(Route.Calendar))
        assertEquals(BackStack.initial, BackStack.switchTo(Route.MailList))
    }

    @Test fun onlyTheThreeRootsShowTheBottomBar() {
        assertEquals(
            listOf(true, true, true),
            listOf(Route.MailList, Route.Calendar, Route.Contacts).map(BackStack::isRoot),
        )
        assertEquals(
            listOf(false, false, false, false),
            listOf(Route.Contact("c1"), Route.EditContact(null), Route.Away, Route.Settings).map(BackStack::isRoot),
        )
    }

    @Test fun theFormOverAContactGoesBackToTheContact() {
        val contact = Route.Contact("c1")
        val stack = BackStack.push(BackStack.push(BackStack.switchTo(Route.Contacts), contact), Route.EditContact("c1"))
        assertEquals(contact, BackStack.pop(stack).last())
    }

    @Test fun aTargetGoesOverAContactForm() {
        val stack = listOf(Route.MailList, Route.Contacts, Route.EditContact(null))
        val shown = BackStack.showOver(stack, BackStack.forReminder("e7", null))
        assertEquals(stack + Route.Event("e7", null), shown)
    }

    @Test fun savingAChangedContactShowsTheContactAgain() {
        val stack = listOf(Route.MailList, Route.Contacts, Route.Contact("c1"), Route.EditContact("c1"))
        assertEquals(listOf(Route.MailList, Route.Contacts, Route.Contact("c1")), BackStack.afterContactSaved(stack))
    }

    @Test fun savingANewContactLeavesTheList() =
        assertEquals(BackStack.switchTo(Route.Contacts), BackStack.afterContactSaved(BackStack.switchTo(Route.Contacts) + Route.EditContact(null)))

    @Test fun theAwayMessageIsAScreenOverSettings() =
        assertEquals(listOf(Route.MailList, Route.Settings, Route.Away), BackStack.push(BackStack.push(BackStack.initial, Route.Settings), Route.Away))

    @Test fun aTargetGoesOverTheAwayMessage() {
        val stack = listOf(Route.MailList, Route.Settings, Route.Away)
        val shown = BackStack.showOver(stack, BackStack.forNotification("t", null, "m"))
        assertEquals(stack + Route.Thread("t", null, "m"), shown)
        assertEquals(stack, BackStack.pop(shown))
    }

    @Test fun theAssistantSettingsAreAScreenOverSettings() =
        assertEquals(listOf(Route.MailList, Route.Settings, Route.Assistant), BackStack.push(BackStack.push(BackStack.initial, Route.Settings), Route.Assistant))

    @Test fun aTargetGoesOverTheAssistantSettingsAndTheyAreNotARoot() {
        val stack = listOf(Route.MailList, Route.Settings, Route.Assistant)
        val shown = BackStack.showOver(stack, BackStack.forNotification("t", null, "m"))
        assertEquals(stack + Route.Thread("t", null, "m"), shown)
        assertEquals(false, BackStack.isRoot(Route.Assistant))
    }
}
