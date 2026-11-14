package ovh.kweb.mail.background

import android.app.Activity
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.util.Log
import java.util.concurrent.Executors
import kotlin.concurrent.thread
import ovh.kweb.mail.accounts.Accounts
import uniffi.kweb_core.Shade

/**
 * A Web Push of the server, handed over by Play services (exported, guarded by the permission only Play services
 * holds). The broadcast is ordered and the receiver has 10 seconds: the result is set to OK at once, the work runs
 * on a thread under `goAsync()`, and if the pass is not finished in 8 seconds or fails the expedited one-off
 * check takes over. Which account the message is for follows from its `subtype` extra. A message for no known
 * account is dropped; nothing of it is logged but that it was dropped.
 */
class PushMessageReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action != GoogleRegistration.ACTION_RECEIVE) return
        if (isOrderedBroadcast) resultCode = Activity.RESULT_OK
        val app = context.applicationContext
        val subtype = intent.getStringExtra(GoogleRegistration.EXTRA_SUBTYPE)
        val body = intent.getByteArrayExtra(GoogleRegistration.EXTRA_RAW_DATA)
        val pending = goAsync()
        thread(name = "kweb-push-message") {
            try {
                handle(app, subtype, body)
            } catch (e: Exception) {
                Log.e(Background.TAG, "a push message could not be handled", e)
                runCatching { CheckScheduler.checkSoon(app) }
            } finally {
                pending.finish()
            }
        }
    }

    private fun handle(app: Context, subtype: String?, body: ByteArray?) {
        val records = Accounts.load(app)
        val id = PushMessageHandler.accountFor(subtype, records.map { it.id to Settings.of(app, it).pushSubtype })
        val record = records.firstOrNull { it.id == id }
        if (record == null || body == null) {
            Log.w(Background.TAG, "dropped a push message for no known account")
            return
        }
        val notifier = Notifier(app)
        val settings = Settings.of(app, record)
        val started = android.os.SystemClock.elapsedRealtime()
        val outcome = PushMessageHandler(
            executor = executor,
            receive = {
                val core = Accounts.session(record.id)?.core ?: error("no session")
                core.webPushReceive(body, Shade.Ids(notifier.activeMailIds(record)))
            },
            apply = { plan ->
                notifier.apply(record, plan)
                settings.recordCheck(System.currentTimeMillis(), null)
            },
            showReminder = { notifier.showReminder(record, it) },
            checkSoon = { CheckScheduler.checkSoon(app) },
        ).run()
        // The kind and the time only: nothing of the message, and not whose it is.
        Log.i(Background.TAG, "push message: $outcome after ${android.os.SystemClock.elapsedRealtime() - started} ms")
    }

    companion object {
        /** Core calls may outlast the budget, so they do not run on the receiver's own thread. */
        private val executor = Executors.newCachedThreadPool { r -> Thread(r, "kweb-push-core").apply { isDaemon = true } }
    }
}
