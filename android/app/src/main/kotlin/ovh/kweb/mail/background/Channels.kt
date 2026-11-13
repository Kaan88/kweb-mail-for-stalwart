package ovh.kweb.mail.background

import android.app.NotificationChannel
import android.app.NotificationManager
import android.content.Context
import ovh.kweb.mail.R

/** The three notification channels. [ensure] is safe to call as often as you like. */
object Channels {
    const val MAIL = "mail"
    const val REMINDERS = "reminders"
    const val CONNECTION = "connection"

    /**
     * Creates the channels. Creating an existing channel changes nothing, so a choice the user
     * made in Android's settings (sound, importance) survives.
     */
    fun ensure(context: Context) {
        val manager = context.getSystemService(NotificationManager::class.java)
        manager.createNotificationChannels(
            listOf(
                NotificationChannel(MAIL, context.getString(R.string.channel_mail), NotificationManager.IMPORTANCE_DEFAULT)
                    .apply { description = context.getString(R.string.channel_mail_description) },
                NotificationChannel(REMINDERS, context.getString(R.string.channel_reminders), NotificationManager.IMPORTANCE_HIGH)
                    .apply { description = context.getString(R.string.channel_reminders_description) },
                NotificationChannel(CONNECTION, context.getString(R.string.channel_connection), NotificationManager.IMPORTANCE_MIN)
                    .apply {
                        description = context.getString(R.string.channel_connection_description)
                        setShowBadge(false)
                        setSound(null, null)
                        enableVibration(false)
                        enableLights(false)
                    },
            ),
        )
    }
}
