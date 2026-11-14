package ovh.kweb.mail.ui.calendar

import java.time.DayOfWeek
import java.time.LocalDate
import java.time.LocalDateTime
import java.time.LocalTime
import java.time.ZoneId
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.kweb_core.Calendar
import uniffi.kweb_core.EventInput
import uniffi.kweb_core.Frequency
import uniffi.kweb_core.RepeatRule

class EventFormsTest {
    private val berlin = ZoneId.of("Europe/Berlin")

    private fun hours(n: Int): ULong = (n * 3600L).toULong()

    private fun days(n: Int): ULong = (n * 86_400L).toULong()

    private fun form(
        start: String = "2026-11-12T10:00",
        end: String = "2026-11-12T11:00",
        zone: String? = "Europe/Berlin",
        allDay: Boolean = false,
        task: Boolean = false,
        title: String = "Dentist",
    ): EventForm {
        val s = LocalDateTime.parse(start)
        val e = LocalDateTime.parse(end)
        return EventForm(
            calendarId = "c1",
            title = title,
            isTask = task,
            allDay = allDay,
            startDate = s.toLocalDate(),
            startTime = s.toLocalTime(),
            endDate = e.toLocalDate(),
            endTime = e.toLocalTime(),
            zone = zone,
        )
    }

    private fun input(
        start: String = "2026-11-12T10:00:00",
        zone: String? = "Europe/Berlin",
        duration: Long = 3600,
        allDay: Boolean = false,
        task: Boolean = false,
        repeat: RepeatRule? = null,
        alerts: List<UInt> = emptyList(),
    ) = EventInput("c1", "Dentist", "", "", start, zone, duration.toULong(), allDay, task, repeat, alerts)

    // ---- a new entry ----

    @Test fun aNewEntryForTodayStartsAtTheNextFullHour() {
        val form = EventForms.blank("c1", LocalDate.of(2026, 11, 12), LocalDateTime.of(2026, 11, 12, 10, 25), berlin)
        assertEquals(LocalTime.of(11, 0), form.startTime)
        assertEquals(LocalTime.of(12, 0), form.endTime)
        assertEquals("Europe/Berlin", form.zone)
    }

    @Test fun aNewEntryForAnotherDayStartsAtNine() {
        val form = EventForms.blank("c1", LocalDate.of(2026, 11, 20), LocalDateTime.of(2026, 11, 12, 10, 25), berlin)
        assertEquals(LocalDate.of(2026, 11, 20), form.startDate)
        assertEquals(LocalTime.of(9, 0), form.startTime)
        assertEquals(LocalTime.of(10, 0), form.endTime)
    }

    @Test fun lateInTheEveningItStaysOnTheSameDayAndEndsAfterMidnight() {
        val form = EventForms.blank("c1", LocalDate.of(2026, 11, 12), LocalDateTime.of(2026, 11, 12, 23, 40), berlin)
        assertEquals(LocalDate.of(2026, 11, 12), form.startDate)
        assertEquals(LocalTime.of(23, 0), form.startTime)
        assertEquals(LocalDate.of(2026, 11, 13), form.endDate)
        assertEquals(LocalTime.of(0, 0), form.endTime)
    }

    @Test fun aNewEntryIsNotChangedUntilTheUserChangesIt() {
        val blank = EventForms.blank("c1", LocalDate.of(2026, 11, 12), LocalDateTime.of(2026, 11, 12, 10, 0), berlin)
        assertEquals(blank, blank.copy())
        assertFalse(blank == blank.copy(title = "x"))
    }

    // ---- the length of a timed entry ----

    @Test fun anOrdinaryHourIsAnHour() = assertEquals(3600uL, EventForms.toInput(form()).durationSecs)

    @Test fun theStartIsALocalDateTimeWithSeconds() {
        val input = EventForms.toInput(form(start = "2026-11-12T10:00"))
        assertEquals("2026-11-12T10:00:00", input.start)
        assertEquals("Europe/Berlin", input.timeZone)
    }

