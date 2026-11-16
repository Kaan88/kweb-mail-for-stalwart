package ovh.kweb.mail.devcal

import android.accounts.Account
import android.content.ContentResolver
import android.content.Context
import android.content.pm.PackageManager
import android.os.Bundle
import android.util.Log
import androidx.core.content.ContextCompat
import java.util.concurrent.ConcurrentHashMap
import ovh.kweb.mail.accounts.AccountRecord
import ovh.kweb.mail.accounts.Accounts
import ovh.kweb.mail.background.Background
import ovh.kweb.mail.background.Settings

/**
 * Switching the device calendar on and off for an account, and asking for a sync of it. Everything here is
 * quick and may be called on any thread except [disable], which may wait for a last upload.
 *
 * A sync is asked for: every hour (the periodic sync of the Android account), when the server says its calendars changed
 * ([onCalendarChanged]), when the app comes to the foreground ([onForeground]), by "Sync now" ([syncNow]), and by Android
 * itself about 30 seconds after the user changes an event in a calendar app (the provider asks for an upload).
 */
object DeviceCalendarController {
    private val lastRequest = ConcurrentHashMap<String, Long>()

    fun hasPermission(context: Context): Boolean =
        DeviceCalendarRules.PERMISSIONS.all { ContextCompat.checkSelfPermission(context, it) == PackageManager.PERMISSION_GRANTED }

    /** The Android account of [record] when the device calendar is on and the account still exists. */
    fun accountOf(context: Context, record: AccountRecord): Account? =
        if (Settings.of(context, record).deviceCalendar) DeviceCalendarAccounts.find(context, record.id) else null

    /** Whether Android's own switch for this account's calendar sync is on (the user may switch it off in Android's settings). */
    fun androidSyncOn(context: Context, record: AccountRecord): Boolean {
        val account = DeviceCalendarAccounts.find(context, record.id) ?: return false
        return ContentResolver.getSyncAutomatically(account, DeviceCalendarRules.AUTHORITY)
    }

    /**
     * Turns the device calendar on: the Android account, the schedule, the first sync. The calendar permission must be
     * granted already (false otherwise, and nothing is done). Blocks briefly.
     */
    fun enable(context: Context, record: AccountRecord): Boolean {
        if (!hasPermission(context)) return false
        val settings = Settings.of(context, record)
        val account = DeviceCalendarAccounts.ensure(context, record, Accounts.records.value)
        settings.deviceCalendar = true
        request(account, manual = true)
        return true
    }

    /**
     * Turns it off: what was changed on the phone and not yet sent goes up first (best effort, a minute at most), then the
     * Android account is removed, which takes its calendars and events from the phone. Blocks.
     */
    fun disable(context: Context, record: AccountRecord): Boolean {
        val settings = Settings.of(context, record)
        // First, so that no queued sync brings anything back while this runs.
        settings.deviceCalendar = false
        DeviceCalendarAccounts.find(context, record.id)?.let { account ->
            ContentResolver.setSyncAutomatically(account, DeviceCalendarRules.AUTHORITY, false)
            ContentResolver.cancelSync(account, DeviceCalendarRules.AUTHORITY)
            if (hasPermission(context)) uploadBeforeRemoval(context, record, account)
        }
        DeviceCalendarNotices.clear(context, record)
        return DeviceCalendarAccounts.remove(context, record.id)
    }

    private fun uploadBeforeRemoval(context: Context, record: AccountRecord, account: Account) {
        try {
            val client = context.contentResolver.acquireContentProviderClient(DeviceCalendarRules.AUTHORITY) ?: return
            try {
                val session = Accounts.session(record.id) ?: return
                session.core.deviceCalendarSync(CalendarContractProvider(client, account), record.id, java.util.TimeZone.getDefault().id)
            } finally {
                client.close()
            }
        } catch (e: Exception) {
            Log.w(Background.TAG, "the last upload of the device calendar did not work (${e.javaClass.simpleName})")
        }
    }

    /** "Sync now". */
    fun syncNow(context: Context, record: AccountRecord) {
        accountOf(context, record)?.let { request(it, manual = true) }
    }

    /** The server's calendars changed (push): a sync, unless one was asked for in the last minute. */
    fun onCalendarChanged(context: Context, record: AccountRecord) {
        val account = accountOf(context, record) ?: return
        val now = System.currentTimeMillis()
        if (!DeviceCalendarRules.mayRequest(lastRequest[record.id] ?: 0L, now)) return
        lastRequest[record.id] = now
        request(account, manual = false)
    }

    /** The app came to the foreground: every account with the device calendar on asks for a sync (at most one a minute). */
    fun onForeground(context: Context) {
        for (record in Accounts.records.value) onCalendarChanged(context, record)
    }

    /**
     * The app account is gone: its Android account and calendars go too. Called whatever the way it was removed; the core
     * of the account is not needed.
     */
    fun accountRemoved(context: Context, record: AccountRecord) {
        lastRequest.remove(record.id)
        try {
            DeviceCalendarNotices.clear(context, record)
            DeviceCalendarAccounts.remove(context, record.id)
        } catch (e: Exception) {
            Log.w(Background.TAG, "removing the device calendar of a removed account failed (${e.javaClass.simpleName})")
        }
    }

    /**
     * Notices an Android account the user removed in Android's settings: the switch goes off and is not turned on again by
     * the app. Cheap; called when the app starts and comes to the foreground.
     */
    fun reconcile(context: Context) {
        for (record in Accounts.records.value) {
            val settings = Settings.of(context, record)
            if (settings.deviceCalendar && DeviceCalendarAccounts.find(context, record.id) == null) {
                settings.deviceCalendar = false
                DeviceCalendarNotices.clear(context, record)
            }
        }
    }

    private fun request(account: Account, manual: Boolean) {
        val extras = Bundle()
        if (manual) {
            extras.putBoolean(ContentResolver.SYNC_EXTRAS_MANUAL, true)
            extras.putBoolean(ContentResolver.SYNC_EXTRAS_EXPEDITED, true)
        }
        ContentResolver.requestSync(account, DeviceCalendarRules.AUTHORITY, extras)
    }
}
