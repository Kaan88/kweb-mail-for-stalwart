package ovh.kweb.mail.background

import android.app.Notification
import android.app.NotificationManager
import android.content.Context
import android.os.Bundle
import android.util.Log
import androidx.core.app.NotificationCompat
import java.time.Instant
import java.time.ZoneId
import java.util.Locale
import java.util.concurrent.Executors
import java.util.concurrent.ScheduledFuture
import java.util.concurrent.TimeUnit
import ovh.kweb.mail.R
import ovh.kweb.mail.accounts.AccountLabels
import ovh.kweb.mail.accounts.AccountRecord
import ovh.kweb.mail.accounts.Accounts
import uniffi.kweb_core.MailNotification
import uniffi.kweb_core.Notice
import uniffi.kweb_core.Plan
import uniffi.kweb_core.Reminder

/**
 * Turns the core's [Plan]s and [Reminder]s into Android notifications.
 *
 * Posting an id that is on screen only refreshes it, silently, and cancelling a missing id does
 * nothing. A plan is applied once, by the account's listener inside the pass that made it
 * ([ovh.kweb.mail.accounts.AccountSession]); the copy a check or a push message returns is not
 * applied again. Cancelling never makes a sound. All methods may be called from any thread.
 *
 * Every notification belongs to an account: Android identifies it by (tag, id), and [NotificationKeys] says
 * which tag, group and intent URIs an account has (the migrated account keeps the old ones, no tag). With
 * more than one account the mail notifications, summaries and reminders say whose they are (a subtext).
 */
class Notifier(context: Context) {
    private val context: Context = context.applicationContext
    private val manager: NotificationManager = this.context.getSystemService(NotificationManager::class.java)

    /**
     * Posts and cancels what [plan] says for [record], as far as the three switches in that account's
     * [Settings] allow. Mail notifications are grouped per account under one summary that goes away with the
     * last one.
     */
    fun apply(record: AccountRecord, plan: Plan) {
        val settings = Settings.of(context, record)
        val allowed = plan.honouring(settings.mailEnabled, settings.removeWhenReadElsewhere)
        if (allowed.post.isEmpty() && allowed.cancel.isEmpty()) return
        // Under the lock [cancelAccount] takes: a plan of a pass that was in flight when its account was removed
        // either lands before the removal clears it or is dropped.
        synchronized(LOCK) { if (!Settings.isGone(record)) applyAllowed(record, allowed) }
    }

    private fun applyAllowed(record: AccountRecord, allowed: Plan) {
        val onScreen = activeMailIds(record).toSet()
        val tag = NotificationKeys.tag(record)
        val label = labelOf(record)
        if (allowed.post.isNotEmpty()) Channels.ensureAccount(context, record, label)
        allowed.cancel.forEach {
            manager.cancel(tag, it.androidId)
            forgetCode(record, it.androidId)
        }
        val settings = Settings.of(context, record)
        val actions = settings.notificationActions
        val preview = settings.previewChoice
        val copyCode = settings.copyCodeEnabled
        allowed.post.forEach {
            // The channel of the address the mail came to is made when first needed.
            it.address?.let { address -> Channels.ensureAddress(context, record, label, address, it.quiet) }
            val channel = ChannelRules.forMail(record, it.quiet, it.address)
            val code = codeFor(record, it, copyCode, preferRemembered = false)
            manager.notify(tag, it.androidId, mailNotification(record, it, label, isSilent(it, onScreen), actions, channel, preview, code))
        }
        updateSummary(record, idsAfter(onScreen, allowed).size)
    }

    /**
     * The Android ids of the mail notifications of [record] really in the shade now: what
     * [uniffi.kweb_core.Shade.Ids] wants, so each core's rule sees only its own shade. Ids below 1000 are the
     * app's own entries (summary, "Connected", reminders, test).
     */
    fun activeMailIds(record: AccountRecord): List<Int> = try {
        manager.activeNotifications
            .filter { it.id >= NotificationKeys.FIRST_MAIL_ID && NotificationKeys.owns(record, it.tag, it.id) }
            .map { it.id }
    } catch (_: RuntimeException) {
        // The system could not answer. An empty list makes the core put every mail back silently.
        emptyList()
    }

