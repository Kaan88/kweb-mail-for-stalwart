package ovh.kweb.mail.ui

import ovh.kweb.mail.R

/** The choices of "Swipes and actions": what each swipe does, when a delete asks, how long Undo of a send lasts. No Android here but string ids. */
object ActionRules {
    /** What a swipe on a conversation in the mail list does. */
    enum class Swipe(val key: String, val label: Int) {
        ARCHIVE("archive", R.string.action_archive),
        DELETE("delete", R.string.action_delete),
        READ("read", R.string.swipe_read_unread),
        STAR("star", R.string.swipe_star_unstar),
        MOVE("move", R.string.swipe_move),
        NONE("none", R.string.swipe_none);

        companion object {
            fun of(key: String?, default: Swipe): Swipe = entries.firstOrNull { it.key == key } ?: default
        }
    }

    val DEFAULT_RIGHT = Swipe.ARCHIVE
    val DEFAULT_LEFT = Swipe.DELETE

    /**
     * Whether the swipe [action] is possible in a folder with [role] while no row is chosen. Archive does nothing in the Archive
     * folder (as before), and no swipe works while rows are being chosen.
     */
    fun enabled(action: Swipe, role: String?, selecting: Boolean): Boolean = when {
        selecting -> false
        action == Swipe.NONE -> false
        action == Swipe.ARCHIVE -> role != "archive"
        else -> true
    }

    /** What a delete asks first: nothing, the "Delete n conversations?" question, or (in Trash, where it is for good) the forever question. */
    enum class DeleteAsk { NONE, CONFIRM, FOREVER }

    /** In Trash the forever question is the only one, so nobody is asked twice; elsewhere the setting decides. */
    fun deleteAsk(inTrash: Boolean, confirmSetting: Boolean): DeleteAsk = when {
        inTrash -> DeleteAsk.FOREVER
        confirmSetting -> DeleteAsk.CONFIRM
        else -> DeleteAsk.NONE
    }

    /** The choices of the time Undo of a send lasts, in seconds. */
    val UNDO_CHOICES = listOf(5, 10, 20, 30)
    const val DEFAULT_UNDO_SECONDS = 10

    /** A stored value outside the choices is the default. */
    fun undoSeconds(stored: Int): Int = if (stored in UNDO_CHOICES) stored else DEFAULT_UNDO_SECONDS

    /**
     * The thread's primary button is Reply all when the setting is on and the message has more than one recipient ([recipients]
     * counts To and Cc); with one recipient (or none known) it is Reply as before.
     */
    fun replyAllPrimary(setting: Boolean, recipients: Int): Boolean = setting && recipients > 1
}
