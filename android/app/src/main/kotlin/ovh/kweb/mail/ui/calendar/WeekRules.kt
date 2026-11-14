package ovh.kweb.mail.ui.calendar

import java.time.DayOfWeek
import java.time.LocalDate
import java.time.format.DateTimeFormatter
import java.time.format.TextStyle
import java.time.temporal.TemporalAdjusters
import java.time.temporal.WeekFields
import java.util.Locale

/** The week strip: which days it shows and how they are named. */
object WeekRules {
    /** The first day of the week in [locale] (Monday in most of Europe, Sunday in the United States). */
    fun firstDayOfWeek(locale: Locale): DayOfWeek = WeekFields.of(locale).firstDayOfWeek

    fun weekStart(date: LocalDate, first: DayOfWeek): LocalDate = date.with(TemporalAdjusters.previousOrSame(first))

    /** The seven days of the week that holds [date], starting with [first]. */
    fun days(date: LocalDate, first: DayOfWeek): List<LocalDate> {
        val start = weekStart(date, first)
        return (0L..6L).map { start.plusDays(it) }
    }

    /** The day [weeks] weeks from [date]: the same weekday, so a swipe keeps the column. */
    fun shift(date: LocalDate, weeks: Int): LocalDate = date.plusWeeks(weeks.toLong())

    /** "November 2026". */
    fun title(date: LocalDate, locale: Locale): String = DateTimeFormatter.ofPattern("LLLL yyyy", locale).format(date)

    /** The one-letter name over a day of the strip. */
    fun initial(day: DayOfWeek, locale: Locale): String = day.getDisplayName(TextStyle.NARROW, locale)

    /** The whole name, for screen readers. */
    fun fullName(day: DayOfWeek, locale: Locale): String = day.getDisplayName(TextStyle.FULL, locale)

    fun shortName(day: DayOfWeek, locale: Locale): String = day.getDisplayName(TextStyle.SHORT, locale)
}
