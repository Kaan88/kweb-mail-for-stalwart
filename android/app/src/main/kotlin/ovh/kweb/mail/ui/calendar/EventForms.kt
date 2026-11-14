package ovh.kweb.mail.ui.calendar

import java.time.DateTimeException
import java.time.DayOfWeek
import java.time.Duration
import java.time.LocalDate
import java.time.LocalDateTime
import java.time.LocalTime
import java.time.ZoneId
import java.time.ZonedDateTime
import java.time.format.DateTimeFormatter
import java.time.temporal.ChronoUnit
import uniffi.kweb_core.EventInput
import uniffi.kweb_core.Frequency
import uniffi.kweb_core.RepeatRule

/** How a repeat rule ends. */
enum class EndKind { NEVER, ON_DATE, AFTER_COUNT }

/** A repeat rule as the form has it. */
data class RepeatForm(
    val frequency: Frequency,
    /** Every n-th day, week, month or year. The form does not change it, but keeps what the server has. */
    val interval: Int = 1,
    /** For a weekly rule: the weekdays. */
    val days: Set<DayOfWeek> = emptySet(),
    val endKind: EndKind = EndKind.NEVER,
    val until: LocalDate? = null,
    /** What is typed in the count field, so that an empty or wrong entry is not lost. */
    val countText: String = "5",
) {
    val count: Int? get() = countText.trim().toIntOrNull()?.takeIf { it >= 1 }

    companion object {
        /** What can be typed: three digits. A larger count the server already has is kept as it is. */
        const val MAX_COUNT = 999
    }
}

/** Something in the form that has to change before it can be saved. */
enum class Problem { TITLE_BLANK, NO_CALENDAR, END_BEFORE_START, UNTIL_MISSING, UNTIL_BEFORE_START, COUNT_INVALID, NO_WEEKDAY }

/**
 * What the Edit screen edits. Times are wall-clock times in [zone] (null: floating), as in the
 * core's [EventInput]. For an all-day entry [endDate] is its last day, not the day after.
 */
