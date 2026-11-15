package ovh.kweb.mail.background

import android.content.Context
import android.util.Log
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import ovh.kweb.mail.accounts.AccountSession
import ovh.kweb.mail.accounts.Accounts

/**
 * The live connection of an account in Google mode, only while the app is on screen (decision 4): writing, Undo send
 * and live updates then behave exactly as with the permanent connection. It starts on foreground and stops
 * [ModeRules.LIVE_STOP_DELAY_MS] after the app left, unless it came back. The foreground service owns the
 * connections of connection-mode accounts only, this object those of Google-mode accounts; neither stops the other's.
 *
 * Whatever the user did that is still owed to the server when no connection runs is found by asking each
 * account's `workWaiting()` and scheduled as one pass ([DueRules], [CheckScheduler.checkAt]): when the app leaves the
 * screen, when the connection is stopped, and at the end of every [CheckWorker] run.
 */
object LiveWindow {
    private val timer = Executors.newSingleThreadScheduledExecutor { r -> Thread(r, "kweb-live-window").apply { isDaemon = true } }

    @Volatile private var app: Context? = null

    private val gate = LiveGate(
        loops = { app?.let { ctx -> googleSessions(ctx).map(::loopOf) } ?: emptyList() },
        later = { ms, task ->
            val future = timer.schedule({ task() }, ms, TimeUnit.MILLISECONDS)
            return@LiveGate { future.cancel(false); Unit }
        },
        owed = { app?.let { scheduleDue(it) } },
    )

    private fun loopOf(session: AccountSession) = object : LiveLoop {
        override fun start() {
            try {
                session.core.start(session.listener)
                session.publishStatus(session.core.status())
            } catch (e: Exception) {
                Log.w(Background.TAG, "starting the live connection of an account failed", e)
            }
        }

        override fun stop() {
            try {
                session.core.stop()
                session.publishStatus(session.core.status())
            } catch (e: Exception) {
                Log.w(Background.TAG, "stopping the live connection of an account failed", e)
            }
        }
    }

    /** True while the app is on screen. */
    val onScreen: Boolean get() = gate.onScreen

    /** `MainActivity.onStart`, on the main thread, at once: the flag follows the activity in order. */
    fun screenOn(context: Context) {
        app = context.applicationContext
        gate.screenOn()
    }

    /** `MainActivity.onStop`, on the main thread, at once: the loops stop 30 seconds later, and owed work is asked for. */
    fun screenOff(context: Context) {
        app = context.applicationContext
        gate.screenOff()
    }

    /** Call from `Background.onAppForeground` (off the main thread): starts the loop of every Google-mode account. */
    fun onForeground(context: Context) {
        app = context.applicationContext
        gate.startLoops()
    }

    /**
     * An account just became a Google-mode one (the switch ended, perhaps after the app was left): with the app away
     * its loop must not go on without a service, and what is owed is scheduled.
     */
    fun settle(context: Context) {
        app = context.applicationContext
        timer.execute { gate.stopNow() }
    }

    private fun googleSessions(context: Context): List<AccountSession> =
        Accounts.sessions().filter { it.settings.deliveryMode == DeliveryMode.Google }

    /**
     * Asks every Google-mode account what is owed and schedules the earliest. Does nothing while the app is on
     * screen (its loop does the work). [fromWorker]: asked at the end of a pass; [ownSlot]: the slot of the due
     * worker that asks, so it never replaces itself; [attempt]: the retries so far ([DueRules.plan]); [skip]: accounts
     * whose pass said "sign in", for which nothing is scheduled; [fresh]: see [DueRules.plan]. Blocks briefly (a
     * cache read per account).
     */
    fun scheduleDue(
        context: Context,
        fromWorker: Boolean = false,
        ownSlot: Int? = null,
        attempt: Int = 0,
        skip: Set<String> = emptySet(),
        fresh: Boolean = false,
    ) {
        if (gate.onScreen) return
        try {
            val waitings = googleSessions(context).filter { it.record.id !in skip }.mapNotNull {
                try {
                    it.core.workWaiting()
                } catch (e: Exception) {
                    Log.w(Background.TAG, "asking for owed work failed", e)
                    null
                }
            }
            val earliest = DueRules.earliest(waitings) ?: return
            CheckScheduler.checkAt(context, earliest, fromWorker, ownSlot, attempt, fresh)
        } catch (e: Exception) {
            Log.w(Background.TAG, "scheduling owed work failed", e)
        }
    }
}
