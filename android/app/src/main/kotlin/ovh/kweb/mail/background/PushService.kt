package ovh.kweb.mail.background

import android.app.ForegroundServiceStartNotAllowedException
import android.app.Notification
import android.app.NotificationManager
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import android.util.Log
import androidx.core.app.NotificationCompat
import androidx.core.content.ContextCompat
import java.util.concurrent.Executors
import kotlin.concurrent.thread
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch
import ovh.kweb.mail.R
import ovh.kweb.mail.accounts.AccountLabels
import ovh.kweb.mail.accounts.Accounts
import ovh.kweb.mail.accounts.ConnectionSummary
import uniffi.kweb_core.ConnectionStatus

/**
 * The foreground service that holds the live connections to the servers: one per account, each in its own
 * `Core` with its own loop and backoff, so an account that fails or needs a new sign-in cannot stall the others.
 *
 * It shows the silent "Connected" entry (Connection channel), whose text follows the accounts' statuses, and
 * registers each session's listener with its core. Start and stop it with [start] and [stop]; [start] can be
 * called from anywhere at any time, and after an account was added it simply picks the new one up. With no
 * account the service ends itself.
 *
 * Foreground service type `specialUse`: Android 15 limits `dataSync` services to six hours a
 * day, and `specialUse` is not among the types a BOOT_COMPLETED receiver may not start on 15.
 */
class PushService : Service() {
    private val log by lazy { ServiceLog.of(this) }
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    private var collecting: Job? = null

    /** Everything that may block (opening the core, starting the connection) runs here, in order. */
    private val worker = Executors.newSingleThreadExecutor { runnable -> Thread(runnable, "kweb-service") }

