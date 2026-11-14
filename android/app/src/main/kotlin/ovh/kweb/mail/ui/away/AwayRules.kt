package ovh.kweb.mail.ui.away

import java.time.Instant
import java.time.LocalDate
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.util.Locale
import uniffi.kweb_core.AwayMessage

/** What the away message screen holds. The `loaded` seconds are what the server had, to send an untouched date as it was. */
data class AwayForm(
    val enabled: Boolean = false,
    val from: LocalDate? = null,
    val until: LocalDate? = null,
    val subject: String = "",
    val text: String = "",
    val loadedFrom: Long? = null,
    val loadedUntil: Long? = null,
)

/** The state of the away message as the Settings row says it. */
sealed interface AwayLine {
    data object Off : AwayLine

    data object On : AwayLine

    /** Switched on, but its end has passed: the server no longer answers. */
    data class Ended(val until: LocalDate) : AwayLine

    data class OnUntil(val until: LocalDate) : AwayLine

    data class OnFrom(val from: LocalDate) : AwayLine

    data class OnFromUntil(val from: LocalDate, val until: LocalDate) : AwayLine
}

/** The wording and the date arithmetic of the away message, as plain functions. */
object AwayRules {
    /** A day as the start of a period: its first moment in [zone], Unix seconds. */
    fun startOfDay(date: LocalDate, zone: ZoneId): Long = date.atStartOfDay(zone).toEpochSecond()

    /** A day as the end of a period: its last second in [zone] (a day with a clock change is as long as it is). */
    fun endOfDay(date: LocalDate, zone: ZoneId): Long = date.plusDays(1).atStartOfDay(zone).toEpochSecond() - 1

    /** The day a start time falls on. */
    fun dayOfStart(seconds: Long, zone: ZoneId): LocalDate = Instant.ofEpochSecond(seconds).atZone(zone).toLocalDate()

    /**
     * The day an end time is shown as: the day of its last second, so a time that is exactly the start of
     * a day (as other mail programs write "until the 21st") belongs to the day before.
     */
    fun dayOfEnd(seconds: Long, zone: ZoneId): LocalDate = Instant.ofEpochSecond(seconds - 1).atZone(zone).toLocalDate()

    fun fromMessage(message: AwayMessage, zone: ZoneId): AwayForm = AwayForm(
        enabled = message.isEnabled,
        from = message.from?.let { dayOfStart(it, zone) },
        until = message.until?.let { dayOfEnd(it, zone) },
        subject = message.subject,
        text = message.text,
        loadedFrom = message.from,
        loadedUntil = message.until,
    )

    /**
     * What is sent. A date the user did not change is sent as the server had it (it may have a time of day
     * this screen has no way to show); a changed one means its whole day.
     */
    fun toMessage(form: AwayForm, zone: ZoneId): AwayMessage {
        val from = form.from?.let { day ->
            form.loadedFrom?.takeIf { dayOfStart(it, zone) == day } ?: startOfDay(day, zone)
        }
        val until = form.until?.let { day ->
            form.loadedUntil?.takeIf { dayOfEnd(it, zone) == day } ?: endOfDay(day, zone)
        }
        return AwayMessage(
            isEnabled = form.enabled,
            from = from,
            until = until,
            subject = form.subject.trim(),
            text = form.text.trim(),
        )
    }

    /** Whether the user changed anything since the form was opened. */
    fun isChanged(form: AwayForm, initial: AwayForm, zone: ZoneId): Boolean = toMessage(form, zone) != toMessage(initial, zone)

    /**
     * The state line of the Settings row for [message] at [nowSeconds]. A start that has passed is not
     * worth saying; an end that has passed is: the message is on but no longer answers. The raw seconds
     * decide, so a message that ends later today still reads as on.
     */
    fun line(message: AwayMessage, nowSeconds: Long, zone: ZoneId): AwayLine {
        if (!message.isEnabled) return AwayLine.Off
        val end = message.until
        if (end != null && end <= nowSeconds) return AwayLine.Ended(dayOfEnd(end, zone))
        val from = message.from?.takeIf { it > nowSeconds }?.let { dayOfStart(it, zone) }
        val until = message.until?.let { dayOfEnd(it, zone) }
        return when {
            from != null && until != null -> AwayLine.OnFromUntil(from, until)
            from != null -> AwayLine.OnFrom(from)
            until != null -> AwayLine.OnUntil(until)
            else -> AwayLine.On
        }
    }

    /** A day as the state line writes it: "21 Nov". */
    fun shortDate(date: LocalDate, locale: Locale): String = DateTimeFormatter.ofPattern("d MMM", locale).format(date)
}
