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
import ovh.kweb.mail.accounts.AccountRecord
import ovh.kweb.mail.accounts.Accounts
import uniffi.kweb_core.Shade
import uniffi.kweb_core.Exception as CoreException

/**
 * One pass of the notification rule without the live connection, for every account: the 15-minute safety check, and
 * the one-off checks after a boot or after an action that the connection could not send.
 * Also restarts the service, so a service Android killed comes back within 15 minutes.
 *
 * The core hands the plan to the service's listener as well as returning it here. That is safe:
 * [Notifier.apply] is idempotent and never re-alerts for a notification already on screen.
 */
class CheckWorker(context: Context, params: WorkerParameters) : CoroutineWorker(context, params) {
    override suspend fun doWork(): Result = withContext(Dispatchers.IO) {
        val records = Accounts.load(applicationContext)
        if (records.isEmpty()) return@withContext Result.success()
        try {
            runChecks(records)
        } finally {
            PushService.start(applicationContext)
        }
    }

    /**
     * Checks every account in order. A failure of one is recorded in that account's [Settings] and does not skip
     * the next. A one-off is retried (with backoff, while the network trouble lasts) when any account had a
     * temporary failure; the periodic job runs again in 15 minutes anyway.
     */
    private fun runChecks(records: List<AccountRecord>): Result {
        val notifier = Notifier(applicationContext)
        val shadeCleared = inputData.getBoolean(KEY_SHADE_CLEARED, false)
        val failures = CheckRound.runAll(records) { record ->
            val settings = Settings.of(applicationContext, record)
            try {
                val core = Accounts.session(record.id)?.core ?: return@runAll
                if (core.account() == null) return@runAll
                val shade = if (shadeCleared) Shade.Cleared else Shade.Ids(notifier.activeMailIds(record))
                notifier.apply(record, core.checkOnce(shade))
                settings.recordCheck(System.currentTimeMillis(), null)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                settings.recordCheck(System.currentTimeMillis(), describe(e))
                throw e
            }
        }
        val retry = failures.any { it.error is CoreException.Temporary } &&
            inputData.getBoolean(KEY_ONE_OFF, false) && runAttemptCount < MAX_ATTEMPTS
        return if (retry) Result.retry() else Result.success()
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
