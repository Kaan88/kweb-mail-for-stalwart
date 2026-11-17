package ovh.kweb.mail.background

import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.util.Log
import androidx.core.app.NotificationCompat
import androidx.core.net.toUri
import androidx.work.Constraints
import androidx.work.CoroutineWorker
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.NetworkType
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.WorkerParameters
import java.util.concurrent.TimeUnit
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import ovh.kweb.mail.BuildConfig
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.UiPrefs
import uniffi.kweb_core.UpdateInfo
import uniffi.kweb_core.updateCheck
import uniffi.kweb_core.updateCheckLocal

/** What the update check decides without Android: whether to tell the user and where a link may lead. */
object UpdateRules {
    /** The id of the one quiet notification (below 1000: the app's own). */
    const val NOTIFICATION_ID = 8
    const val NOTIFICATION_TAG = "update"

    /** A newer version is announced once; the same version again is not. */
    fun shouldNotify(newer: Boolean, latest: String, alreadyNotified: String): Boolean =
        newer && latest.isNotEmpty() && latest != alreadyNotified

    /** The release page is opened in the browser only when it is on the project's own GitHub page. */
    fun safePage(url: String): Boolean = url.startsWith("https://github.com/")
}

/** Asks the core whether a newer release is published and remembers the answer. Blocks: run it off the main thread. */
object UpdateChecker {
    fun run(context: Context): Result<UpdateInfo> {
        val prefs = UiPrefs(context)
        return try {
            val origin = UpdateSource.debugOrigin(context)
            val info = if (origin != null) updateCheckLocal(origin, BuildConfig.VERSION_NAME) else updateCheck(BuildConfig.VERSION_NAME)
            prefs.rememberApk(info)
            prefs.updateLastChecked = System.currentTimeMillis()
            prefs.updateLatest = info.latest
            prefs.updatePageUrl = info.pageUrl
            prefs.updateNewer = info.newer
            Result.success(info)
        } catch (e: Exception) {
            // The class only: nothing of the answer goes into the log.
            Log.w(Background.TAG, "the update check failed: ${e.javaClass.simpleName}")
            Result.failure(e)
        }
    }

    /** One quiet notification for a version not announced before; a tap opens the release page in the browser. */
    fun announce(context: Context, info: UpdateInfo) {
        val prefs = UiPrefs(context)
        if (!UpdateRules.shouldNotify(info.newer, info.latest, prefs.updateNotified)) return
        if (!UpdateRules.safePage(info.pageUrl)) return
        val view = Intent(Intent.ACTION_VIEW, info.pageUrl.toUri()).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        val tap = PendingIntent.getActivity(context, UpdateRules.NOTIFICATION_ID, view, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
        val notification = NotificationCompat.Builder(context, Channels.UPDATES)
            .setSmallIcon(R.drawable.ic_stat_mail)
            .setContentTitle(context.getString(R.string.update_notice_title))
            .setContentText(context.getString(R.string.update_notice_text, info.latest))
            .setAutoCancel(true)
            .setOnlyAlertOnce(true)
            .setPriority(NotificationCompat.PRIORITY_LOW)
            .setContentIntent(tap)
            .withNotes(context.getString(R.string.update_notice_text, info.latest), info.notes)
            .build()
        try {
            context.getSystemService(NotificationManager::class.java).notify(UpdateRules.NOTIFICATION_TAG, UpdateRules.NOTIFICATION_ID, notification)
            prefs.updateNotified = info.latest
        } catch (_: SecurityException) {
            // No permission to notify: the About screen still shows it.
        }
    }

    fun pageUri(url: String): Uri? = url.takeIf(UpdateRules::safePage)?.toUri()
}

/** The weekly check (on unless the user switched it off in About). */
class UpdateWorker(context: Context, params: WorkerParameters) : CoroutineWorker(context, params) {
    override suspend fun doWork(): Result = withContext(Dispatchers.IO) {
        Channels.ensure(applicationContext)
        UpdateChecker.run(applicationContext).fold(
            onSuccess = { info ->
                val prefs = UiPrefs(applicationContext)
                if (info.newer && info.apkUrl.isNotEmpty() && UpdateInstallRules.safeVersion(info.latest)) {
                    when {
                        !prefs.updateAuto -> UpdateChecker.announce(applicationContext, info)
                        UpdateInstallRules.mayDownloadNow(true, isUnmetered(applicationContext)) -> {
                            // The download itself is queued on an unmetered network; the "ready" notice follows it.
                            UpdateDownloadWorker.enqueue(applicationContext, info.latest, anyNetwork = false)
                        }
                        else -> {
                            // Metered: announced with a Download button, and queued for the next unmetered network.
                            UpdateNotices.available(applicationContext, info.latest)
                            UpdateDownloadWorker.enqueue(applicationContext, info.latest, anyNetwork = false)
                        }
                    }
                } else {
                    UpdateChecker.announce(applicationContext, info)
                }
                Result.success()
            },
            // A failed week is not retried hard: the next period tries again.
            onFailure = { Result.success() },
        )
    }
}

object UpdateScheduler {
    private const val WEEKLY = "kweb.update.weekly"

    /** Starts or stops the weekly check. An existing schedule is kept. */
    fun apply(context: Context, enabled: Boolean) {
        val manager = WorkManager.getInstance(context)
        if (!enabled) {
            manager.cancelUniqueWork(WEEKLY)
            return
        }
        val request = PeriodicWorkRequestBuilder<UpdateWorker>(7, TimeUnit.DAYS)
            .setConstraints(Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build())
            .build()
        manager.enqueueUniquePeriodicWork(WEEKLY, ExistingPeriodicWorkPolicy.KEEP, request)
    }
}
