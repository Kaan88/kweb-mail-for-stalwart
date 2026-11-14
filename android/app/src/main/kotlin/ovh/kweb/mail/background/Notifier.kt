package ovh.kweb.mail.background

import android.app.Notification
import android.app.NotificationManager
import android.content.Context
import androidx.core.app.NotificationCompat
import java.time.Instant
import java.time.ZoneId
import java.util.Locale
import ovh.kweb.mail.R
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
 */
class Notifier(context: Context, private val settings: Settings = Settings.of(context)) {
    private val context: Context = context.applicationContext
    private val manager: NotificationManager = this.context.getSystemService(NotificationManager::class.java)

    /**
     * Posts and cancels what [plan] says, as far as the three switches in [Settings] allow.
     * Mail notifications are grouped per account under one summary that goes away with the last one.
     */
    fun apply(plan: Plan) {
        val allowed = plan.honouring(settings.mailEnabled, settings.removeWhenReadElsewhere)
        if (allowed.post.isEmpty() && allowed.cancel.isEmpty()) return
        synchronized(LOCK) { applyAllowed(allowed) }
    }

    private fun applyAllowed(allowed: Plan) {
        val onScreen = activeMailIds().toSet()
        val group = groupKey()
        allowed.cancel.forEach { manager.cancel(it.androidId) }
        allowed.post.forEach { manager.notify(it.androidId, mailNotification(it, group, isSilent(it, onScreen))) }
        updateSummary(idsAfter(onScreen, allowed).size, group)
    }

    /**
     * The Android ids of the mail notifications really in the shade now: what [uniffi.kweb_core.Shade.Ids]
     * wants. Ids below 1000 are the app's own entries (summary, "Connected", reminders, test).
     */
    fun activeMailIds(): List<Int> = try {
        manager.activeNotifications.map { it.id }.filter { it >= FIRST_MAIL_ID }
    } catch (_: RuntimeException) {
        // The system could not answer. An empty list makes the core put every mail back silently.
        emptyList()
    }

    /** Removes one mail notification at once (the user acted on it) and the summary if it was the last. */
    fun cancelMail(androidId: Int) = synchronized(LOCK) {
        // The id comes from an intent the exported activity received: it must not reach the app's own entries.
        if (androidId < FIRST_MAIL_ID) return@synchronized
        manager.cancel(androidId)
        updateSummary((activeMailIds().toSet() - androidId).size, groupKey())
    }

    /**
     * Removes a reminder's notification (the user opened it). The id comes from an intent the exported
     * activity received: only ids in the reminders' own range are taken, never a mail notification or the
     * service entry.
     */
    fun cancelReminder(androidId: Int) {
        if (ReminderRules.isReminderId(androidId)) manager.cancel(androidId)
    }

    /** Removes every notification of the app. The service entry stays while the service runs. */
    fun cancelAll() = manager.cancelAll()

    /** Shows a calendar or task reminder, unless the user turned reminders off. */
    fun showReminder(reminder: Reminder) {
        if (!settings.remindersEnabled) return
        val id = ReminderRules.notificationId(reminder.eventId, reminder.alertId)
        val open = NotificationIntents.openReminder(context, id, reminder)
        val notification = NotificationCompat.Builder(context, Channels.REMINDERS)
            .setSmallIcon(R.drawable.ic_stat_mail)
            .setContentTitle(reminderTitle(reminder))
            .setCategory(NotificationCompat.CATEGORY_REMINDER)
            .setPriority(NotificationCompat.PRIORITY_HIGH)
            .setAutoCancel(true)
            .setContentIntent(open)
            .addAction(0, context.getString(R.string.action_snooze), snoozeIntent(id, reminder))
            .addAction(0, context.getString(R.string.action_open), open)
            .apply { reminderText(reminder)?.let { setContentText(it) } }
            .build()
        manager.notify(id, notification)
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

    private fun mailNotification(n: MailNotification, group: String, silent: Boolean): Notification {
        val builder = NotificationCompat.Builder(context, Channels.MAIL)
            .setSmallIcon(R.drawable.ic_stat_mail)
            .setContentTitle(n.from)
            .setContentText(n.subject)
            .setStyle(NotificationCompat.BigTextStyle().bigText(n.preview.ifBlank { n.subject }))
            .setWhen(whenMillis(n.receivedAt, System.currentTimeMillis()))
            .setShowWhen(true)
            .setCategory(NotificationCompat.CATEGORY_EMAIL)
            .setGroup(group)
            // The channel has a sound; a silent posting must not use it. For a child of a group,
            // "alert the summary only" mutes it, and the summary itself never alerts.
            .setGroupAlertBehavior(if (silent) NotificationCompat.GROUP_ALERT_SUMMARY else NotificationCompat.GROUP_ALERT_CHILDREN)
            .setSilent(silent)
            .setOnlyAlertOnce(true)
            .setAutoCancel(true)
            .setContentIntent(NotificationIntents.openApp(context, n.androidId, "message", n.messageId, EXTRA_MESSAGE_ID))
            .setDeleteIntent(NotificationIntents.forMessage(context, ActionReceiver.ACTION_SWIPED, n.androidId, n.messageId))
            .addAction(0, context.getString(R.string.action_archive), forMessage(ActionReceiver.ACTION_ARCHIVE, n))
            .addAction(0, context.getString(R.string.action_mark_read), forMessage(ActionReceiver.ACTION_MARK_READ, n))
            .addAction(0, context.getString(R.string.action_reply), NotificationIntents.reply(context, n.androidId, n.messageId))
        return builder.build()
    }

    private fun forMessage(action: String, n: MailNotification) =
        NotificationIntents.forMessage(context, action, n.androidId, n.messageId)

    private fun updateSummary(count: Int, group: String) {
        if (count <= 0) {
            manager.cancel(SUMMARY_ID)
            return
        }
        val summary = NotificationCompat.Builder(context, Channels.MAIL)
            .setSmallIcon(R.drawable.ic_stat_mail)
            .setContentTitle(context.getString(R.string.app_name))
            .setContentText(context.resources.getQuantityString(R.plurals.mail_summary, count, count))
            .setCategory(NotificationCompat.CATEGORY_EMAIL)
            .setGroup(group)
            .setGroupSummary(true)
            .setGroupAlertBehavior(NotificationCompat.GROUP_ALERT_CHILDREN)
            .setOnlyAlertOnce(true)
            .setAutoCancel(true)
            .setContentIntent(NotificationIntents.openAppPlain(context, SUMMARY_ID))
            .build()
        manager.notify(SUMMARY_ID, summary)
    }

    private fun snoozeIntent(id: Int, reminder: Reminder) =
        NotificationIntents.forReminder(context, ActionReceiver.ACTION_SNOOZE, id, reminder)

    private fun groupKey(): String = GROUP_PREFIX + (Background.core(context).account()?.id ?: "none")

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

        /** Ids below this are the app's own; the core keeps mail ids at or above it. */
        private const val FIRST_MAIL_ID = 1000
        internal const val SUMMARY_ID = 1
        internal const val SERVICE_ID = 2
        internal const val TEST_ID = 3
        internal const val CHECK_ID = 4
        private const val GROUP_PREFIX = "ovh.kweb.mail.mail."
        private val LOCK = Any()
    }
}
