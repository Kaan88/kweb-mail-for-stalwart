package ovh.kweb.mail.ui.calendar

import uniffi.kweb_core.Frequency

/** The English of the app's own texts, for tests of the rules that use [Words]. */
object TestWords : Words {
    override val allDay = "All day"
    override fun allDayDays(days: Int) = "All day · $days days"
    override val task = "Task"
    override fun taskDue(time: String) = "Task · due $time"
    override val invitation = "Invitation"
    override fun withZone(text: String, zone: String) = "$text ($zone)"
    override fun every(frequency: Frequency, interval: Int): String {
        val unit = when (frequency) {
            Frequency.DAILY -> "day"
            Frequency.WEEKLY -> "week"
            Frequency.MONTHLY -> "month"
            Frequency.YEARLY -> "year"
        }
        return if (interval == 1) "Every $unit" else "Every $interval ${unit}s"
    }
    override fun on(base: String, days: String) = "$base on $days"
    override fun until(base: String, date: String) = "$base, until $date"
    override fun times(base: String, count: Int) = if (count == 1) "$base, 1 time" else "$base, $count times"
    override val repeats = "Repeats"
    override val atStart = "At the start"
    override fun minutesBefore(minutes: Int) = "$minutes minutes before"
    override fun hoursBefore(hours: Int) = if (hours == 1) "1 hour before" else "$hours hours before"
    override fun daysBefore(days: Int) = if (days == 1) "1 day before" else "$days days before"
    override fun status(status: String) = when (status) {
        "accepted" -> "Accepted"
        "tentative" -> "Maybe"
        "declined" -> "Declined"
        else -> "No answer yet"
    }
}
