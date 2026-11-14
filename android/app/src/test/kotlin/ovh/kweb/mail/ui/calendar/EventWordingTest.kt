package ovh.kweb.mail.ui.calendar

import java.time.LocalDate
import java.time.LocalDateTime
import java.time.ZoneId
import java.util.Locale
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.kweb_core.EventInput
import uniffi.kweb_core.Frequency
import uniffi.kweb_core.RepeatRule

class EventWordingTest {
    private val uk = Locale.UK
    private val berlin = ZoneId.of("Europe/Berlin")
    private val words = TestWords

    private fun timed(start: String, end: String, task: Boolean = false, note: String? = null) =
        Span(LocalDateTime.parse(start), LocalDateTime.parse(end), allDay = false, isTask = task, zoneNote = note)

    private fun allDay(first: String, lastInclusive: String, task: Boolean = false) = Span(
        LocalDate.parse(first).atStartOfDay(),
        LocalDate.parse(lastInclusive).plusDays(1).atStartOfDay(),
        allDay = true,
        isTask = task,
    )

    private fun input(
        start: String = "2026-11-12T10:00:00",
        zone: String? = "Europe/Berlin",
        duration: Long = 2700,
        allDay: Boolean = false,
        task: Boolean = false,
    ) = EventInput("c1", "t", "", "", start, zone, duration.toULong(), allDay, task, null, emptyList())

    // ---- the agenda line ----

    @Test fun aTimedEntryShowsItsSpan() = assertEquals("10:00 – 10:45", EventWording.agendaTime(timed("2026-11-12T10:00", "2026-11-12T10:45"), uk, words))

    @Test fun anEntryWithoutALengthShowsItsStartOnly() = assertEquals("10:00", EventWording.agendaTime(timed("2026-11-12T10:00", "2026-11-12T10:00"), uk, words))

    @Test fun anEntryOverMidnightNamesTheEndDay() =
        assertEquals("22:00 – Fri 13, 02:00", EventWording.agendaTime(timed("2026-11-12T22:00", "2026-11-13T02:00"), uk, words))

    @Test fun anAllDayEntryIsAllDay() = assertEquals("All day", EventWording.agendaTime(allDay("2026-11-12", "2026-11-12"), uk, words))

    @Test fun aSeveralDayEntryCountsItsDays() = assertEquals("All day · 3 days", EventWording.agendaTime(allDay("2026-11-12", "2026-11-14"), uk, words))

    @Test fun aTaskShowsWhenItIsDue() =
        assertEquals("Task · due 12:00", EventWording.agendaTime(timed("2026-11-13T12:00", "2026-11-13T12:00", task = true), uk, words))

    @Test fun aTaskDueOnADayHasNoTime() = assertEquals("Task", EventWording.agendaTime(allDay("2026-11-13", "2026-11-13", task = true), uk, words))

    @Test fun theLineJoinsTimePlaceAndInvitation() {
        val span = timed("2026-11-12T10:00", "2026-11-12T10:45")
        assertEquals("10:00 – 10:45 · Çankaya · Invitation", EventWording.agendaLine(span, "Çankaya", true, uk, words))
    }

    @Test fun theLineLeavesOutWhatIsMissing() {
        val span = timed("2026-11-12T10:00", "2026-11-12T10:45")
        assertEquals("10:00 – 10:45", EventWording.agendaLine(span, "  ", false, uk, words))
        assertEquals("10:00 – 10:45 · Invitation", EventWording.agendaLine(span, "", true, uk, words))
    }

    @Test fun onlyAnUnansweredInvitationIsMarked() {
        assertTrue(EventWording.needsAnswer("needs-action"))
        assertEquals(listOf(false, false, false, false), listOf("accepted", "tentative", "declined", null).map { EventWording.needsAnswer(it) })
    }

    @Test fun anOccurrenceIsReadAsWallClockTimeInTheZone() {
        val start = LocalDateTime.of(2026, 11, 12, 10, 0).atZone(berlin).toEpochSecond()
        val span = Span.ofOccurrence(start, start + 2700, false, false, berlin)
        assertEquals(LocalDateTime.of(2026, 11, 12, 10, 45), span.end)
        assertEquals("10:00 – 10:45", EventWording.agendaTime(span, uk, words))
    }

    @Test fun anAllDayOccurrenceKnowsItsLength() {
        val start = LocalDate.of(2026, 11, 12).atStartOfDay(berlin).toEpochSecond()
        val end = LocalDate.of(2026, 11, 15).atStartOfDay(berlin).toEpochSecond()
        val span = Span.ofOccurrence(start, end, true, false, berlin)
        assertEquals(3, span.allDayLength)
        assertEquals(LocalDate.of(2026, 11, 14), span.lastDate)
    }

    @Test fun anEndBeforeTheStartIsTakenAsNoLength() {
        val start = LocalDateTime.of(2026, 11, 12, 10, 0).atZone(berlin).toEpochSecond()
        assertEquals("10:00", EventWording.agendaTime(Span.ofOccurrence(start, start - 100, false, false, berlin), uk, words))
    }

    // ---- an event in full ----

    @Test fun anEventReadsItsSpanFromItsInput() {
        val span = Span.ofInput(input(), berlin)
        assertEquals(LocalDateTime.of(2026, 11, 12, 10, 45), span.end)
        assertNull(span.zoneNote)
    }

    @Test fun aNightOverTheSpringChangeEndsWhereTheClockSaysAfterSevenHours() {
        val span = Span.ofInput(input(start = "2026-03-28T22:00:00", duration = 7 * 3600), berlin)
        assertEquals(LocalDateTime.of(2026, 3, 29, 6, 0), span.end)
    }

