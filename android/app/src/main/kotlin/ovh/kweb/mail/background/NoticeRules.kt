package ovh.kweb.mail.background

import ovh.kweb.mail.R
import ovh.kweb.mail.accounts.AccountRecord
import uniffi.kweb_core.Notice
import uniffi.kweb_core.NoticeKind
import uniffi.kweb_core.NoticeThing

/**
 * What the server's notices (another person invited, changed, cancelled, shared) become as Android notifications, free of
 * Android so it can be tested on the JVM. A notice is its own notification: the tag names the account and the notice, the
 * id is one for all of them, so two notices never replace each other and the same notice never shows twice.
 */
internal object NoticeRules {
    /** The id every notice notification has; the tag tells them apart. Below the core's mail ids and clear of the app's own. */
    const val ID = 7

    /** At most this many are posted from one look; the newest. A server that kept many would otherwise flood the phone. */
    const val MOST = 10

    fun tag(record: AccountRecord, notice: Notice): String = "notice:${record.id}:${notice.id}"

    /** The notices to post, newest first, at most [MOST]. */
    fun newest(notices: List<Notice>): List<Notice> = notices.sortedByDescending { it.at }.take(MOST)

    /** Where a tap goes: the agenda for an event, "Shared with me" for a share. */
    fun opens(notice: Notice): String = if (notice.thing == NoticeThing.EVENT) Notifier.OPEN_CALENDAR else Notifier.OPEN_SHARED

    /** A request code for the tap's intent that differs between notices of one account. */
    fun requestCode(notice: Notice): Int = notice.id.hashCode()

    /** The title's string: `%1$s` is the person, and for a share `%2$s` is [thingRes]. */
    fun titleRes(kind: NoticeKind): Int = when (kind) {
        NoticeKind.EVENT_INVITED -> R.string.notice_event_invited
        NoticeKind.EVENT_ADDED -> R.string.notice_event_added
        NoticeKind.EVENT_CHANGED -> R.string.notice_event_changed
        NoticeKind.EVENT_CANCELLED -> R.string.notice_event_cancelled
        NoticeKind.SHARE_NEW -> R.string.notice_share_new
        NoticeKind.SHARE_CHANGED -> R.string.notice_share_changed
        NoticeKind.SHARE_ENDED -> R.string.notice_share_ended
    }

    /** What was shared, with its article ("a calendar"). */
    fun thingRes(thing: NoticeThing): Int = when (thing) {
        NoticeThing.MAILBOX -> R.string.notice_thing_mailbox
        NoticeThing.CALENDAR -> R.string.notice_thing_calendar
        NoticeThing.ADDRESS_BOOK -> R.string.notice_thing_address_book
        NoticeThing.EVENT, NoticeThing.OTHER -> R.string.notice_thing_other
    }

    /** True for the notices about an event: they have a title and a time to show under the sentence. */
    fun isEvent(notice: Notice): Boolean = notice.thing == NoticeThing.EVENT
}
