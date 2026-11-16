package ovh.kweb.mail.ui.attention

import android.content.res.Resources
import androidx.annotation.PluralsRes
import androidx.annotation.StringRes
import java.time.Instant
import java.time.LocalDate
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle
import java.time.temporal.ChronoUnit
import java.util.Locale
import ovh.kweb.mail.R
import uniffi.kweb_core.AttentionRule
import uniffi.kweb_core.Finding
import uniffi.kweb_core.PhoneAction
import uniffi.kweb_core.ServiceAction

/** Where the words come from: string resources in the app, a fake in the tests. */
interface Texts {
    fun string(@StringRes res: Int, vararg args: Any): String

    /** A plural: [quantity] picks the form and is the first argument, [args] follow. */
    fun plural(@PluralsRes res: Int, quantity: Int, vararg args: Any): String
}

class ResourceTexts(private val resources: Resources) : Texts {
    override fun string(res: Int, vararg args: Any): String = resources.getString(res, *args)
    override fun plural(res: Int, quantity: Int, vararg args: Any): String = resources.getQuantityString(res, quantity, quantity, *args)
}

/** The phone's clock and place, for the words a finding says about times. */
data class WordClock(val zone: ZoneId, val locale: Locale, val nowSeconds: Long)

/**
 * The sentences of the findings. The core says which rule fired and with which names, times and count; the wording
 * is the app's, from string resources, so it follows the user's language.
 */
object AttentionWords {
    private fun local(ts: Long, c: WordClock) = Instant.ofEpochSecond(ts).atZone(c.zone)

