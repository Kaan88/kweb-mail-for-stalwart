package ovh.kweb.mail.devcal

import android.accounts.Account
import android.app.NotificationManager
import android.app.Service
import android.content.AbstractThreadedSyncAdapter
import android.content.ContentProviderClient
import android.content.Context
import android.content.Intent
import android.content.SyncResult
import android.os.Bundle
import android.os.IBinder
import android.util.Log
import androidx.core.app.NotificationCompat
import java.util.TimeZone
import ovh.kweb.mail.R
import ovh.kweb.mail.accounts.AccountLabels
import ovh.kweb.mail.accounts.AccountRecord
import ovh.kweb.mail.accounts.Accounts
import ovh.kweb.mail.background.Background
import ovh.kweb.mail.background.Channels
import ovh.kweb.mail.background.NotificationIntents
import ovh.kweb.mail.background.Settings
import uniffi.kweb_core.Exception as CoreException

/**
 * The sync adapter of the phone's calendar for one Android account. It runs in the app's process and does one thing: it asks
 * the core of the app account it is for to sync, with the provider as its port. The adapter never talks to the server and
 * holds no secret.
 */
class DeviceCalendarSyncAdapter(context: Context) : AbstractThreadedSyncAdapter(context, false, false) {
    override fun onPerformSync(account: Account, extras: Bundle, authority: String, provider: ContentProviderClient, syncResult: SyncResult) {
        val context = context
        val id = DeviceCalendarAccounts.appAccountId(context, account)
        if (id == null) {
            // An account that is not for any app account (made by hand): nothing to sync it with.
            syncResult.stats.numAuthExceptions++
            return
        }
        Accounts.load(context)
        val record = Accounts.records.value.firstOrNull { it.id == id }
        if (record == null) {
            DeviceCalendarAccounts.remove(context, id)
            return
        }
        val settings = Settings.of(context, record)
        // Switched off while this was waiting: nothing to bring back.
        if (!settings.deviceCalendar) return
        val finish = { run: DeviceRun -> finish(context, record, settings, run) }
        if (!DeviceCalendarController.hasPermission(context)) {
            finish(DeviceRun(DeviceRun.Kind.Denied))
            syncResult.databaseError = true
            return
        }
        try {
            val session = Accounts.session(id)
            if (session == null) {
                finish(DeviceRun(DeviceRun.Kind.Failed))
                return
            }
            val report = session.core.deviceCalendarSync(CalendarContractProvider(provider, account), id, TimeZone.getDefault().id)
            val run = DeviceRun.of(report)
            syncResult.stats.numEntries += report.written.toLong()
            syncResult.stats.numUpdates += report.uploaded.toLong() + report.created.toLong() + report.destroyed.toLong()
            if (!run.good) syncResult.databaseError = true
            finish(run)
        } catch (e: CoreException.SignInNeeded) {
            syncResult.stats.numAuthExceptions++
            finish(DeviceRun(DeviceRun.Kind.SignIn))
        } catch (e: CoreException.Temporary) {
            // The server could not be reached: the framework tries again later; the user is not told for one miss.
            syncResult.stats.numIoExceptions++
            finish(DeviceRun(DeviceRun.Kind.Failed))
        } catch (e: CoreException) {
            // The class of the error only: its text may be the provider's or the server's own words.
            Log.w(Background.TAG, "the device calendar sync failed (${e.javaClass.simpleName})")
            syncResult.databaseError = true
            finish(DeviceRun(DeviceRun.Kind.Other))
        } catch (e: RuntimeException) {
            Log.w(Background.TAG, "the device calendar sync failed (${e.javaClass.simpleName})")
            syncResult.databaseError = true
            finish(DeviceRun(DeviceRun.Kind.Other))
        }
    }

    private fun finish(context: Context, record: AccountRecord, settings: Settings, run: DeviceRun) {
        settings.recordDeviceCalendarRun(System.currentTimeMillis(), run.encode())
        if (run.good || run.kind == DeviceRun.Kind.Failed) {
            if (run.good) DeviceCalendarNotices.clear(context, record)
        } else {
            DeviceCalendarNotices.problem(context, record)
        }
    }
}

/** Binds the system to the sync adapter. */
class DeviceCalendarSyncService : Service() {
    override fun onCreate() {
        super.onCreate()
        synchronized(lock) { if (adapter == null) adapter = DeviceCalendarSyncAdapter(applicationContext) }
    }

    override fun onBind(intent: Intent?): IBinder? = adapter?.syncAdapterBinder

    private companion object {
        private val lock = Any()
        private var adapter: DeviceCalendarSyncAdapter? = null
    }
}

/** The one notification per account that says the sync has stopped; it opens the Device calendar screen. */
object DeviceCalendarNotices {
    private const val ID = 6

    private fun tag(record: AccountRecord) = "devcal:${record.id}"

    fun problem(context: Context, record: AccountRecord) {
        val records = Accounts.records.value
        val label = AccountLabels.forNotice(record, records, Accounts.names.value) ?: record.username
        val notification = NotificationCompat.Builder(context, Channels.SYNC_PROBLEMS)
            .setSmallIcon(R.drawable.ic_stat_mail)
            .setContentTitle(context.getString(R.string.devcal_notice_title))
            .setContentText(context.getString(R.string.devcal_notice_text, label))
            .setAutoCancel(true)
            .setOnlyAlertOnce(true)
            .setContentIntent(NotificationIntents.openDeviceCalendar(context, record, ID))
            .build()
        context.getSystemService(NotificationManager::class.java).notify(tag(record), ID, notification)
    }

    fun clear(context: Context, record: AccountRecord) {
        context.getSystemService(NotificationManager::class.java).cancel(tag(record), ID)
    }
}
