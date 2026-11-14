package ovh.kweb.mail.ui.list

import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle
import java.time.temporal.ChronoUnit
import java.util.Locale
import uniffi.kweb_core.Address

/** The time at the right of a message row. */
object RowTime {
    /**
     * Time of day for today, [yesterday] for yesterday, the weekday for the days before that up to
     * a week, and the date after that. A time in the future (a clock that is off) counts as today.
     */
    fun format(receivedAtSeconds: Long, nowMillis: Long, zone: ZoneId, locale: Locale, yesterday: String): String {
        val received = Instant.ofEpochSecond(receivedAtSeconds).atZone(zone)
        val today = Instant.ofEpochMilli(nowMillis).atZone(zone).toLocalDate()
        return when (ChronoUnit.DAYS.between(received.toLocalDate(), today)) {
            in Long.MIN_VALUE..0L -> DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT).withLocale(locale).format(received)
            1L -> yesterday
            in 2L..6L -> DateTimeFormatter.ofPattern("EEE", locale).format(received)
            else -> DateTimeFormatter.ofLocalizedDate(FormatStyle.MEDIUM).withLocale(locale).format(received)
        }
    }
}

/** The preview line of a row. */
object Preview {
    private val blanks = Regex("\\s+")

    /** The server's preview on one line: line breaks and runs of blanks become one space. */
    fun oneLine(preview: String): String = preview.replace(blanks, " ").trim()
}

/** The sender as a row shows it. */
object Sender {
    /** The display name, or the address when the server gave no name, or null when neither exists. */
    fun label(name: String, email: String): String? = name.trim().ifEmpty { email.trim() }.ifEmpty { null }

    /** First letter or digit of the sender, upper case, for the round avatar; "?" when there is none. */
    fun initial(name: String, email: String): String {
        val text = label(name, email) ?: return "?"
        var i = 0
        while (i < text.length) {
            val cp = text.codePointAt(i)
            if (Character.isLetterOrDigit(cp)) return String(Character.toChars(cp)).uppercase()
            i += Character.charCount(cp)
        }
        return "?"
    }
}

/** The line above a conversation's subject. The words around the names ("To: ") are the screen's. */
object SenderLine {
    /**
     * One sender: the whole name. Several: their first names joined by ", ". Null when there are
     * none. A sender that is only an address stays whole, since it has no first name.
     */
    fun senders(senders: List<String>): String? {
        val names = senders.map { it.trim() }.filter { it.isNotEmpty() }
        return when (names.size) {
            0 -> null
            1 -> names[0]
            else -> names.joinToString(", ") { it.substringBefore(' ') }
        }
    }

    /** The recipients of a sent or draft message by name, or by address when they have none. */
    fun recipients(to: List<Address>): String? =
        to.mapNotNull { Sender.label(it.name, it.email) }.joinToString(", ").ifEmpty { null }

    /** " · n" after the names when the conversation has more than one message. */
    fun withCount(text: String, messageCount: Int): String = if (messageCount > 1) "$text · $messageCount" else text
}
