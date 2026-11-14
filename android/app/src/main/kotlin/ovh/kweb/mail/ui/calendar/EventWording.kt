package ovh.kweb.mail.ui.calendar

import java.time.Instant
import java.time.LocalDate
import java.time.LocalDateTime
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle
import java.time.temporal.ChronoUnit
import java.util.Locale
import uniffi.kweb_core.EventInput
import uniffi.kweb_core.Frequency
import uniffi.kweb_core.RepeatRule

/**
 * When something happens, in words. All-day entries are dates, [end] of one is the day after its
 * last day (midnight), like the core's `ends_at`. [zoneNote] names the zone of a timed entry that
 * is not in the phone's own.
 */
data class Span(
    val start: LocalDateTime,
    val end: LocalDateTime,
    val allDay: Boolean,
    val isTask: Boolean,
    val zoneNote: String? = null,
) {
    /** The last day of an entry, for an all-day entry the day before [end]. */
    val lastDate: LocalDate
        get() = when {
            allDay -> maxOf(start.toLocalDate(), end.toLocalDate().minusDays(1))
            else -> end.toLocalDate()
        }

    /** How many days an all-day entry covers (1 or more). */
    val allDayLength: Int get() = maxOf(1L, ChronoUnit.DAYS.between(start.toLocalDate(), end.toLocalDate())).toInt()

    companion object {
        /** An agenda entry: Unix seconds read as wall-clock time in [zone]. */
        fun ofOccurrence(startsAt: Long, endsAt: Long, allDay: Boolean, isTask: Boolean, zone: ZoneId): Span {
            val start = Instant.ofEpochSecond(startsAt).atZone(zone).toLocalDateTime()
            val end = Instant.ofEpochSecond(maxOf(startsAt, endsAt)).atZone(zone).toLocalDateTime()
            return Span(start, end, allDay, isTask)
        }

        /**
         * An event in full, as the core gives it (for an occurrence its own start). [deviceZone] is the
         * phone's: a different zone of the event is named.
         */
        fun ofInput(input: EventInput, deviceZone: ZoneId): Span {
            val start = LocalDateTime.parse(input.start)
            val zone = EventForms.zoneOrNull(input.timeZone)
            val end = if (input.allDay) {
                start.toLocalDate().plusDays(maxOf(1L, input.durationSecs.toLong() / EventForms.DAY_SECS)).atStartOfDay()
            } else {
                EventForms.addExact(start, zone, input.durationSecs.toLong())
            }
            val note = input.timeZone?.takeIf { !input.allDay && !input.isTask && zone != null && zone != deviceZone }
            return Span(start, end, input.allDay, input.isTask, note)
        }
    }
}

object EventWording {
    private fun time(value: LocalDateTime, locale: Locale): String =
        DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT).withLocale(locale).format(value)

    private fun medium(value: LocalDate, locale: Locale): String =
        DateTimeFormatter.ofLocalizedDate(FormatStyle.MEDIUM).withLocale(locale).format(value)

    private fun full(value: LocalDate, locale: Locale): String =
        DateTimeFormatter.ofLocalizedDate(FormatStyle.FULL).withLocale(locale).format(value)

    private fun dayShort(value: LocalDate, locale: Locale): String = DateTimeFormatter.ofPattern("EEE d", locale).format(value)

    /** The time part of an agenda entry's second line: "10:00 – 10:45", "All day", "Task · due 12:00". */
    fun agendaTime(span: Span, locale: Locale, words: Words): String = when {
        span.isTask -> if (span.allDay) words.task else words.taskDue(time(span.start, locale))
        span.allDay -> if (span.allDayLength == 1) words.allDay else words.allDayDays(span.allDayLength)
        else -> timeRange(span, locale)
    }

    /** "10:00 – 10:45"; just "10:00" without a length; "22:00 – Fri 13, 02:00" over midnight. */
    private fun timeRange(span: Span, locale: Locale): String {
        val start = time(span.start, locale)
        return when {
            span.end == span.start -> start
            span.end.toLocalDate() == span.start.toLocalDate() -> "$start – ${time(span.end, locale)}"
            else -> "$start – ${dayShort(span.end.toLocalDate(), locale)}, ${time(span.end, locale)}"
        }
    }

    /**
     * The whole second line of an agenda entry: time, place and for an invitation that waits for an
     * answer "Invitation", joined by a middle dot. Parts that are empty are left out.
     */
    fun agendaLine(span: Span, location: String, needsAnswer: Boolean, locale: Locale, words: Words): String =
        listOfNotNull(
            agendaTime(span, locale, words),
            location.trim().ifEmpty { null },
            words.invitation.takeIf { needsAnswer },
        ).joinToString(" · ")

    /** An invitation that the user has not answered yet. */
    fun needsAnswer(myStatus: String?): Boolean = myStatus == "needs-action"

    /** The date and the time of an entry on its own screen, as two lines; the second is null when there is nothing to add. */
    data class When(val date: String, val time: String?)

    fun whenOf(span: Span, locale: Locale, words: Words): When {
        val first = span.start.toLocalDate()
        val last = span.lastDate
        val date = if (last == first) full(first, locale) else "${medium(first, locale)} – ${medium(last, locale)}"
        val time = when {
            span.isTask -> if (span.allDay) words.task else words.taskDue(time(span.start, locale))
            span.allDay -> if (span.allDayLength == 1) words.allDay else words.allDayDays(span.allDayLength)
            span.end.toLocalDate() == span.start.toLocalDate() || span.end == span.start -> timeRange(span, locale)
            // Over midnight the date line already has both days, so the clock times alone are enough.
            else -> "${time(span.start, locale)} – ${time(span.end, locale)}"
        }
        return When(date, span.zoneNote?.let { words.withZone(time, it) } ?: time)
    }

    /** "Every week on Mon, Wed, until 31 Dec 2026". [start] is the date of the first occurrence. */
    fun repeat(rule: RepeatRule, locale: Locale, words: Words): String {
        var text = words.every(rule.frequency, maxOf(1, rule.interval.toInt()))
        if (rule.frequency == Frequency.WEEKLY) {
            val days = rule.byDay.mapNotNull { EventForms.dayOfCode(it) }.distinct().sorted()
            if (days.isNotEmpty()) text = words.on(text, days.joinToString(", ") { WeekRules.shortName(it, locale) })
        }
        rule.until?.let { until ->
            val date = runCatching { LocalDate.parse(until.take(10)) }.getOrNull()
            if (date != null) text = words.until(text, medium(date, locale))
        }
        rule.count?.let { if (it > 0u) text = words.times(text, it.toInt()) }
        return text
    }

    /** How long before the start a reminder fires: "At the start", "15 minutes before", "1 day before". */
    fun reminder(minutes: Int, words: Words): String = when {
        minutes <= 0 -> words.atStart
        minutes % 1440 == 0 -> words.daysBefore(minutes / 1440)
        minutes % 60 == 0 -> words.hoursBefore(minutes / 60)
        else -> words.minutesBefore(minutes)
    }
}