    @Test fun aNightOverTheAutumnChangeEndsWhereTheClockSaysAfterNineHours() {
        val span = Span.ofInput(input(start = "2026-10-24T22:00:00", duration = 9 * 3600), berlin)
        assertEquals(LocalDateTime.of(2026, 10, 25, 6, 0), span.end)
    }

    @Test fun aFloatingTimeAddsWallClockTime() {
        val span = Span.ofInput(input(start = "2026-03-28T22:00:00", zone = null, duration = 8 * 3600), berlin)
        assertEquals(LocalDateTime.of(2026, 3, 29, 6, 0), span.end)
        assertNull(span.zoneNote)
    }

    @Test fun anAllDayInputCountsWholeDays() {
        val span = Span.ofInput(input(allDay = true, duration = 3 * 86_400L, start = "2026-11-12T00:00:00"), berlin)
        assertEquals(3, span.allDayLength)
        assertEquals(LocalDate.of(2026, 11, 14), span.lastDate)
    }

    @Test fun aZoneThatIsNotThePhonesIsNamed() {
        val span = Span.ofInput(input(zone = "America/New_York"), berlin)
        assertEquals("America/New_York", span.zoneNote)
    }

    @Test fun anAllDayEntryNeverNamesAZone() = assertNull(Span.ofInput(input(zone = "America/New_York", allDay = true, duration = 86_400), berlin).zoneNote)

    @Test fun aZoneThePhoneDoesNotKnowIsNotNamedEither() = assertNull(Span.ofInput(input(zone = "Mars/Base"), berlin).zoneNote)

    // ---- the event screen's lines ----

    @Test fun aSingleTimedDayHasADateAndATime() {
        val lines = EventWording.whenOf(timed("2026-11-12T10:00", "2026-11-12T10:45"), uk, words)
        assertTrue(lines.date, lines.date.contains("12 November 2026"))
        assertEquals("10:00 – 10:45", lines.time)
    }

    @Test fun aSeveralDayEntryHasBothEndsInTheDateLine() {
        val lines = EventWording.whenOf(allDay("2026-11-12", "2026-11-14"), uk, words)
        assertEquals("12 Nov 2026 – 14 Nov 2026", lines.date)
        assertEquals("All day · 3 days", lines.time)
    }

    @Test fun aSingleAllDayEntryHasOneDate() {
        val lines = EventWording.whenOf(allDay("2026-11-12", "2026-11-12"), uk, words)
        assertTrue(lines.date.contains("12 November 2026"))
        assertEquals("All day", lines.time)
    }

    @Test fun aTimedEntryOverMidnightKeepsBothDays() {
        val lines = EventWording.whenOf(timed("2026-03-28T22:00", "2026-03-29T06:00"), uk, words)
        assertEquals("28 Mar 2026 – 29 Mar 2026", lines.date)
        assertEquals("22:00 – 06:00", lines.time)
    }

    @Test fun aTaskSaysWhenItIsDue() {
        val lines = EventWording.whenOf(timed("2026-11-13T12:00", "2026-11-13T12:00", task = true), uk, words)
        assertEquals("Task · due 12:00", lines.time)
    }

    @Test fun aZoneNoteGoesAfterTheTime() {
        val lines = EventWording.whenOf(timed("2026-11-12T10:00", "2026-11-12T10:45", note = "America/New_York"), uk, words)
        assertEquals("10:00 – 10:45 (America/New_York)", lines.time)
    }

    // ---- repeating ----

    private fun rule(
        frequency: Frequency,
        interval: UInt = 1u,
        days: List<String> = emptyList(),
        until: String? = null,
        count: UInt? = null,
    ) = RepeatRule(frequency, interval, days, until, count)

    @Test fun everyDay() = assertEquals("Every day", EventWording.repeat(rule(Frequency.DAILY), uk, words))

    @Test fun everyOtherWeekOnDaysInWeekOrder() =
        assertEquals("Every 2 weeks on Mon, Wed", EventWording.repeat(rule(Frequency.WEEKLY, 2u, listOf("we", "mo")), uk, words))

    @Test fun aWeeklyRuleWithoutDaysIsJustWeekly() = assertEquals("Every week", EventWording.repeat(rule(Frequency.WEEKLY), uk, words))

    @Test fun anEndDateIsNamed() =
        assertEquals("Every month, until 31 Dec 2026", EventWording.repeat(rule(Frequency.MONTHLY, until = "2026-12-31"), uk, words))

    @Test fun anEndDateWithATimeIsStillADate() =
        assertEquals("Every month, until 31 Dec 2026", EventWording.repeat(rule(Frequency.MONTHLY, until = "2026-12-31T00:00:00"), uk, words))

    @Test fun aCountIsNamed() = assertEquals("Every year, 5 times", EventWording.repeat(rule(Frequency.YEARLY, count = 5u), uk, words))

    @Test fun anIntervalOfZeroCountsAsOne() = assertEquals("Every day", EventWording.repeat(rule(Frequency.DAILY, 0u), uk, words))

    @Test fun daysOnlyCountForAWeeklyRule() = assertEquals("Every day", EventWording.repeat(rule(Frequency.DAILY, days = listOf("mo")), uk, words))

    // ---- reminders ----

    @Test fun reminderWording() {
        assertEquals(
            listOf("At the start", "5 minutes before", "90 minutes before", "1 hour before", "2 hours before", "1 day before", "2 days before"),
            listOf(0, 5, 90, 60, 120, 1440, 2880).map { EventWording.reminder(it, words) },
        )
    }
}