    /** "14:00" today, "Thu 14:00" within a week, "19 Nov 14:00" otherwise. */
    fun time(ts: Long, c: WordClock): String {
        val at = local(ts, c)
        val days = ChronoUnit.DAYS.between(local(c.nowSeconds, c).toLocalDate(), at.toLocalDate())
        // A date found with no time ("Fri") is midnight: say the day only.
        val clock = if (at.toLocalTime() == java.time.LocalTime.MIDNIGHT && days != 0L) "" else DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT).withLocale(c.locale).format(at)
        return when {
            clock.isEmpty() && days in 1..6 -> DateTimeFormatter.ofPattern("EEE", c.locale).format(at)
            clock.isEmpty() -> DateTimeFormatter.ofPattern("d MMM", c.locale).format(at)
            days == 0L -> clock
            days in 1..6 -> DateTimeFormatter.ofPattern("EEE", c.locale).format(at) + " " + clock
            else -> DateTimeFormatter.ofPattern("d MMM", c.locale).format(at) + " " + clock
        }
    }

    /** "Thu 19 Nov". */
    fun day(date: LocalDate, c: WordClock): String = DateTimeFormatter.ofPattern("EEE d MMM", c.locale).format(date)

    /** "Today", "Tomorrow" or "Thu 19 Nov": the word for the day that holds [ts]. */
    fun dayWord(ts: Long, c: WordClock, t: Texts): String {
        val date = local(ts, c).toLocalDate()
        val today = local(c.nowSeconds, c).toLocalDate()
        return when (date) {
            today -> t.string(R.string.att_today)
            today.plusDays(1) -> t.string(R.string.att_tomorrow)
            else -> day(date, c)
        }
    }

    /** How long [seconds] is, rounded up to the minute: "42 min", "3 h", "1 h 20". */
    fun span(seconds: Long, t: Texts): String {
        val minutes = (seconds.coerceAtLeast(0) + 59) / 60
        val (h, m) = (minutes / 60).toInt() to (minutes % 60).toInt()
        return when {
            h == 0 -> t.string(R.string.home_unit_minutes, m)
            m == 0 -> t.string(R.string.home_unit_hours, h)
            else -> t.string(R.string.home_unit_hours_minutes, h, m)
        }
    }

    private fun name(f: Finding, i: Int) = f.names.getOrNull(i).orEmpty()

    private fun at(f: Finding, i: Int, c: WordClock): String = f.times.getOrNull(i)?.let { time(it, c) }.orEmpty()

    /** What the finding says in one line. */
    fun title(f: Finding, c: WordClock, t: Texts): String = when (f.rule) {
        AttentionRule.OVERLAP -> t.string(R.string.att_overlap, f.times.maxOrNull()?.let { time(it, c) }.orEmpty(), name(f, 0), name(f, 1))
        AttentionRule.NO_PLACE -> t.string(R.string.att_no_place, name(f, 0), at(f, 0, c))
        AttentionRule.OPEN_INVITATION -> t.string(R.string.att_invitation, name(f, 0), at(f, 0, c))
        AttentionRule.BUSY_DAY -> t.plural(R.plurals.att_busy, f.count.toInt(), dayWord(f.times.firstOrNull() ?: c.nowSeconds, c, t))
        AttentionRule.TIGHT_CHANGE -> t.string(R.string.att_tight, name(f, 0), name(f, 1))
        AttentionRule.OVERDUE -> t.plural(R.plurals.att_overdue, f.count.toInt())
        AttentionRule.NO_TIME_TODAY -> t.plural(R.plurals.att_no_time, f.count.toInt())
        AttentionRule.TOO_MANY -> t.plural(R.plurals.att_too_many, f.count.toInt())
        AttentionRule.MAIL_MAY_FINISH -> t.string(R.string.att_may_finish, name(f, 0))
        AttentionRule.DATE_IN_MAIL -> t.string(R.string.att_date_mail, name(f, 0))
        AttentionRule.DEADLINE_WORDS -> t.string(R.string.att_deadline, name(f, 0))
        AttentionRule.INVITATION_FILE -> t.string(R.string.att_invitation_file, name(f, 0))
        AttentionRule.WAITING_FOR_YOU -> t.string(R.string.att_waiting, name(f, 0))
    }

    /** The second line, or null when there is none. */
    fun meta(f: Finding, c: WordClock, t: Texts): String? = when (f.rule) {
        AttentionRule.OVERLAP -> null
        AttentionRule.NO_PLACE -> {
            val rest = span((f.times.firstOrNull() ?: c.nowSeconds) - c.nowSeconds, t)
            if (f.count > 0u) t.plural(R.plurals.att_no_place_mails, f.count.toInt(), rest) else t.string(R.string.att_no_place_in, rest)
        }
        AttentionRule.OPEN_INVITATION ->
            name(f, 1).takeIf { it.isNotBlank() }?.let { t.string(R.string.att_invitation_overlaps, it) } ?: t.string(R.string.att_invitation_free)
        AttentionRule.BUSY_DAY -> if (f.slots.isEmpty()) t.string(R.string.att_busy_no_free) else null
        AttentionRule.TIGHT_CHANGE -> t.string(R.string.att_tight_places, name(f, 2), name(f, 3))
        AttentionRule.OVERDUE, AttentionRule.NO_TIME_TODAY, AttentionRule.TOO_MANY -> {
            val joined = f.names.joinToString(", ")
            if (joined.isEmpty()) null else if (f.count.toInt() > f.names.size) t.string(R.string.att_more_names, joined) else joined
        }
        AttentionRule.MAIL_MAY_FINISH, AttentionRule.DATE_IN_MAIL, AttentionRule.DEADLINE_WORDS, AttentionRule.INVITATION_FILE ->
            t.string(R.string.att_quoted_meta, name(f, 1), at(f, 0, c))
        AttentionRule.WAITING_FOR_YOU -> {
            val days = ((c.nowSeconds - (f.times.firstOrNull() ?: c.nowSeconds)) / 86_400).toInt().coerceAtLeast(1)
            t.plural(R.plurals.att_waiting_meta, days, name(f, 1))
        }
    }

    @StringRes
    fun serviceLabel(a: ServiceAction): Int = when (a) {
        ServiceAction.LOOK_IN_MAIL -> R.string.att_svc_look_in_mail
        ServiceAction.PLAN_MY_DAY -> R.string.att_svc_plan_my_day
        ServiceAction.CHECK_THE_MAIL -> R.string.att_svc_check_the_mail
        ServiceAction.ADD_DATES -> R.string.att_svc_add_dates
        ServiceAction.REPLY_IDEAS -> R.string.att_svc_reply_ideas
    }

    /** The label of a phone button; [f] names the event for "Move ...". */
    fun phoneLabel(a: PhoneAction, f: Finding, t: Texts): String = when (a) {
        is PhoneAction.MoveEvent -> t.string(R.string.att_move, f.names.lastOrNull().orEmpty())
        is PhoneAction.FindSlot -> t.string(R.string.att_find_slot)
        is PhoneAction.OpenMails -> t.string(R.string.att_open_mails)
        is PhoneAction.Respond -> t.string(R.string.att_answer)
        is PhoneAction.FreeTime -> t.string(R.string.att_free_time)
        is PhoneAction.Directions -> t.string(R.string.att_directions)
        is PhoneAction.MoveToday -> t.string(R.string.att_move_today)
        is PhoneAction.OpenMail -> t.string(R.string.att_open)
        is PhoneAction.MarkDone -> t.string(R.string.att_mark_done)
        is PhoneAction.AddToCalendar -> t.string(R.string.att_add_calendar)
        is PhoneAction.AddTask -> t.string(R.string.att_add_task)
    }

    /** The length of a slot: "30 min", "1 h", "1 h 30 min". */
    fun length(seconds: Long, t: Texts): String {
        val minutes = (seconds / 60).toInt().coerceAtLeast(1)
        val (h, m) = minutes / 60 to minutes % 60
        return when {
            h == 0 -> t.string(R.string.att_duration_min, m)
            m == 0 -> t.string(R.string.att_duration_h, h)
            else -> t.string(R.string.att_duration_hm, h, m)
        }
    }

    /** The chip of a slot start: the clock time, with the weekday when it is not on the day of [firstStart]. */
    fun slotChip(start: Long, c: WordClock, firstStart: Long): String {
        val at = local(start, c)
        val clock = DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT).withLocale(c.locale).format(at)
        return if (at.toLocalDate() == local(firstStart, c).toLocalDate()) clock else DateTimeFormatter.ofPattern("EEE", c.locale).format(at) + " " + clock
    }

    /** "13:00 to 16:00": a gap of the day, for the Free time list. */
    fun range(start: Long, end: Long, c: WordClock, t: Texts): String {
        val f = DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT).withLocale(c.locale)
        return t.string(R.string.att_range, f.format(local(start, c)), f.format(local(end, c)))
    }
}
