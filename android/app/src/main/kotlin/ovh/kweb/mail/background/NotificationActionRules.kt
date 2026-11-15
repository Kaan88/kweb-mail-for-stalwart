package ovh.kweb.mail.background

import uniffi.kweb_core.Action
import uniffi.kweb_core.Mailbox

/** A button a mail notification can carry. [key] is what is stored and never changes. */
enum class MailAction(val key: String) {
    Archive("archive"),
    Delete("delete"),
    MarkRead("read"),
    Reply("reply"),
}

/**
 * Which buttons a mail notification has and in which order: the user's choice per account, as plain functions
 * so they can be tested on the JVM.
 *
 * - Android shows at most three buttons, so at most [MAX] are chosen; fewer, or none, is allowed.
 * - An account that never chose has [DEFAULT]. That also holds for an install from before the setting existed
 *   (it stored nothing), which is the whole migration: its next notification has Delete where Archive was.
 * - An empty stored text is a choice too ("no buttons") and is not the default.
 */
object NotificationActionRules {
    const val MAX = 3
    private const val SEPARATOR = ","
    private const val TRASH_ROLE = "trash"

    /** What an account has that never chose. */
    val DEFAULT: List<MailAction> = listOf(MailAction.Delete, MailAction.MarkRead, MailAction.Reply)

    /** Every action, in the order the ones that are not chosen are listed. */
    val ALL: List<MailAction> = MailAction.entries

    /**
     * The choice a stored text stands for. Nothing stored: [DEFAULT]. Keys this version does not know (a newer
     * version wrote them) and repeats are left out, and no more than [MAX] are taken.
     */
    fun parse(stored: String?): List<MailAction> {
        if (stored == null) return DEFAULT
        val byKey = ALL.associateBy { it.key }
        return clean(stored.split(SEPARATOR).mapNotNull { byKey[it.trim()] })
    }

    /** The text to store for [chosen]; empty for no buttons. */
    fun encode(chosen: List<MailAction>): String = clean(chosen).joinToString(SEPARATOR) { it.key }

    /** [chosen] without repeats and cut to [MAX], the order kept. */
    fun clean(chosen: List<MailAction>): List<MailAction> = chosen.distinct().take(MAX)

    /** Whether one more can be switched on. */
    fun hasRoom(chosen: List<MailAction>): Boolean = clean(chosen).size < MAX

    /**
     * [action] switched on (it becomes the last button) or off. Switching on with [MAX] already chosen changes
     * nothing: the screen does not offer it.
     */
    fun set(chosen: List<MailAction>, action: MailAction, on: Boolean): List<MailAction> {
        val now = clean(chosen)
        return when {
            !on -> now - action
            action in now || now.size >= MAX -> now
            else -> now + action
        }
    }

    /** [action] one place earlier ([up]) or later; nothing changes at the end of the row or when it is not chosen. */
    fun move(chosen: List<MailAction>, action: MailAction, up: Boolean): List<MailAction> {
        val now = clean(chosen)
        val from = now.indexOf(action)
        val to = if (up) from - 1 else from + 1
        if (from < 0 || to !in now.indices) return now
        return now.toMutableList().apply { this[from] = now[to]; this[to] = action }
    }

    /**
     * What the Delete button of a notification submits for [messageId]: a move to the folder with the role
     * "trash" among [mailboxes], or nothing when the account has none.
     *
     * Not the core's `Action.Delete`: that one deletes for good when the mail is in the Trash already, which is
     * right in the list (the user sees where the mail is) and wrong here. A notification can stay on screen after
     * its mail went to the Trash (deleted in the app or elsewhere with "Remove when read elsewhere" off, or in the
     * moment before the removal is applied). A move to the Trash does to mail outside it exactly what Delete
     * does, and leaves mail in it where it is.
     */
    fun deleteAction(messageId: String, mailboxes: List<Mailbox>): Action? =
        mailboxes.firstOrNull { it.role == TRASH_ROLE }?.let { Action.Move(messageId, it.id) }

    /**
     * Whether a mail notification on screen must be posted again after the setting changed to [chosen].
     * [shownMessageId] and [shownActions] are what the notification carries in its extras: the message it is
     * for and the buttons it was built with ([encode]). One without a message id (a version from before the
     * setting posted it) cannot be built again; one that has the chosen buttons already is left alone, so a
     * change costs no more postings than it needs (Android drops updates that come too fast).
     */
    fun needsRefresh(shownMessageId: String?, shownActions: String?, chosen: List<MailAction>): Boolean =
        shownMessageId != null && shownActions != encode(chosen)

    /** One row of the settings screen. [position] is 0 for the first button, null when the action is not shown. */
    data class Row(
        val action: MailAction,
        val position: Int?,
        val canMoveUp: Boolean,
        val canMoveDown: Boolean,
        /** False for an action that is off while [MAX] are on: its switch cannot be used. */
        val canSwitch: Boolean,
    )

    /** The rows of the screen: the chosen actions in their order, then the others in the order of [ALL]. */
    fun rows(chosen: List<MailAction>): List<Row> {
        val now = clean(chosen)
        val shown = now.mapIndexed { index, action -> Row(action, index, index > 0, index < now.lastIndex, true) }
        val room = now.size < MAX
        return shown + ALL.filter { it !in now }.map { Row(it, null, canMoveUp = false, canMoveDown = false, canSwitch = room) }
    }
}
