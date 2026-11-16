package ovh.kweb.mail.ui.attention

import java.time.LocalDate
import java.time.ZoneId
import java.util.Locale
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.kweb_core.AttentionFamily
import uniffi.kweb_core.AttentionRule
import uniffi.kweb_core.HomeTile
import uniffi.kweb_core.PhoneAction
import uniffi.kweb_core.Prefill
import uniffi.kweb_core.ServiceAction

class AttentionHooksTest {
    private val zone = ZoneId.of("Europe/Berlin")
    private val now = LocalDate.of(2026, 11, 17).atTime(9, 0).atZone(zone).toEpochSecond()
    private val clock = WordClock(zone, Locale.ENGLISH, now)
    private val thu = Prefill("Call with Merve", "2026-11-19T15:00:00", false, false, 60u)
    private val todo = Prefill("Send the agenda", "2026-11-20T00:00:00", true, true, 0u)

    private val date = finding(
        AttentionRule.DATE_IN_MAIL, AttentionFamily.MAIL, HomeTile.UNREAD, id = "d", threadId = "th", messageIds = listOf("m1"),
        phone = listOf(PhoneAction.AddToCalendar(thu), PhoneAction.OpenMail("m1", "th")), service = ServiceAction.ADD_DATES,
    )
    private val waiting = finding(
        AttentionRule.WAITING_FOR_YOU, AttentionFamily.MAIL, HomeTile.UNREAD, id = "w", threadId = "th", messageIds = listOf("m1"),
        phone = listOf(PhoneAction.OpenMail("m1", "th")), service = ServiceAction.REPLY_IDEAS,
    )
    private val deadline = finding(
        AttentionRule.DEADLINE_WORDS, AttentionFamily.MAIL, HomeTile.UNREAD, id = "dl", threadId = "th", messageIds = listOf("m1"),
        phone = listOf(PhoneAction.AddTask(todo), PhoneAction.OpenMail("m1", "th")), service = ServiceAction.ADD_DATES,
    )

    @Test fun theMenuHoldsPhoneActionsThenEachServiceOnce() {
        val entries = ThreadMenu.entries(listOf(waiting, date, deadline))
        assertEquals(
            listOf("Calendar", "Task", "Service:ADD_DATES", "Service:REPLY_IDEAS"),
            entries.map { if (it is ThreadMenuEntry.Service) "Service:${it.action}" else it.javaClass.simpleName },
        )
    }

    @Test fun withoutAServiceThereAreNoSparkleEntries() {
        val entries = ThreadMenu.entries(listOf(date.copy(service = null)))
        assertEquals(1, entries.size)
        assertTrue(entries[0] is ThreadMenuEntry.Calendar)
    }

    @Test fun aFindingWithOnlyOpenMailHasNoEntry() {
        assertTrue(ThreadMenu.entries(listOf(waiting.copy(service = null))).isEmpty())
    }

    @Test fun theLabelsNameTheDayAndTime() {
        val text = ThreadMenu.whenText(thu, clock)
        assertTrue(text, text.startsWith("Thu 15:00") || text.startsWith("Thu 3:00"))
        assertEquals("Fri", ThreadMenu.whenText(todo, clock))
        assertEquals("Thu 26 Nov", ThreadMenu.whenText(thu.copy(start = "2026-11-26T00:00:00", allDay = true), clock))
    }

    @Test fun aRowWearsTheMarkOnlyWithHomeOff() {
        val marks = AttentionRules.rowMarks(listOf(date))
        assertTrue(AttentionRules.rowMarked(false, marks, "th", listOf("m9")))
        assertTrue(AttentionRules.rowMarked(false, marks, "other", listOf("m1")))
        assertFalse(AttentionRules.rowMarked(false, marks, "other", listOf("m9")))
        assertFalse(AttentionRules.rowMarked(true, marks, "th", listOf("m1")))
    }

    @Test fun calendarAndTaskFindingsDoNotMarkMailRows() {
        val overdue = finding(AttentionRule.OVERDUE, AttentionFamily.TASKS, HomeTile.DUE_TODAY, messageIds = listOf("m1"), threadId = "th")
        assertFalse(AttentionRules.rowMarked(false, AttentionRules.rowMarks(listOf(overdue)), "th", listOf("m1")))
    }

    @Test fun aTasksFindingWithoutAMailOpensTheTasksView() {
        val overdue = finding(AttentionRule.OVERDUE, AttentionFamily.TASKS, HomeTile.DUE_TODAY, phone = listOf(PhoneAction.MoveToday(listOf("t"))))
        val finish = finding(AttentionRule.MAIL_MAY_FINISH, AttentionFamily.TASKS, HomeTile.DUE_TODAY, phone = listOf(PhoneAction.OpenMail("m", "t")))
        assertTrue(AttentionRules.opensTasks(overdue))
        assertFalse(AttentionRules.opensTasks(finish))
        assertFalse(AttentionRules.opensTasks(date))
    }
}
