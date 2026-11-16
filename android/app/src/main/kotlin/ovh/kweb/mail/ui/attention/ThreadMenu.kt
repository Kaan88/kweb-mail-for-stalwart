package ovh.kweb.mail.ui.attention

import androidx.annotation.StringRes
import java.time.LocalDateTime
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle
import ovh.kweb.mail.R
import uniffi.kweb_core.Finding
import uniffi.kweb_core.PhoneAction
import uniffi.kweb_core.Prefill
import uniffi.kweb_core.ServiceAction

/** An entry of the menu behind the sparkle in a thread's top bar. */
sealed interface ThreadMenuEntry {
    val finding: Finding

    /** "Add Thu 15:00 to calendar…": opens the editor with [prefill]; the user saves it himself. */
    data class Calendar(override val finding: Finding, val prefill: Prefill) : ThreadMenuEntry

    /** "Add task…" with [prefill]. */
    data class Task(override val finding: Finding, val prefill: Prefill) : ThreadMenuEntry

    /** A button with a sparkle: sends this mail to the service. Phase 2 wires it; it is only here when an assistant is set up. */
    data class Service(override val finding: Finding, val action: ServiceAction) : ThreadMenuEntry
}

/** The editor's form of what a finding found. */
fun Prefill.toEventPrefill() = ovh.kweb.mail.ui.calendar.EventPrefill(title, start, allDay, isTask, durationMinutes.toInt())

object ThreadMenu {
    /**
     * The menu of one mail: every phone action of its findings that does something (not "Open", the mail is open), then the
     * service actions once each, most useful kinds first. Nothing for a finding with neither.
     */
    fun entries(findings: List<Finding>): List<ThreadMenuEntry> {
        val phone = findings.flatMap { f ->
            f.phone.mapNotNull { a ->
                when (a) {
                    is PhoneAction.AddToCalendar -> ThreadMenuEntry.Calendar(f, a.prefill)
                    is PhoneAction.AddTask -> ThreadMenuEntry.Task(f, a.prefill)
                    else -> null
                }
            }
        }
        val service = findings.mapNotNull { f -> f.service?.let { ThreadMenuEntry.Service(f, it) } }
            .distinctBy { it.action }
            .sortedBy { it.action.ordinal }
        return phone + service
    }

    @StringRes
    fun serviceLabel(a: ServiceAction): Int = when (a) {
        ServiceAction.ADD_DATES -> R.string.att_menu_find_dates
        ServiceAction.REPLY_IDEAS -> R.string.att_svc_reply_ideas
        ServiceAction.LOOK_IN_MAIL -> R.string.att_svc_look_in_mail
        ServiceAction.PLAN_MY_DAY -> R.string.att_svc_plan_my_day
        ServiceAction.CHECK_THE_MAIL -> R.string.att_svc_check_the_mail
    }

    /** The words of an entry. A date with no time of day ("Fri") is named by its day alone. */
    fun label(entry: ThreadMenuEntry, clock: WordClock, t: Texts): String = when (entry) {
        is ThreadMenuEntry.Calendar -> t.string(R.string.att_menu_add_calendar_at, whenText(entry.prefill, clock))
        is ThreadMenuEntry.Task -> t.string(R.string.att_menu_add_task)
        is ThreadMenuEntry.Service -> t.string(serviceLabel(entry.action))
    }

    /** "Thu 15:00", or "Fri" for a day without a time, or "Thu 19 Nov" outside the next week. */
    fun whenText(p: Prefill, clock: WordClock): String {
        val at = runCatching { LocalDateTime.parse(p.start.trim()) }.getOrNull() ?: return ""
        val today = java.time.Instant.ofEpochSecond(clock.nowSeconds).atZone(clock.zone).toLocalDate()
        val days = java.time.temporal.ChronoUnit.DAYS.between(today, at.toLocalDate())
        val day = DateTimeFormatter.ofPattern(if (days in -1..6) "EEE" else "EEE d MMM", clock.locale).format(at)
        if (p.allDay || at.toLocalTime() == java.time.LocalTime.MIDNIGHT) return day
        return day + " " + DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT).withLocale(clock.locale).format(at)
    }
}
