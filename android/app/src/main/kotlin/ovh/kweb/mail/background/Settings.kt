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

    /** The last background check (every 15 minutes, or one after an action or a boot). */
    val lastCheck: CheckRecord?
        get() = values.getLong(CHECK_AT, 0L).takeIf { it > 0 }?.let { CheckRecord(it, values.getString(CHECK_ERROR)) }

    fun recordCheck(atMillis: Long, error: String?) = write { putLongAndString(CHECK_AT, atMillis, CHECK_ERROR, error) }

    /** Takes the three switches of [old] (the settings an older, single-account install left behind). */
    fun takeSwitchesFrom(old: Settings) {
        mailEnabled = old.mailEnabled
        remindersEnabled = old.remindersEnabled
        removeWhenReadElsewhere = old.removeWhenReadElsewhere
    }

    /** Forgets the bookkeeping (not the switches). Used when the user removes the account. */
    fun clearBookkeeping() = write { remove(CHECK_AT, CHECK_ERROR) }

    private fun write(block: KeyValues.() -> Unit) {
        values.block()
        ticks.value = ticks.value + 1
    }

    companion object {
        private const val MAIL = "show_mail"
        private const val REMINDERS = "show_reminders"
        private const val REMOVE_ELSEWHERE = "remove_when_read_elsewhere"
        private const val CHECK_AT = "check_at"
        private const val CHECK_ERROR = "check_error"

        private val instances = HashMap<String, Settings>()

        /** The one [Settings] of [record] in this process. The migrated account keeps the file `settings`. */
        fun of(context: Context, record: AccountRecord): Settings = synchronized(instances) {
            instances.getOrPut(record.id) {
                val file = AccountPaths.settingsPrefs(record.id, record.legacy)
                Settings(PrefsKeyValues(context.applicationContext.getSharedPreferences(file, Context.MODE_PRIVATE)))
            }
        }

        /** The file the single-account app used (`settings`), read to carry its switches to the first account added after a sign-out. */
        fun legacy(context: Context): Settings =
            Settings(PrefsKeyValues(context.applicationContext.getSharedPreferences(AccountPaths.settingsPrefs("", true), Context.MODE_PRIVATE)))

        /** Drops the instance of a removed account (its file is deleted with the rest of its storage). */
        fun forget(record: AccountRecord) {
            synchronized(instances) { instances.remove(record.id) }
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
