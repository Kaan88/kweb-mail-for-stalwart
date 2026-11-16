package ovh.kweb.mail.ui.calendar

import java.time.LocalDate
import java.time.ZoneId
import java.util.Locale
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.kweb_core.Occurrence

class TaskRulesTest {
    private val zone = ZoneId.of("Europe/Berlin")
    private val today = LocalDate.of(2026, 11, 17)

    private fun at(date: LocalDate, hour: Int = 0, minute: Int = 0) = date.atTime(hour, minute).atZone(zone).toEpochSecond()

    private fun task(title: String, due: LocalDate, hour: Int = 0, done: Boolean = false, isTask: Boolean = true) = Occurrence(
        eventId = title, recurrenceId = null, calendarId = "c", title = title, location = "", startsAt = at(due, hour), endsAt = at(due, hour),
        allDay = hour == 0, isTask = isTask, isDone = done, isRecurring = false, myStatus = null,
    )

    @Test fun onlyOpenTasksAreListed() {
        val all = listOf(task("a", today), task("b", today, done = true), task("c", today, isTask = false))
        assertEquals(listOf("a"), TaskRules.open(all).map { it.title })
    }

    @Test fun bucketsFollowTheDay() {
        assertEquals(TaskRules.Bucket.OVERDUE, TaskRules.bucket(today.minusDays(1), today))
        assertEquals(TaskRules.Bucket.TODAY, TaskRules.bucket(today, today))
        assertEquals(TaskRules.Bucket.TOMORROW, TaskRules.bucket(today.plusDays(1), today))
        assertEquals(TaskRules.Bucket.LATER, TaskRules.bucket(today.plusDays(2), today))
    }

    @Test fun sectionsAreOrderedAndEmptyOnesLeftOut() {
        val entries = listOf(
            task("later", today.plusDays(13)),
            task("late b", today.minusDays(2)),
            task("today 18", today, hour = 18),
            task("today 10", today, hour = 10),
            task("late a", today.minusDays(5)),
        )
        val sections = TaskRules.sections(entries, today, zone)
        assertEquals(listOf(TaskRules.Bucket.OVERDUE, TaskRules.Bucket.TODAY, TaskRules.Bucket.LATER), sections.map { it.bucket })
        assertEquals(listOf("late a", "late b"), sections[0].items.map { it.title })
        assertEquals(listOf("today 10", "today 18"), sections[1].items.map { it.title })
    }

    @Test fun lateIsOnlyOverdue() {
        assertTrue(TaskRules.isLate(TaskRules.Bucket.OVERDUE))
        assertFalse(TaskRules.isLate(TaskRules.Bucket.TODAY))
    }

    @Test fun theDueWordIsTheTimeTodayAndTheDayLater() {
        val en = Locale.ENGLISH
        assertEquals("Today", TaskRules.dueText(task("a", today), today, zone, en, "Today"))
        assertTrue(TaskRules.dueText(task("a", today, hour = 10), today, zone, en, "Today").startsWith("10:00"))
        assertEquals("Thu", TaskRules.dueText(task("a", today.minusDays(5)), today, zone, en, "Today"))
        assertEquals("5 Dec", TaskRules.dueText(task("a", LocalDate.of(2026, 12, 5)), today, zone, en, "Today"))
    }

    @Test fun theWindowIsSixtyDaysEachWay() {
        val (from, to) = TaskRules.window(today, zone)
        assertEquals(at(today.minusDays(60)), from)
        assertEquals(at(today.plusDays(61)), to)
    }
}
