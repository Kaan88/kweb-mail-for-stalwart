package ovh.kweb.mail.background

import android.content.Context
import androidx.core.app.NotificationCompat
import androidx.work.CoroutineWorker
import androidx.work.ForegroundInfo
import androidx.work.WorkerParameters
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import ovh.kweb.mail.R
import uniffi.kweb_core.Shade
import uniffi.kweb_core.Exception as CoreException

/**
 * One pass of the notification rule without the live connection: the 15-minute safety check, and
 * the one-off checks after a boot or after an action that the connection could not send.
 * Also restarts the service, so a service Android killed comes back within 15 minutes.
 *
 * The core hands the plan to the service's listener as well as returning it here. That is safe:
 * [Notifier.apply] is idempotent and never re-alerts for a notification already on screen.
 */
class CheckWorker(context: Context, params: WorkerParameters) : CoroutineWorker(context, params) {
    override suspend fun doWork(): Result = withContext(Dispatchers.IO) {
        val core = Background.core(applicationContext)
        if (core.account() == null) return@withContext Result.success()
        try {
            runCheck(core)
        } finally {
            PushService.start(applicationContext)
        }
    }

    private fun runCheck(core: uniffi.kweb_core.Core): Result {
        val settings = Settings.of(applicationContext)
        val notifier = Notifier(applicationContext)
        return try {
            val shade = if (inputData.getBoolean(KEY_SHADE_CLEARED, false)) Shade.Cleared else Shade.Ids(notifier.activeMailIds())
            notifier.apply(core.checkOnce(shade))
            settings.recordCheck(System.currentTimeMillis(), null)
            Result.success()
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            settings.recordCheck(System.currentTimeMillis(), describe(e))
            // A one-off is retried with backoff while the network trouble lasts; the periodic job
            // runs again in 15 minutes anyway.
            val retry = e is CoreException.Temporary && inputData.getBoolean(KEY_ONE_OFF, false) && runAttemptCount < MAX_ATTEMPTS
            if (retry) Result.retry() else Result.success()
        }
    }

    /** Needed by expedited work on Android 11 and older, where it runs as a foreground service. */
    override suspend fun getForegroundInfo(): ForegroundInfo {
        Channels.ensure(applicationContext)
        val notification = NotificationCompat.Builder(applicationContext, Channels.CONNECTION)
            .setSmallIcon(R.drawable.ic_stat_mail)
            .setContentTitle(applicationContext.getString(R.string.app_name))
            .setContentText(applicationContext.getString(R.string.connection_checking))
            .setOngoing(true)
            .setSilent(true)
            .build()
        return ForegroundInfo(Notifier.CHECK_ID, notification)
    }

    private fun describe(e: Exception): String = when (e) {
        is CoreException.SignInNeeded -> e.msg
        is CoreException.Temporary -> e.msg
        is CoreException.Store -> e.msg
        is CoreException.Invalid -> e.msg
        else -> e.message ?: e.javaClass.simpleName
    }

    companion object {
        /** Input flag: the notification shade was emptied behind the app's back (after a boot). */
        const val KEY_SHADE_CLEARED = "shadeCleared"

        /** Input flag: this is a one-off check, which may be retried. */
        const val KEY_ONE_OFF = "oneOff"
        private const val MAX_ATTEMPTS = 5
    }
}