    /** Removes one mail notification at once (the user acted on it) and the summary if it was the last. */
    fun cancelMail(record: AccountRecord, androidId: Int) = synchronized(LOCK) {
        // The id comes from an intent the exported activity received: it must not reach the app's own entries.
        if (androidId < NotificationKeys.FIRST_MAIL_ID) return@synchronized
        manager.cancel(NotificationKeys.tag(record), androidId)
        forgetCode(record, androidId)
        updateSummary(record, (activeMailIds(record).toSet() - androidId).size)
    }

    /**
     * Removes a reminder's notification (the user opened it). The id comes from an intent the exported
     * activity received: only ids in the reminders' own range are taken, never a mail notification or the
     * service entry.
     */
    fun cancelReminder(record: AccountRecord, androidId: Int) {
        if (ReminderRules.isReminderId(androidId)) manager.cancel(NotificationKeys.tag(record), androidId)
    }

    /**
     * Removes every notification of [record]: its mail, summary and reminders. Nothing of another account and
     * not the service entry.
     */
    fun cancelAccount(record: AccountRecord) = synchronized(LOCK) {
        val mine = try {
            manager.activeNotifications.filter { NotificationKeys.owns(record, it.tag, it.id) }
        } catch (_: RuntimeException) {
            emptyList()
        }
        mine.forEach { manager.cancel(it.tag, it.id) }
        Channels.forget(context, record)
    }

    /**
     * Posts the summaries again, so that their group headers say whose they are after the number of accounts
     * changed. Children already on screen change on their next refresh.
     */
    fun refreshSummaries() = synchronized(LOCK) {
        for (record in Accounts.records.value) updateSummary(record, activeMailIds(record).size)
    }

    /** Shows a calendar or task reminder of [record], unless the user turned reminders off for it. */
    fun showReminder(record: AccountRecord, reminder: Reminder) {
        if (Settings.isGone(record) || !Settings.of(context, record).remindersEnabled) return
        val id = ReminderRules.notificationId(reminder.eventId, reminder.alertId)
        // The live loop and the push both bring a fired alert: it alerts once.
        val key = NotificationKeys.tag(record) + "/" + id
        val now = System.currentTimeMillis()
        synchronized(recentReminders) {
            if (ReminderRules.isRepeat(recentReminders[key], now)) return
            recentReminders[key] = now
        }
        val open = NotificationIntents.openReminder(context, record, id, reminder)
        val notification = NotificationCompat.Builder(context, Channels.REMINDERS)
            .setSmallIcon(R.drawable.ic_stat_mail)
            .setContentTitle(reminderTitle(reminder))
            .setCategory(NotificationCompat.CATEGORY_REMINDER)
            .setPriority(NotificationCompat.PRIORITY_HIGH)
            .setAutoCancel(true)
            .setContentIntent(open)
            .addAction(0, context.getString(R.string.action_snooze), snoozeIntent(record, id, reminder))
            .addAction(0, context.getString(R.string.action_open), open)
            .apply {
                reminderText(reminder)?.let { setContentText(it) }
                labelOf(record)?.let { setSubText(it) }
            }
            .build()
        manager.notify(NotificationKeys.tag(record), id, notification)
    }

    /**
     * Shows the server's notices (invited, changed, cancelled, shared) of [record], each as a notification of its own on the
     * Invitations and sharing channel, the newest [NoticeRules.MOST] at most. The words are the other person's (a name, an
     * event title) and are shown as text only. Returns true: the notices are handled, and the core tells the server so.
     */
    fun showNotices(record: AccountRecord, notices: List<Notice>): Boolean {
        if (Settings.isGone(record)) return true
        Channels.ensure(context)
        val label = labelOf(record)
        val now = Instant.now()
        for (notice in NoticeRules.newest(notices)) {
            val open = NotificationIntents.openNotice(context, record, NoticeRules.requestCode(notice), NoticeRules.opens(notice))
            val title = if (NoticeRules.isEvent(notice)) {
                context.getString(NoticeRules.titleRes(notice.kind), notice.who)
            } else {
                context.getString(NoticeRules.titleRes(notice.kind), notice.who, context.getString(NoticeRules.thingRes(notice.thing)))
            }
            val text = if (NoticeRules.isEvent(notice)) noticeEventText(notice, now) else null
            val notification = NotificationCompat.Builder(context, Channels.NOTICES)
                .setSmallIcon(R.drawable.ic_stat_mail)
                .setContentTitle(title)
                .setCategory(NotificationCompat.CATEGORY_EVENT)
                .setAutoCancel(true)
                .setContentIntent(open)
                .apply {
                    text?.let { setContentText(it) }
                    label?.let { setSubText(it) }
                }
                .build()
            manager.notify(NoticeRules.tag(record, notice), NoticeRules.ID, notification)
        }
        return true
    }

