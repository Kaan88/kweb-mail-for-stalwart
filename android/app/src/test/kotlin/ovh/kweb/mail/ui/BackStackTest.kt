package ovh.kweb.mail.ui

import org.junit.Assert.assertEquals
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
}