    @Test fun aNightOverTheSpringChangeIsSevenHoursLongAndNotEight() {
        // The clocks go from 02:00 to 03:00 on 29 March 2026.
        val input = EventForms.toInput(form(start = "2026-03-28T22:00", end = "2026-03-29T06:00"))
        assertEquals(hours(7), input.durationSecs)
    }

    @Test fun aNightOverTheAutumnChangeIsNineHoursLongAndNotEight() {
        // The clocks go from 03:00 back to 02:00 on 25 October 2026.
        val input = EventForms.toInput(form(start = "2026-10-24T22:00", end = "2026-10-25T06:00"))
        assertEquals(hours(9), input.durationSecs)
    }

    @Test fun twoHoursOnTheClockAcrossTheGapIsOneHourLong() {
        assertEquals(3600uL, EventForms.toInput(form(start = "2026-03-29T01:00", end = "2026-03-29T03:00")).durationSecs)
    }

    @Test fun theSameDayInAnotherZoneChangesAtAnotherDate() {
        // New York changed its clocks on 8 March 2026, Berlin not until the 29th.
        val input = EventForms.toInput(form(start = "2026-03-07T22:00", end = "2026-03-08T06:00", zone = "America/New_York"))
        assertEquals(hours(7), input.durationSecs)
        assertEquals(hours(8), EventForms.toInput(form(start = "2026-03-07T22:00", end = "2026-03-08T06:00")).durationSecs)
    }

    @Test fun aFloatingTimeIsWallClockTime() {
        assertEquals(hours(8), EventForms.toInput(form(start = "2026-03-28T22:00", end = "2026-03-29T06:00", zone = null)).durationSecs)
    }

    @Test fun aZoneThePhoneDoesNotKnowIsKeptAndTreatedAsWallClockTime() {
        val input = EventForms.toInput(form(start = "2026-03-28T22:00", end = "2026-03-29T06:00", zone = "Mars/Base"))
        assertEquals(hours(8), input.durationSecs)
        assertEquals("Mars/Base", input.timeZone)
    }

    @Test fun anEndBeforeTheStartIsNeverANegativeLength() {
        assertEquals(0uL, EventForms.toInput(form(start = "2026-11-12T10:00", end = "2026-11-12T09:00")).durationSecs)
    }

    @Test fun theEndComesBackAsTheClockShowsIt() {
        val back = EventForms.fromInput(input(start = "2026-03-28T22:00:00", duration = 7 * 3600), false)
        assertEquals(LocalDate.of(2026, 3, 29), back.endDate)
        assertEquals(LocalTime.of(6, 0), back.endTime)
    }

    @Test fun aFormSurvivesAJourneyThroughTheCoreOverEachClockChange() {
        for ((start, end) in listOf("2026-03-28T22:00" to "2026-03-29T06:00", "2026-10-24T22:00" to "2026-10-25T06:00", "2026-03-29T01:00" to "2026-03-29T03:00")) {
            val original = form(start = start, end = end)
            assertEquals(original.copy(loaded = null), EventForms.fromInput(EventForms.toInput(original), false).copy(loaded = null))
        }
    }

    @Test fun anEndInTheHourThatHappensTwiceKeepsTheLoadedLengthUntilTheTimingChanges() {
        // 01:30 CEST plus two real hours is 02:30 CET, a wall-clock time that also exists once in CEST.
        val loaded = EventForms.fromInput(input(start = "2026-10-25T01:30:00", duration = 7200), false)
        assertEquals(LocalTime.of(2, 30), loaded.endTime)
        assertEquals(hours(2), EventForms.toInput(loaded).durationSecs)
        // Without the loaded timing the wall clock alone reads it as the first 02:30, an hour after the start.
        assertEquals(hours(1), EventForms.toInput(loaded.copy(loaded = null)).durationSecs)
        // Touching the timing drops it: 01:30 CEST to 03:00 CET is two and a half hours.
        assertEquals(9000uL, EventForms.toInput(loaded.withEnd(time = LocalTime.of(3, 0))).durationSecs)
    }