    /** The event's title, and when it is when the notice says. */
    private fun noticeEventText(notice: Notice, now: Instant): String {
        val title = notice.title.ifBlank { context.getString(R.string.notice_event_untitled) }
        val time = ReminderRules.formatTime(notice.localStart, notice.timeZone, notice.allDay, ZoneId.systemDefault(), Locale.getDefault(), now)
        return if (time == null) title else context.getString(R.string.notice_event_text, title, time)
    }

    /**
     * Posts a test notification on the Mail channel of [record], with its sound. Returns false when Android
     * will drop it because notifications are off for the app (the permission or the app setting).
     */
    fun sendTest(record: AccountRecord): Boolean {
        Channels.ensureAccount(context, record, labelOf(record))
        val notification = NotificationCompat.Builder(context, ChannelRules.mail(record))
            .setSmallIcon(R.drawable.ic_stat_mail)
            .setContentTitle(context.getString(R.string.test_title))
            .setContentText(context.getString(R.string.test_text))
            .setAutoCancel(true)
            .setContentIntent(NotificationIntents.openAppPlain(context, TEST_ID))
            .build()
        manager.notify(TEST_ID, notification)
        return manager.areNotificationsEnabled()
    }

    /**
     * Gives the mail notifications of [record] that are on screen the buttons the account's setting says, a
     * moment after the last change: the screen calls this on every tap, and only the last call within
     * [REFRESH_DELAY_MILLIS] does the work. It runs on a thread of its own, so it also finishes after the
     * screen was left.
     */
    fun refreshActionsSoon(record: AccountRecord) {
        synchronized(pendingRefreshes) {
            pendingRefreshes.remove(record.id)?.cancel(false)
            pendingRefreshes[record.id] = refresher.schedule({
                try {
                    refreshActions(record)
                } catch (e: Exception) {
                    Log.w(Background.TAG, "refreshing the notification buttons failed", e)
                }
            }, REFRESH_DELAY_MILLIS, TimeUnit.MILLISECONDS)
        }
    }

    /**
     * Posts again, silently and without moving them, the mail notifications of [record] whose buttons are not
     * what the account's setting now says ([NotificationActionRules.needsRefresh]). A notification says in its
     * extras which message it is for and which buttons it has; one posted by a version from before the setting
     * does not, and keeps the buttons it has (they still work) until it goes away.
     *
     * One at a time with a pause between them: Android drops updates of a package that come faster than about
     * five a second, and a dropped one would keep its old buttons. Each is looked up again under the lock just
     * before it is posted, so one that went away meanwhile is not put back and a newer choice is the one used.
     * Blocks for [REFRESH_GAP_MILLIS] per notification.
     */
    fun refreshActions(record: AccountRecord) {
        val tried = HashSet<Int>()
        while (synchronized(LOCK) { refreshNext(record, tried) }) Thread.sleep(REFRESH_GAP_MILLIS)
    }

