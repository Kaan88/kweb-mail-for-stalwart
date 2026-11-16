package ovh.kweb.mail.background

import android.content.Context
import android.content.SharedPreferences
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import ovh.kweb.mail.accounts.AccountPaths
import ovh.kweb.mail.accounts.AccountRecord

/** The result of the last background check. [error] is null when it went well. */
data class CheckRecord(val atMillis: Long, val error: String?)

/** Why [PushService] could not be started the last time, and when. */
data class StartFailure(val atMillis: Long, val text: String)

/** The few typed reads and writes [Settings] needs, so tests can use a map instead of Android. */
interface KeyValues {
    fun getBoolean(key: String, default: Boolean): Boolean
    fun putBoolean(key: String, value: Boolean)
    fun putString(key: String, value: String)
    fun putLong(key: String, value: Long)
    fun getLong(key: String, default: Long): Long
    fun getString(key: String): String?

    /** Writes a time and a text in one step; a null text removes the text. */
    fun putLongAndString(longKey: String, long: Long, stringKey: String, string: String?)
    fun remove(vararg keys: String)
}

/** [KeyValues] over a private SharedPreferences file. */
class PrefsKeyValues(private val prefs: SharedPreferences) : KeyValues {
    override fun getBoolean(key: String, default: Boolean) = prefs.getBoolean(key, default)
    override fun putBoolean(key: String, value: Boolean) = prefs.edit().putBoolean(key, value).apply()
    override fun putString(key: String, value: String) = prefs.edit().putString(key, value).apply()
    override fun putLong(key: String, value: Long) = prefs.edit().putLong(key, value).apply()
    override fun getLong(key: String, default: Long) = prefs.getLong(key, default)
    override fun getString(key: String): String? = prefs.getString(key, null)

    /** One edit, so a reader never sees the time of one record with the text of another. */
    override fun putLongAndString(longKey: String, long: Long, stringKey: String, string: String?) {
        prefs.edit().apply {
            putLong(longKey, long)
            if (string == null) remove(stringKey) else putString(stringKey, string)
        }.apply()
    }

    override fun remove(vararg keys: String) {
        prefs.edit().apply { keys.forEach { remove(it) } }.apply()
    }
}

/**
 * What the user chose to show for one account, and what the Delivery check screen reports about it.
 *
 * Get it with [Settings.of]. Each account has its own (the three switches and the last check); what belongs
 * to the device (why the service could not start) is in [ServiceLog]. Reads are cheap and work from any thread. [changes] ticks after
 * every write (also from the service and the workers), so a screen can reload when it moves.
 */
class Settings(private val values: KeyValues) {
    private val ticks = MutableStateFlow(0L)

    /** Increases after every write; collect it to know when to read the settings again. */
    val changes: StateFlow<Long> get() = ticks

    /** "New mail" (Inbox and the folders the user switched on). Off: no mail notification is posted. */
    var mailEnabled: Boolean
        get() = values.getBoolean(MAIL, true)
        set(value) = write { putBoolean(MAIL, value) }

    /** "Calendar and task reminders". Off: no reminder is shown. */
    var remindersEnabled: Boolean
        get() = values.getBoolean(REMINDERS, true)
        set(value) = write { putBoolean(REMINDERS, value) }

    /** "Remove when read elsewhere". Off: cancellations the core asks for are not applied. */
    var removeWhenReadElsewhere: Boolean
        get() = values.getBoolean(REMOVE_ELSEWHERE, true)
        set(value) = write { putBoolean(REMOVE_ELSEWHERE, value) }

    /**
     * The buttons of this account's mail notifications, in order ([NotificationActionRules]). Nothing stored (the
     * user never chose, or the install is older than the setting) reads as the default.
     */
    var notificationActions: List<MailAction>
        get() = NotificationActionRules.parse(values.getString(NOTIFICATION_ACTIONS))
        set(value) = write { putString(NOTIFICATION_ACTIONS, NotificationActionRules.encode(value)) }