    @Test fun aSpringNightKeepsTheLoadedLength() {
        val loaded = EventForms.fromInput(input(start = "2026-03-29T01:30:00", duration = 3600), false)
        assertEquals(3600uL, EventForms.toInput(loaded).durationSecs)
    }

    // ---- all-day entries and tasks ----

    @Test fun anAllDayEntryStartsAtMidnightAndLastsWholeDays() {
        val input = EventForms.toInput(form(start = "2026-11-12T15:00", end = "2026-11-14T16:00", allDay = true))
        assertEquals("2026-11-12T00:00:00", input.start)
        assertEquals(days(3), input.durationSecs)
        assertTrue(input.allDay)
    }

    @Test fun oneAllDayIsOneDayLong() {
        assertEquals(86_400uL, EventForms.toInput(form(start = "2026-11-12T09:00", end = "2026-11-12T10:00", allDay = true)).durationSecs)
    }

    @Test fun wholeDaysAreNotCountedByTheClock() {
        // The clocks change inside these three days; the length is still three days.
        assertEquals(days(3), EventForms.toInput(form(start = "2026-03-28T00:00", end = "2026-03-30T00:00", allDay = true)).durationSecs)
    }

    @Test fun anAllDayEntryComesBackWithItsLastDay() {
        val back = EventForms.fromInput(input(start = "2026-11-12T00:00:00", duration = 3 * 86_400L, allDay = true), false)
        assertEquals(LocalDate.of(2026, 11, 14), back.endDate)
        assertTrue(back.allDay)
    }

    @Test fun anAllDayEntryWithoutALengthIsOneDay() {
        assertEquals(LocalDate.of(2026, 11, 12), EventForms.fromInput(input(duration = 0, allDay = true), false).endDate)
    }

    @Test fun aTaskHasNoLength() {
        val input = EventForms.toInput(form(start = "2026-11-13T12:00", end = "2026-11-13T15:00", task = true))
        assertEquals(0uL, input.durationSecs)
        assertEquals("2026-11-13T12:00:00", input.start)
        assertTrue(input.isTask)
    }

    @Test fun aTaskDueOnAWholeDayStartsAtMidnight() {
        val input = EventForms.toInput(form(start = "2026-11-13T12:00", end = "2026-11-13T12:00", task = true, allDay = true))
        assertEquals("2026-11-13T00:00:00", input.start)
        assertEquals(0uL, input.durationSecs)
    }

    @Test fun textIsTrimmed() {
        val input = EventForms.toInput(form(title = "  Dentist ").copy(location = " Çankaya ", description = "\nBring the card\n"))
        assertEquals("Dentist", input.title)
        assertEquals("Çankaya", input.location)
        assertEquals("Bring the card", input.description)
    }

    // ---- moving the start and the end ----

    @Test fun aNewStartTimeMovesTheEndWithIt() {
        val moved = form(start = "2026-11-12T10:00", end = "2026-11-12T11:30").withStart(time = LocalTime.of(14, 0))
        assertEquals(LocalTime.of(15, 30), moved.endTime)
        assertEquals(LocalDate.of(2026, 11, 12), moved.endDate)
    }

    @Test fun aNewStartDateMovesTheEndDay() {
        val moved = form(start = "2026-11-12T22:00", end = "2026-11-13T02:00").withStart(date = LocalDate.of(2026, 11, 20))
        assertEquals(LocalDate.of(2026, 11, 21), moved.endDate)
        assertEquals(LocalTime.of(2, 0), moved.endTime)
    }

    @Test fun aStartLateInTheDayPushesTheEndPastMidnight() {
        val moved = form(start = "2026-11-12T10:00", end = "2026-11-12T11:30").withStart(time = LocalTime.of(23, 0))
        assertEquals(LocalDate.of(2026, 11, 13), moved.endDate)
        assertEquals(LocalTime.of(0, 30), moved.endTime)
    }

