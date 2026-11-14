package ovh.kweb.mail.background

import android.app.AlarmManager
import android.app.NotificationManager
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.os.SystemClock
import android.util.Log
import kotlin.concurrent.thread
import ovh.kweb.mail.accounts.AccountRecord
import ovh.kweb.mail.accounts.Accounts
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
        val extra = intent.getStringExtra(Notifier.EXTRA_ACCOUNT)
        val record = IntentAccount.resolve(extra, Accounts.load(context))
        if (record == null) return dropOrphan(context, intent, extra)
        when (intent.action) {
            ACTION_ARCHIVE -> actOnMessage(context, record, intent) { Action.Archive(it) }
            ACTION_MARK_READ -> actOnMessage(context, record, intent) { Action.MarkRead(it) }
            ACTION_SWIPED -> intent.getStringExtra(EXTRA_MESSAGE_ID)?.let { Accounts.session(record.id)?.core?.notificationSwiped(it) }
            ACTION_SNOOZE -> ReminderExtras.read(intent)?.let { snooze(context, record, it) }
            ACTION_REMINDER_DUE -> ReminderExtras.read(intent)?.let { showSnoozed(context, record, it) }
        }
    }

    /**
     * An intent for an account that is gone (removed since the notification was posted), or one without an
     * account when no migrated account exists: nothing is done, and its notification is taken away. The id of
     * an unknown account is the notification's tag.
     */
    private fun dropOrphan(context: Context, intent: Intent, tag: String?) {
        Log.w(Background.TAG, "dropped ${intent.action} for an unknown account")
        val androidId = intent.getIntExtra(EXTRA_ANDROID_ID, 0)
        if (androidId >= NotificationKeys.FIRST_MAIL_ID) {
            context.getSystemService(NotificationManager::class.java).cancel(tag, androidId)
        }
    }

    /**
     * Removes the notification first so the tap feels instant, then hands the action to the core,
     * which applies it to the cache at once. If the live connection is not up, nobody would send it
     * to the server soon, so a check is queued for that.
     */
    private fun actOnMessage(context: Context, record: AccountRecord, intent: Intent, action: (String) -> Action) {
        val messageId = intent.getStringExtra(EXTRA_MESSAGE_ID) ?: return
        Notifier(context).cancelMail(record, intent.getIntExtra(EXTRA_ANDROID_ID, 0))
        val core = Accounts.session(record.id)?.core ?: return
        core.submit(action(messageId))
        if (core.status() !is ConnectionStatus.Live) CheckScheduler.checkSoon(context)
    }

    /**
     * Removes the reminder and shows it again in [SNOOZE_MILLIS]. The alarm is inexact and allowed
     * while idle, so no exact-alarm permission is needed; it can be a few minutes late. It does not
     * survive a restart of the phone.
     */
    private fun snooze(context: Context, record: AccountRecord, reminder: uniffi.kweb_core.Reminder) {
        val id = ReminderRules.notificationId(reminder.eventId, reminder.alertId)
        context.getSystemService(NotificationManager::class.java).cancel(NotificationKeys.tag(record), id)
        val due = NotificationIntents.forReminder(context, record, ACTION_REMINDER_DUE, id, reminder)
        context.getSystemService(AlarmManager::class.java)
            .setAndAllowWhileIdle(AlarmManager.ELAPSED_REALTIME_WAKEUP, SystemClock.elapsedRealtime() + SNOOZE_MILLIS, due)
    }

    /** The snooze ended. After the account was removed an old alarm may still fire; it shows nothing then. */
    private fun showSnoozed(context: Context, record: AccountRecord, reminder: uniffi.kweb_core.Reminder) {
        if (Accounts.session(record.id)?.core?.account() != null) Notifier(context).showReminder(record, reminder)
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
