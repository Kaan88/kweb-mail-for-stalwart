package ovh.kweb.mail.accounts

import android.content.Context
import android.util.Log
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import ovh.kweb.mail.background.Background
import ovh.kweb.mail.background.Notifier
import ovh.kweb.mail.background.Settings
import uniffi.kweb_core.ConnectionStatus
import uniffi.kweb_core.Core
import uniffi.kweb_core.Listener
import uniffi.kweb_core.Plan
import uniffi.kweb_core.Reminder

/**
 * One account at work: its [Core] (its own cache file and secrets), its [settings], what its live
 * connection last reported and a counter for "the cache changed".
 *
 * The listener lives here and not in the service: the core keeps a listener beyond `stop()`, so it must not
 * belong to a service instance. It arrives on the core's own threads and must return quickly.
 */
class AccountSession internal constructor(
    record: AccountRecord,
    val core: Core,
    context: Context,
    private val onStatus: (AccountRecord, ConnectionStatus) -> Unit,
    private val onCache: (AccountRecord) -> Unit,
) {
    private val app = context.applicationContext

    @Volatile var record: AccountRecord = record
        internal set

    val settings: Settings get() = Settings.of(app, record)

    private val statusFlow = MutableStateFlow(forRegistered(core.status()))
    private val cacheFlow = MutableStateFlow(0L)

    /** The state of the live connection, as last reported by the core. */
    val status: StateFlow<ConnectionStatus> get() = statusFlow

    /** Counts every "the cache changed" call of the core. The value means nothing; reload when it changes. */
    val cacheChanges: StateFlow<Long> get() = cacheFlow

    internal fun publishStatus(status: ConnectionStatus) {
        val shown = forRegistered(status)
        statusFlow.value = shown
        onStatus(record, shown)
    }

    internal fun publishCacheChanged() {
        cacheFlow.value = cacheFlow.value + 1
        onCache(record)
    }

    /** Hand this to `core.start()`. An exception must not travel back into Rust, so every call is guarded. */
    val listener: Listener = object : Listener {
        override fun status(status: ConnectionStatus) = guarded("status") { publishStatus(status) }
        override fun notifications(plan: Plan) = guarded("notifications") { Notifier(app).apply(record, plan) }
        override fun cacheChanged() = guarded("cacheChanged") { publishCacheChanged() }
        override fun reminder(reminder: Reminder) = guarded("reminder") { Notifier(app).showReminder(record, reminder) }
    }

    private fun guarded(what: String, block: () -> Unit) {
        try {
            block()
        } catch (e: Exception) {
            Log.e(Background.TAG, "listener $what failed", e)
        }
    }
}

/**
 * An account that is in the registry but whose core has no session (a removal that was cut short, or a damaged
 * sign-in entry) is not "signed out": it needs a new sign-in, and the screens offer one (and the removal).
 */
internal fun forRegistered(status: ConnectionStatus): ConnectionStatus =
    if (status == ConnectionStatus.SignedOut) ConnectionStatus.SignInNeeded else status

/** One account and what its connection last said. */
data class AccountStatus(val record: AccountRecord, val status: ConnectionStatus)

/** All accounts with their connection state, in the order of the registry. */
data class ConnectionSummary(val items: List<AccountStatus>)