    @Test fun anAllDayEntryKeepsItsNumberOfDaysWhenItMoves() {
        val moved = form(start = "2026-11-12T00:00", end = "2026-11-14T00:00", allDay = true).withStart(date = LocalDate.of(2026, 12, 1))
        assertEquals(LocalDate.of(2026, 12, 3), moved.endDate)
    }

    @Test fun anEndBeforeTheStartBecomesTheStart() {
        val changed = form(start = "2026-11-12T10:00", end = "2026-11-12T11:00").withEnd(time = LocalTime.of(9, 0))
        assertEquals(LocalTime.of(10, 0), changed.endTime)
        assertEquals(LocalDate.of(2026, 11, 12), changed.endDate)
    }

    @Test fun anEndDateBeforeTheStartDateBecomesTheStartDate() {
        val changed = form().withEnd(date = LocalDate.of(2026, 11, 1))
        assertEquals(LocalDate.of(2026, 11, 12), changed.endDate)
        assertEquals(LocalTime.of(10, 0), changed.endTime)
    }

    @Test fun anEndAfterTheStartIsKept() {
        assertEquals(LocalTime.of(18, 0), form().withEnd(time = LocalTime.of(18, 0)).endTime)
    }

    @Test fun anAllDayEndBeforeTheStartBecomesTheStartDay() {
        val changed = form(allDay = true).withEnd(date = LocalDate.of(2026, 11, 1))
        assertEquals(LocalDate.of(2026, 11, 12), changed.endDate)
    }

    @Test fun switchingAllDayOffAgainNeverLeavesTheEndBeforeTheStart() {
        val allDay = form(start = "2026-11-12T15:00", end = "2026-11-12T09:00", allDay = true)
        val timed = allDay.withAllDay(false, "Europe/Berlin")
        assertFalse(timed.end.isBefore(timed.start))
    }

    @Test fun switchingAllDayOffGivesAnEntryWithoutAZoneThePhonesZone() {
        val floating = form(allDay = true, zone = null)
        assertEquals("Europe/Berlin", floating.withAllDay(false, "Europe/Berlin").zone)
        assertNull(floating.withAllDay(true, "Europe/Berlin").zone)
    }

    @Test fun anEntryLoadedAsTimedAndFloatingKeepsNoZone() {
        val loaded = EventForms.fromInput(input(zone = null), false)
        assertNull(loaded.withAllDay(true, "Europe/Berlin").withAllDay(false, "Europe/Berlin").zone)
    }

    @Test fun anAllDayEntryLoadedWithoutAZoneGetsOneWhenMadeTimed() {
        val loaded = EventForms.fromInput(input(start = "2026-11-12T00:00:00", zone = null, duration = 86_400, allDay = true), false)
        assertEquals("Europe/Berlin", loaded.withAllDay(false, "Europe/Berlin").zone)
    }

    @Test fun theZoneIsNamedOnlyWhenItIsSetAndNotThePhones() {
        assertNull(EventForms.zoneNote(null, berlin))
        assertNull(EventForms.zoneNote("Europe/Berlin", berlin))
        assertEquals("America/New_York", EventForms.zoneNote("America/New_York", berlin))
        assertEquals("Mars/Base", EventForms.zoneNote("Mars/Base", berlin))
    }

    @Test fun aLoadedCountAboveWhatCanBeTypedStaysSaveable() {
        val loaded = EventForms.fromInput(input(repeat = RepeatRule(Frequency.DAILY, 1u, emptyList(), null, 1500u)), false)
        assertEquals(emptyList<Problem>(), EventForms.problems(loaded))
        assertEquals(1500u, EventForms.toInput(loaded).repeat?.count)
        assertEquals("123", loaded.withCountText("123456").repeat?.countText)
    }

