package ovh.kweb.mail.ui.sending

import java.time.Instant
import java.time.LocalDate
import java.time.LocalTime
import java.time.ZoneId
import java.time.ZonedDateTime
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class SendLaterRulesTest {
    private val zone = ZoneId.of("Europe/Istanbul")
    private fun at(h: Int, m: Int = 0, day: Int = 14) = ZonedDateTime.of(2026, 11, day, h, m, 0, 0, zone).toInstant()

    @Test fun inTheMorningAllThreeAreOffered() {
        val now = at(9, 30)
        val c = SendLaterRules.choices(now, zone)
        assertEquals(listOf(SendLaterRules.Kind.InOneHour, SendLaterRules.Kind.ThisEvening, SendLaterRules.Kind.TomorrowMorning), c.map { it.kind })
        assertEquals(now.epochSecond + 3600, c[0].at)
        assertEquals(at(18).epochSecond, c[1].at)
        assertEquals(at(8, 0, 15).epochSecond, c[2].at)
    }

    @Test fun anEveningThatHasPassedIsLeftOut() {
        val c = SendLaterRules.choices(at(19, 10), zone)
        assertEquals(listOf(SendLaterRules.Kind.InOneHour, SendLaterRules.Kind.TomorrowMorning), c.map { it.kind })
    }

    @Test fun anEveningNextToTheHourChoiceIsLeftOut() {
        // One hour from 16:50 is 17:50, ten minutes before 18:00: pointless.
        val c = SendLaterRules.choices(at(16, 50), zone)
        assertFalse(c.any { it.kind == SendLaterRules.Kind.ThisEvening })
        // From 16:00 the hour is 17:00 and the evening is an hour later: useful.
        assertTrue(SendLaterRules.choices(at(16, 0), zone).any { it.kind == SendLaterRules.Kind.ThisEvening })
    }

    @Test fun everyQuickChoiceIsAcceptedByTheCheck() {
        for (hour in 0..23) {
            val now = at(hour, 15)
            for (choice in SendLaterRules.choices(now, zone)) assertEquals(SendLaterRules.Check.Ok, SendLaterRules.check(choice.at, now.epochSecond))
        }
    }

    @Test fun theLimitsAreAMinuteAndSevenDays() {
        val now = 1_000_000L
        assertEquals(SendLaterRules.Check.TooSoon, SendLaterRules.check(now - 5, now))
        assertEquals(SendLaterRules.Check.TooSoon, SendLaterRules.check(now + 59, now))
        assertEquals(SendLaterRules.Check.Ok, SendLaterRules.check(now + 60, now))
        assertEquals(SendLaterRules.Check.Ok, SendLaterRules.check(now + 7 * 86400, now))
        assertEquals(SendLaterRules.Check.TooFar, SendLaterRules.check(now + 7 * 86400 + 1, now))
    }

    @Test fun theDatePickerAllowsTodayAndSevenDays() {
        val today = LocalDate.of(2026, 11, 14)
        assertFalse(SendLaterRules.dateAllowed(today.minusDays(1), today))
        assertTrue(SendLaterRules.dateAllowed(today, today))
        assertTrue(SendLaterRules.dateAllowed(today.plusDays(7), today))
        assertFalse(SendLaterRules.dateAllowed(today.plusDays(8), today))
    }

    @Test fun aDayAndATimeBecomeAMomentInTheZone() {
        val seconds = SendLaterRules.at(LocalDate.of(2026, 11, 20), 7, 45, zone)
        assertEquals(ZonedDateTime.of(2026, 11, 20, 7, 45, 0, 0, zone).toEpochSecond(), seconds)
    }

    @Test fun aTimeThatAClockChangeSkipsMovesOn() {
        val berlin = ZoneId.of("Europe/Berlin")
        // 02:30 does not exist on 29 March 2026 in Berlin.
        val seconds = SendLaterRules.at(LocalDate.of(2026, 3, 29), 2, 30, berlin)
        assertEquals(3, Instant.ofEpochSecond(seconds).atZone(berlin).hour)
    }

    @Test fun thePickersStartAnHourFromNow() {
        val (day, time) = SendLaterRules.defaultPick(at(23, 40), zone)
        assertEquals(LocalDate.of(2026, 11, 15), day)
        assertEquals(LocalTime.of(0, 40), time)
    }

    @Test fun aTimeIsNamedByItsDay() {
        val now = at(10).epochSecond
        assertEquals(SendLaterRules.Day.Today, SendLaterRules.describe(at(18).epochSecond, now, zone).day)
        assertEquals(SendLaterRules.Day.Tomorrow, SendLaterRules.describe(at(8, 0, 15).epochSecond, now, zone).day)
        val later = SendLaterRules.describe(at(8, 30, 18).epochSecond, now, zone)
        assertEquals(SendLaterRules.Day.Other, later.day)
        assertEquals(LocalTime.of(8, 30), later.time)
        assertEquals(LocalDate.of(2026, 11, 18), later.date)
    }

    @Test fun theDayIsTheOneInTheDevicesZoneNotUtc() {
        // 23:30 in Istanbul (UTC+3) on the 14th is still the 14th there and the 14th in UTC; 01:00 on the 15th there is 22:00 UTC on the 14th.
        val now = at(23, 30).epochSecond
        assertEquals(SendLaterRules.Day.Tomorrow, SendLaterRules.describe(at(1, 0, 15).epochSecond, now, zone).day)
    }
}
