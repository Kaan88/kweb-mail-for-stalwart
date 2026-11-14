package ovh.kweb.mail.ui.calendar

import java.time.LocalDate
import java.time.ZoneId
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class AgendaWindowTest {
    private val week = LocalDate.of(2026, 11, 9)

    @Test fun theLongestWindowIsWhatTheServerExpands() = assertEquals(364L, AgendaWindow.MAX_DAYS)

    @Test fun theWindowIsFourWeeksBackAndEightAhead() {
        val window = AgendaWindow.around(week)
        assertEquals(LocalDate.of(2026, 10, 12), window.from)
        // The week itself and eight more: the end is exclusive.
        assertEquals(LocalDate.of(2027, 1, 11), window.to)
        assertEquals(13 * 7L, window.days)
    }

    @Test fun theSelectedWeekIsCoveredWithAWeekOfMargin() {
        val window = AgendaWindow.around(week)
        assertTrue(AgendaWindow.covers(window, week))
        assertTrue(AgendaWindow.covers(window, week.plusWeeks(7)))
        assertFalse(AgendaWindow.covers(window, week.plusWeeks(8)))
        assertTrue(AgendaWindow.covers(window, week.minusWeeks(3)))
        assertFalse(AgendaWindow.covers(window, week.minusWeeks(4)))
    }

    @Test fun aWindowThatStillCoversStaysAsItIs() {
        val window = AgendaWindow.around(week)
        assertEquals(window, AgendaWindow.next(window, week.plusWeeks(2)))
    }

    @Test fun noWindowYetIsOneAroundTheWeek() = assertEquals(AgendaWindow.around(week), AgendaWindow.next(null, week))

    @Test fun nearTheEdgeTheWindowGrowsAndKeepsWhatItHad() {
        val old = AgendaWindow.around(week)
        val moved = week.plusWeeks(8)
        val next = AgendaWindow.next(old, moved)
        assertEquals(old.from, next.from)
        assertEquals(AgendaWindow.around(moved).to, next.to)
        assertTrue(AgendaWindow.covers(next, moved))
    }

    @Test fun movingBackGrowsTheOtherWay() {
        val old = AgendaWindow.around(week)
        val moved = week.minusWeeks(4)
        val next = AgendaWindow.next(old, moved)
        assertEquals(AgendaWindow.around(moved).from, next.from)
        assertEquals(old.to, next.to)
    }

    @Test fun aJumpFarAwayStartsOver() {
        val old = AgendaWindow.around(week)
        val far = week.plusWeeks(80)
        assertEquals(AgendaWindow.around(far), AgendaWindow.next(old, far))
    }

    @Test fun neverMoreThanAYearIsAsked() {
        var window = AgendaWindow.around(week)
        var selected = week
        repeat(120) {
            selected = selected.plusWeeks(1)
            window = AgendaWindow.next(window, selected)
            assertTrue("${window.days} days", window.days <= AgendaWindow.MAX_DAYS)
            assertTrue(AgendaWindow.covers(window, selected))
        }
    }

    @Test fun movingBackAWholeYearNeverAsksForMoreEither() {
        var window = AgendaWindow.around(week)
        var selected = week
        repeat(120) {
            selected = selected.minusWeeks(1)
            window = AgendaWindow.next(window, selected)
            assertTrue(window.days <= AgendaWindow.MAX_DAYS)
            assertTrue(AgendaWindow.covers(window, selected))
        }
    }

    @Test fun theCoreGetsMidnightsInTheZone() {
        val window = AgendaWindow.Window(LocalDate.of(2026, 3, 23), LocalDate.of(2026, 3, 30))
        val berlin = ZoneId.of("Europe/Berlin")
        val (from, to) = AgendaWindow.seconds(window, berlin)
        assertEquals(LocalDate.of(2026, 3, 23).atStartOfDay(berlin).toEpochSecond(), from)
        // The clocks change on 29 March: the span is an hour shorter than seven times 24 hours.
        assertEquals(7 * 86_400L - 3_600L, to - from)
    }
}