    // ---- repeating ----

    @Test fun aWeeklyRuleStartsOnTheWeekdayOfTheStart() {
        val repeating = form().withFrequency(Frequency.WEEKLY)
        assertEquals(setOf(DayOfWeek.THURSDAY), repeating.repeat?.days)
    }

    @Test fun theWeekdayFollowsTheStartWhileItIsTheOnlyOne() {
        val moved = form().withFrequency(Frequency.WEEKLY).withStart(date = LocalDate.of(2026, 11, 14))
        assertEquals(setOf(DayOfWeek.SATURDAY), moved.repeat?.days)
    }

    @Test fun chosenWeekdaysDoNotFollowTheStart() {
        val chosen = form().withFrequency(Frequency.WEEKLY).withWeekday(DayOfWeek.MONDAY, true).withStart(date = LocalDate.of(2026, 11, 14))
        assertEquals(setOf(DayOfWeek.THURSDAY, DayOfWeek.MONDAY), chosen.repeat?.days)
    }

    @Test fun theLastWeekdayCannotBeCleared() {
        val one = form().withFrequency(Frequency.WEEKLY).withWeekday(DayOfWeek.THURSDAY, false)
        assertEquals(setOf(DayOfWeek.THURSDAY), one.repeat?.days)
    }

    @Test fun changingTheFrequencyClearsTheWeekdays() {
        val monthly = form().withFrequency(Frequency.WEEKLY).withFrequency(Frequency.MONTHLY)
        assertEquals(emptySet<DayOfWeek>(), monthly.repeat?.days)
        assertEquals(Frequency.MONTHLY, monthly.repeat?.frequency)
    }

    @Test fun theSameFrequencyChosenAgainChangesNothing() {
        val weekly = form().withFrequency(Frequency.WEEKLY).withWeekday(DayOfWeek.MONDAY, true)
        assertEquals(weekly, weekly.withFrequency(Frequency.WEEKLY))
    }

    @Test fun neverClearsTheRepeat() = assertNull(form().withFrequency(Frequency.DAILY).withFrequency(null).repeat)

    @Test fun anotherFrequencyKeepsHowTheRuleEnds() {
        val ending = form().withFrequency(Frequency.DAILY).withEndKind(EndKind.AFTER_COUNT).withCountText("7").withFrequency(Frequency.WEEKLY)
        assertEquals(EndKind.AFTER_COUNT, ending.repeat?.endKind)
        assertEquals(7, ending.repeat?.count)
    }

    @Test fun anEndDateStartsFourWeeksAfterTheStart() {
        val ending = form().withFrequency(Frequency.DAILY).withEndKind(EndKind.ON_DATE)
        assertEquals(LocalDate.of(2026, 12, 10), ending.repeat?.until)
    }

    @Test fun theCountFieldTakesDigitsOnly() {
        assertEquals("12", form().withFrequency(Frequency.DAILY).withCountText("1a2").repeat?.countText)
        assertEquals("123", form().withFrequency(Frequency.DAILY).withCountText("12345").repeat?.countText)
    }

    @Test fun aWeeklyRuleGoesToTheCoreWithSortedDayCodes() {
        val weekly = form().withFrequency(Frequency.WEEKLY).withWeekday(DayOfWeek.SUNDAY, true).withWeekday(DayOfWeek.MONDAY, true)
        assertEquals(listOf("mo", "th", "su"), EventForms.toInput(weekly).repeat?.byDay)
    }

    @Test fun otherRulesSendNoDays() {
        assertEquals(emptyList<String>(), EventForms.toInput(form().withFrequency(Frequency.MONTHLY)).repeat?.byDay)
    }

    @Test fun anEndDateGoesAsALocalDate() {
        val ending = form().withFrequency(Frequency.DAILY).withEndKind(EndKind.ON_DATE).withUntil(LocalDate.of(2026, 12, 31))
        val rule = EventForms.toInput(ending).repeat
        assertEquals("2026-12-31", rule?.until)
        assertNull(rule?.count)
    }

