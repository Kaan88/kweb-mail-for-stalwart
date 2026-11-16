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
    const val NOTICES = "notices"
    const val CONNECTION = "connection"
    const val DOWNLOADS = "downloads"
    const val SYNC_PROBLEMS = "sync_problems"
    const val UPDATES = "updates"

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
                NotificationChannel(NOTICES, context.getString(R.string.channel_notices), NotificationManager.IMPORTANCE_DEFAULT)
                    .apply { description = context.getString(R.string.channel_notices_description) },
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
        manager.createNotificationChannel(
            NotificationChannel(DOWNLOADS, context.getString(R.string.channel_downloads), NotificationManager.IMPORTANCE_LOW)
                .apply {
                    description = context.getString(R.string.channel_downloads_description)
                    setShowBadge(false)
                },
        )
        manager.createNotificationChannel(
            NotificationChannel(SYNC_PROBLEMS, context.getString(R.string.channel_sync_problems), NotificationManager.IMPORTANCE_DEFAULT)
                .apply { description = context.getString(R.string.channel_sync_problems_description) },
        )
        manager.createNotificationChannel(
            NotificationChannel(UPDATES, context.getString(R.string.channel_updates), NotificationManager.IMPORTANCE_LOW)
                .apply {
                    description = context.getString(R.string.channel_updates_description)
                    setShowBadge(false)
                },
        )
        val records = Accounts.records.value
        if (records.isEmpty()) {
            // Nobody is signed in yet (or the registry is not read): the channel the old version had, so the
            // first account that arrives (the migrated one) finds it as it was.
            ensureMail(context, manager, null, null)
        } else {
            val names = Accounts.names.value
            records.forEach { ensureMail(context, manager, it, AccountLabels.forNotice(it, records, names)) }
        }
    }

    /** Creates the mail channels of [record] ([label]: its name where several accounts are shown, else null). */
    fun ensureAccount(context: Context, record: AccountRecord, label: String?) {
        ensureMail(context, context.getSystemService(NotificationManager::class.java), record, label)
    }

    /**
     * Makes the channel of one receiving address of [record] ([key] in lower case): the quiet one when [quiet], else
     * the loud one, named after the address. Made when first needed; an existing one keeps what the user set in
     * Android's settings.
     */
    fun ensureAddress(context: Context, record: AccountRecord, label: String?, key: String, quiet: Boolean) {
        val manager = context.getSystemService(NotificationManager::class.java)
        val group = ChannelRules.group(record)
        if (group != null) manager.createNotificationChannelGroup(NotificationChannelGroup(group, label ?: record.username))
        val base = if (quiet) context.getString(R.string.channel_address_quiet, key) else key
        val name = ChannelRules.name(base, record, label)
        manager.createNotificationChannel(
            if (quiet) quietChannel(context, ChannelRules.addressQuiet(record, key), name, group)
            else loudChannel(context, ChannelRules.address(record, key), name, group),
        )
    }

    /**
     * Deletes the address channels of [record] that are not in [wanted] (an address that was set to "Do not
     * notify", changed between loud and quiet, or is no longer one of the account's). A channel a notification
     * on screen is still in is left for the next time: deleting it would take that notification away with it. A
     * deleted channel that is made again later (same id) comes back with the settings it had.
     */
    fun reconcileAddresses(context: Context, record: AccountRecord, wanted: Set<String>) {
        val manager = context.getSystemService(NotificationManager::class.java)
        val inUse = try {
            manager.activeNotifications.map { it.notification.channelId }.toSet()
        } catch (_: RuntimeException) {
            return
        }
        for (channel in manager.notificationChannels) {
            val id = channel.id
            if (ChannelRules.addressOf(record, id) != null && id !in wanted && id !in inUse) manager.deleteNotificationChannel(id)
        }
    }

    /** Removes the channels and the group of a removed account. The legacy account's channel `mail` stays: it holds the user's choice. */
    fun forget(context: Context, record: AccountRecord) {
        val manager = context.getSystemService(NotificationManager::class.java)
        manager.notificationChannels.filter { ChannelRules.addressOf(record, it.id) != null }
            .forEach { manager.deleteNotificationChannel(it.id) }
        if (record.legacy) return
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