    /** Posts one notification that needs it and is not in [tried] yet; false when none is left. */
    private fun refreshNext(record: AccountRecord, tried: MutableSet<Int>): Boolean {
        if (Settings.isGone(record)) return false
        val settings = Settings.of(context, record)
        val actions = settings.notificationActions
        val shown = try {
            manager.activeNotifications.filter { it.id >= NotificationKeys.FIRST_MAIL_ID && NotificationKeys.owns(record, it.tag, it.id) }
        } catch (_: RuntimeException) {
            emptyList()
        }
        val entry = shown.firstOrNull {
            val extras = it.notification.extras
            it.id !in tried &&
                NotificationActionRules.needsRefresh(extras.getString(EXTRA_SHOWN_MESSAGE_ID), extras.getString(EXTRA_SHOWN_ACTIONS), actions)
        } ?: return false
        tried += entry.id
        val extras = entry.notification.extras
        val again = MailNotification(
            messageId = extras.getString(EXTRA_SHOWN_MESSAGE_ID).orEmpty(),
            androidId = entry.id,
            from = extras.getCharSequence(Notification.EXTRA_TITLE)?.toString().orEmpty(),
            subject = extras.getCharSequence(Notification.EXTRA_TEXT)?.toString().orEmpty(),
            preview = extras.getCharSequence(Notification.EXTRA_BIG_TEXT)?.toString().orEmpty(),
            receivedAt = entry.notification.`when` / 1000,
            silent = true,
            quiet = false,
            address = null,
        )
        // A refresh keeps the channel the mail was posted in. Its text may be shortened by the preview choice, so the
        // code found when the mail arrived is the one kept (in memory only; after a restart the shown text is searched).
        val code = codeFor(record, again, settings.copyCodeEnabled, preferRemembered = true)
        manager.notify(
            entry.tag,
            entry.id,
            mailNotification(record, again, labelOf(record), true, actions, entry.notification.channelId, settings.previewChoice, code),
        )
        return true
    }

    private fun mailNotification(
        record: AccountRecord,
        n: MailNotification,
        label: String?,
        silent: Boolean,
        actions: List<MailAction>,
        channelId: String,
        preview: PreviewChoice,
        code: String?,
    ): Notification {
        val texts = PreviewRules.shown(preview, n.from, n.subject, n.preview, context.getString(R.string.notification_new_mail))
        val builder = NotificationCompat.Builder(context, channelId)
            .setSmallIcon(R.drawable.ic_stat_mail)
            .setContentTitle(texts.title)
            .apply {
                if (texts.text.isNotEmpty()) setContentText(texts.text)
                if (texts.big.isNotEmpty()) setStyle(NotificationCompat.BigTextStyle().bigText(texts.big))
            }
            .setWhen(whenMillis(n.receivedAt, System.currentTimeMillis()))
            .setShowWhen(true)
            .setCategory(NotificationCompat.CATEGORY_EMAIL)
            .setGroup(NotificationKeys.group(record))
            // The channel has a sound; a silent posting must not use it. For a child of a group,
            // "alert the summary only" mutes it, and the summary itself never alerts.
            .setGroupAlertBehavior(if (silent) NotificationCompat.GROUP_ALERT_SUMMARY else NotificationCompat.GROUP_ALERT_CHILDREN)
            .setSilent(silent)
            .setOnlyAlertOnce(true)
            .setAutoCancel(true)
            .setContentIntent(NotificationIntents.openApp(context, record, n.androidId, "message", n.messageId, EXTRA_MESSAGE_ID))
            .setDeleteIntent(NotificationIntents.forMessage(context, record, ActionReceiver.ACTION_SWIPED, n.androidId, n.messageId))
            // Which message this is and which buttons it has, so [refreshActions] can post it again with other buttons.
            .addExtras(
                Bundle().apply {
                    putString(EXTRA_SHOWN_MESSAGE_ID, n.messageId)
                    putString(EXTRA_SHOWN_ACTIONS, NotificationActionRules.encode(actions))
                },
            )
        // Copy code goes first and may push the user's last button out when three are chosen (OneTimeCodeRules.plan).
        // The code is only in this button's intent, never in the notification's text.
        val buttons = OneTimeCodeRules.plan(actions, code != null)
        if (buttons.copy && code != null) {
            builder.addAction(NotificationCompat.Action.Builder(0, context.getString(R.string.action_copy_code), NotificationIntents.copyCode(context, record, n.androidId, n.messageId, code)).setAuthenticationRequired(true).build())
        }
        for (action in buttons.actions) {
            when (action) {
                MailAction.Archive -> builder.addAction(0, context.getString(R.string.action_archive), forMessage(record, ActionReceiver.ACTION_ARCHIVE, n))
                MailAction.Delete -> builder.addAction(0, context.getString(R.string.action_delete), forMessage(record, ActionReceiver.ACTION_DELETE, n))
                MailAction.MarkRead -> builder.addAction(0, context.getString(R.string.action_mark_read), forMessage(record, ActionReceiver.ACTION_MARK_READ, n))
                MailAction.Reply -> builder.addAction(0, context.getString(R.string.action_reply), NotificationIntents.reply(context, record, n.androidId, n.messageId))
            }
        }
        label?.let { builder.setSubText(it) }
        if (PreviewRules.hasPublicVersion(preview)) {
            // The lock screen shows what the user's choice allows, no more.
            val publicVersion = NotificationCompat.Builder(context, channelId)
                .setSmallIcon(R.drawable.ic_stat_mail)
                .setContentTitle(texts.title)
                .apply { if (texts.text.isNotEmpty()) setContentText(texts.text) }
                .setWhen(whenMillis(n.receivedAt, System.currentTimeMillis()))
                .setCategory(NotificationCompat.CATEGORY_EMAIL)
                .apply { label?.let { setSubText(it) } }
                .build()
            builder.setPublicVersion(publicVersion)
        }
        return builder.build()
    }

