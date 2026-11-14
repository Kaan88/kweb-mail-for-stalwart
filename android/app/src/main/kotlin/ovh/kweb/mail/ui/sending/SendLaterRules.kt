package ovh.kweb.mail.ui.sending

import java.time.Instant
import java.time.LocalDate
import java.time.LocalTime
import java.time.ZoneId
import java.time.ZonedDateTime

/** The rules of "Send later" that need no Android. Times are Unix seconds; the day is the one in the device's zone. */
object SendLaterRules {
    /** The least the core takes: a minute from now. */
    const val MIN_SECONDS = 60L

    /** The most the server takes: seven days. */
    const val MAX_DAYS = 7L
    const val MAX_SECONDS = MAX_DAYS * 24 * 3600

    const val EVENING_HOUR = 18
    const val MORNING_HOUR = 8

    /** A quick choice that is left out when it would be about the same as the one before it. */
    private const val APART_SECONDS = 30 * 60L

    enum class Kind { InOneHour, ThisEvening, TomorrowMorning }

    data class Choice(val kind: Kind, val at: Long)

    enum class Check { Ok, TooSoon, TooFar }

    enum class Day { Today, Tomorrow, Other }

    /** A time named relative to now: which day it is and the clock time; [date] is for [Day.Other]. */
    data class When(val day: Day, val date: LocalDate, val time: LocalTime)

    /**
     * The quick choices at [now]: in one hour, this evening at 18:00 and tomorrow at 08:00. The evening is
     * left out when it has passed or is less than half an hour from the hour choice; tomorrow morning is
     * left out when it is beyond the limit (it never is) or too near.
     */
    fun choices(now: Instant, zone: ZoneId): List<Choice> {
        val nowSeconds = now.epochSecond
        val oneHour = Choice(Kind.InOneHour, nowSeconds + 3600)
        val today = now.atZone(zone).toLocalDate()
        val evening = Choice(Kind.ThisEvening, at(today, EVENING_HOUR, 0, zone))
        val morning = Choice(Kind.TomorrowMorning, at(today.plusDays(1), MORNING_HOUR, 0, zone))
        val out = mutableListOf(oneHour)
        if (evening.at >= oneHour.at + APART_SECONDS) out += evening
        val last = out.last().at
        if (morning.at >= last + APART_SECONDS && check(morning.at, nowSeconds) == Check.Ok) out += morning
        return out
    }

    /** Whether [at] can be asked of the core at [now]. */
    fun check(at: Long, now: Long): Check = when {
        at < now + MIN_SECONDS -> Check.TooSoon
        at > now + MAX_SECONDS -> Check.TooFar
        else -> Check.Ok
    }

    /** The days the date picker lets through: today and the seven after it. */
    fun dateAllowed(date: LocalDate, today: LocalDate): Boolean = !date.isBefore(today) && !date.isAfter(today.plusDays(MAX_DAYS))

    /** The moment of [date] at [hour]:[minute] in [zone] (a time that a clock change skips moves on). */
    fun at(date: LocalDate, hour: Int, minute: Int, zone: ZoneId): Long =
        ZonedDateTime.of(date, LocalTime.of(hour, minute), zone).toEpochSecond()

    /** Where the pickers start: an hour from now, to the minute. */
    fun defaultPick(now: Instant, zone: ZoneId): Pair<LocalDate, LocalTime> {
        val z = now.plusSeconds(3600).atZone(zone)
        return z.toLocalDate() to LocalTime.of(z.hour, z.minute)
    }

    /** [at] as a day and a clock time, for words. */
    fun describe(at: Long, now: Long, zone: ZoneId): When {
        val z = Instant.ofEpochSecond(at).atZone(zone)
        val today = Instant.ofEpochSecond(now).atZone(zone).toLocalDate()
        val date = z.toLocalDate()
        val day = when (date) {
            today -> Day.Today
            today.plusDays(1) -> Day.Tomorrow
            else -> Day.Other
        }
        return When(day, date, LocalTime.of(z.hour, z.minute))
    }
}
