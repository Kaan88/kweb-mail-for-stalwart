package ovh.kweb.mail.background

import android.app.NotificationChannel
import android.app.NotificationChannelGroup
import android.app.NotificationManager
import android.content.Context
import android.content.Intent
import android.provider.Settings
import ovh.kweb.mail.R
import ovh.kweb.mail.accounts.AccountLabels
import ovh.kweb.mail.accounts.AccountRecord
import ovh.kweb.mail.accounts.Accounts

/**
 * The notification channels: Reminders and Connection for the device, and for each account two mail channels
 * ([ChannelRules]). [ensure] and [ensureAccount] are safe to call as often as you like.
 *
 * What was decided about the old single `mail` channel: it stays, unchanged in id, as the loud channel of the
 * account that was already there (the legacy account), so a sound or importance the user set in Android's
 * settings is kept. Only its name and description are refreshed (the one thing Android lets an app change).
 */
object Channels {
    const val MAIL = "mail"
    const val QUIET = "mail_quiet"
    const val REMINDERS = "reminders"
    const val CONNECTION = "connection"

    /**
     * Creates the device's channels and those of every account the app knows. Creating an existing channel
     * changes nothing but its name, so a choice the user made in Android's settings (sound, importance)
     * survives.
     */
    fun ensure(context: Context) {
        val manager = context.getSystemService(NotificationManager::class.java)
        manager.createNotificationChannels(
            listOf(
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
        val records = Accounts.records.value
        if (records.isEmpty()) {
            // Nobody is signed in yet (or the registry is not read): the channel the old version had, so the
            // first account that arrives (the migrated one) finds it as it was.
            ensureMail(context, manager, null, null)
        } else {
            val labels = AccountLabels.of(records)
            records.forEach { ensureMail(context, manager, it, labels[it.id].takeIf { _ -> records.size > 1 }) }
        }
    }

    /** Creates the mail channels of [record] ([label]: its name where several accounts are shown, else null). */
    fun ensureAccount(context: Context, record: AccountRecord, label: String?) {
        ensureMail(context, context.getSystemService(NotificationManager::class.java), record, label)
    }

    /** Removes the channels and the group of a removed account. The legacy account's channel `mail` stays: it holds the user's choice. */
    fun forget(context: Context, record: AccountRecord) {
        if (record.legacy) return
        val manager = context.getSystemService(NotificationManager::class.java)
        manager.deleteNotificationChannel(ChannelRules.mail(record))
        manager.deleteNotificationChannel(ChannelRules.quiet(record))
        ChannelRules.group(record)?.let { manager.deleteNotificationChannelGroup(it) }
    }

    /** The system screen of one channel, for the "Sound and vibration" rows. The channel is made first. */
    fun settingsIntent(context: Context, channelId: String): Intent =
        Intent(Settings.ACTION_CHANNEL_NOTIFICATION_SETTINGS)
            .putExtra(Settings.EXTRA_APP_PACKAGE, context.packageName)
            .putExtra(Settings.EXTRA_CHANNEL_ID, channelId)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)

    private fun ensureMail(context: Context, manager: NotificationManager, record: AccountRecord?, label: String?) {
        val loud = context.getString(R.string.channel_mail)
        val quiet = context.getString(R.string.channel_mail_quiet)
        if (record == null) {
            manager.createNotificationChannel(loudChannel(context, Channels.MAIL, loud, null))
            manager.createNotificationChannel(quietChannel(context, Channels.QUIET, quiet, null))
            return
        }
        val group = ChannelRules.group(record)
        if (group != null) {
            manager.createNotificationChannelGroup(NotificationChannelGroup(group, label ?: record.username))
        }
        manager.createNotificationChannel(loudChannel(context, ChannelRules.mail(record), ChannelRules.name(loud, record, label), group))
        manager.createNotificationChannel(quietChannel(context, ChannelRules.quiet(record), ChannelRules.name(quiet, record, label), group))
    }

    private fun loudChannel(context: Context, id: String, name: String, group: String?) =
        NotificationChannel(id, name, NotificationManager.IMPORTANCE_DEFAULT).apply {
            description = context.getString(R.string.channel_mail_description)
            group?.let { setGroup(it) }
        }

    private fun quietChannel(context: Context, id: String, name: String, group: String?) =
        NotificationChannel(id, name, NotificationManager.IMPORTANCE_LOW).apply {
            description = context.getString(R.string.channel_mail_quiet_description)
            setSound(null, null)
            enableVibration(false)
            group?.let { setGroup(it) }
        }
}
