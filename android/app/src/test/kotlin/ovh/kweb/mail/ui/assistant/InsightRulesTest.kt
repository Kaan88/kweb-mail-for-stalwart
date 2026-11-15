package ovh.kweb.mail.ui.assistant

import java.time.LocalDate
import java.time.LocalDateTime
import java.time.LocalTime
import java.time.ZoneId
import java.util.Locale
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import ovh.kweb.mail.ui.assistant.AssistantRules.Kind
import ovh.kweb.mail.ui.calendar.EventForms
import ovh.kweb.mail.ui.calendar.EventPrefill
import ovh.kweb.mail.ui.calendar.EventPrefills
import uniffi.kweb_core.AiFinding

class InsightRulesTest {
    private val today: LocalDate = LocalDate.of(2027, 3, 1)
    private val zone: ZoneId = ZoneId.of("Europe/Berlin")

    /** The blank form of a new entry opened on [today] at ten past eight: 09:00 to 10:00. */
    private fun blank() = EventForms.blank("cal-1", today, LocalDateTime.of(today, LocalTime.of(8, 10)), zone)

    // ---- the prefill ----

    @Test fun aDateWithATimeBecomesATimedEvent() {
        val prefill = InsightRules.prefill(AiFinding.Date("Dentist", "2027-03-04T14:30:00", allDay = false), today)
        assertEquals(EventPrefill("Dentist", "2027-03-04T14:30:00", allDay = false, isTask = false), prefill)
    }

    @Test fun aWholeDayBecomesAnAllDayEventAtMidnight() {
        assertEquals(
            EventPrefill("Conference", "2027-03-04T00:00:00", allDay = true, isTask = false),
            InsightRules.prefill(AiFinding.Date("Conference", "2027-03-04T00:00:00", allDay = true), today),
        )
        // A time that came with an all-day finding is dropped.
        assertEquals("2027-03-04T00:00:00", InsightRules.prefill(AiFinding.Date("Conference", "2027-03-04T09:15:00", allDay = true), today).start)
    }

    @Test fun aToDoBecomesATaskForToday() {
        assertEquals(
            EventPrefill("Send the signed form back", "2027-03-01T00:00:00", allDay = true, isTask = true),
            InsightRules.prefill(AiFinding.Todo("Send the signed form back"), today),
        )
    }

    @Test fun aDateThatCannotBeReadFallsOnTodayForTheUserToSet() {
        assertEquals(
            EventPrefill("Meeting", "2027-03-01T00:00:00", allDay = true, isTask = false),
            InsightRules.prefill(AiFinding.Date("Meeting", "next Tuesday", allDay = false), today),
        )
    }

    @Test fun aStartWithoutSecondsIsWrittenInFull() =
        assertEquals("2027-03-04T14:30:00", InsightRules.prefill(AiFinding.Date("Call", " 2027-03-04T14:30 ", allDay = false), today).start)

    @Test fun aTitleIsOneLineOfBoundedLength() {
        assertEquals("Pay the invoice now", InsightRules.prefill(AiFinding.Todo("  Pay the\ninvoice\t now "), today).title)
        assertEquals(InsightRules.TITLE_CHARS, InsightRules.prefill(AiFinding.Todo("x".repeat(900)), today).title.length)
    }

    @Test fun theStartsDayIsWhatTheRouteTakesAsItsHint() {
        // AppContent opens the form with the first ten characters as the day.
        val prefill = InsightRules.prefill(AiFinding.Date("Dentist", "2027-03-04T14:30:00", allDay = false), today)
        assertEquals(LocalDate.of(2027, 3, 4), LocalDate.parse(prefill.start.take(10)))
    }

    // ---- the prefill in the form of a new entry ----

    @Test fun aTimedPrefillSetsTitleAndStartAndKeepsTheLength() {
        val form = EventPrefills.apply(blank(), EventPrefill("Dentist", "2027-03-04T14:30:00", allDay = false, isTask = false))
        assertEquals("Dentist", form.title)
        assertFalse(form.allDay)
        assertFalse(form.isTask)
        assertEquals(LocalDateTime.of(2027, 3, 4, 14, 30), form.start)
        assertEquals(LocalDateTime.of(2027, 3, 4, 15, 30), form.end)
        assertEquals("cal-1", form.calendarId)
        assertEquals(zone.id, form.zone)
        assertNull(form.repeat)
        assertTrue(form.alerts.isEmpty())
    }

    @Test fun anAllDayPrefillIsThatOneDay() {
        val form = EventPrefills.apply(blank(), EventPrefill("Conference", "2027-03-04T00:00:00", allDay = true, isTask = false))
        assertTrue(form.allDay)
        assertEquals(LocalDate.of(2027, 3, 4), form.startDate)
        assertEquals(LocalDate.of(2027, 3, 4), form.endDate)
        assertEquals(emptyList<Any>(), EventForms.problems(form, hasCalendar = true))
        assertEquals(EventForms.DAY_SECS.toULong(), EventForms.toInput(form).durationSecs)
    }