data class EventForm(
    val calendarId: String,
    val title: String = "",
    val location: String = "",
    val description: String = "",
    val isTask: Boolean = false,
    val allDay: Boolean = false,
    val startDate: LocalDate,
    val startTime: LocalTime,
    val endDate: LocalDate,
    val endTime: LocalTime,
    val zone: String?,
    val repeat: RepeatForm? = null,
    /** The server's rule says more than the form can: it is left as it is. */
    val repeatLocked: Boolean = false,
    /** Reminders as minutes before the start. */
    val alerts: List<Int> = emptyList(),
    /** The entry was loaded as a timed one without a zone (floating): switching "All day" off keeps it so. */
    val loadedFloating: Boolean = false,
    /** The timing as loaded, so that a save that changes none of it sends the length the server has. */
    val loaded: LoadedTiming? = null,
) {
    val start: LocalDateTime get() = LocalDateTime.of(startDate, startTime)
    val end: LocalDateTime get() = LocalDateTime.of(endDate, endTime)

    /** More than one reminder: the form cannot show them one by one, so it leaves them alone. */
    val alertsLocked: Boolean get() = alerts.size > 1

    /** The wall-clock length of the entry, for keeping it when the start moves. */
    private fun shiftSpan(): Duration = if (allDay) {
        Duration.ofDays(ChronoUnit.DAYS.between(startDate, endDate))
    } else {
        Duration.between(start, end)
    }.coerceAtLeast(Duration.ZERO)

    /** The end is never before the start: if it would be, it becomes the start. */
    fun clampEnd(): EventForm = when {
        allDay && endDate.isBefore(startDate) -> copy(endDate = startDate)
        !allDay && end.isBefore(start) -> copy(endDate = startDate, endTime = startTime)
        else -> this
    }

    /** A new start moves the end with it, so the length stays. */
    fun withStart(date: LocalDate = startDate, time: LocalTime = startTime): EventForm {
        val span = shiftSpan()
        val newStart = LocalDateTime.of(date, time)
        val newEnd = if (allDay) LocalDateTime.of(date.plusDays(span.toDays()), endTime) else newStart.plus(span)
        val weekdayFollows = repeat?.takeIf { it.frequency == Frequency.WEEKLY && it.days == setOf(startDate.dayOfWeek) }
        return copy(
            startDate = date,
            startTime = time,
            endDate = newEnd.toLocalDate(),
            endTime = if (allDay) endTime else newEnd.toLocalTime(),
            repeat = if (weekdayFollows != null) weekdayFollows.copy(days = setOf(date.dayOfWeek)) else repeat,
        )
    }

    /** A new end; one before the start becomes the start. */
    fun withEnd(date: LocalDate = endDate, time: LocalTime = endTime): EventForm = copy(endDate = date, endTime = time).clampEnd()

    /**
     * Switching "All day" off on an entry that has no zone gives it [deviceZone] (an all-day entry is floating by
     * nature, a timed floating one would follow the phone). An entry loaded as timed and floating keeps none.
     */
    fun withAllDay(on: Boolean, deviceZone: String): EventForm {
        val zone = if (allDay && !on && this.zone == null && !loadedFloating) deviceZone else this.zone
        return copy(allDay = on, zone = zone).clampEnd()
    }

    /** Event or task. A task has a due time and no end, so nothing is lost by switching back. */
    fun withTask(task: Boolean): EventForm = copy(isTask = task)

    fun withFrequency(frequency: Frequency?): EventForm {
        if (frequency == null) return copy(repeat = null)
        val current = repeat
        if (current != null && current.frequency == frequency) return this
        val days = if (frequency == Frequency.WEEKLY) setOf(startDate.dayOfWeek) else emptySet()
        val next = current?.copy(frequency = frequency, interval = 1, days = days) ?: RepeatForm(frequency, days = days)
        return copy(repeat = next)
    }

    /** Selects or clears a weekday of a weekly rule. The last one cannot be cleared. */
    fun withWeekday(day: DayOfWeek, selected: Boolean): EventForm {
        val rule = repeat ?: return this
        val days = if (selected) rule.days + day else rule.days - day
        return if (days.isEmpty()) this else copy(repeat = rule.copy(days = days))
    }

    fun withEndKind(kind: EndKind): EventForm {
        val rule = repeat ?: return this
        val until = if (kind == EndKind.ON_DATE) rule.until ?: startDate.plusWeeks(4) else rule.until
        return copy(repeat = rule.copy(endKind = kind, until = until))
    }

    fun withUntil(date: LocalDate): EventForm = repeat?.let { copy(repeat = it.copy(until = date)) } ?: this

    fun withCountText(text: String): EventForm = repeat?.let { copy(repeat = it.copy(countText = text.filter(Char::isDigit).take(3))) } ?: this
}

/** The timing of an entry as the server gave it. */
data class LoadedTiming(
    val startDate: LocalDate,
    val startTime: LocalTime,
    val endDate: LocalDate,
    val endTime: LocalTime,
    val allDay: Boolean,
    val isTask: Boolean,
    val zone: String?,
    val durationSecs: ULong,
)

/** Turning the form into the core's [EventInput] and back, and checking it. */
object EventForms {
    const val DAY_SECS = 86_400L

    private val codes = mapOf(
        "mo" to DayOfWeek.MONDAY, "tu" to DayOfWeek.TUESDAY, "we" to DayOfWeek.WEDNESDAY, "th" to DayOfWeek.THURSDAY,
        "fr" to DayOfWeek.FRIDAY, "sa" to DayOfWeek.SATURDAY, "su" to DayOfWeek.SUNDAY,
    )

    fun dayOfCode(code: String): DayOfWeek? = codes[code.trim().lowercase()]

    fun codeOf(day: DayOfWeek): String = codes.entries.first { it.value == day }.key

    /** The zone [id] names, or null for a floating time and for a name this phone does not know. */
    fun zoneOrNull(id: String?): ZoneId? = id?.let { runCatching { ZoneId.of(it) }.getOrNull() }

    /** [start] plus [seconds] of real time in [zone]: the clocks may change inside. Without a zone it is wall-clock time. */
    fun addExact(start: LocalDateTime, zone: ZoneId?, seconds: Long): LocalDateTime =
        if (zone == null) start.plusSeconds(seconds) else ZonedDateTime.of(start, zone).plusSeconds(seconds).toLocalDateTime()

