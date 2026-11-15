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

    /**
     * Schedules the check for work owed to the server that no connection will send (decision 4): [inMs] from now.
     * Up to a minute ahead it is an expedited one-off that starts at once and waits inside the worker; further
     * ahead a plain one-off with that delay. [ownSlot]: the slot of the due worker that asks, so a worker never
     * enqueues its own name with REPLACE (that would cancel itself); it uses the other one. At the end of a worker
     * ([fromWorker]) work owed *now* means the pass could not do it: the retry backs off and ends ([DueRules.plan],
     * [attempt] retries so far), unless the work only appeared while the pass ran ([fresh]).
     */
    fun checkAt(context: Context, inMs: Long, fromWorker: Boolean = false, ownSlot: Int? = null, attempt: Int = 0, fresh: Boolean = false) {
        val plan = DueRules.plan(inMs, fromWorker, attempt, fresh) ?: return
        val slot = ownSlot?.let { DueRules.otherSlot(it) } ?: 0
        val builder = OneTimeWorkRequestBuilder<CheckWorker>()
            .setConstraints(needsNetwork)
        val wait = when (plan) {
            is DuePlan.Delayed -> plan.delayMs
            is DuePlan.Expedited -> plan.waitMs
        }
        val retry = fromWorker && inMs <= 0L && plan is DuePlan.Delayed
        val data = workDataOf(
            CheckWorker.KEY_ONE_OFF to true,
            CheckWorker.KEY_SLOT to slot,
            CheckWorker.KEY_NOT_BEFORE to System.currentTimeMillis() + wait,
            CheckWorker.KEY_ATTEMPT to (if (retry) attempt + 1 else 0),
        )
        val request = when (plan) {
            is DuePlan.Expedited -> builder.setInputData(data).setExpedited(OutOfQuotaPolicy.RUN_AS_NON_EXPEDITED_WORK_REQUEST).build()
            is DuePlan.Delayed -> builder.setInputData(data).setInitialDelay(plan.delayMs, TimeUnit.MILLISECONDS).build()
        }
        WorkManager.getInstance(context).enqueueUniqueWork(DueRules.workName(slot), ExistingWorkPolicy.REPLACE, request)
    }

    /** Cancels every scheduled and waiting check (sign-out). */
    fun cancelAll(context: Context) {
        WorkManager.getInstance(context).run {
            cancelUniqueWork(PERIODIC)
            cancelUniqueWork(ONE_OFF)
            cancelUniqueWork(ONE_OFF_AFTER_BOOT)
            for (slot in 0 until DueRules.SLOTS) cancelUniqueWork(DueRules.workName(slot))
        }
    }
}
