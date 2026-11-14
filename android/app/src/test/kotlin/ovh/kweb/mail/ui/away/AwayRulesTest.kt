package ovh.kweb.mail.ui.away

import java.time.LocalDate
import java.time.ZoneId
import java.util.Locale
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.kweb_core.AwayMessage

class AwayRulesTest {
    private val ankara = ZoneId.of("Europe/Istanbul")
    private val berlin = ZoneId.of("Europe/Berlin")
    private val day = LocalDate.of(2026, 11, 21)

    private fun message(enabled: Boolean = true, from: Long? = null, until: Long? = null) = AwayMessage(enabled, from, until, "Away", "Back soon")

    @Test fun aStartIsTheFirstMomentOfTheDay() =
        assertEquals(java.time.ZonedDateTime.of(2026, 11, 21, 0, 0, 0, 0, ankara).toEpochSecond(), AwayRules.startOfDay(day, ankara))

    @Test fun anEndIsTheLastSecondOfTheDay() =
        assertEquals(java.time.ZonedDateTime.of(2026, 11, 21, 23, 59, 59, 0, ankara).toEpochSecond(), AwayRules.endOfDay(day, ankara))

    @Test fun aDayWithAClockChangeEndsAtItsOwnEnd() {
        val changeDay = LocalDate.of(2026, 3, 29)
        val start = AwayRules.startOfDay(changeDay, berlin)
        val end = AwayRules.endOfDay(changeDay, berlin)
        assertEquals(23 * 3600L - 1, end - start)
        assertEquals(changeDay, AwayRules.dayOfEnd(end, berlin))
    }

    @Test fun theZoneDecidesTheDay() {
        val seconds = AwayRules.startOfDay(day, ankara)
        assertEquals(day, AwayRules.dayOfStart(seconds, ankara))
        assertEquals(day.minusDays(1), AwayRules.dayOfStart(seconds, ZoneId.of("America/New_York")))
    }

    @Test fun anEndAtMidnightBelongsToTheDayBefore() =
        assertEquals(day.minusDays(1), AwayRules.dayOfEnd(AwayRules.startOfDay(day, ankara), ankara))

    @Test fun aMessageComesBackAsItWas() {
        val original = message(from = AwayRules.startOfDay(day, ankara), until = AwayRules.endOfDay(day.plusDays(3), ankara))
        assertEquals(original, AwayRules.toMessage(AwayRules.fromMessage(original, ankara), ankara))
    }

    @Test fun aDateTheUserDidNotChangeKeepsItsTimeOfDay() {
        val tenOClock = AwayRules.startOfDay(day, ankara) + 10 * 3600
        val original = message(from = tenOClock, until = tenOClock + 3 * 86400)
        val form = AwayRules.fromMessage(original, ankara)
        assertEquals(original, AwayRules.toMessage(form, ankara))
        assertFalse(AwayRules.isChanged(form, form, ankara))
    }

    @Test fun aChangedDateMeansItsWholeDay() {
        val form = AwayRules.fromMessage(message(from = 1000, until = 2000), ankara).copy(from = day, until = day.plusDays(1))
        val sent = AwayRules.toMessage(form, ankara)
        assertEquals(AwayRules.startOfDay(day, ankara), sent.from)
        assertEquals(AwayRules.endOfDay(day.plusDays(1), ankara), sent.until)
    }

    @Test fun clearingADateSendsNone() {
        val form = AwayRules.fromMessage(message(from = 1000, until = 2000), ankara).copy(from = null, until = null)
        val sent = AwayRules.toMessage(form, ankara)
        assertEquals(null, sent.from)
        assertEquals(null, sent.until)
    }

    @Test fun subjectAndTextAreTrimmed() {
        val sent = AwayRules.toMessage(AwayForm(subject = " Away ", text = "\nBack soon \n"), ankara)
        assertEquals("Away", sent.subject)
        assertEquals("Back soon", sent.text)
    }

    @Test fun theSwitchIsAChange() {
        val form = AwayForm()
        assertTrue(AwayRules.isChanged(form.copy(enabled = true), form, ankara))
    }

    @Test fun offIsOff() = assertEquals(AwayLine.Off, AwayRules.line(message(enabled = false, until = 5), 0, ankara))

    @Test fun onWithoutDatesIsOn() = assertEquals(AwayLine.On, AwayRules.line(message(), 0, ankara))

    @Test fun onWithAnEndSaysUntil() {
        val until = AwayRules.endOfDay(day, ankara)
        assertEquals(AwayLine.OnUntil(day), AwayRules.line(message(until = until), 0, ankara))
    }

    @Test fun anEndThatHasPassedSaysEnded() {
        val until = AwayRules.endOfDay(day, ankara)
        assertEquals(AwayLine.Ended(day), AwayRules.line(message(until = until), until + 1, ankara))
        assertEquals(AwayLine.Ended(day), AwayRules.line(message(until = until), until + 86_400L * 30, ankara))
    }

    @Test fun aMessageThatEndsLaterTodayIsStillOn() {
        val until = AwayRules.endOfDay(day, ankara)
        assertEquals(AwayLine.OnUntil(day), AwayRules.line(message(until = until), until - 3600, ankara))
    }

    @Test fun aStartThatHasPassedIsNotMentioned() {
        val from = AwayRules.startOfDay(day, ankara)
        assertEquals(AwayLine.On, AwayRules.line(message(from = from), from + 1, ankara))
    }

    @Test fun aStartInTheFutureIsMentioned() {
        val from = AwayRules.startOfDay(day, ankara)
        assertEquals(AwayLine.OnFrom(day), AwayRules.line(message(from = from), from - 1, ankara))
        assertEquals(
            AwayLine.OnFromUntil(day, day.plusDays(2)),
            AwayRules.line(message(from = from, until = AwayRules.endOfDay(day.plusDays(2), ankara)), from - 1, ankara),
        )
    }

    @Test fun theShortDateIsDayAndMonth() = assertEquals("21 Nov", AwayRules.shortDate(day, Locale.UK))
}
