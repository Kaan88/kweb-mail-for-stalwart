package ovh.kweb.mail.ui.calendar

import android.content.res.Resources
import ovh.kweb.mail.R
import uniffi.kweb_core.Frequency

/**
 * The fragments of text the calendar rules put together, so that the rules stay plain Kotlin that
 * a unit test can run: [ResourceWords] reads them from the string resources, the tests bring their
 * own. Dates and times are not here; the rules format those with java.time.
 */
interface Words {
    val allDay: String

    /** An all-day entry that lasts [days] days (2 or more). */
    fun allDayDays(days: Int): String

    val task: String

    /** A task with a due time; [time] is already formatted. */
    fun taskDue(time: String): String

    val invitation: String

    /** [text] (a time) with the zone it is in, when that is not the phone's. */
    fun withZone(text: String, zone: String): String

    /** "Every day", "Every 3 weeks". */
    fun every(frequency: Frequency, interval: Int): String

    /** [base] on the weekdays [days] ("Every week on Mon, Wed"). */
    fun on(base: String, days: String): String

    fun until(base: String, date: String): String

    fun times(base: String, count: Int): String

    /** A repeat rule the app cannot describe. */
    val repeats: String

    val atStart: String

    fun minutesBefore(minutes: Int): String

    fun hoursBefore(hours: Int): String

    fun daysBefore(days: Int): String

    /** The answer to an invitation: "needs-action", "accepted", "tentative", "declined". */
    fun status(status: String): String
}

class ResourceWords(private val resources: Resources) : Words {
    override val allDay get() = resources.getString(R.string.cal_all_day)

    override fun allDayDays(days: Int) = resources.getQuantityString(R.plurals.cal_all_day_days, days, days)

    override val task get() = resources.getString(R.string.cal_task)

    override fun taskDue(time: String) = resources.getString(R.string.cal_task_due, time)

    override val invitation get() = resources.getString(R.string.cal_invitation)

    override fun withZone(text: String, zone: String) = resources.getString(R.string.cal_with_zone, text, zone)

    override fun every(frequency: Frequency, interval: Int): String {
        val plural = when (frequency) {
            Frequency.DAILY -> R.plurals.cal_every_days
            Frequency.WEEKLY -> R.plurals.cal_every_weeks
            Frequency.MONTHLY -> R.plurals.cal_every_months
            Frequency.YEARLY -> R.plurals.cal_every_years
        }
        return resources.getQuantityString(plural, interval, interval)
    }

    override fun on(base: String, days: String) = resources.getString(R.string.cal_repeat_on, base, days)

    override fun until(base: String, date: String) = resources.getString(R.string.cal_repeat_until, base, date)

    override fun times(base: String, count: Int) = resources.getQuantityString(R.plurals.cal_repeat_times, count, base, count)

    override val repeats get() = resources.getString(R.string.cal_repeats)

    override val atStart get() = resources.getString(R.string.cal_reminder_at_start)

    override fun minutesBefore(minutes: Int) = resources.getQuantityString(R.plurals.cal_reminder_minutes, minutes, minutes)

    override fun hoursBefore(hours: Int) = resources.getQuantityString(R.plurals.cal_reminder_hours, hours, hours)

    override fun daysBefore(days: Int) = resources.getQuantityString(R.plurals.cal_reminder_days, days, days)

    override fun status(status: String) = resources.getString(
        when (status) {
            "accepted" -> R.string.cal_status_accepted
            "tentative" -> R.string.cal_status_tentative
            "declined" -> R.string.cal_status_declined
            else -> R.string.cal_status_none
        },
    )
}
