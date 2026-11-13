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
import ovh.kweb.mail.R
import uniffi.kweb_core.ConnectionStatus
import uniffi.kweb_core.Listener
import uniffi.kweb_core.Plan
import uniffi.kweb_core.Reminder

/**
 * The foreground service that holds the live connection to the server.
 *
 * It shows the silent "Connected" entry (Connection channel), whose text follows the core's
 * status, and registers a [Listener] with the core. Start and stop it with [start] and [stop];
 * [start] can be called from anywhere at any time. With no account the service ends itself.
 *
 * Foreground service type `specialUse`: Android 15 limits `dataSync` services to six hours a
 * day, and `specialUse` is not among the types a BOOT_COMPLETED receiver may not start on 15.
 */
class PushService : Service() {
    private val settings by lazy { Settings.of(this) }
    private val notifier by lazy { Notifier(this) }

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
        worker.shutdown()
        val core = Background.core(this)
        // stop() blocks until the connection is down, so never on the main thread.
        // A service started right after this one waits for it ([begin]), or it would have its
        // fresh connection stopped by this call.
        pendingStop = thread(name = "kweb-service-stop") {
            try {
                core.stop()
            } catch (e: Exception) {
                Log.e(Background.TAG, "stopping the core failed", e)
            }
        }
        super.onDestroy()
    }

    private fun enterForeground(): Boolean = try {
        val entry = entry(ConnectionStatus.Connecting)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            startForeground(Notifier.SERVICE_ID, entry, ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE)
        } else {
            // Before 14 there is no specialUse type; the manifest's type is used.
            startForeground(Notifier.SERVICE_ID, entry)
        }
        settings.clearStartFailure()
        true
    } catch (e: Exception) {
        recordStartFailure(this, e)
        false
    }

    /** Runs on [worker]: registers the listener, which starts the connection. */
    private fun begin() {
        try {
            pendingStop?.join()
            val core = Background.core(this)
            if (core.account() == null) {
                stopSelf()
                return
            }
            core.start(listener)
            report(core.status())
        } catch (e: Exception) {
            Log.e(Background.TAG, "starting the connection failed", e)
            settings.recordStartFailure(System.currentTimeMillis(), "The core did not start: ${e.message ?: e.javaClass.simpleName}")
            stopSelf()
        }
    }

    private fun report(status: ConnectionStatus) {
        Background.publishStatus(status)
        render(status)
        if (status is ConnectionStatus.SignedOut) stopSelf()
    }

    /** Updates the "Connected" entry; does nothing once the service is gone, so it cannot come back as a plain notification. */
    private fun render(status: ConnectionStatus) = synchronized(renderLock) {
        if (!destroyed) getSystemService(NotificationManager::class.java).notify(Notifier.SERVICE_ID, entry(status))
    }

    private fun entry(status: ConnectionStatus): Notification =
        NotificationCompat.Builder(this, Channels.CONNECTION)
            .setSmallIcon(R.drawable.ic_stat_mail)
            .setContentTitle(getString(R.string.app_name))
            .setContentText(statusText(status))
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

    private fun statusText(status: ConnectionStatus): String = when (status) {
        is ConnectionStatus.Live -> getString(R.string.connection_connected)
        is ConnectionStatus.Offline -> getString(R.string.connection_offline, status.reason)
        ConnectionStatus.SignInNeeded -> getString(R.string.connection_sign_in_needed)
        ConnectionStatus.Connecting, ConnectionStatus.SignedOut -> getString(R.string.connection_connecting)
    }

    /**
     * The core's callbacks. They arrive on the core's own threads and must return quickly and
     * must not call a blocking `Core` function. An exception must not travel back into Rust.
     */
    private val listener = object : Listener {
        override fun status(status: ConnectionStatus) = guarded("status") { report(status) }
        override fun notifications(plan: Plan) = guarded("notifications") { notifier.apply(plan) }
        override fun cacheChanged() = guarded("cacheChanged") { Background.publishCacheChanged() }
        override fun reminder(reminder: Reminder) = guarded("reminder") { notifier.showReminder(reminder) }
    }

    private fun guarded(what: String, block: () -> Unit) {
        try {
            block()
        } catch (e: Exception) {
            Log.e(Background.TAG, "listener $what failed", e)
        }
    }

    companion object {
        /** The `core.stop()` of the service that was destroyed last, so the next one can wait for it. */
        @Volatile private var pendingStop: Thread? = null

        /**
         * Starts the service (or tells it to look again). Safe from any thread, any process state
         * and any Android version: it never throws. From 12 on Android refuses to start a
         * foreground service from the background unless the app is exempt from battery
         * optimisation; then this returns false and [Settings.lastStartFailure] says why, for the
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
            Settings.of(context).recordStartFailure(System.currentTimeMillis(), describe(e))
        }

        private fun describe(e: Exception): String =
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S && e is ForegroundServiceStartNotAllowedException) {
                "Android does not allow starting the service from the background (battery optimisation is on for the app)"
            } else {
                "${e.javaClass.simpleName}: ${e.message ?: "no details"}"
            }
    }
}
