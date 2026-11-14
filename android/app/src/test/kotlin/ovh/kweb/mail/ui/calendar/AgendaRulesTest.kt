package ovh.kweb.mail.ui.calendar

import java.time.LocalDate
import java.time.ZoneId
import java.util.Locale
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import uniffi.kweb_core.Occurrence

class AgendaRulesTest {
    private val zone = ZoneId.of("Europe/Berlin")
    private val today = LocalDate.of(2026, 11, 12)

    private fun at(date: LocalDate, hour: Int, minute: Int = 0) = date.atTime(hour, minute).atZone(zone).toEpochSecond()

    private fun entry(
        title: String,
        start: Long,
        id: String = title,
        recurrence: String? = null,
        allDay: Boolean = false,
        calendar: String = "c1",
        task: Boolean = false,
    ) = Occurrence(
        eventId = id,
        recurrenceId = recurrence,
        calendarId = calendar,
        title = title,
        location = "",
        startsAt = start,
        endsAt = start,
        allDay = allDay,
        isTask = task,
        isDone = false,
        isRecurring = recurrence != null,
        myStatus = null,
    )

    private fun span(title: String, start: Long, end: Long, allDay: Boolean = false) = entry(title, start, allDay = allDay).copy(endsAt = end)

    @Test fun anEntryOfSeveralDaysIsUnderEachDayItCovers() {
        val trip = span("trip", at(today, 20), at(today.plusDays(2), 8))
        val days = AgendaRules.group(listOf(trip), zone)
        assertEquals(listOf(today, today.plusDays(1), today.plusDays(2)), days.map { it.date })
        val rows = AgendaRules.rows(days, zone).filterIsInstance<AgendaRules.Row.Entry>()
        assertEquals(listOf(false, true, true), rows.map { it.continues })
        assertEquals(3, rows.map { it.key }.toSet().size)
    }

    @Test fun anEntryEndingExactlyAtMidnightDoesNotCoverTheLastDay() {
        val allDay = span("holiday", at(today, 0), at(today.plusDays(2), 0), allDay = true)
        assertEquals(listOf(today, today.plusDays(1)), AgendaRules.group(listOf(allDay), zone).map { it.date })
        assertEquals(listOf(today), AgendaRules.group(listOf(span("late", at(today, 22), at(today.plusDays(1), 0))), zone).map { it.date })
    }

    @Test fun anEntryWithoutALengthCoversItsStartDayOnly() {
        assertEquals(listOf(today), AgendaRules.group(listOf(entry("a", at(today, 9))), zone).map { it.date })
    }

    @Test fun anEntryOfSeveralDaysIsCutAtTheWindow() {
        val long = span("long", at(today.minusDays(3), 9), at(today.plusDays(5), 9))
        val days = AgendaRules.group(listOf(long), zone, from = today, to = today.plusDays(2))
        assertEquals(listOf(today, today.plusDays(1)), days.map { it.date })
    }

    @Test fun nothingGivesNoDays() = assertEquals(emptyList<AgendaRules.Day>(), AgendaRules.group(emptyList(), zone))

    @Test fun daysWithoutEntriesAreLeftOut() {
        val days = AgendaRules.group(listOf(entry("a", at(today, 9)), entry("b", at(today.plusDays(5), 9))), zone)
        assertEquals(listOf(today, today.plusDays(5)), days.map { it.date })
    }

    @Test fun daysComeEarliestFirstWhateverTheOrderGiven() {
        val days = AgendaRules.group(listOf(entry("late", at(today.plusDays(2), 9)), entry("early", at(today, 9))), zone)
        assertEquals(listOf(today, today.plusDays(2)), days.map { it.date })
    }

    @Test fun allDayEntriesComeFirstThenByTimeThenTitle() {
        val day = AgendaRules.group(
            listOf(
                entry("lunch", at(today, 12)),
                entry("b", at(today, 9)),
                entry("A", at(today, 9)),
                entry("holiday", at(today, 0), allDay = true),
                entry("morning", at(today, 7)),
            ),
            zone,
        ).single()
        assertEquals(listOf("holiday", "morning", "A", "b", "lunch"), day.entries.map { it.title })
    }

    @Test fun anEntryBelongsToTheDayItStartsOnInTheZone() {
        // 23:30 in Berlin is already the next day in Tokyo.
        val start = at(today, 23, 30)
        assertEquals(today, AgendaRules.dateOf(entry("x", start), zone))
        assertEquals(today.plusDays(1), AgendaRules.dateOf(entry("x", start), ZoneId.of("Asia/Tokyo")))
    }

