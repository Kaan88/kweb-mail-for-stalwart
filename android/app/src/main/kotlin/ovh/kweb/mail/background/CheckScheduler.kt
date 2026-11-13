package ovh.kweb.mail.background

import android.content.Context
import androidx.work.Constraints
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.ExistingWorkPolicy
import androidx.work.NetworkType
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.OutOfQuotaPolicy
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.workDataOf
import java.util.concurrent.TimeUnit

/** Schedules [CheckWorker]: the 15-minute safety check and the one-off checks. */
object CheckScheduler {
    private const val PERIODIC = "kweb.check.periodic"
    private const val ONE_OFF = "kweb.check.now"
    private const val ONE_OFF_AFTER_BOOT = "kweb.check.after-boot"

    private val needsNetwork = Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build()

    /**
     * Makes sure the check runs every 15 minutes (the shortest period WorkManager allows). An
     * existing schedule is kept, so calling this at every start and after every sign-in is cheap.
     */
    fun schedulePeriodic(context: Context) {
        val request = PeriodicWorkRequestBuilder<CheckWorker>(15, TimeUnit.MINUTES)
            .setConstraints(needsNetwork)
            .build()
        WorkManager.getInstance(context)
            .enqueueUniquePeriodicWork(PERIODIC, ExistingPeriodicWorkPolicy.KEEP, request)
    }

    /**
     * Runs one check as soon as there is a network, expedited where Android allows it. With
     * [shadeCleared] the check is told the shade was emptied (after a boot) and puts everything
     * back silently. A check of the same kind that is already waiting is kept, not doubled.
     */
    fun checkSoon(context: Context, shadeCleared: Boolean = false) {
        val request = OneTimeWorkRequestBuilder<CheckWorker>()
            .setConstraints(needsNetwork)
            .setInputData(workDataOf(CheckWorker.KEY_SHADE_CLEARED to shadeCleared, CheckWorker.KEY_ONE_OFF to true))
            .setExpedited(OutOfQuotaPolicy.RUN_AS_NON_EXPEDITED_WORK_REQUEST)
            .build()
        WorkManager.getInstance(context)
            .enqueueUniqueWork(if (shadeCleared) ONE_OFF_AFTER_BOOT else ONE_OFF, ExistingWorkPolicy.KEEP, request)
    }

    /** Cancels every scheduled and waiting check (sign-out). */
    fun cancelAll(context: Context) {
        WorkManager.getInstance(context).run {
            cancelUniqueWork(PERIODIC)
            cancelUniqueWork(ONE_OFF)
            cancelUniqueWork(ONE_OFF_AFTER_BOOT)
        }
    }
}
