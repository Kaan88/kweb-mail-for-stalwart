package ovh.kweb.mail.background

import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.net.Uri
import ovh.kweb.mail.MainActivity
import uniffi.kweb_core.Reminder

/**
 * Builds the PendingIntents of notifications. All of them are immutable (nothing outside the app
 * can rewrite them).
 *
 * Two PendingIntents are the same object to Android when their intents match without looking at
 * extras. Every intent here therefore gets its own `data` URI (kind plus message or reminder
 * id), and the request code is the notification id as well, so extras of one message never
 * overwrite those of another.
 */
internal object NotificationIntents {
    private const val SCHEME = "kwebmail"
    private const val FLAGS = PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT

    /** Opens [MainActivity] with one string extra, reusing the running activity (see its onNewIntent). */
    fun openApp(context: Context, requestCode: Int, kind: String, id: String, extraName: String): PendingIntent {
        val intent = Intent(context, MainActivity::class.java)
            .setData(uri(kind, id))
            .putExtra(extraName, id)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP or Intent.FLAG_ACTIVITY_SINGLE_TOP)
        return PendingIntent.getActivity(context, requestCode, intent, FLAGS)
    }

    /**
     * Opens [MainActivity] on a reminder: its event, the occurrence it is for (when it repeats) and the
     * Android id of the notification, so that opening it takes the notification away.
     */
    fun openReminder(context: Context, notificationId: Int, reminder: Reminder): PendingIntent {
        val intent = Intent(context, MainActivity::class.java)
            .setData(uri("reminder", "${reminder.eventId}/${reminder.alertId}/${reminder.recurrenceId.orEmpty()}"))
            .putExtra(Notifier.EXTRA_EVENT_ID, reminder.eventId)
            .putExtra(Notifier.EXTRA_REMINDER_ANDROID_ID, notificationId)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP or Intent.FLAG_ACTIVITY_SINGLE_TOP)
        reminder.recurrenceId?.let { intent.putExtra(Notifier.EXTRA_RECURRENCE_ID, it) }
        return PendingIntent.getActivity(context, notificationId, intent, FLAGS)
    }

    /**
     * The Reply button: opens [MainActivity] on a new answer to [messageId]. It carries the
     * notification's id too, because an action button does not always remove its notification and
     * the activity then has to.
     */
    fun reply(context: Context, androidId: Int, messageId: String): PendingIntent {
        val intent = Intent(context, MainActivity::class.java)
            .setData(uri("reply", messageId))
            .putExtra(Notifier.EXTRA_REPLY_MESSAGE_ID, messageId)
            .putExtra(Notifier.EXTRA_REPLY_ANDROID_ID, androidId)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP or Intent.FLAG_ACTIVITY_SINGLE_TOP)
        return PendingIntent.getActivity(context, androidId, intent, FLAGS)
    }

    /** Opens [MainActivity] without extras (the group summary and the "Connected" entry). */
    fun openAppPlain(context: Context, requestCode: Int): PendingIntent {
        val intent = Intent(context, MainActivity::class.java)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP)
        return PendingIntent.getActivity(context, requestCode, intent, FLAGS)
    }

    /** A broadcast to [ActionReceiver] for a button or a swipe of a mail notification. */
    fun forMessage(context: Context, action: String, androidId: Int, messageId: String): PendingIntent {
        val intent = receiverIntent(context, action, uri(action, messageId))
            .putExtra(ActionReceiver.EXTRA_MESSAGE_ID, messageId)
            .putExtra(ActionReceiver.EXTRA_ANDROID_ID, androidId)
        return PendingIntent.getBroadcast(context, androidId, intent, FLAGS)
    }

    /** A broadcast to [ActionReceiver] that carries a whole reminder (snooze button and the alarm). */
    fun forReminder(context: Context, action: String, notificationId: Int, reminder: Reminder): PendingIntent {
        val intent = receiverIntent(context, action, uri(action, "${reminder.eventId}/${reminder.alertId}"))
        ReminderExtras.put(intent, reminder)
        return PendingIntent.getBroadcast(context, notificationId, intent, FLAGS)
    }

    private fun receiverIntent(context: Context, action: String, data: Uri) =
        Intent(context, ActionReceiver::class.java).setAction(action).setData(data)

    private fun uri(kind: String, id: String): Uri =
        Uri.Builder().scheme(SCHEME).authority("notification").appendPath(kind).appendPath(id).build()
}

/** A [Reminder] carried in an intent, because a snoozed one is shown again by an alarm later. */
internal object ReminderExtras {
    private const val EVENT_ID = "reminder.eventId"
    private const val ALERT_ID = "reminder.alertId"
    private const val IS_TASK = "reminder.isTask"
    private const val TITLE = "reminder.title"
    private const val LOCATION = "reminder.location"
    private const val LOCAL_START = "reminder.localStart"
    private const val TIME_ZONE = "reminder.timeZone"
    private const val ALL_DAY = "reminder.allDay"
    private const val RECURRENCE_ID = "reminder.recurrenceId"

    fun put(intent: Intent, reminder: Reminder): Intent = intent
        .putExtra(EVENT_ID, reminder.eventId)
        .putExtra(ALERT_ID, reminder.alertId)
        .putExtra(IS_TASK, reminder.isTask)
        .putExtra(TITLE, reminder.title)
        .putExtra(LOCATION, reminder.location)
        .putExtra(LOCAL_START, reminder.localStart)
        .putExtra(TIME_ZONE, reminder.timeZone)
        .putExtra(ALL_DAY, reminder.allDay)
        .putExtra(RECURRENCE_ID, reminder.recurrenceId)

    /** The reminder in [intent], or null when it does not carry one. */
    fun read(intent: Intent): Reminder? {
        val eventId = intent.getStringExtra(EVENT_ID) ?: return null
        return Reminder(
            eventId = eventId,
            alertId = intent.getStringExtra(ALERT_ID).orEmpty(),
            isTask = intent.getBooleanExtra(IS_TASK, false),
            title = intent.getStringExtra(TITLE).orEmpty(),
            location = intent.getStringExtra(LOCATION).orEmpty(),
            localStart = intent.getStringExtra(LOCAL_START),
            timeZone = intent.getStringExtra(TIME_ZONE),
            allDay = intent.getBooleanExtra(ALL_DAY, false),
            recurrenceId = intent.getStringExtra(RECURRENCE_ID),
        )
    }
}