    /**
     * The code to offer on [n]'s notification, or null (switched off, or none in the mail). The text searched is what
     * the core handed over (subject and cached first lines), whatever the preview choice shows. The last code found
     * for a notification is kept in memory (never stored, never logged) because a refresh only has the shown text.
     */
    private fun codeFor(record: AccountRecord, n: MailNotification, enabled: Boolean, preferRemembered: Boolean): String? {
        if (!enabled) return null
        val key = NotificationKeys.tag(record) + "/" + n.androidId
        synchronized(codes) {
            if (preferRemembered) codes[key]?.let { return it }
            val found = OneTimeCodeRules.find(n.subject, n.preview)
            if (found != null) codes[key] = found
            return found ?: codes[key]
        }
    }

    private fun forgetCode(record: AccountRecord, androidId: Int) {
        synchronized(codes) { codes.remove(NotificationKeys.tag(record) + "/" + androidId) }
    }

    private fun forMessage(record: AccountRecord, action: String, n: MailNotification) =
        NotificationIntents.forMessage(context, record, action, n.androidId, n.messageId)

    private fun updateSummary(record: AccountRecord, count: Int) {
        val tag = NotificationKeys.tag(record)
        if (count <= 0) {
            manager.cancel(tag, SUMMARY_ID)
            return
        }
        Channels.ensureAccount(context, record, labelOf(record))
        val summary = NotificationCompat.Builder(context, ChannelRules.mail(record))
            .setSmallIcon(R.drawable.ic_stat_mail)
            .setContentTitle(context.getString(R.string.app_name))
            .setContentText(context.resources.getQuantityString(R.plurals.mail_summary, count, count))
            .setCategory(NotificationCompat.CATEGORY_EMAIL)
            .setGroup(NotificationKeys.group(record))
            .setGroupSummary(true)
            .setGroupAlertBehavior(NotificationCompat.GROUP_ALERT_CHILDREN)
            .setOnlyAlertOnce(true)
            .setAutoCancel(true)
            .setContentIntent(NotificationIntents.openAccount(context, record, SUMMARY_ID))
            .apply { labelOf(record)?.let { setSubText(it) } }
            .build()
        manager.notify(tag, SUMMARY_ID, summary)
    }

    private fun snoozeIntent(record: AccountRecord, id: Int, reminder: Reminder) =
        NotificationIntents.forReminder(context, record, ActionReceiver.ACTION_SNOOZE, id, reminder)

    /** The name to show on [record]'s notifications: the user's name for it, else its login with several accounts; null with one unnamed account. */
    private fun labelOf(record: AccountRecord): String? {
        val records = Accounts.records.value
        return AccountLabels.forNotice(record, records, Accounts.names.value)
    }

    private fun reminderTitle(r: Reminder): String = r.title.ifBlank {
        context.getString(if (r.isTask) R.string.reminder_task_generic else R.string.reminder_event_generic)
    }

