package ovh.kweb.mail.background

import android.content.Context
import kotlinx.coroutines.flow.Flow
import ovh.kweb.mail.accounts.AccountRecord
import ovh.kweb.mail.accounts.Accounts
import uniffi.kweb_core.ConnectionStatus
import uniffi.kweb_core.Core

/**
 * The one place the screens use to reach the background half of the app.
 *
 * - The app runs one `Core` per account (see [ovh.kweb.mail.accounts.Accounts]); a screen gets the one of its
 *   account from its [ovh.kweb.mail.accounts.AccountSession], never from here. Core functions block (see
 *   core/src/ffi.rs): call them from `Dispatchers.IO`, except `account()`, `status()` and `poke()`.
 * - [onAppForeground], [afterAccountAdded], [afterAccountRemoved]: call them at the moments their names say.
 * - Elsewhere in this package: [PushService] (`start`, `stop`), [CheckScheduler] (periodic and
 *   one-off checks), [Notifier] (`sendTest`, `activeMailIds`), [Settings] (the switches and the
 *   bookkeeping of the Delivery check of one account), [ServiceLog] (the device's), [Notifier.EXTRA_MESSAGE_ID]
 *   and [Notifier.EXTRA_EVENT_ID] (the extras of the intent a notification tap sends to MainActivity, next to
 *   [Notifier.EXTRA_ACCOUNT]; handle them in both `onCreate` and `onNewIntent`, and mark that mail read when it
 *   is opened).
 */
object Background {
    /** Tag of every log line of the background half. */
    const val TAG = "KwebMail"

    /** Sets the status of every open account from its core's own answer. Does not block. */
    fun refreshStatus(@Suppress("UNUSED_PARAMETER") context: Context) {
        Accounts.openSessions().forEach { it.publishStatus(it.core.status()) }
    }

    /**
     * Call when the app comes to the foreground (`onStart`), signed in or not. It is the one
     * moment Android always allows a foreground service to start, so it also brings the
     * service back after Android killed it. Asks every live connection for a pass now.
     */
    fun onAppForeground(context: Context) {
        if (Accounts.load(context).isEmpty()) return
        PushService.start(context)
        CheckScheduler.schedulePeriodic(context)
        // Accounts.sessions() leaves out an account that cannot be opened, so it cannot stop the others.
        Accounts.sessions().forEach {
            it.core.poke()
            it.publishStatus(it.core.status())
        }
    }

    /**
     * Call right after an account was added or signed in again: starts the service (which starts a live
     * connection for an account that has none yet) and schedules the 15-minute check.
     */
    fun afterAccountAdded(context: Context) {
        ServiceLog.of(context).clearStartFailure()
        CheckScheduler.schedulePeriodic(context)
        PushService.start(context)
        Notifier(context).refreshSummaries()
        Accounts.sessions().forEach { it.publishStatus(it.core.status()) }
    }

    /**
     * Call right after an account was removed: the notifications that belong to it are gone, the pending
     * snooze alarms of its reminders show nothing (their account is unknown). With no account left it stops
     * the service and cancels every scheduled check; otherwise the service re-renders its entry.
     */
    fun afterAccountRemoved(context: Context, record: AccountRecord) {
        Notifier(context).cancelAccount(record)
        if (Accounts.records.value.isEmpty()) {
            PushService.stop(context)
            CheckScheduler.cancelAll(context)
        } else {
            PushService.start(context)
            Notifier(context).refreshSummaries()
        }
    }
}
