package ovh.kweb.mail.ui.calendar

import java.time.Instant
import java.time.LocalDate
import java.time.ZoneId
import uniffi.kweb_core.Occurrence

/** The Tasks view of the Calendar tab: which tasks, grouped by when they are due. Plain Kotlin, no screen. */
object TaskRules {
    /** How far around today the core is asked for tasks. */
    const val DAYS_BACK = 60L
    const val DAYS_AHEAD = 60L

    enum class Bucket { OVERDUE, TODAY, TOMORROW, LATER }

    data class Section(val bucket: Bucket, val items: List<Occurrence>)

    /** The window `[from, to)` in Unix seconds around [today]. */
    fun window(today: LocalDate, zone: ZoneId): Pair<Long, Long> =
        today.minusDays(DAYS_BACK).atStartOfDay(zone).toEpochSecond() to today.plusDays(DAYS_AHEAD + 1).atStartOfDay(zone).toEpochSecond()

    /** The open tasks of an agenda answer. */
    fun open(entries: List<Occurrence>): List<Occurrence> = entries.filter { it.isTask && !it.isDone }

    fun dueDate(entry: Occurrence, zone: ZoneId): LocalDate = Instant.ofEpochSecond(entry.startsAt).atZone(zone).toLocalDate()

    fun bucket(due: LocalDate, today: LocalDate): Bucket = when {
        due.isBefore(today) -> Bucket.OVERDUE
        due == today -> Bucket.TODAY
        due == today.plusDays(1) -> Bucket.TOMORROW
        else -> Bucket.LATER
    }

    /** The tasks as sections in the order Overdue, Today, Tomorrow, Later; a section with none is left out. Each is by due time, then title. */
    fun sections(entries: List<Occurrence>, today: LocalDate, zone: ZoneId): List<Section> {
        val sorted = entries.sortedWith(compareBy<Occurrence>({ it.startsAt }, { it.title.lowercase() }))
        val by = sorted.groupBy { bucket(dueDate(it, zone), today) }
        return Bucket.entries.mapNotNull { b -> by[b]?.takeIf { it.isNotEmpty() }?.let { Section(b, it) } }
    }

    /** Whether the due date of this section is shown as data in the error colour. */
    fun isLate(bucket: Bucket): Boolean = bucket == Bucket.OVERDUE

    /**
     * The due word of a row: the time for a task due today at a time, [todayWord] for one without; the weekday within a week
     * (with the time when it has one), the date after that.
     */
    fun dueText(entry: Occurrence, today: LocalDate, zone: ZoneId, locale: java.util.Locale, todayWord: String): String {
        val at = Instant.ofEpochSecond(entry.startsAt).atZone(zone)
        val time = if (entry.allDay) null else java.time.format.DateTimeFormatter.ofLocalizedTime(java.time.format.FormatStyle.SHORT).withLocale(locale).format(at)
        val days = java.time.temporal.ChronoUnit.DAYS.between(today, at.toLocalDate())
        if (days == 0L) return time ?: todayWord
        val near = days in -6..6
        val day = java.time.format.DateTimeFormatter.ofPattern(if (near) "EEE" else "d MMM", locale).format(at)
        return if (time != null && near) "$day $time" else day
    }

    /** A key that is the same for the same entry across loads. */
    fun key(entry: Occurrence): String = "${entry.eventId}/${entry.recurrenceId.orEmpty()}/${entry.startsAt}"
}
