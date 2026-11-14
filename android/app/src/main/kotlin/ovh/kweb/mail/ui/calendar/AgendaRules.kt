package ovh.kweb.mail.ui.calendar

import java.time.Instant
import java.time.LocalDate
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.util.Locale
import uniffi.kweb_core.Occurrence

/** How the agenda's entries are grouped, ordered, named and coloured. */
object AgendaRules {
    data class Day(val date: LocalDate, val entries: List<Occurrence>)

    /** One line of the agenda list: a day heading or an entry under it. */
    sealed interface Row {
        data class Header(val date: LocalDate) : Row

        data class Entry(val date: LocalDate, val occurrence: Occurrence, val continues: Boolean = false) : Row {
            /** Unique among the rows of one list, for the list's keys: an entry of several days has a row per day. */
            val key: String get() = "$date/${occurrence.eventId}/${occurrence.recurrenceId.orEmpty()}/${occurrence.startsAt}"
        }
    }

    /** The day an entry belongs to: the day it starts, in [zone]. */
    fun dateOf(entry: Occurrence, zone: ZoneId): LocalDate = Instant.ofEpochSecond(entry.startsAt).atZone(zone).toLocalDate()

    /**
     * The last day an entry covers, in [zone]: the day its end falls on, except that an end exactly at
     * midnight belongs to the day before. An entry without a length covers its start day only.
     */
    fun lastDateOf(entry: Occurrence, zone: ZoneId): LocalDate {
        val first = dateOf(entry, zone)
        if (entry.endsAt <= entry.startsAt) return first
        val last = Instant.ofEpochSecond(entry.endsAt - 1).atZone(zone).toLocalDate()
        return maxOf(first, last)
    }

    /**
     * The entries by day, earliest day first, days without entries left out. An entry of several days
     * is under each day it covers; with [from] and [to] (the window, [to] exclusive) only those days
     * inside it. Within a day the all-day entries come first, then by start time, then by title. The
     * same occurrence twice counts once.
     */
    fun group(entries: List<Occurrence>, zone: ZoneId, from: LocalDate? = null, to: LocalDate? = null): List<Day> {
        val distinct = entries.distinctBy { Triple(it.eventId, it.recurrenceId, it.startsAt) }
        val perDay = sortedMapOf<LocalDate, MutableList<Occurrence>>()
        for (entry in distinct) {
            val first = dateOf(entry, zone)
            val last = lastDateOf(entry, zone)
            var date = if (from != null && first.isBefore(from)) from else first
            val end = if (to != null && last.isAfter(to.minusDays(1))) to.minusDays(1) else last
            while (!date.isAfter(end)) {
                perDay.getOrPut(date) { mutableListOf() } += entry
                date = date.plusDays(1)
            }
        }
        return perDay.map { (date, list) ->
            Day(date, list.sortedWith(compareBy<Occurrence>({ !it.allDay }, { it.startsAt }, { it.title.lowercase() })))
        }
    }

    fun rows(days: List<Day>, zone: ZoneId): List<Row> = days.flatMap { day ->
        listOf<Row>(Row.Header(day.date)) + day.entries.map { Row.Entry(day.date, it, continues = dateOf(it, zone) != day.date) }
    }

    /** The row to scroll to for [date]: its heading, or that of the next day that has entries. Null when there is none. */
    fun indexOfDay(rows: List<Row>, date: LocalDate): Int? =
        rows.indexOfFirst { it is Row.Header && !it.date.isBefore(date) }.takeIf { it >= 0 }

    /**
     * "Thu 12" for a day of the month [today] is in, "Sat 21 Nov" for another month and
     * "Mon 4 Jan 2027" for another year. For [today] itself [todayWord] comes first: "Today · Thu 12".
     */
    fun heading(date: LocalDate, today: LocalDate, locale: Locale, todayWord: String): String {
        val pattern = when {
            date.year != today.year -> "EEE d MMM yyyy"
            date.month != today.month -> "EEE d MMM"
            else -> "EEE d"
        }
        val label = DateTimeFormatter.ofPattern(pattern, locale).format(date)
        return if (date == today) "$todayWord · $label" else label
    }

    /** Which of the three container colours a calendar takes: by its position, cycling. 0 for one that is not known. */
    fun tone(calendarId: String, calendarIds: List<String>): Int = calendarIds.indexOf(calendarId).coerceAtLeast(0) % TONES

    const val TONES = 3
}