    /** "10:00 · Çankaya", with whichever of the two parts is known. */
    private fun reminderText(r: Reminder): String? {
        val time = ReminderRules.formatTime(r.localStart, r.timeZone, r.allDay, ZoneId.systemDefault(), Locale.getDefault(), Instant.now())
        return when {
            time != null && r.location.isNotBlank() -> context.getString(R.string.reminder_text_with_location, time, r.location)
            time != null -> time
            r.location.isNotBlank() -> r.location
            else -> null
        }
    }

    companion object {
        /** When a reminder notification was last posted, by tag and id (see [ReminderRules.isRepeat]). */
        private val recentReminders = HashMap<String, Long>()

        /** The code found for a mail notification, by tag and id; in memory only, the newest 64 (see [codeFor]). */
        private val codes = object : LinkedHashMap<String, String>() {
            override fun removeEldestEntry(eldest: MutableMap.MutableEntry<String, String>?): Boolean = size > 64
        }

        /** Extra of the intent that opens the app from a mail notification: the JMAP message id (String). */
        const val EXTRA_MESSAGE_ID = "ovh.kweb.mail.extra.MESSAGE_ID"

        /** In the extras of a posted mail notification itself (not of an intent): the JMAP message id it is for (String). */
        internal const val EXTRA_SHOWN_MESSAGE_ID = "ovh.kweb.mail.extra.SHOWN_MESSAGE_ID"

        /** Next to [EXTRA_SHOWN_MESSAGE_ID]: the buttons the notification was built with, as the setting stores them (String). */
        internal const val EXTRA_SHOWN_ACTIONS = "ovh.kweb.mail.extra.SHOWN_ACTIONS"

        /** How long after the last change of the buttons setting the notifications on screen follow. */
        private const val REFRESH_DELAY_MILLIS = 500L

        /** The pause between two notifications posted again: under four a second, below Android's limit of five. */
        private const val REFRESH_GAP_MILLIS = 300L

        /** Runs [refreshActionsSoon]'s work, one account after the other. */
        private val refresher = Executors.newSingleThreadScheduledExecutor { Thread(it, "kweb-notification-buttons").apply { isDaemon = true } }

        /** The refresh that waits for its moment, by account id. */
        private val pendingRefreshes = HashMap<String, ScheduledFuture<*>>()

        /** Extra of the intent behind the Reply button of a mail notification: the JMAP message id (String). */
        const val EXTRA_REPLY_MESSAGE_ID = "ovh.kweb.mail.extra.REPLY_MESSAGE_ID"

        /** Extra next to [EXTRA_REPLY_MESSAGE_ID]: the Android id of the notification to remove (Int). */
        const val EXTRA_REPLY_ANDROID_ID = "ovh.kweb.mail.extra.REPLY_ANDROID_ID"

        /** Extra of the intent that opens the app from a reminder: the event or task id (String). */
        const val EXTRA_EVENT_ID = "ovh.kweb.mail.extra.EVENT_ID"

        /** Extra next to [EXTRA_EVENT_ID]: the occurrence of a repeating event the reminder is for (String, absent when it does not repeat). */
        const val EXTRA_RECURRENCE_ID = "ovh.kweb.mail.extra.RECURRENCE_ID"

        /** Extra next to [EXTRA_EVENT_ID]: the Android id of the reminder's notification (Int). */
        const val EXTRA_REMINDER_ANDROID_ID = "ovh.kweb.mail.extra.REMINDER_ANDROID_ID"

        /** Extra of every intent a notification sends: the id of the account it is for ([AccountRecord.id], String). No extra means the migrated account. */
        const val EXTRA_ACCOUNT = "ovh.kweb.mail.extra.ACCOUNT"

        /** Extra of a notification intent that asks for a screen of the app instead of a message ([OPEN_DEVICE_CALENDAR]). */
        const val EXTRA_OPEN = "ovh.kweb.mail.extra.OPEN"
        const val OPEN_DEVICE_CALENDAR = "device_calendar"

        /** A notice about an event was tapped: the agenda opens. */
        const val OPEN_CALENDAR = "calendar"

        /** A notice about a share was tapped: "Shared with me" opens. */
        const val OPEN_SHARED = "shared"

        internal const val SUMMARY_ID = 1
        internal const val SERVICE_ID = 2
        internal const val TEST_ID = 3
        internal const val CHECK_ID = 4
        private val LOCK = Any()
    }
}