    /**
     * How much of a mail this account's notifications say ([PreviewRules]). Nothing stored reads as the full text;
     * it applies to notifications posted from then on.
     */
    var previewChoice: PreviewChoice
        get() = PreviewChoice.parse(values.getString(PREVIEW))
        set(value) = write { putString(PREVIEW, value.key) }

    /** The device calendar is on: this account's own calendars are mirrored into the phone's calendar (see `devcal`). */
    var deviceCalendar: Boolean
        get() = values.getBoolean(DEVICE_CALENDAR, false)
        set(value) = write { putBoolean(DEVICE_CALENDAR, value) }

    /** The last run of the device calendar sync: when, and what it left ([ovh.kweb.mail.devcal.DeviceRun.encode]). */
    val deviceCalendarRun: Pair<Long, String>?
        get() = values.getLong(DEVICE_CALENDAR_AT, 0L).takeIf { it > 0 }?.let { at -> values.getString(DEVICE_CALENDAR_RESULT)?.let { at to it } }

    fun recordDeviceCalendarRun(atMillis: Long, result: String) = write { putLongAndString(DEVICE_CALENDAR_AT, atMillis, DEVICE_CALENDAR_RESULT, result) }

    /** Back to "never chose": the default, also a later version's. */
    fun resetNotificationActions() = write { remove(NOTIFICATION_ACTIONS) }

    /**
     * The last known answer about this account's admin key (never the key): whether the server accepted one and whether
     * it opens the dashboard. Read by the drawer, which makes no call; written whenever the access is read or changes.
     */
    val adminKeyAccepted: Boolean get() = values.getBoolean(ADMIN_KEY, false)
    val adminMayDashboard: Boolean get() = values.getBoolean(ADMIN_DASHBOARD, false)

    fun rememberAdminAccess(keyAccepted: Boolean, mayDashboard: Boolean) {
        if (keyAccepted == adminKeyAccepted && (mayDashboard && keyAccepted) == adminMayDashboard) return
        write {
            putBoolean(ADMIN_KEY, keyAccepted)
            putBoolean(ADMIN_DASHBOARD, keyAccepted && mayDashboard)
        }
    }

    /** The last background check (every 15 minutes, or one after an action or a boot). */
    val lastCheck: CheckRecord?
        get() = values.getLong(CHECK_AT, 0L).takeIf { it > 0 }?.let { CheckRecord(it, values.getString(CHECK_ERROR)) }

    fun recordCheck(atMillis: Long, error: String?) = write { putLongAndString(CHECK_AT, atMillis, CHECK_ERROR, error) }

    /** How this account's news reaches the phone. Written to [DeliveryMode.Google] only once the registration is verified. */
    var deliveryMode: DeliveryMode
        get() = DeliveryMode.parse(values.getString(DELIVERY_MODE))
        set(value) = write { putString(DELIVERY_MODE, value.key) }

    /** True while a switch to Google or a new registration is under way (a process that died leaves it true). */
    var pushSetup: Boolean
        get() = values.getBoolean(PUSH_SETUP, false)
        set(value) = write { putBoolean(PUSH_SETUP, value) }

    /** The `wp:` subtype of this account's registration: how a message finds its account. */
    val pushSubtype: String? get() = values.getString(PUSH_SUBTYPE)

    /** The server key the registration was made for (public; needed to unregister without asking the server). */
    val pushSender: String? get() = values.getString(PUSH_SENDER)

    /** The token Play services gave, null before the first registration. */
    val pushToken: String? get() = values.getString(PUSH_TOKEN)
    val pushTokenAt: Long get() = values.getLong(PUSH_TOKEN_AT, 0L)

    /** The app's version code at the last registration. */
    val pushAppVersion: Long get() = values.getLong(PUSH_APP_VERSION, 0L)

