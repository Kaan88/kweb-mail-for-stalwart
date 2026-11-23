package ovh.kweb.mail.ui.home

import uniffi.kweb_core.Mailbox
import uniffi.kweb_core.MessageHeader
import uniffi.kweb_core.Occurrence

/** What the Home tiles show, worked out from lists the core already has. Pure. */
object HomeRules {
    /** The next timed event that has not ended and is not declined, and the one after it. */
    fun nextEvents(entries: List<Occurrence>, now: Long): Pair<Occurrence?, Occurrence?> {
        val upcoming = entries
            .filter { !it.isTask && !it.allDay && it.endsAt > now && it.myStatus != "declined" }
            .sortedBy { it.startsAt }
        return upcoming.getOrNull(0) to upcoming.getOrNull(1)
    }

    /** Open tasks due today, due before today (overdue) and due tomorrow, by local day boundaries as seconds. */
    data class TaskCounts(val today: List<Occurrence>, val overdue: Int, val tomorrow: Int)

    fun tasks(entries: List<Occurrence>, todayStart: Long, tomorrowStart: Long, afterTomorrow: Long): TaskCounts {
        val open = entries.filter { it.isTask && !it.isDone }.sortedBy { it.startsAt }
        return TaskCounts(
            today = open.filter { it.startsAt in todayStart until tomorrowStart },
            overdue = open.count { it.startsAt < todayStart },
            tomorrow = open.count { it.startsAt in tomorrowStart until afterTomorrow },
        )
    }

    /** Roles the Home view never covers (and the picker never lists): the notification rule is not touched by this. */
    private val EXCLUDED_ROLES = setOf("junk", "trash")

    /** The folders the picker lists: the account's own, without Spam and Trash, in the server's order. */
    fun pickable(all: List<Mailbox>): List<Mailbox> = all.filter { it.role !in EXCLUDED_ROLES }.sortedBy { it.sortOrder }

    /**
     * The folders the unread tile covers. [chosen] empty (the default) is the Inbox alone. Otherwise the chosen folders that
     * still exist and are listable; when none of them does (all deleted), the Inbox again.
     */
    fun covered(all: List<Mailbox>, chosen: Set<String>): List<Mailbox> {
        val inbox = all.filter { it.role == "inbox" }
        if (chosen.isEmpty()) return inbox
        val picked = pickable(all).filter { it.id in chosen }
        return picked.ifEmpty { inbox }
    }

    /** "Parent / Child" for a folder that sits inside another. */
    fun path(box: Mailbox, all: List<Mailbox>): String {
        val names = ArrayList<String>()
        var at: Mailbox? = box
        var guard = 0
        while (at != null && guard++ < 16) {
            names.add(0, at.name)
            val parent = at.parentId
            at = if (parent == null) null else all.firstOrNull { it.id == parent }
        }
        return names.joinToString(" / ")
    }

    /** The unread count over [boxes]. */
    fun unreadIn(boxes: List<Mailbox>): Int = boxes.sumOf { it.unreadEmails.toLong() }.coerceAtMost(Int.MAX_VALUE.toLong()).toInt()

    /** Messages from several folders as one list, each message once (it can sit in more than one folder). */
    fun merge(lists: List<List<MessageHeader>>): List<MessageHeader> = lists.flatten().distinctBy { it.id }

    /** The newest unread mails of the Inbox, at most [n], by who they are from. */
    fun newestUnread(inbox: List<MessageHeader>, n: Int = 3): List<MessageHeader> = inbox.filter { it.isUnread }.sortedByDescending { it.receivedAt }.take(n)

    /** The name to show for a sender: the display name, else the address. */
    fun sender(m: MessageHeader): String = m.fromName.ifBlank { m.fromEmail }

    fun flagged(inbox: List<MessageHeader>): List<MessageHeader> = inbox.filter { it.isFlagged }

    /** How many whole days old the oldest of [flagged] is at [now]; null when there is none. */
    fun oldestFlaggedDays(flagged: List<MessageHeader>, now: Long): Int? =
        flagged.minOfOrNull { it.receivedAt }?.let { ((now - it) / 86_400).toInt().coerceAtLeast(0) }

    /** A greeting by the hour of the day. */
    enum class Greeting { MORNING, AFTERNOON, EVENING }

    fun greeting(hour: Int): Greeting = when (hour) {
        in 5..11 -> Greeting.MORNING
        in 12..17 -> Greeting.AFTERNOON
        else -> Greeting.EVENING
    }

    /** The delivery problem the status chip shows, from the connection state; null while all is well or still connecting. */
    enum class Problem { OFFLINE, SIGN_IN }

    /** The away chip: on while the message is enabled and not over. */
    fun awayOn(enabled: Boolean, until: Long?, now: Long): Boolean = enabled && (until == null || until > now)
}