    /** The real time between two wall-clock times in [zone], never negative. */
    fun betweenExact(start: LocalDateTime, end: LocalDateTime, zone: ZoneId?): Long {
        val seconds = if (zone == null) {
            Duration.between(start, end).seconds
        } else {
            Duration.between(ZonedDateTime.of(start, zone), ZonedDateTime.of(end, zone)).seconds
        }
        return maxOf(0L, seconds)
    }

    /** What a new entry starts as: the selected day, at the next full hour when that is today, else 09:00, for an hour. */
    fun blank(calendarId: String, day: LocalDate, now: LocalDateTime, zone: ZoneId): EventForm {
        val hour = if (day == now.toLocalDate()) minOf(now.hour + 1, 23) else 9
        val start = LocalDateTime.of(day, LocalTime.of(hour, 0))
        val end = start.plusHours(1)
        return EventForm(
            calendarId = calendarId,
            startDate = start.toLocalDate(),
            startTime = start.toLocalTime(),
            endDate = end.toLocalDate(),
            endTime = end.toLocalTime(),
            zone = zone.id,
        )
    }

    /** The form for an existing entry. Throws [DateTimeException] when its start cannot be read. */
    fun fromInput(input: EventInput, hasComplexRepeat: Boolean): EventForm {
        val start = LocalDateTime.parse(input.start)
        val zone = zoneOrNull(input.timeZone)
        val end: LocalDateTime
        val endDate: LocalDate
        if (input.allDay) {
            val days = maxOf(1L, input.durationSecs.toLong() / DAY_SECS)
            endDate = start.toLocalDate().plusDays(days - 1)
            end = LocalDateTime.of(endDate, start.toLocalTime())
        } else {
            end = addExact(start, zone, input.durationSecs.toLong())
            endDate = end.toLocalDate()
        }
        return EventForm(
            loadedFloating = !input.allDay && input.timeZone == null,
            loaded = LoadedTiming(
                start.toLocalDate(), start.toLocalTime(), endDate, end.toLocalTime(),
                input.allDay, input.isTask, input.timeZone, input.durationSecs,
            ),
            calendarId = input.calendarId,
            title = input.title,
            location = input.location,
            description = input.description,
            isTask = input.isTask,
            allDay = input.allDay,
            startDate = start.toLocalDate(),
            startTime = start.toLocalTime(),
            endDate = endDate,
            endTime = end.toLocalTime(),
            zone = input.timeZone,
            repeat = if (hasComplexRepeat) null else input.repeat?.let { repeatFrom(it, start.toLocalDate()) },
            repeatLocked = hasComplexRepeat,
            alerts = input.alertMinutes.map { it.toInt() },
        )
    }

    private fun repeatFrom(rule: RepeatRule, startDate: LocalDate): RepeatForm {
        val days = rule.byDay.mapNotNull { dayOfCode(it) }.toSet()
        val until = rule.until?.let { runCatching { LocalDate.parse(it.take(10)) }.getOrNull() }
        val kind = when {
            rule.count != null -> EndKind.AFTER_COUNT
            until != null -> EndKind.ON_DATE
            else -> EndKind.NEVER
        }
        return RepeatForm(
            frequency = rule.frequency,
            interval = maxOf(1, rule.interval.toInt()),
            days = if (rule.frequency == Frequency.WEEKLY && days.isEmpty()) setOf(startDate.dayOfWeek) else days,
            endKind = kind,
            until = until,
            countText = rule.count?.toString() ?: "5",
        )
    }

    /** What is wrong with [form], in the order the screen shows it; empty when it can be saved. */
    fun problems(form: EventForm, hasCalendar: Boolean = true): List<Problem> = buildList {
        if (!hasCalendar || form.calendarId.isBlank()) add(Problem.NO_CALENDAR)
        if (form.title.isBlank()) add(Problem.TITLE_BLANK)
        if (!form.isTask && (if (form.allDay) form.endDate.isBefore(form.startDate) else form.end.isBefore(form.start))) {
            add(Problem.END_BEFORE_START)
        }
        val rule = form.repeat
        if (rule != null && !form.repeatLocked) {
            if (rule.frequency == Frequency.WEEKLY && rule.days.isEmpty()) add(Problem.NO_WEEKDAY)
            when (rule.endKind) {
                EndKind.ON_DATE -> when {
                    rule.until == null -> add(Problem.UNTIL_MISSING)
                    rule.until.isBefore(form.startDate) -> add(Problem.UNTIL_BEFORE_START)
                }
                EndKind.AFTER_COUNT -> if (rule.count == null) add(Problem.COUNT_INVALID)
                EndKind.NEVER -> Unit
            }
        }
    }