    /** Verification tries used (0 to 3). */
    var pushTries: Int
        get() = values.getLong(PUSH_TRIES, 0L).toInt()
        set(value) = write { putLong(PUSH_TRIES, value.toLong()) }

    /** Failures of Play services in a row while registering again (see [ModeRules.fallsBack]). */
    var pushFailures: Int
        get() = values.getLong(PUSH_FAILURES, 0L).toInt()
        set(value) = write { putLong(PUSH_FAILURES, value.toLong()) }

    fun recordPushRegistration(subtype: String, sender: String, token: String, atMillis: Long, appVersion: Long) = write {
        putString(PUSH_SUBTYPE, subtype)
        putString(PUSH_SENDER, sender)
        putString(PUSH_TOKEN, token)
        putLong(PUSH_TOKEN_AT, atMillis)
        putLong(PUSH_APP_VERSION, appVersion)
    }

    /** Why the account is in connection mode although Google was wanted, until the next switch on. */
    val pushFallback: PushFallback?
        get() = FallbackReason.parse(values.getString(PUSH_FALLBACK))?.let {
            PushFallback(it, values.getLong(PUSH_FALLBACK_AT, 0L), values.getString(PUSH_FALLBACK_DETAIL))
        }

    fun recordFallback(reason: FallbackReason, atMillis: Long, detail: String?) = write {
        putString(PUSH_FALLBACK, reason.key)
        putLong(PUSH_FALLBACK_AT, atMillis)
        if (detail == null) remove(PUSH_FALLBACK_DETAIL) else putString(PUSH_FALLBACK_DETAIL, detail)
    }

    fun clearFallback() = write { remove(PUSH_FALLBACK, PUSH_FALLBACK_AT, PUSH_FALLBACK_DETAIL) }

    /** Forgets the registration (not the mode and not the fallback reason). */
    fun clearPushRegistration() = write {
        remove(PUSH_SETUP, PUSH_SUBTYPE, PUSH_SENDER, PUSH_TOKEN, PUSH_TOKEN_AT, PUSH_APP_VERSION, PUSH_TRIES, PUSH_FAILURES)
    }

    /**
     * Takes the three switches of [old] (the settings an older, single-account install left behind), and its
     * choice of notification buttons if it made one.
     */
    fun takeSwitchesFrom(old: Settings) {
        mailEnabled = old.mailEnabled
        remindersEnabled = old.remindersEnabled
        removeWhenReadElsewhere = old.removeWhenReadElsewhere
        old.values.getString(NOTIFICATION_ACTIONS)?.let { stored -> write { putString(NOTIFICATION_ACTIONS, stored) } }
        old.values.getString(PREVIEW)?.let { stored -> write { putString(PREVIEW, stored) } }
    }

    /** Forgets the bookkeeping (not the switches). Used when the user removes the account. */
    fun clearBookkeeping() = write { remove(CHECK_AT, CHECK_ERROR) }

    /** Set when the account was removed: its file is deleted, and nothing still running may write it back. */
    @Volatile private var retired = false

    internal fun retire() {
        retired = true
    }

    private fun write(block: KeyValues.() -> Unit) {
        if (retired) return
        values.block()
        ticks.value = ticks.value + 1
    }

