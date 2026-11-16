package ovh.kweb.mail.ui.home

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.kweb_core.MessageHeader
import uniffi.kweb_core.Occurrence

class HomeRulesTest {
    private fun entry(id: String, start: Long, end: Long = start + 1800, task: Boolean = false, done: Boolean = false, allDay: Boolean = false, status: String? = null) =
        Occurrence(id, null, "c", id, "", start, end, allDay, task, done, false, status)

    private fun mail(id: String, at: Long, unread: Boolean = true, flagged: Boolean = false, from: String = "", email: String = "a@x.org") =
        MessageHeader(id, "t$id", listOf("inbox"), from, email, emptyList(), "s$id", "p", at, unread, flagged, false, 100uL)

    @Test fun theNextEventsSkipTasksAllDayEndedAndDeclined() {
        val entries = listOf(
            entry("past", 100, 200),
            entry("task", 1200, task = true),
            entry("allday", 1300, allDay = true),
            entry("declined", 1400, status = "declined"),
            entry("second", 5000),
            entry("first", 3000),
        )
        val (next, then) = HomeRules.nextEvents(entries, now = 1000)
        assertEquals("first", next?.eventId)
        assertEquals("second", then?.eventId)
    }

    @Test fun noEventsIsNothing() = assertEquals(null to null, HomeRules.nextEvents(emptyList(), 0))

    @Test fun tasksAreSplitByDay() {
        val entries = listOf(
            entry("over", 50, task = true), entry("today", 150, task = true), entry("done", 160, task = true, done = true),
            entry("tomorrow", 250, task = true), entry("later", 900, task = true), entry("event", 150),
        )
        val c = HomeRules.tasks(entries, todayStart = 100, tomorrowStart = 200, afterTomorrow = 300)
        assertEquals(listOf("today"), c.today.map { it.eventId })
        assertEquals(1, c.overdue)
        assertEquals(1, c.tomorrow)
    }

    @Test fun theNewestThreeUnreadSendersAreShownByNameOrAddress() {
        val inbox = listOf(mail("a", 10, from = "Ann"), mail("b", 40, unread = false), mail("c", 30), mail("d", 20), mail("e", 50), mail("f", 5))
        val newest = HomeRules.newestUnread(inbox)
        assertEquals(listOf("e", "c", "d"), newest.map { it.id })
        assertEquals("Ann", HomeRules.sender(inbox[0]))
        assertEquals("a@x.org", HomeRules.sender(inbox[2]))
    }

    @Test fun theOldestFlaggedMailGivesItsAge() {
        val flagged = HomeRules.flagged(listOf(mail("a", 0, flagged = true), mail("b", 300_000, flagged = true), mail("c", 5)))
        assertEquals(2, flagged.size)
        assertEquals(3, HomeRules.oldestFlaggedDays(flagged, now = 3 * 86_400 + 10))
        assertNull(HomeRules.oldestFlaggedDays(emptyList(), 10))
    }

    @Test fun theGreetingFollowsTheHour() {
        assertEquals(HomeRules.Greeting.MORNING, HomeRules.greeting(9))
        assertEquals(HomeRules.Greeting.AFTERNOON, HomeRules.greeting(13))
        assertEquals(HomeRules.Greeting.EVENING, HomeRules.greeting(23))
        assertEquals(HomeRules.Greeting.EVENING, HomeRules.greeting(3))
    }

    @Test fun awayIsOnWhileItIsEnabledAndNotOver() {
        assertTrue(HomeRules.awayOn(true, null, 10))
        assertTrue(HomeRules.awayOn(true, 20, 10))
        assertFalse(HomeRules.awayOn(true, 5, 10))
        assertFalse(HomeRules.awayOn(false, null, 10))
    }

    @Test fun anExtensionIsUpperCaseAndShort() {
        assertEquals("PDF", extensionOf("Contract-v3.pdf"))
        assertEquals("XLSX", extensionOf("Budget 2027.xlsx"))
        assertEquals("FILE", extensionOf("README"))
    }
}
