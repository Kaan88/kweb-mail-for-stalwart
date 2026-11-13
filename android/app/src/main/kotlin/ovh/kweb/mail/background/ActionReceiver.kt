package ovh.kweb.mail.background

import android.app.AlarmManager
import android.app.NotificationManager
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.os.SystemClock
import android.util.Log
import kotlin.concurrent.thread
import uniffi.kweb_core.Action
import uniffi.kweb_core.ConnectionStatus

/**
 * Handles the buttons and swipes of notifications: Archive, Mark read, a swipe, and the
 * reminder's Snooze (plus the alarm that ends a snooze). Not exported.
 *
 * The work runs on its own thread under `goAsync()`, because the core's functions block.
 */
class ActionReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val app = context.applicationContext
        val pending = goAsync()
        thread(name = "kweb-action") {
            try {
                handle(app, intent)
            } catch (e: Exception) {
                Log.e(Background.TAG, "action ${intent.action} failed", e)
            } finally {
                pending.finish()
            }
        }
    }

    private fun handle(context: Context, intent: Intent) {
        when (intent.action) {
            ACTION_ARCHIVE -> actOnMessage(context, intent) { Action.Archive(it) }
            ACTION_MARK_READ -> actOnMessage(context, intent) { Action.MarkRead(it) }
            ACTION_SWIPED -> intent.getStringExtra(EXTRA_MESSAGE_ID)?.let { Background.core(context).notificationSwiped(it) }
            ACTION_SNOOZE -> ReminderExtras.read(intent)?.let { snooze(context, it) }
            ACTION_REMINDER_DUE -> ReminderExtras.read(intent)?.let { showSnoozed(context, it) }
        }
    }

    /**
     * Removes the notification first so the tap feels instant, then hands the action to the core,
     * which applies it to the cache at once. If the live connection is not up, nobody would send it
     * to the server soon, so a check is queued for that.
     */
    private fun actOnMessage(context: Context, intent: Intent, action: (String) -> Action) {
        val messageId = intent.getStringExtra(EXTRA_MESSAGE_ID) ?: return
        Notifier(context).cancelMail(intent.getIntExtra(EXTRA_ANDROID_ID, 0))
        val core = Background.core(context)
        core.submit(action(messageId))
        if (core.status() !is ConnectionStatus.Live) CheckScheduler.checkSoon(context)
    }

    /**
     * Removes the reminder and shows it again in [SNOOZE_MILLIS]. The alarm is inexact and allowed
     * while idle, so no exact-alarm permission is needed; it can be a few minutes late. It does not
     * survive a restart of the phone.
     */
    private fun snooze(context: Context, reminder: uniffi.kweb_core.Reminder) {
        val id = ReminderRules.notificationId(reminder.eventId, reminder.alertId)
        context.getSystemService(NotificationManager::class.java).cancel(id)
        val due = NotificationIntents.forReminder(context, ACTION_REMINDER_DUE, id, reminder)
        context.getSystemService(AlarmManager::class.java)
            .setAndAllowWhileIdle(AlarmManager.ELAPSED_REALTIME_WAKEUP, SystemClock.elapsedRealtime() + SNOOZE_MILLIS, due)
    }

    /** The snooze ended. After a sign-out an old alarm may still fire; it shows nothing then. */
    private fun showSnoozed(context: Context, reminder: uniffi.kweb_core.Reminder) {
        if (Background.core(context).account() != null) Notifier(context).showReminder(reminder)
    }

    companion object {
        const val ACTION_ARCHIVE = "ovh.kweb.mail.action.ARCHIVE"
        const val ACTION_MARK_READ = "ovh.kweb.mail.action.MARK_READ"
        const val ACTION_SWIPED = "ovh.kweb.mail.action.SWIPED"
        const val ACTION_SNOOZE = "ovh.kweb.mail.action.SNOOZE"
        const val ACTION_REMINDER_DUE = "ovh.kweb.mail.action.REMINDER_DUE"
        const val EXTRA_MESSAGE_ID = "ovh.kweb.mail.extra.ACTION_MESSAGE_ID"
        const val EXTRA_ANDROID_ID = "ovh.kweb.mail.extra.ACTION_ANDROID_ID"
        private const val SNOOZE_MILLIS = 10 * 60 * 1000L
    }
}
