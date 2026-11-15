package ovh.kweb.mail.ui.assistant.chat

import android.content.res.Resources
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.assistant.chat.ChatRules.ChangeKind
import ovh.kweb.mail.ui.assistant.chat.ChatRules.StepKind

/** The fragments of text the chat's rules put together, so that the rules stay plain Kotlin a unit test can run. */
interface ChatWords {
    /** "Searching mail". */
    fun step(kind: StepKind): String

    /** "Searching mail: invoice" ([line] is [step]'s text). */
    fun stepWithDetail(line: String, detail: String): String

    /** The question of a card: "Archive 3 messages?", "Move 2 messages to Bills?" ([target] is the folder or the label). */
    fun title(kind: ChangeKind, count: Int, target: String): String

    /** What a card says once it was done: "Archived 3 messages". */
    fun done(kind: ChangeKind, count: Int, target: String): String

    /** The warning of a change over many messages. */
    fun warning(count: Int): String

    /** "Thursday, 12 November 2026, all day". */
    fun allDay(day: String): String

    /** "... 10:00 (Europe/Berlin)", for a time in another zone than the phone's. */
    fun inZone(time: String, zone: String): String

    /** "15 minutes before". */
    fun reminder(minutes: Int): String

    /** What the "Repeats" line says. */
    fun repeats(): String
}

/** The chat's wording from the string resources. */
class ResourceChatWords(private val resources: Resources) : ChatWords {
    override fun step(kind: StepKind): String = resources.getString(
        when (kind) {
            StepKind.SEARCH -> R.string.chat_step_search
            StepKind.LIST -> R.string.chat_step_list
            StepKind.READ_MESSAGE -> R.string.chat_step_read_message
            StepKind.READ_THREAD -> R.string.chat_step_read_thread
            StepKind.FOLDERS -> R.string.chat_step_folders
            StepKind.LABELS -> R.string.chat_step_labels
            StepKind.EVENTS -> R.string.chat_step_events
            StepKind.EVENT -> R.string.chat_step_event
            StepKind.OPEN -> R.string.chat_step_open
            StepKind.DRAFT -> R.string.chat_step_draft
            StepKind.SUGGEST -> R.string.chat_step_suggest
            StepKind.OTHER -> R.string.chat_step_other
        },
    )

    override fun stepWithDetail(line: String, detail: String): String = resources.getString(R.string.chat_step_with_detail, line, detail)

    override fun title(kind: ChangeKind, count: Int, target: String): String = when (kind) {
        ChangeKind.ARCHIVE -> resources.getQuantityString(R.plurals.chat_archive_q, count, count)
        ChangeKind.MOVE -> resources.getQuantityString(R.plurals.chat_move_q, count, count, target)
        ChangeKind.MARK_READ -> resources.getQuantityString(R.plurals.chat_mark_read_q, count, count)
        ChangeKind.MARK_UNREAD -> resources.getQuantityString(R.plurals.chat_mark_unread_q, count, count)
        ChangeKind.STAR -> resources.getQuantityString(R.plurals.chat_star_q, count, count)
        ChangeKind.UNSTAR -> resources.getQuantityString(R.plurals.chat_unstar_q, count, count)
        ChangeKind.TAG -> resources.getQuantityString(R.plurals.chat_tag_q, count, count, target)
        ChangeKind.UNTAG -> resources.getQuantityString(R.plurals.chat_untag_q, count, count, target)
        ChangeKind.ADD_EVENT -> resources.getString(R.string.chat_add_event_q)
        ChangeKind.ADD_TASK -> resources.getString(R.string.chat_add_task_q)
    }

    override fun done(kind: ChangeKind, count: Int, target: String): String = when (kind) {
        ChangeKind.ARCHIVE -> resources.getQuantityString(R.plurals.chat_archived, count, count)
        ChangeKind.MOVE -> resources.getQuantityString(R.plurals.chat_moved, count, count, target)
        ChangeKind.MARK_READ -> resources.getQuantityString(R.plurals.chat_marked_read, count, count)
        ChangeKind.MARK_UNREAD -> resources.getQuantityString(R.plurals.chat_marked_unread, count, count)
        ChangeKind.STAR -> resources.getQuantityString(R.plurals.chat_starred, count, count)
        ChangeKind.UNSTAR -> resources.getQuantityString(R.plurals.chat_unstarred, count, count)
        ChangeKind.TAG -> resources.getQuantityString(R.plurals.chat_tagged, count, count, target)
        ChangeKind.UNTAG -> resources.getQuantityString(R.plurals.chat_untagged, count, count, target)
        ChangeKind.ADD_EVENT -> resources.getString(R.string.chat_added_event)
        ChangeKind.ADD_TASK -> resources.getString(R.string.chat_added_task)
    }

    override fun warning(count: Int): String = resources.getQuantityString(R.plurals.chat_warning, count, count)

    override fun allDay(day: String): String = resources.getString(R.string.chat_event_all_day, day)

    override fun inZone(time: String, zone: String): String = resources.getString(R.string.chat_event_zone, time, zone)

    override fun reminder(minutes: Int): String = resources.getQuantityString(R.plurals.chat_event_reminder, minutes, minutes)

    override fun repeats(): String = resources.getString(R.string.chat_event_repeats_value)
}
