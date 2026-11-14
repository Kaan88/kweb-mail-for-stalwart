package ovh.kweb.mail.ui.calendar

import java.time.DayOfWeek
import java.time.LocalDate
import java.util.Locale
import org.junit.Assert.assertEquals
import org.junit.Test

class WeekRulesTest {
    // Thursday 12 November 2026.
    private val thursday = LocalDate.of(2026, 11, 12)

    @Test fun mondayStartsTheWeekInGermanyAndBritain() {
        assertEquals(DayOfWeek.MONDAY, WeekRules.firstDayOfWeek(Locale.GERMANY))
        assertEquals(DayOfWeek.MONDAY, WeekRules.firstDayOfWeek(Locale.UK))
    }

    @Test fun sundayStartsTheWeekInTheUnitedStates() = assertEquals(DayOfWeek.SUNDAY, WeekRules.firstDayOfWeek(Locale.US))

    @Test fun theWeekOfAThursdayFromMonday() {
        val days = WeekRules.days(thursday, DayOfWeek.MONDAY)
        assertEquals(LocalDate.of(2026, 11, 9), days.first())
        assertEquals(LocalDate.of(2026, 11, 15), days.last())
        assertEquals(7, days.size)
        assertEquals(true, thursday in days)
    }

    @Test fun theWeekOfAThursdayFromSunday() {
        val days = WeekRules.days(thursday, DayOfWeek.SUNDAY)
        assertEquals(LocalDate.of(2026, 11, 8), days.first())
        assertEquals(LocalDate.of(2026, 11, 14), days.last())
    }

    @Test fun theFirstDayItselfStartsItsWeek() {
        val monday = LocalDate.of(2026, 11, 9)
        assertEquals(monday, WeekRules.days(monday, DayOfWeek.MONDAY).first())
    }

    @Test fun theLastDayBelongsToTheWeekBefore() {
        val sunday = LocalDate.of(2026, 11, 15)
        assertEquals(LocalDate.of(2026, 11, 9), WeekRules.weekStart(sunday, DayOfWeek.MONDAY))
        assertEquals(sunday, WeekRules.weekStart(sunday, DayOfWeek.SUNDAY))
    }

    @Test fun aWeekCanCrossAMonthAndAYear() {
        val days = WeekRules.days(LocalDate.of(2026, 12, 31), DayOfWeek.MONDAY)
        assertEquals(LocalDate.of(2026, 12, 28), days.first())
        assertEquals(LocalDate.of(2027, 1, 3), days.last())
    }

    @Test fun aSwipeKeepsTheWeekday() {
        assertEquals(LocalDate.of(2026, 11, 19), WeekRules.shift(thursday, 1))
        assertEquals(LocalDate.of(2026, 11, 5), WeekRules.shift(thursday, -1))
        assertEquals(DayOfWeek.THURSDAY, WeekRules.shift(thursday, 30).dayOfWeek)
    }

    @Test fun theTitleIsMonthAndYear() = assertEquals("November 2026", WeekRules.title(thursday, Locale.UK))

    @Test fun theTitleFollowsTheLanguage() = assertEquals("November 2026", WeekRules.title(thursday, Locale.GERMANY))

    @Test fun narrowAndFullNames() {
        assertEquals("T", WeekRules.initial(DayOfWeek.THURSDAY, Locale.UK))
        assertEquals("Thursday", WeekRules.fullName(DayOfWeek.THURSDAY, Locale.UK))
        assertEquals("Thu", WeekRules.shortName(DayOfWeek.THURSDAY, Locale.UK))
    }
}
