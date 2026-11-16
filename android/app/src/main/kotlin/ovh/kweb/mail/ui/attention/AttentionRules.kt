package ovh.kweb.mail.ui.attention

import androidx.annotation.StringRes
import java.time.LocalDate
import java.time.ZoneId
import ovh.kweb.mail.R
import uniffi.kweb_core.AttentionFamily
import uniffi.kweb_core.Finding
import uniffi.kweb_core.HomeTile
import uniffi.kweb_core.PhoneAction
import uniffi.kweb_core.ServiceAction
import uniffi.kweb_core.TimeSlot

/** Where the app lists findings. The core's one call returns all of them; the places pick from that. */
sealed interface Place {
    data object Home : Place
    data object Calendar : Place
    data object Tasks : Place
    data class Thread(val threadId: String) : Place

    /** The mail findings of every conversation: for the small mark beside a row. */
    data object MailRows : Place
}

/** The groups of the sheet, in the order of the tiles. */
enum class Group(@StringRes val title: Int, val tile: HomeTile) {
    MAIL(R.string.att_group_mail, HomeTile.UNREAD),
    CALENDAR(R.string.att_group_calendar, HomeTile.NEXT_EVENT),
    TASKS(R.string.att_group_tasks, HomeTile.DUE_TODAY),
}

/** The buttons a finding card shows: at most two, a phone action first and a service action after it. */
data class Buttons(val phone: List<PhoneAction>, val service: ServiceAction?)

object AttentionRules {
    /** The findings of [place] out of the list of all of them. */
    fun forPlace(all: List<Finding>, place: Place): List<Finding> = when (place) {
        Place.Home -> all
        Place.Calendar -> all.filter { it.family == AttentionFamily.CALENDAR }
        Place.Tasks -> all.filter { it.family == AttentionFamily.TASKS }
        is Place.Thread -> all.filter { it.family == AttentionFamily.MAIL && it.threadId == place.threadId }
        Place.MailRows -> all.filter { it.family == AttentionFamily.MAIL }
    }

    fun groupOf(f: Finding): Group = when (f.family) {
        AttentionFamily.MAIL -> Group.MAIL
        AttentionFamily.CALENDAR -> Group.CALENDAR
        AttentionFamily.TASKS -> Group.TASKS
    }

    /** The findings by group, in the order of the tiles; a group with none is left out. */
    fun groups(all: List<Finding>): List<Pair<Group, List<Finding>>> =
        Group.entries.mapNotNull { g -> all.filter { groupOf(it) == g }.takeIf { it.isNotEmpty() }?.let { g to it } }

    /** How many findings belong to a tile (the number on its sparkle). */
    fun count(all: List<Finding>, tile: HomeTile): Int = all.count { it.tile == tile }

    /** The group a tapped tile's sparkle scrolls the sheet to. */
    fun groupOfTile(tile: HomeTile): Group = Group.entries.first { it.tile == tile }

    /** Ids of the messages that a finding of the Unread tile is about: they get the small mark in the tile. */
    fun markedMessages(all: List<Finding>): Set<String> =
        all.filter { it.tile == HomeTile.UNREAD }.flatMap { it.messageIds }.toSet()

    /** Which phone action is the one to show beside a service button: the one that does something, before the ones that only show. */
    private fun rank(a: PhoneAction): Int = when (a) {
        is PhoneAction.MarkDone -> 0
        is PhoneAction.AddToCalendar -> 1
        is PhoneAction.AddTask -> 1
        is PhoneAction.MoveToday -> 2
        is PhoneAction.Respond -> 3
        is PhoneAction.Directions -> 4
        is PhoneAction.FreeTime -> 5
        is PhoneAction.OpenMails -> 6
        is PhoneAction.MoveEvent -> 7
        is PhoneAction.FindSlot -> 8
        is PhoneAction.OpenMail -> 9
    }

    /**
     * Two buttons at most. With a service action: the best phone action beside it. Without: the first two phone actions
     * as the core ordered them.
     */
    fun buttons(f: Finding): Buttons {
        val service = f.service
        return if (service != null) Buttons(listOfNotNull(f.phone.minByOrNull { rank(it) }), service) else Buttons(f.phone.take(2), null)
    }

    /** What a tap on the card itself does: open the mail a mail finding is about. */
    fun cardAction(f: Finding): PhoneAction? = f.phone.firstOrNull { it is PhoneAction.OpenMail }

    /** The conversations and messages that a mail finding is about: the small mark in a mail row. */
    data class RowMarks(val threads: Set<String>, val messages: Set<String>) {
        fun has(threadId: String, messageIds: Collection<String>): Boolean = threadId in threads || messageIds.any { it in messages }
    }

    fun rowMarks(all: List<Finding>): RowMarks {
        val mail = forPlace(all, Place.MailRows)
        return RowMarks(mail.mapNotNull { it.threadId }.toSet(), mail.flatMap { it.messageIds }.toSet())
    }

    /** Whether a mail row wears the mark: only with Home off (with Home on the Unread tile carries it) and only for a conversation that has a mail finding. */
    fun rowMarked(homeOn: Boolean, marks: RowMarks, threadId: String, messageIds: Collection<String>): Boolean = !homeOn && marks.has(threadId, messageIds)

    /** What a tap on a tasks-family finding that opens no mail does: show the Tasks view. */
    fun opensTasks(f: Finding): Boolean = f.family == AttentionFamily.TASKS && cardAction(f) == null

    /** The local start "YYYY-MM-DDTHH:MM:SS" of [seconds] in [zone], for the editor. */
    fun localStart(seconds: Long, zone: ZoneId): String =
        java.time.LocalDateTime.ofInstant(java.time.Instant.ofEpochSecond(seconds), zone).withNano(0).toString().let { if (it.length == 16) "$it:00" else it }

    /** The gaps of a finding's slots on the day of the first one, then the later days: for the chips. */
    fun slotsOf(f: Finding): List<TimeSlot> = f.slots

    /** The date of [seconds] in [zone]. */
    fun dateOf(seconds: Long, zone: ZoneId): LocalDate = java.time.Instant.ofEpochSecond(seconds).atZone(zone).toLocalDate()

    /**
     * The start a task gets when it is moved to [today]: the time of day it had, on [today]. A task with a time of
     * "00:00:00" or none keeps that, which the core reads as "no time". [start] is the task's "YYYY-MM-DDTHH:MM:SS" or empty.
     */
    fun movedToToday(start: String, today: LocalDate): String {
        val time = start.substringAfter('T', "00:00:00").ifBlank { "00:00:00" }
        return "${today}T${if (time.length == 5) "$time:00" else time}"
    }

    /** Whether it is time to ask the server again: the last refresh is older than [every] seconds. */
    fun dueForRefresh(lastRefreshSeconds: Long?, nowSeconds: Long, every: Long = REFRESH_EVERY_SECONDS): Boolean =
        lastRefreshSeconds == null || nowSeconds - lastRefreshSeconds >= every || nowSeconds < lastRefreshSeconds

    const val REFRESH_EVERY_SECONDS = 180L
}
