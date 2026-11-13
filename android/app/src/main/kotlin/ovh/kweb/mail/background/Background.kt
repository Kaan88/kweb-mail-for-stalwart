package ovh.kweb.mail.background

import android.content.Context
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import ovh.kweb.mail.KwebApp
import uniffi.kweb_core.ConnectionStatus
import uniffi.kweb_core.Core

/**
 * The one place the screens use to reach the background half of the app.
 *
 * - [core]: the process-wide `Core`. Its functions block (see core/src/ffi.rs): call them from
 *   `Dispatchers.IO`, except `account()`, `status()` and `poke()`.
 * - [status] and [cacheChanges]: what the service's listener reports, for the UI to observe.
 * - [onAppForeground], [afterSignIn], [afterSignOut]: call them at the moments their names say.
 * - Elsewhere in this package: [PushService] (`start`, `stop`), [CheckScheduler] (periodic and
 *   one-off checks), [Notifier] (`sendTest`, `activeMailIds`), [Settings] (the switches and the
 *   bookkeeping of the Delivery check), [Notifier.EXTRA_MESSAGE_ID] and
 *   [Notifier.EXTRA_EVENT_ID] (the extras of the intent a notification tap sends to MainActivity;
 *   handle them in both `onCreate` and `onNewIntent`, and mark that mail read when it is opened).
 */
object Background {
    /** Tag of every log line of the background half. */
    const val TAG = "KwebMail"

    private val statusFlow = MutableStateFlow<ConnectionStatus>(ConnectionStatus.SignedOut)
    private val cacheFlow = MutableStateFlow(0L)

    /**
     * The state of the live connection, as last reported by the core. Fed by the service's
     * listener; [refreshStatus] and [onAppForeground] bring it up to date when no service runs.
     */
    val status: StateFlow<ConnectionStatus> get() = statusFlow

    /**
     * Counts every "the cache changed" call of the core. The value means nothing; collect it and
     * reload the lists on screen whenever it changes.
     */
    val cacheChanges: StateFlow<Long> get() = cacheFlow

    /** The `Core` of this process, created on first use. */
    fun core(context: Context): Core = (context.applicationContext as KwebApp).core

    /** Sets [status] from the core's own answer. Does not block. */
    fun refreshStatus(context: Context) = publishStatus(core(context).status())

    /**
     * Call when the app comes to the foreground (`onStart`), signed in or not. It is the one
     * moment Android always allows a foreground service to start, so it also brings the
     * service back after Android killed it. Asks the live connection for a pass now.
     */
    fun onAppForeground(context: Context) {
        val core = core(context)
        if (core.account() == null) return publishStatus(core.status())
        PushService.start(context)
        CheckScheduler.schedulePeriodic(context)
        core.poke()
        publishStatus(core.status())
    }

    /**
     * Call right after `finishSignIn()` succeeded: starts the service and schedules the
     * 15-minute check.
     */
    fun afterSignIn(context: Context) {
        Settings.of(context).clearStartFailure()
        CheckScheduler.schedulePeriodic(context)
        PushService.start(context)
        publishStatus(core(context).status())
    }

    /**
     * Call right after `signOut()` returned: stops the service, cancels every scheduled check and
     * removes all notifications and the Delivery check bookkeeping. The switches are kept.
     */
    fun afterSignOut(context: Context) {
        PushService.stop(context)
        CheckScheduler.cancelAll(context)
        Notifier(context).cancelAll()
        Settings.of(context).clearBookkeeping()
        publishStatus(ConnectionStatus.SignedOut)
    }

    internal fun publishStatus(status: ConnectionStatus) {
        statusFlow.value = status
    }

    internal fun publishCacheChanged() {
        cacheFlow.value = cacheFlow.value + 1
    }
}
