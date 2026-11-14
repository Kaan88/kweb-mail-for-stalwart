package ovh.kweb.mail.background

import android.content.Context
import androidx.core.app.NotificationCompat
import androidx.work.CoroutineWorker
import androidx.work.ForegroundInfo
import androidx.work.WorkerParameters
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
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
    /** The accounts whose pass said "sign in": only a sign-in cures that, so nothing is scheduled for them. */
    private var signInNeeded: Set<String> = emptySet()

    override suspend fun doWork(): Result = withContext(Dispatchers.IO) {
        val records = Accounts.load(applicationContext)
        if (records.isEmpty()) return@withContext Result.success()
        // Work owed at a certain moment (the end of an Undo window): the pass starts then, at most a minute ahead.
        inputData.getLong(KEY_NOT_BEFORE, 0L).takeIf { it > 0 }?.let { delay(DueRules.waitLeft(it, System.currentTimeMillis())) }
        try {
            runChecks(records)
        } finally {
            PushService.start(applicationContext)
            afterRun(records)
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
        signInNeeded = failures.filter { it.error is CoreException.SignInNeeded }.map { it.item.id }.toSet()
        val retry = failures.any { it.error is CoreException.Temporary } &&
            inputData.getBoolean(KEY_ONE_OFF, false) && runAttemptCount < MAX_ATTEMPTS
        return if (retry) Result.retry() else Result.success()
    }

    /**
     * The steps for accounts in Google mode: renew the subscription (or fall back, or register again), then ask
     * what is still owed to the server and schedule it, since no connection runs for them.
     */
    private suspend fun afterRun(records: List<AccountRecord>) {
        for (record in records) GooglePush.renewStep(applicationContext, record)
        val ownSlot = inputData.getInt(KEY_SLOT, -1).takeIf { it >= 0 && inputData.getLong(KEY_NOT_BEFORE, 0L) > 0 }
        val attempt = if (ownSlot != null) inputData.getInt(KEY_ATTEMPT, 0) else 0
        LiveWindow.scheduleDue(applicationContext, fromWorker = true, ownSlot = ownSlot, attempt = attempt, skip = signInNeeded)
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

        /** Input of the due check: its slot ([DueRules.workName]) and the wall-clock time (ms) before which it does nothing. */
        const val KEY_SLOT = "slot"
        const val KEY_NOT_BEFORE = "notBefore"

        /** Input of a retry of the due check: how many retries came before it (they back off, see [DueRules.plan]). */
        const val KEY_ATTEMPT = "attempt"
        private const val MAX_ATTEMPTS = 5
    }
}
