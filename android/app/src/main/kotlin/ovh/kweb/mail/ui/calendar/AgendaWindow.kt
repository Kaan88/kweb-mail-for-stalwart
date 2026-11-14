package ovh.kweb.mail.ui.calendar

import java.time.LocalDate
import java.time.ZoneId
import java.time.temporal.ChronoUnit

/**
 * The span of days the agenda has asked the core for. It sits around the selected week and is
 * extended when the user moves close to its edge, but never grows beyond a year.
 */
object AgendaWindow {
    const val BACK_WEEKS = 4L
    const val AHEAD_WEEKS = 8L

    /** The longest span ever asked for: the server expands 365 days at most, and the core refuses more. */
    const val MAX_DAYS = 364L

    /** From [from] on, up to but not including [to]. */
    data class Window(val from: LocalDate, val to: LocalDate) {
        val days: Long get() = ChronoUnit.DAYS.between(from, to)
    }

    /** [BACK_WEEKS] weeks before the week starting [weekStart], the week itself and [AHEAD_WEEKS] weeks after it. */
    fun around(weekStart: LocalDate) = Window(weekStart.minusWeeks(BACK_WEEKS), weekStart.plusWeeks(AHEAD_WEEKS + 1))

    /** The week has a whole week of margin on both sides inside [window]. */
    fun covers(window: Window, weekStart: LocalDate): Boolean =
        !weekStart.isBefore(window.from.plusWeeks(1)) && !weekStart.plusWeeks(2).isAfter(window.to)

    /**
     * The window to use once the week starting [weekStart] is selected: [current] while it still
     * covers the week; otherwise [current] extended to cover it, or a fresh one around the week
     * when the old one is far away or the extension would pass a year.
     */
    fun next(current: Window?, weekStart: LocalDate): Window {
        val fresh = around(weekStart)
        if (current == null) return fresh
        if (covers(current, weekStart)) return current
        if (fresh.to < current.from || fresh.from > current.to) return fresh
        val hull = Window(minOf(current.from, fresh.from), maxOf(current.to, fresh.to))
        return if (hull.days > MAX_DAYS) fresh else hull
    }

    /** The window as Unix seconds `[from, to)` at midnight in [zone], for the core. */
    fun seconds(window: Window, zone: ZoneId): Pair<Long, Long> =
        window.from.atStartOfDay(zone).toEpochSecond() to window.to.atStartOfDay(zone).toEpochSecond()
}