    @Test fun aCountGoesAsACount() {
        val ending = form().withFrequency(Frequency.DAILY).withEndKind(EndKind.AFTER_COUNT).withCountText("10")
        val rule = EventForms.toInput(ending).repeat
        assertEquals(10u, rule?.count)
        assertNull(rule?.until)
    }

    @Test fun aRuleThatNeverEndsSendsNeitherEvenIfTheyWereSetBefore() {
        val ending = form().withFrequency(Frequency.DAILY).withEndKind(EndKind.ON_DATE).withEndKind(EndKind.NEVER)
        val rule = EventForms.toInput(ending).repeat
        assertNull(rule?.until)
        assertNull(rule?.count)
    }

    @Test fun theIntervalOfAnExistingRuleIsKept() {
        val back = EventForms.fromInput(input(repeat = RepeatRule(Frequency.WEEKLY, 2u, listOf("mo"), null, null)), false)
        assertEquals(2u, EventForms.toInput(back).repeat?.interval)
    }

    @Test fun aRuleFromTheCoreFillsTheForm() {
        val back = EventForms.fromInput(input(repeat = RepeatRule(Frequency.WEEKLY, 1u, listOf("mo", "we"), "2026-12-31", null)), false)
        val rule = back.repeat!!
        assertEquals(setOf(DayOfWeek.MONDAY, DayOfWeek.WEDNESDAY), rule.days)
        assertEquals(EndKind.ON_DATE, rule.endKind)
        assertEquals(LocalDate.of(2026, 12, 31), rule.until)
    }

    @Test fun aCountedRuleFromTheCoreIsCounted() {
        val rule = EventForms.fromInput(input(repeat = RepeatRule(Frequency.DAILY, 1u, emptyList(), null, 5u)), false).repeat!!
        assertEquals(EndKind.AFTER_COUNT, rule.endKind)
        assertEquals(5, rule.count)
    }

    @Test fun aWeeklyRuleWithoutDaysMeansTheWeekdayOfTheStart() {
        val rule = EventForms.fromInput(input(repeat = RepeatRule(Frequency.WEEKLY, 1u, emptyList(), null, null)), false).repeat!!
        assertEquals(setOf(DayOfWeek.THURSDAY), rule.days)
    }

    @Test fun everyRuleSurvivesTheJourney() {
        val rules = listOf(
            RepeatRule(Frequency.DAILY, 1u, emptyList(), null, null),
            RepeatRule(Frequency.WEEKLY, 2u, listOf("mo", "th", "su"), "2026-12-31", null),
            RepeatRule(Frequency.MONTHLY, 3u, emptyList(), null, 12u),
            RepeatRule(Frequency.YEARLY, 1u, emptyList(), null, null),
        )
        for (rule in rules) {
            val original = input(repeat = rule)
            assertEquals(original, EventForms.toInput(EventForms.fromInput(original, false)))
        }
    }

    @Test fun aRuleTheAppCannotEditIsLeftAlone() {
        val back = EventForms.fromInput(input(repeat = null), hasComplexRepeat = true)
        assertTrue(back.repeatLocked)
        assertNull(back.repeat)
        assertNull(EventForms.toInput(back).repeat)
    }

    @Test fun aLockedRuleIsNotCheckedEither() {
        val locked = form().copy(repeatLocked = true, repeat = RepeatForm(Frequency.WEEKLY, days = emptySet(), endKind = EndKind.AFTER_COUNT, countText = ""))
        assertEquals(emptyList<Problem>(), EventForms.problems(locked))
        assertNull(EventForms.toInput(locked).repeat)
    }

    // ---- reminders ----

    @Test fun remindersGoThereAndBack() {
        val original = input(alerts = listOf(15u))
        assertEquals(original, EventForms.toInput(EventForms.fromInput(original, false)))
    }

