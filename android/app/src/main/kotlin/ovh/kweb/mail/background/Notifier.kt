package ovh.kweb.mail.background

import android.app.Notification
import android.app.NotificationManager
import android.content.Context
import androidx.core.app.NotificationCompat
import java.time.Instant
import java.time.ZoneId
import java.util.Locale
import ovh.kweb.mail.R
import ovh.kweb.mail.accounts.AccountLabels
import ovh.kweb.mail.accounts.AccountRecord
import ovh.kweb.mail.accounts.Accounts
import uniffi.kweb_core.MailNotification
import uniffi.kweb_core.Plan
import uniffi.kweb_core.Reminder

/**
 * Turns the core's [Plan]s and [Reminder]s into Android notifications.
 *
 * Applying a plan is idempotent: posting an id that is on screen only refreshes it, silently,
 * and cancelling a missing id does nothing. That matters because a check hands its plan to its
 * caller and to the service's listener, so the same plan can arrive twice. Cancelling never
 * makes a sound. All methods may be called from any thread.
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
        synchronized(LOCK) { applyAllowed(record, allowed) }
    }

    private fun applyAllowed(record: AccountRecord, allowed: Plan) {
        val onScreen = activeMailIds(record).toSet()
        val tag = NotificationKeys.tag(record)
        val label = labelOf(record)
        allowed.cancel.forEach { manager.cancel(tag, it.androidId) }
        allowed.post.forEach { manager.notify(tag, it.androidId, mailNotification(record, it, label, isSilent(it, onScreen))) }
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
        if (!Settings.of(context, record).remindersEnabled) return
        val id = ReminderRules.notificationId(reminder.eventId, reminder.alertId)
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
     * Posts a test notification on the Mail channel, with its sound. Returns false when Android
     * will drop it because notifications are off for the app (the permission or the app setting).
     */
    fun sendTest(): Boolean {
        val notification = NotificationCompat.Builder(context, Channels.MAIL)
            .setSmallIcon(R.drawable.ic_stat_mail)
            .setContentTitle(context.getString(R.string.test_title))
            .setContentText(context.getString(R.string.test_text))
            .setAutoCancel(true)
            .setContentIntent(NotificationIntents.openAppPlain(context, TEST_ID))
            .build()
        manager.notify(TEST_ID, notification)
        return manager.areNotificationsEnabled()
    }

    private fun mailNotification(record: AccountRecord, n: MailNotification, label: String?, silent: Boolean): Notification {
        val builder = NotificationCompat.Builder(context, Channels.MAIL)
            .setSmallIcon(R.drawable.ic_stat_mail)
            .setContentTitle(n.from)
            .setContentText(n.subject)
            .setStyle(NotificationCompat.BigTextStyle().bigText(n.preview.ifBlank { n.subject }))
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
            .addAction(0, context.getString(R.string.action_archive), forMessage(record, ActionReceiver.ACTION_ARCHIVE, n))
            .addAction(0, context.getString(R.string.action_mark_read), forMessage(record, ActionReceiver.ACTION_MARK_READ, n))
            .addAction(0, context.getString(R.string.action_reply), NotificationIntents.reply(context, record, n.androidId, n.messageId))
        label?.let { builder.setSubText(it) }
        return builder.build()
    }

    private fun forMessage(record: AccountRecord, action: String, n: MailNotification) =
        NotificationIntents.forMessage(context, record, action, n.androidId, n.messageId)

    private fun updateSummary(record: AccountRecord, count: Int) {
        val tag = NotificationKeys.tag(record)
        if (count <= 0) {
            manager.cancel(tag, SUMMARY_ID)
            return
        }
        val summary = NotificationCompat.Builder(context, Channels.MAIL)
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

    /** The name to show on [record]'s notifications; null with one account, where nothing is added. */
    private fun labelOf(record: AccountRecord): String? {
        val records = Accounts.records.value
        return if (records.size > 1) AccountLabels.of(records)[record.id] else null
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
        /** Extra of the intent that opens the app from a mail notification: the JMAP message id (String). */
        const val EXTRA_MESSAGE_ID = "ovh.kweb.mail.extra.MESSAGE_ID"

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

        internal const val SUMMARY_ID = 1
        internal const val SERVICE_ID = 2
        internal const val TEST_ID = 3
        internal const val CHECK_ID = 4
        private val LOCK = Any()
    }
}