    @Test fun anAllDayEntryStaysOnItsOwnDay() {
        val holiday = entry("holiday", at(today, 0), allDay = true)
        assertEquals(today, AgendaRules.dateOf(holiday, zone))
    }

    @Test fun theSameOccurrenceTwiceCountsOnce() {
        val one = entry("a", at(today, 9), id = "e1", recurrence = "r1")
        assertEquals(1, AgendaRules.group(listOf(one, one.copy()), zone).single().entries.size)
    }

    @Test fun twoOccurrencesOfOneEventAreTwoEntries() {
        val first = entry("a", at(today, 9), id = "e1", recurrence = "r1")
        val second = entry("a", at(today.plusDays(7), 9), id = "e1", recurrence = "r2")
        assertEquals(2, AgendaRules.group(listOf(first, second), zone).size)
    }

    @Test fun rowsAreAHeadingAndItsEntries() {
        val days = AgendaRules.group(listOf(entry("a", at(today, 9)), entry("b", at(today, 10)), entry("c", at(today.plusDays(1), 9))), zone)
        val rows = AgendaRules.rows(days, zone)
        assertEquals(5, rows.size)
        assertEquals(AgendaRules.Row.Header(today), rows[0])
        assertEquals(AgendaRules.Row.Header(today.plusDays(1)), rows[3])
    }

    @Test fun rowKeysAreUniqueEvenForARepeatingEvent() {
        val days = AgendaRules.group(
            listOf(
                entry("a", at(today, 9), id = "e1", recurrence = "r1"),
                entry("a", at(today.plusDays(1), 9), id = "e1", recurrence = "r2"),
            ),
            zone,
        )
        val keys = AgendaRules.rows(days, zone).filterIsInstance<AgendaRules.Row.Entry>().map { it.key }
        assertEquals(keys.size, keys.toSet().size)
    }

    @Test fun scrollingToADayFindsItsHeading() {
        val rows = AgendaRules.rows(AgendaRules.group(listOf(entry("a", at(today, 9)), entry("b", at(today.plusDays(3), 9))), zone), zone)
        assertEquals(0, AgendaRules.indexOfDay(rows, today))
        assertEquals(2, AgendaRules.indexOfDay(rows, today.plusDays(3)))
    }

    @Test fun scrollingToADayWithoutEntriesGoesToTheNextDayThatHasSome() {
        val rows = AgendaRules.rows(AgendaRules.group(listOf(entry("a", at(today, 9)), entry("b", at(today.plusDays(3), 9))), zone), zone)
        assertEquals(2, AgendaRules.indexOfDay(rows, today.plusDays(1)))
    }

    @Test fun scrollingPastTheLastDayFindsNothing() {
        val rows = AgendaRules.rows(AgendaRules.group(listOf(entry("a", at(today, 9))), zone), zone)
        assertNull(AgendaRules.indexOfDay(rows, today.plusDays(1)))
    }

    @Test fun todayIsNamedInItsHeading() = assertEquals("Today · Thu 12", AgendaRules.heading(today, today, Locale.UK, "Today"))

    @Test fun otherDaysOfTheMonthAreJustWeekdayAndNumber() = assertEquals("Fri 13", AgendaRules.heading(today.plusDays(1), today, Locale.UK, "Today"))

    @Test fun aDayInAnotherMonthNamesTheMonth() = assertEquals("Tue 1 Dec", AgendaRules.heading(LocalDate.of(2026, 12, 1), today, Locale.UK, "Today"))

    @Test fun aDayInAnotherYearNamesTheYear() = assertEquals("Fri 1 Jan 2027", AgendaRules.heading(LocalDate.of(2027, 1, 1), today, Locale.UK, "Today"))

    @Test fun theHeadingFollowsTheLanguage() = assertEquals("Fr. 13", AgendaRules.heading(today.plusDays(1), today, Locale.GERMANY, "Heute"))

    @Test fun aCalendarsToneFollowsItsPosition() {
        val ids = listOf("a", "b", "c", "d")
        assertEquals(listOf(0, 1, 2, 0), ids.map { AgendaRules.tone(it, ids) })
    }

    @Test fun anUnknownCalendarTakesTheFirstTone() = assertEquals(0, AgendaRules.tone("zzz", listOf("a", "b")))
}
