package ovh.kweb.mail.background

import java.time.DateTimeException
import java.time.Instant
import java.time.LocalDate
import java.time.LocalDateTime
import java.time.ZoneId
import java.time.ZonedDateTime
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle
import java.util.Locale

/** Notification ids and time text of reminders, free of Android so they can be tested on the JVM. */
internal object ReminderRules {
    /** Reminder ids live in 100..899: below 1000 (the core keeps mail ids above) and clear of the app's own ids. */
    private const val FIRST_ID = 100
    private const val SPAN = 800

    /**
     * The id of a reminder's notification: the same alert always gets the same id, so a snoozed
     * reminder replaces the one it came from, and different alerts rarely collide.
     */
    fun notificationId(eventId: String, alertId: String): Int {
        var hash = 0x811c9dc5.toInt()
        for (byte in "$eventId\u0000$alertId".toByteArray()) {
            hash = (hash xor (byte.toInt() and 0xff)) * 0x01000193
        }
        return FIRST_ID + Integer.remainderUnsigned(hash, SPAN)
    }

    /**
     * When the reminder is, in the phone's [zone] and [locale]: the time alone for today, the
     * date and time for another day, the date alone for an all-day entry. A floating time (no
     * [timeZone]) is shown as given. Null when there is nothing usable in [localStart].
     */
    fun formatTime(
        localStart: String?,
        timeZone: String?,
        allDay: Boolean,
        zone: ZoneId,
        locale: Locale,
        now: Instant,
    ): String? {
        if (localStart.isNullOrBlank()) return null
        return try {
            if (allDay || !localStart.contains('T')) {
                val date = LocalDate.parse(localStart.substringBefore('T'))
                return DateTimeFormatter.ofLocalizedDate(FormatStyle.MEDIUM).withLocale(locale).format(date)
            }
            val local = toPhoneTime(LocalDateTime.parse(localStart), timeZone, zone)
            val style = if (local.toLocalDate() == now.atZone(zone).toLocalDate()) {
                DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT)
            } else {
                DateTimeFormatter.ofLocalizedDateTime(FormatStyle.MEDIUM, FormatStyle.SHORT)
            }
            style.withLocale(locale).format(local)
        } catch (_: DateTimeException) {
            null
        }
    }

    private fun toPhoneTime(local: LocalDateTime, timeZone: String?, phone: ZoneId): LocalDateTime {
        val source = timeZone?.let { runCatching { ZoneId.of(it) }.getOrNull() } ?: return local
        return ZonedDateTime.of(local, source).withZoneSameInstant(phone).toLocalDateTime()
    }
}
