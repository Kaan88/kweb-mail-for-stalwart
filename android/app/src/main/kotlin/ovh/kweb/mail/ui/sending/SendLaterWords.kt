package ovh.kweb.mail.ui.sending

import android.content.res.Resources
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle
import java.util.Locale
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.receiveAsFlow
import ovh.kweb.mail.R

/**
 * Tells the app that a message was handed over to be sent later, so that the confirmation is shown whatever
 * screen is up (the compose screen may be gone when the core answers). A process-wide channel like the compose
 * screen's own outbox; one reader: the navigation view model.
 */
object ScheduledNotices {
    private val channel = Channel<Long>(Channel.BUFFERED)

    /** The Unix second each message goes out at. */
    val scheduled: Flow<Long> get() = channel.receiveAsFlow()

    fun announce(sendAt: Long) {
        channel.trySend(sendAt)
    }
}

/** The words for times: "Today, 18:00" in lists, "today at 18:00" inside a sentence. */
object SendLaterWords {
    /** The time of [at] as the user reads it. [sentence]: lower case, for the middle of a sentence. */
    fun whenText(resources: Resources, at: Long, now: Long, zone: ZoneId, locale: Locale, sentence: Boolean = false): String {
        val w = SendLaterRules.describe(at, now, zone)
        val time = DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT).withLocale(locale).format(w.time)
        return when (w.day) {
            SendLaterRules.Day.Today -> resources.getString(if (sentence) R.string.send_later_today_sentence else R.string.send_later_today, time)
            SendLaterRules.Day.Tomorrow -> resources.getString(if (sentence) R.string.send_later_tomorrow_sentence else R.string.send_later_tomorrow, time)
            SendLaterRules.Day.Other -> {
                val date = DateTimeFormatter.ofLocalizedDate(FormatStyle.MEDIUM).withLocale(locale).format(w.date)
                resources.getString(if (sentence) R.string.send_later_other_sentence else R.string.send_later_other, date, time)
            }
        }
    }

    /** "Scheduled for tomorrow at 08:00". */
    fun confirmation(resources: Resources, at: Long, now: Long, zone: ZoneId, locale: Locale): String =
        resources.getString(R.string.send_later_confirmation, whenText(resources, at, now, zone, locale, sentence = true))
}