    fun toRule(rule: RepeatForm): RepeatRule = RepeatRule(
        frequency = rule.frequency,
        interval = rule.interval.coerceAtLeast(1).toUInt(),
        byDay = if (rule.frequency == Frequency.WEEKLY) rule.days.sorted().map(::codeOf) else emptyList(),
        until = if (rule.endKind == EndKind.ON_DATE) rule.until?.toString() else null,
        count = if (rule.endKind == EndKind.AFTER_COUNT) rule.count?.toUInt() else null,
    )

    /**
     * The core's input for [form]. An all-day entry starts at midnight and lasts whole days; a task has
     * no length; a timed entry lasts the real time between its two wall-clock times in its zone, so a
     * night that crosses a change of the clocks is an hour longer or shorter than it looks. A repeat
     * rule the form cannot edit is not sent: the core leaves it as it is.
     */
    fun toInput(form: EventForm): EventInput {
        val zone = zoneOrNull(form.zone)
        val start = if (form.allDay) form.startDate.atStartOfDay() else form.start
        // The hour that happens twice cannot be told from the wall clock: what was loaded stays while the timing is untouched.
        val unchanged = form.loaded?.takeIf {
            it.startDate == form.startDate && it.startTime == form.startTime && it.endDate == form.endDate &&
                it.endTime == form.endTime && it.allDay == form.allDay && it.isTask == form.isTask && it.zone == form.zone
        }
        val duration = when {
            unchanged != null -> unchanged.durationSecs.toLong()
            form.isTask -> 0L
            form.allDay -> (ChronoUnit.DAYS.between(form.startDate, maxOf(form.endDate, form.startDate)) + 1) * DAY_SECS
            else -> betweenExact(form.start, form.end, zone)
        }
        return EventInput(
            calendarId = form.calendarId,
            title = form.title.trim(),
            location = form.location.trim(),
            description = form.description.trim(),
            start = DateTimeFormatter.ISO_LOCAL_DATE_TIME.format(start.withNano(0)),
            timeZone = form.zone,
            durationSecs = duration.toULong(),
            allDay = form.allDay,
            isTask = form.isTask,
            repeat = if (form.repeatLocked) null else form.repeat?.let(::toRule),
            alertMinutes = form.alerts.map { it.toUInt() },
        )
    }

    /** The zone to name under the times: the entry's own when it is set and is not the phone's, else null. */
    fun zoneNote(zone: String?, device: ZoneId): String? =
        zone?.takeIf { it.isNotBlank() && zoneOrNull(it) != device }

    /** The reminders the menu offers: the usual ones, and the entry's own when it is something else. */
    val standardAlerts = listOf(0, 5, 15, 30, 60, 1440)

    fun alertOptions(current: List<Int>): List<Int> = (standardAlerts + current.singleOrNull().let(::listOfNotNull)).distinct().sorted()
}

/** The calendars the form's menu offers, and how the pickers talk about dates. */
object FormChoices {
    /**
     * The writable calendars, the default one first. The calendar [currentId] an entry is in stays
     * in the menu even when it is not writable, so that the form never shows a calendar it cannot name.
     */
    fun calendars(all: List<uniffi.kweb_core.Calendar>, currentId: String?): List<uniffi.kweb_core.Calendar> =
        all.filter { it.mayWrite || it.id == currentId }.sortedByDescending { it.isDefault }

    /** The calendar a new entry goes into: the default one when it can be written to, else the first. */
    fun defaultCalendar(menu: List<uniffi.kweb_core.Calendar>): uniffi.kweb_core.Calendar? =
        menu.firstOrNull { it.isDefault && it.mayWrite } ?: menu.firstOrNull { it.mayWrite }

    /** A date picker speaks in milliseconds at midnight UTC. */
    fun toPickerMillis(date: LocalDate): Long = date.atStartOfDay(java.time.ZoneOffset.UTC).toInstant().toEpochMilli()

    fun fromPickerMillis(millis: Long): LocalDate = java.time.Instant.ofEpochMilli(millis).atZone(java.time.ZoneOffset.UTC).toLocalDate()
}