    companion object {
        private const val MAIL = "show_mail"
        private const val REMINDERS = "show_reminders"
        private const val REMOVE_ELSEWHERE = "remove_when_read_elsewhere"
        private const val NOTIFICATION_ACTIONS = "notification_actions"
        private const val PREVIEW = "notification_preview"
        private const val ADMIN_KEY = "admin_key_accepted"
        private const val ADMIN_DASHBOARD = "admin_may_dashboard"
        private const val DEVICE_CALENDAR = "device_calendar"
        private const val DEVICE_CALENDAR_AT = "device_calendar_at"
        private const val DEVICE_CALENDAR_RESULT = "device_calendar_result"
        private const val CHECK_AT = "check_at"
        private const val CHECK_ERROR = "check_error"
        private const val DELIVERY_MODE = "delivery_mode"
        private const val PUSH_SETUP = "push_setup"
        private const val PUSH_SUBTYPE = "push_subtype"
        private const val PUSH_SENDER = "push_sender"
        private const val PUSH_TOKEN = "push_token"
        private const val PUSH_TOKEN_AT = "push_token_at"
        private const val PUSH_APP_VERSION = "push_app_version"
        private const val PUSH_TRIES = "push_tries"
        private const val PUSH_FAILURES = "push_failures"
        private const val PUSH_FALLBACK = "push_fallback"
        private const val PUSH_FALLBACK_AT = "push_fallback_at"
        private const val PUSH_FALLBACK_DETAIL = "push_fallback_detail"

        private val instances = HashMap<String, Settings>()

        /** The ids of accounts removed in this process (an id is random and never given out again). */
        private val gone = HashSet<String>()

        /** The one [Settings] of [record] in this process. The migrated account keeps the file `settings`. */
        fun of(context: Context, record: AccountRecord): Settings = synchronized(instances) {
            val open = {
                val file = AccountPaths.settingsPrefs(record.id, record.legacy)
                Settings(PrefsKeyValues(context.applicationContext.getSharedPreferences(file, Context.MODE_PRIVATE)))
            }
            // Work that was in flight when the account was removed still asks: it reads defaults and writes nothing.
            if (record.id in gone) open().also { it.retire() } else instances.getOrPut(record.id, open)
        }

        /** True once [forget] was called for this account: nothing of it may be shown or written any more. */
        fun isGone(record: AccountRecord): Boolean = synchronized(instances) { record.id in gone }

        /** The file the single-account app used (`settings`), read to carry its switches to the first account added after a sign-out. */
        fun legacy(context: Context): Settings =
            Settings(PrefsKeyValues(context.applicationContext.getSharedPreferences(AccountPaths.settingsPrefs("", true), Context.MODE_PRIVATE)))

        /**
         * Drops the instance of a removed account (its file is deleted with the rest of its storage). Call it before
         * the files are deleted: from then on a pass or a registration of that account that is still running cannot
         * write the file back, and [Notifier] shows nothing more for it.
         */
        fun forget(record: AccountRecord) {
            synchronized(instances) {
                gone.add(record.id)
                instances.remove(record.id)?.retire()
            }
        }
    }
}

/**
 * What belongs to the device, not to an account: why the foreground service could not be started the last
 * time. Its own prefs file (`service`). Get it with [ServiceLog.of]; [changes] ticks after every write.
 */
class ServiceLog(private val values: KeyValues) {
    private val ticks = MutableStateFlow(0L)

    val changes: StateFlow<Long> get() = ticks

    /** The last time the service could not be started; null once it has started since. */
    val lastStartFailure: StartFailure?
        get() = values.getLong(FAILURE_AT, 0L).takeIf { it > 0 }
            ?.let { at -> values.getString(FAILURE_TEXT)?.let { StartFailure(at, it) } }

    fun recordStartFailure(atMillis: Long, text: String) {
        values.putLongAndString(FAILURE_AT, atMillis, FAILURE_TEXT, text)
        ticks.value = ticks.value + 1
    }

    fun clearStartFailure() {
        values.remove(FAILURE_AT, FAILURE_TEXT)
        ticks.value = ticks.value + 1
    }

    companion object {
        private const val FILE = "service"
        private const val FAILURE_AT = "start_failure_at"
        private const val FAILURE_TEXT = "start_failure_text"

        @Volatile private var instance: ServiceLog? = null

        fun of(context: Context): ServiceLog = instance ?: synchronized(this) {
            instance ?: ServiceLog(
                PrefsKeyValues(context.applicationContext.getSharedPreferences(FILE, Context.MODE_PRIVATE)),
            ).also { instance = it }
        }
    }
}