    private val renderLock = Any()
    @Volatile private var destroyed = false

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        Channels.ensure(this)
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        // Android gives five seconds after startForegroundService; do this before anything slow.
        if (!enterForeground()) {
            stopSelf()
            return START_NOT_STICKY
        }
        worker.execute { begin() }
        return START_STICKY
    }

    override fun onDestroy() {
        synchronized(renderLock) { destroyed = true }
        scope.cancel()
        worker.shutdown()
        val cores = Accounts.openSessions().map { it.core }
        // stop() blocks until the connection is down, so never on the main thread.
        // A service started right after this one waits for it ([begin]), or it would have its
        // fresh connections stopped by this call.
        pendingStop = thread(name = "kweb-service-stop") {
            for (core in cores) {
                try {
                    core.stop()
                } catch (e: Exception) {
                    Log.e(Background.TAG, "stopping the core failed", e)
                }
            }
        }
        super.onDestroy()
    }

    private fun enterForeground(): Boolean = try {
        val entry = entry(ServiceEntryPlan.Single(ConnectionStatus.Connecting))
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            startForeground(Notifier.SERVICE_ID, entry, ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE)
        } else {
            // Before 14 there is no specialUse type; the manifest's type is used.
            startForeground(Notifier.SERVICE_ID, entry)
        }
        log.clearStartFailure()
        true
    } catch (e: Exception) {
        recordStartFailure(this, e)
        false
    }

    /** Runs on [worker]: registers each account's listener, which starts its connection. One that fails does not stop the others. */
    private fun begin() {
        try {
            pendingStop?.join()
            val records = Accounts.load(this)
            if (records.isEmpty()) {
                stopSelf()
                return
            }
            var started = 0
            var lastError: Exception? = null
            for (record in records) {
                try {
                    val session = Accounts.session(record.id) ?: continue
                    session.core.start(session.listener)
                    session.publishStatus(session.core.status())
                    started++
                } catch (e: Exception) {
                    Log.e(Background.TAG, "starting the connection of an account failed", e)
                    lastError = e
                }
            }
            if (started == 0) {
                val e = lastError
                log.recordStartFailure(System.currentTimeMillis(), "The core did not start: ${e?.message ?: e?.javaClass?.simpleName ?: "no account"}")
                stopSelf()
                return
            }
            follow()
            // A start command on a running service put "Connecting" back; a status that did not change emits nothing.
            renderCurrent()
        } catch (e: Exception) {
            Log.e(Background.TAG, "starting the connection failed", e)
            log.recordStartFailure(System.currentTimeMillis(), "The core did not start: ${e.message ?: e.javaClass.simpleName}")
            stopSelf()
        }
    }

    /** Keeps the entry in step with the accounts' statuses, once per service. With no account left the service ends. */
    private fun follow() {
        if (collecting != null) return
        collecting = scope.launch {
            Accounts.connection.collect { summary ->
                if (summary.items.isEmpty()) stopSelf() else render(summary)
            }
        }
    }

    private fun renderCurrent() {
        val summary = Accounts.connection.value
        if (summary.items.isNotEmpty()) render(summary)
    }

    private fun render(summary: ConnectionSummary) {
        val labels = AccountLabels.of(summary.items.map { it.record })
        render(ServiceEntryRules.plan(summary.items.map { labels.getValue(it.record.id) to it.status }))
    }

    /** Updates the "Connected" entry; does nothing once the service is gone, so it cannot come back as a plain notification. */
    private fun render(plan: ServiceEntryPlan) = synchronized(renderLock) {
        if (!destroyed) getSystemService(NotificationManager::class.java).notify(Notifier.SERVICE_ID, entry(plan))
    }

    private fun entry(plan: ServiceEntryPlan): Notification =
        NotificationCompat.Builder(this, Channels.CONNECTION)
            .setSmallIcon(R.drawable.ic_stat_mail)
            .setContentTitle(getString(R.string.app_name))
            .setContentText(statusText(plan))
            .apply { details(plan)?.let { setStyle(NotificationCompat.BigTextStyle().bigText(it)) } }
            .setCategory(NotificationCompat.CATEGORY_SERVICE)
            .setPriority(NotificationCompat.PRIORITY_MIN)
            .setVisibility(NotificationCompat.VISIBILITY_SECRET)
            .setOngoing(true)
            .setSilent(true)
            .setOnlyAlertOnce(true)
            .setShowWhen(false)
            .setForegroundServiceBehavior(NotificationCompat.FOREGROUND_SERVICE_IMMEDIATE)
            .setContentIntent(NotificationIntents.openAppPlain(this, Notifier.SERVICE_ID))
            .build()

    private fun statusText(plan: ServiceEntryPlan): String = when (plan) {
        is ServiceEntryPlan.Single -> when (val status = plan.status) {
            is ConnectionStatus.Live -> getString(R.string.connection_connected)
            is ConnectionStatus.Offline -> getString(R.string.connection_offline, status.reason)
            ConnectionStatus.SignInNeeded -> getString(R.string.connection_sign_in_needed)
            ConnectionStatus.Connecting, ConnectionStatus.SignedOut -> getString(R.string.connection_connecting)
        }
        is ServiceEntryPlan.AllConnected -> getString(R.string.connection_all_accounts, plan.count)
        is ServiceEntryPlan.Partial -> getString(R.string.connection_some_accounts, plan.connected, plan.total)
    }

    /** One line per account in trouble, for the expanded entry; null when there is nothing more to say. */
    private fun details(plan: ServiceEntryPlan): String? {
        if (plan !is ServiceEntryPlan.Partial || plan.troubles.isEmpty()) return null
        val lines = plan.troubles.map { t ->
            when (val status = t.status) {
                is ConnectionStatus.Offline -> getString(R.string.connection_account_offline, t.label, status.reason)
                else -> getString(R.string.connection_account_sign_in_needed, t.label)
            }
        }
        return (listOf(statusText(plan)) + lines).joinToString("\n")
    }

    companion object {
        /** The `core.stop()` calls of the service that was destroyed last, so the next one can wait for them. */
        @Volatile private var pendingStop: Thread? = null

        /**
         * Starts the service (or tells it to look again). Safe from any thread, any process state
         * and any Android version: it never throws. From 12 on Android refuses to start a
         * foreground service from the background unless the app is exempt from battery
         * optimisation; then this returns false and [ServiceLog.lastStartFailure] says why, for the
         * Delivery check screen. A true result means Android accepted the request; the failure
         * is cleared once the service is really in the foreground.
         */
        fun start(context: Context): Boolean {
            val app = context.applicationContext
            return try {
                ContextCompat.startForegroundService(app, Intent(app, PushService::class.java))
                true
            } catch (e: Exception) {
                recordStartFailure(app, e)
                false
            }
        }

        /** Stops the service and, through its `onDestroy`, the live connection. */
        fun stop(context: Context) {
            val app = context.applicationContext
            app.stopService(Intent(app, PushService::class.java))
        }

        private fun recordStartFailure(context: Context, e: Exception) {
            Log.w(Background.TAG, "the service could not be started", e)
            ServiceLog.of(context).recordStartFailure(System.currentTimeMillis(), describe(e))
        }

        private fun describe(e: Exception): String =
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S && e is ForegroundServiceStartNotAllowedException) {
                "Android does not allow starting the service from the background (battery optimisation is on for the app)"
            } else {
                "${e.javaClass.simpleName}: ${e.message ?: "no details"}"
            }
    }
}