    @Test fun anAllDayPrefillIsOneDayEvenWhenTheBlankFormRunsOverMidnight() {
        // Opened at 22:40: the blank form is 23:00 to 00:00 of the next day.
        val late = EventForms.blank("cal-1", today, LocalDateTime.of(today, LocalTime.of(22, 40)), zone)
        assertEquals(today.plusDays(1), late.endDate)
        val form = EventPrefills.apply(late, EventPrefill("Conference", "2027-03-04T00:00:00", allDay = true, isTask = false))
        assertEquals(form.startDate, form.endDate)
    }

    @Test fun aToDoPrefillIsATask() {
        val form = EventPrefills.apply(blank(), InsightRules.prefill(AiFinding.Todo("Send the form"), today))
        assertTrue(form.isTask)
        assertTrue(form.allDay)
        assertEquals("Send the form", form.title)
        assertEquals(today, form.startDate)
        val input = EventForms.toInput(form)
        assertTrue(input.isTask)
        assertEquals("2027-03-01T00:00:00", input.start)
    }

    @Test fun aStartThatCannotBeReadKeepsTheBlankFormsTimes() {
        val blank = blank()
        val form = EventPrefills.apply(blank, EventPrefill("Meeting", "soon", allDay = false, isTask = false))
        assertEquals(blank.copy(title = "Meeting"), form)
    }

    @Test fun aPrefillNeverBringsARepeatAnAlertOrAPlace() {
        val form = EventPrefills.apply(blank(), EventPrefill("Dentist", "2027-03-04T14:30:00", allDay = false, isTask = false))
        val input = EventForms.toInput(form)
        assertNull(input.repeat)
        assertTrue(input.alertMinutes.isEmpty())
        assertEquals("", input.location)
        assertEquals("", input.description)
    }

    // ---- the rows ----

    @Test fun findingsAreTwoGroupsInTheOrderTheyCame() {
        val a = AiFinding.Todo("Send the form")
        val b = AiFinding.Date("Dentist", "2027-03-04T14:30:00", allDay = false)
        val c = AiFinding.Todo("Call back")
        val d = AiFinding.Date("Conference", "2027-03-05T00:00:00", allDay = true)
        val rows = InsightRules.rows(listOf(a, b, c, d), Locale.GERMANY)
        assertEquals(listOf(InsightRules.Row("Send the form", null, a), InsightRules.Row("Call back", null, c)), rows.todos)
        assertEquals(
            listOf(InsightRules.Row("Dentist", "04.03.2027, 14:30", b), InsightRules.Row("Conference", "05.03.2027", d)),
            rows.dates,
        )
        assertFalse(rows.isEmpty)
    }

    @Test fun nothingFoundIsEmpty() {
        assertTrue(InsightRules.rows(emptyList(), Locale.US).isEmpty)
        // A finding without a text is no row.
        assertTrue(InsightRules.rows(listOf(AiFinding.Todo("  "), AiFinding.Date("\n", "2027-03-04T14:30:00", allDay = false)), Locale.US).isEmpty)
    }

    @Test fun aDateThatCannotBeReadIsShownWithoutItsTime() {
        val finding = AiFinding.Date("Meeting", "next Tuesday", allDay = false)
        assertEquals(listOf(InsightRules.Row("Meeting", null, finding)), InsightRules.rows(listOf(finding), Locale.US).dates)
        assertNull(InsightRules.whenText("", allDay = true, Locale.US))
    }

    @Test fun aRowIsOneLine() {
        val finding = AiFinding.Todo("Pay\nthe   invoice")
        assertEquals("Pay the invoice", InsightRules.rows(listOf(finding), Locale.US).todos.single().title)
    }

    @Test fun ideasAreRowsOfOneLineAndBlankOnesAreDropped() =
        assertEquals(listOf("Accept and propose Tuesday", "Decline politely"), InsightRules.ideaRows(listOf(" Accept and\npropose Tuesday ", "", "Decline politely")))

    // ---- which tap an idea takes ----

    @Test fun anIdeaBecomesADraftedReplyWhenDraftsAreAllowed() = assertEquals(InsightRules.IdeaTap.WRITE_DRAFT, InsightRules.ideaTap(draftsAllowed = true))

    @Test fun anIdeaBecomesTheReplysTextWhenDraftsAreOff() = assertEquals(InsightRules.IdeaTap.USE_AS_TEXT, InsightRules.ideaTap(draftsAllowed = false))

    // ---- what the sheet shows as text ----

    @Test fun ideasAndFindingsAreNotShownAsRawText() {
        assertFalse(InsightRules.showsAnswer(Kind.IDEAS, done = false, ideas = 0))
        assertFalse(InsightRules.showsAnswer(Kind.IDEAS, done = true, ideas = 2))
        assertFalse(InsightRules.showsAnswer(Kind.EXTRACT, done = true, ideas = 0))
        assertFalse(InsightRules.showsAnswer(Kind.EXTRACT, done = false, ideas = 0))
    }

    @Test fun anIdeasAnswerWithoutIdeasIsShownAsItCame() = assertTrue(InsightRules.showsAnswer(Kind.IDEAS, done = true, ideas = 0))

    @Test fun theOtherKindsShowTheirText() {
        for (kind in listOf(Kind.SUMMARY, Kind.DRAFT, Kind.REWRITE, Kind.EXPLAIN)) {
            assertTrue(InsightRules.showsAnswer(kind, done = false, ideas = 0))
            assertTrue(InsightRules.showsAnswer(kind, done = true, ideas = 0))
        }
    }
}
