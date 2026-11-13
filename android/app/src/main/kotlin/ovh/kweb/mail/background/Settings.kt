package ovh.kweb.mail.background

import android.content.Context
import android.content.SharedPreferences
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow

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
 * What the user chose to show, and what the Delivery check screen reports.
 *
 * Get it with [Settings.of]. Reads are cheap and work from any thread. [changes] ticks after
 * every write (also from the service and the workers), so a screen can reload when it moves.
 */
class Settings(private val values: KeyValues) {
    private val ticks = MutableStateFlow(0L)

    /** Increases after every write; collect it to know when to read the settings again. */
    val changes: StateFlow<Long> get() = ticks

    /** "New mail in Inbox". Off: no mail notification is posted. */
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

    /** The last time the service could not be started; null once it has started since. */
    val lastStartFailure: StartFailure?
        get() = values.getLong(FAILURE_AT, 0L).takeIf { it > 0 }
            ?.let { at -> values.getString(FAILURE_TEXT)?.let { StartFailure(at, it) } }

    fun recordStartFailure(atMillis: Long, text: String) = write { putLongAndString(FAILURE_AT, atMillis, FAILURE_TEXT, text) }

    fun clearStartFailure() = write { remove(FAILURE_AT, FAILURE_TEXT) }

    /** Forgets the bookkeeping (not the switches). Used when the user signs out. */
    fun clearBookkeeping() = write { remove(CHECK_AT, CHECK_ERROR, FAILURE_AT, FAILURE_TEXT) }

    private fun write(block: KeyValues.() -> Unit) {
        values.block()
        ticks.value = ticks.value + 1
    }

    companion object {
        private const val FILE = "settings"
        private const val MAIL = "show_mail"
        private const val REMINDERS = "show_reminders"
        private const val REMOVE_ELSEWHERE = "remove_when_read_elsewhere"
        private const val CHECK_AT = "check_at"
        private const val CHECK_ERROR = "check_error"
        private const val FAILURE_AT = "start_failure_at"
        private const val FAILURE_TEXT = "start_failure_text"

        @Volatile private var instance: Settings? = null

        /** The one [Settings] of the process. */
        fun of(context: Context): Settings = instance ?: synchronized(this) {
            instance ?: Settings(
                PrefsKeyValues(context.applicationContext.getSharedPreferences(FILE, Context.MODE_PRIVATE)),
            ).also { instance = it }
        }
    }
}