    @Test fun severalRemindersAreLeftAloneAndStillSent() {
        val several = EventForms.fromInput(input(alerts = listOf(5u, 60u)), false)
        assertTrue(several.alertsLocked)
        assertEquals(listOf(5u, 60u), EventForms.toInput(several).alertMinutes)
    }

    @Test fun oneReminderIsNotLocked() = assertFalse(EventForms.fromInput(input(alerts = listOf(5u)), false).alertsLocked)

    @Test fun theMenuOffersTheUsualRemindersAndTheEntrysOwn() {
        assertEquals(listOf(0, 5, 15, 30, 60, 1440), EventForms.alertOptions(emptyList()))
        assertEquals(listOf(0, 5, 10, 15, 30, 60, 1440), EventForms.alertOptions(listOf(10)))
        assertEquals(listOf(0, 5, 15, 30, 60, 1440), EventForms.alertOptions(listOf(15)))
        assertEquals(listOf(0, 5, 15, 30, 60, 1440), EventForms.alertOptions(listOf(5, 10)))
    }

    // ---- checking the form ----

    @Test fun aGoodFormHasNoProblem() = assertEquals(emptyList<Problem>(), EventForms.problems(form()))

    @Test fun aBlankTitleIsAProblem() {
        assertEquals(listOf(Problem.TITLE_BLANK), EventForms.problems(form(title = "   ")))
    }

    @Test fun noCalendarIsAProblem() {
        assertEquals(listOf(Problem.NO_CALENDAR), EventForms.problems(form(), hasCalendar = false))
        assertEquals(listOf(Problem.NO_CALENDAR), EventForms.problems(form().copy(calendarId = "")))
    }

    @Test fun anEndBeforeTheStartIsAProblem() {
        assertEquals(listOf(Problem.END_BEFORE_START), EventForms.problems(form(start = "2026-11-12T10:00", end = "2026-11-12T09:00")))
    }

    @Test fun aTaskHasNoEndToCheck() {
        assertEquals(emptyList<Problem>(), EventForms.problems(form(start = "2026-11-12T10:00", end = "2026-11-12T09:00", task = true)))
    }

    @Test fun aWeeklyRuleWithoutDaysIsAProblem() {
        val weekly = form().copy(repeat = RepeatForm(Frequency.WEEKLY))
        assertEquals(listOf(Problem.NO_WEEKDAY), EventForms.problems(weekly))
    }

    @Test fun anEndDateBeforeTheStartIsAProblem() {
        val ending = form().withFrequency(Frequency.DAILY).withEndKind(EndKind.ON_DATE).withUntil(LocalDate.of(2026, 11, 1))
        assertEquals(listOf(Problem.UNTIL_BEFORE_START), EventForms.problems(ending))
    }

    @Test fun anEndDateOnTheStartDayIsFine() {
        val ending = form().withFrequency(Frequency.DAILY).withEndKind(EndKind.ON_DATE).withUntil(LocalDate.of(2026, 11, 12))
        assertEquals(emptyList<Problem>(), EventForms.problems(ending))
    }

    @Test fun aMissingEndDateIsAProblem() {
        val ending = form().copy(repeat = RepeatForm(Frequency.DAILY, endKind = EndKind.ON_DATE, until = null))
        assertEquals(listOf(Problem.UNTIL_MISSING), EventForms.problems(ending))
    }

    @Test fun aCountThatIsNotPositiveIsAProblem() {
        for (text in listOf("", "0", " ")) {
            val ending = form().withFrequency(Frequency.DAILY).withEndKind(EndKind.AFTER_COUNT).copy().let {
                it.copy(repeat = it.repeat!!.copy(countText = text))
            }
            assertEquals(text, listOf(Problem.COUNT_INVALID), EventForms.problems(ending))
        }
        val fine = form().withFrequency(Frequency.DAILY).withEndKind(EndKind.AFTER_COUNT).withCountText("999")
        assertEquals(emptyList<Problem>(), EventForms.problems(fine))
    }

    @Test fun problemsComeInTheOrderOfTheScreen() {
        val bad = form(title = "", start = "2026-11-12T10:00", end = "2026-11-12T09:00")
        assertEquals(listOf(Problem.TITLE_BLANK, Problem.END_BEFORE_START), EventForms.problems(bad))
    }

    // ---- reading and the helpers ----

    @Test fun anEntryFillsTheForm() {
        val back = EventForms.fromInput(input(), false)
        assertEquals("Dentist", back.title)
        assertEquals("c1", back.calendarId)
        assertEquals(LocalTime.of(11, 0), back.endTime)
    }

    @Test fun anUnchangedEntryIsNotDirty() {
        val opened = EventForms.fromInput(input(repeat = RepeatRule(Frequency.WEEKLY, 1u, emptyList(), null, null)), false)
        assertEquals(opened, EventForms.fromInput(input(repeat = RepeatRule(Frequency.WEEKLY, 1u, emptyList(), null, null)), false))
    }

    @Test fun anEntryWhoseStartCannotBeReadIsRefused() {
        val result = runCatching { EventForms.fromInput(input(start = "tomorrow"), false) }
        assertTrue(result.isFailure)
    }

    @Test fun dayCodesGoBothWays() {
        for (day in DayOfWeek.entries) assertEquals(day, EventForms.dayOfCode(EventForms.codeOf(day)))
        assertNull(EventForms.dayOfCode("xx"))
        assertEquals(DayOfWeek.TUESDAY, EventForms.dayOfCode(" TU "))
    }

    @Test fun zones() {
        assertEquals(berlin, EventForms.zoneOrNull("Europe/Berlin"))
        assertNull(EventForms.zoneOrNull(null))
        assertNull(EventForms.zoneOrNull("Mars/Base"))
    }

    // ---- the menu of calendars and the pickers ----

    private fun calendar(id: String, write: Boolean = true, default: Boolean = false) =
        Calendar(id, id.uppercase(), null, default, true, write)

    @Test fun theMenuHasTheWritableCalendarsWithTheDefaultFirst() {
        val menu = FormChoices.calendars(listOf(calendar("a"), calendar("b", write = false), calendar("c", default = true)), null)
        assertEquals(listOf("c", "a"), menu.map { it.id })
    }

    @Test fun theCalendarOfAnEntryStaysInTheMenu() {
        val menu = FormChoices.calendars(listOf(calendar("a"), calendar("b", write = false)), "b")
        assertEquals(listOf("a", "b"), menu.map { it.id })
    }

    @Test fun aNewEntryGoesIntoTheDefaultCalendarOrTheFirstWritableOne() {
        assertEquals("c", FormChoices.defaultCalendar(FormChoices.calendars(listOf(calendar("a"), calendar("c", default = true)), null))?.id)
        assertEquals("a", FormChoices.defaultCalendar(FormChoices.calendars(listOf(calendar("a"), calendar("b")), null))?.id)
    }

    @Test fun noWritableCalendarMeansNowhereToAdd() {
        assertNull(FormChoices.defaultCalendar(FormChoices.calendars(listOf(calendar("a", write = false)), null)))
    }

    @Test fun aReadOnlyDefaultIsNotUsedForANewEntry() {
        val menu = FormChoices.calendars(listOf(calendar("a", write = false, default = true), calendar("b")), null)
        assertEquals("b", FormChoices.defaultCalendar(menu)?.id)
    }

    @Test fun aDatePickerSpeaksMidnightUtc() {
        val date = LocalDate.of(2026, 11, 12)
        assertEquals(date, FormChoices.fromPickerMillis(FormChoices.toPickerMillis(date)))
        assertEquals(0L, FormChoices.toPickerMillis(LocalDate.of(1970, 1, 1)))
        assertEquals(LocalDate.of(1969, 12, 31), FormChoices.fromPickerMillis(-86_400_000L))
    }
}
