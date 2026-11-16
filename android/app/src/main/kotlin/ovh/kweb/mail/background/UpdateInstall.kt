package ovh.kweb.mail.background

import android.app.Activity
import android.app.AlertDialog
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.pm.PackageInstaller
import android.content.pm.PackageManager
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.net.Uri
import android.os.Bundle
import android.provider.Settings
import android.util.Log
import androidx.core.content.ContextCompat
import androidx.work.Constraints
import androidx.work.CoroutineWorker
import androidx.work.ExistingWorkPolicy
import androidx.work.NetworkType
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.WorkerParameters
import java.io.File
import java.security.MessageDigest
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import ovh.kweb.mail.BuildConfig
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.UiPrefs
import uniffi.kweb_core.DownloadCancel
import uniffi.kweb_core.DownloadListener
import uniffi.kweb_core.UpdateInfo
import uniffi.kweb_core.updateDownload
import uniffi.kweb_core.updateDownloadLocal

/**
 * What decides whether a downloaded file may be installed, without Android: the package must be this app, the
 * version must be higher than the installed one and the signing certificates must be the installed app's. Anything
 * else is refused and the file is deleted.
 */
object UpdateInstallRules {
    enum class Refusal { Unreadable, WrongApp, NotNewer, Signature }

    /** `null`: the file may be installed. The signer sets are SHA-256 hex strings of the signing certificates. */
    fun refusal(
        filePackage: String?,
        fileCode: Long?,
        fileSigners: Set<String>,
        ownPackage: String,
        ownCode: Long,
        ownSigners: Set<String>,
    ): Refusal? = when {
        filePackage == null || fileCode == null -> Refusal.Unreadable
        filePackage != ownPackage -> Refusal.WrongApp
        fileCode <= ownCode -> Refusal.NotNewer
        fileSigners.isEmpty() || ownSigners.isEmpty() || fileSigners != ownSigners -> Refusal.Signature
        else -> null
    }

    /** A version name safe to put in a file name. */
    fun safeVersion(version: String): Boolean = Regex("[0-9A-Za-z.]{1,24}").matches(version)

    fun fileName(version: String): String = "kweb-mail-$version.apk"

    /** A file left in the updates folder is kept only while it is a newer version than the installed one. */
    fun keep(fileCode: Long?, ownCode: Long): Boolean = fileCode != null && fileCode > ownCode

    /** The automatic download: switched on, the weekly check is on, and the network is unmetered. */
    fun mayDownloadNow(auto: Boolean, unmetered: Boolean): Boolean = auto && unmetered

    /** Percent for the progress line; `null` when the total is not known. */
    fun percent(done: Long, total: Long?): Int? = total?.takeIf { it > 0 }?.let { ((done * 100) / it).toInt().coerceIn(0, 100) }
}

/** The downloaded files: `cacheDir/updates/`. */
object UpdateFiles {
    fun dir(context: Context): File = File(context.cacheDir, "updates")

    fun file(context: Context, version: String): File = File(dir(context), UpdateInstallRules.fileName(version))

    private fun sha256(bytes: ByteArray): String = MessageDigest.getInstance("SHA-256").digest(bytes).joinToString("") { "%02x".format(it) }

    /** What the archive at [file] is, or why it may not be installed. `null`: fine. */
    fun inspect(context: Context, file: File): UpdateInstallRules.Refusal? {
        val pm = context.packageManager
        val flags = PackageManager.GET_SIGNING_CERTIFICATES
        val archive = try {
            pm.getPackageArchiveInfo(file.path, flags)
        } catch (_: Exception) {
            null
        }
        val own = try {
            pm.getPackageInfo(context.packageName, flags)
        } catch (_: Exception) {
            return UpdateInstallRules.Refusal.Unreadable
        }
        return UpdateInstallRules.refusal(
            archive?.packageName,
            archive?.longVersionCode,
            signers(archive),
            context.packageName,
            own.longVersionCode,
            signers(own),
        )
    }

    private fun signers(info: android.content.pm.PackageInfo?): Set<String> =
        info?.signingInfo?.apkContentsSigners?.map { sha256(it.toByteArray()) }?.toSet().orEmpty()

    /** Deletes what is not a newer version than the installed one, and every part file. Called at app start. */
    fun sweep(context: Context) {
        val own = try {
            context.packageManager.getPackageInfo(context.packageName, 0).longVersionCode
        } catch (_: Exception) {
            return
        }
        for (f in dir(context).listFiles().orEmpty()) {
            val code = if (f.name.endsWith(".apk")) {
                try {
                    context.packageManager.getPackageArchiveInfo(f.path, 0)?.longVersionCode
                } catch (_: Exception) {
                    null
                }
            } else {
                null
            }
            if (!UpdateInstallRules.keep(code, own)) f.delete()
        }
    }

    /** Removes everything except [keep]. */
    fun clearOthers(context: Context, keep: File?) {
        for (f in dir(context).listFiles().orEmpty()) if (f != keep) f.delete()
    }
}

/** For a debug build only: a local address standing in for the project's release page, read from a file the developer puts there. */
object UpdateSource {
    fun debugOrigin(context: Context): String? {
        if (!BuildConfig.DEBUG) return null
        return try {
            File(context.filesDir, "update_origin.txt").takeIf { it.isFile }?.readText()?.trim()?.takeIf { it.isNotEmpty() }
        } catch (_: Exception) {
            null
        }
    }
}

/** Downloads the release file into [UpdateFiles] and checks it. Blocks: run it off the main thread. */
object UpdateDownloader {
    sealed class Outcome {
        data class Done(val file: File) : Outcome()

        data object Cancelled : Outcome()

        data class Refused(val why: UpdateInstallRules.Refusal) : Outcome()

        data class Failed(val message: String?) : Outcome()
    }

    fun download(context: Context, version: String, url: String, size: Long, sha: String, cancel: DownloadCancel, onProgress: (Long, Long?) -> Unit): Outcome {
        if (!UpdateInstallRules.safeVersion(version) || url.isEmpty()) return Outcome.Failed(null)
        val dir = UpdateFiles.dir(context).also { it.mkdirs() }
        val target = UpdateFiles.file(context, version)
        // A file already here that passes the check is the answer; anything else here is from another version.
        if (target.isFile && UpdateFiles.inspect(context, target) == null) {
            UpdateFiles.clearOthers(context, target)
            return Outcome.Done(target)
        }
        UpdateFiles.clearOthers(context, null)
        val listener = object : DownloadListener {
            override fun progress(done: ULong, total: ULong?) {
                try {
                    onProgress(done.toLong(), total?.toLong())
                } catch (_: Exception) {
                }
            }
        }
        return try {
            val origin = UpdateSource.debugOrigin(context)
            val complete = if (origin != null) {
                updateDownloadLocal(origin, url, target.path, size.toULong(), sha, cancel, listener)
            } else {
                updateDownload(url, target.path, size.toULong(), sha, cancel, listener)
            }
            if (!complete) {
                target.delete()
                return Outcome.Cancelled
            }
            val refusal = UpdateFiles.inspect(context, target)
            if (refusal != null) {
                target.delete()
                Outcome.Refused(refusal)
            } else {
                dir.listFiles()?.forEach { if (it != target) it.delete() }
                Outcome.Done(target)
            }
        } catch (e: uniffi.kweb_core.Exception) {
            target.delete()
            // The core's words are plain and carry nothing of the answer; only the class goes to the log.
            Log.w(Background.TAG, "the update download failed: ${e.javaClass.simpleName}")
            Outcome.Failed((e as? uniffi.kweb_core.Exception.Temporary)?.msg ?: (e as? uniffi.kweb_core.Exception.Invalid)?.msg)
        } catch (e: java.lang.Exception) {
            target.delete()
            Log.w(Background.TAG, "the update download failed: ${e.javaClass.simpleName}")
            Outcome.Failed(null)
        }
    }

    fun refusalText(context: Context, why: UpdateInstallRules.Refusal): String = context.getString(
        when (why) {
            UpdateInstallRules.Refusal.WrongApp -> R.string.update_refused_wrong_app
            UpdateInstallRules.Refusal.NotNewer -> R.string.update_refused_not_newer
            UpdateInstallRules.Refusal.Signature -> R.string.update_refused_signature
            UpdateInstallRules.Refusal.Unreadable -> R.string.update_refused_unreadable
        },
    )
}

/** Whether the active network is unmetered (Wi-Fi, ethernet). */
fun isUnmetered(context: Context): Boolean {
    val cm = context.getSystemService(ConnectivityManager::class.java) ?: return false
    val caps = cm.getNetworkCapabilities(cm.activeNetwork) ?: return false
    return caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_NOT_METERED)
}

/** Remembers what the release's file is, so About and the background download know it after the check. */
fun UiPrefs.rememberApk(info: UpdateInfo) {
    updateApkUrl = info.apkUrl
    updateApkSize = info.apkSize.toLong()
    updateApkSha = info.apkSha256
}

/**
 * The background download of the file of [version]: queued on an unmetered network (or on any network when the user
 * asked for it with the notice's Download button), then one notice "ready to install" whose tap opens
 * [UpdateInstallActivity]. Nothing is installed here.
 */
class UpdateDownloadWorker(context: Context, params: WorkerParameters) : CoroutineWorker(context, params) {
    override suspend fun doWork(): Result = withContext(Dispatchers.IO) {
        val version = inputData.getString(VERSION).orEmpty()
        val prefs = UiPrefs(applicationContext)
        // The release moved on since this was queued: the newer one has its own request.
        if (version.isEmpty() || version != prefs.updateLatest || !prefs.updateNewer) return@withContext Result.success()
        val outcome = UpdateDownloader.download(
            applicationContext,
            version,
            prefs.updateApkUrl,
            prefs.updateApkSize,
            prefs.updateApkSha,
            DownloadCancel(),
        ) { _, _ -> }
        when (outcome) {
            is UpdateDownloader.Outcome.Done -> {
                UpdateNotices.ready(applicationContext, version)
                Result.success()
            }
            is UpdateDownloader.Outcome.Failed -> if (runAttemptCount < 3) Result.retry() else Result.success()
            else -> Result.success()
        }
    }

    companion object {
        const val VERSION = "version"
        private const val UNIQUE = "kweb.update.download"

        /** Queues the download; [anyNetwork] is the user's own request over a metered network. */
        fun enqueue(context: Context, version: String, anyNetwork: Boolean) {
            val request = OneTimeWorkRequestBuilder<UpdateDownloadWorker>()
                .setInputData(androidx.work.workDataOf(VERSION to version))
                .setConstraints(
                    Constraints.Builder()
                        .setRequiredNetworkType(if (anyNetwork) NetworkType.CONNECTED else NetworkType.UNMETERED)
                        .build(),
                )
                .build()
            WorkManager.getInstance(context).enqueueUniqueWork(UNIQUE, if (anyNetwork) ExistingWorkPolicy.REPLACE else ExistingWorkPolicy.KEEP, request)
        }
    }
}

/** The two notices about an update file. Their ids are below 1000, the app's own. */
object UpdateNotices {
    private const val READY_ID = 9
    private const val AVAILABLE_ID = 10

    fun ready(context: Context, version: String) {
        context.getSystemService(android.app.NotificationManager::class.java).cancel(UpdateRules.NOTIFICATION_TAG, AVAILABLE_ID)
        val open = Intent(context, UpdateInstallActivity::class.java).putExtra(UpdateInstallActivity.EXTRA_VERSION, version)
        val tap = PendingIntent.getActivity(context, READY_ID, open, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
        post(context, READY_ID, context.getString(R.string.update_ready_title, version), context.getString(R.string.update_ready_text), tap, null)
    }

    /** The metered case: the version is announced with a Download button instead of being downloaded. */
    fun available(context: Context, version: String) {
        val tap = PendingIntent.getBroadcast(
            context,
            AVAILABLE_ID,
            Intent(context, UpdateActionReceiver::class.java).setAction(UpdateActionReceiver.DOWNLOAD).putExtra(UpdateActionReceiver.VERSION, version),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        post(context, AVAILABLE_ID, context.getString(R.string.update_available_title, version), context.getString(R.string.update_available_text), tap, context.getString(R.string.update_action_download) to tap)
    }

    fun clear(context: Context) {
        val nm = context.getSystemService(android.app.NotificationManager::class.java)
        nm.cancel(UpdateRules.NOTIFICATION_TAG, READY_ID)
        nm.cancel(UpdateRules.NOTIFICATION_TAG, AVAILABLE_ID)
    }

    private fun post(context: Context, id: Int, title: String, text: String, tap: PendingIntent, action: Pair<String, PendingIntent>?) {
        val b = androidx.core.app.NotificationCompat.Builder(context, Channels.UPDATES)
            .setSmallIcon(R.drawable.ic_stat_mail)
            .setContentTitle(title)
            .setContentText(text)
            .setAutoCancel(true)
            .setOnlyAlertOnce(true)
            .setPriority(androidx.core.app.NotificationCompat.PRIORITY_LOW)
            .setContentIntent(tap)
        action?.let { b.addAction(0, it.first, it.second) }
        try {
            context.getSystemService(android.app.NotificationManager::class.java).notify(UpdateRules.NOTIFICATION_TAG, id, b.build())
        } catch (_: SecurityException) {
            // No permission to notify: About still shows it.
        }
    }
}

/** The Download button of the "available" notice: the user asked for it, so a metered network is allowed. */
class UpdateActionReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action != DOWNLOAD) return
        val version = intent.getStringExtra(VERSION).orEmpty()
        if (!UpdateInstallRules.safeVersion(version)) return
        UpdateDownloadWorker.enqueue(context, version, anyNetwork = true)
        UpdateNotices.clear(context)
    }

    companion object {
        const val DOWNLOAD = "ovh.kweb.mail.UPDATE_DOWNLOAD"
        const val VERSION = "version"
    }
}

/**
 * Installs a downloaded update through a PackageInstaller session. It is only ever started by the user's own tap
 * (the notice or About). It checks the file again (this app, newer, same signing key), asks for "install unknown
 * apps" when that is not allowed yet, commits the session, and hands the system's confirmation to the screen. The
 * system's own prompt is the only way the install can go on.
 */
class UpdateInstallActivity : Activity() {
    private var version = ""
    private var sessionId = -1
    private var waitingForSetting = false
    private var receiver: BroadcastReceiver? = null
    private var dialog: AlertDialog? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        version = intent.getStringExtra(EXTRA_VERSION).orEmpty()
        if (!UpdateInstallRules.safeVersion(version)) {
            finish()
            return
        }
        if (savedInstanceState != null) {
            sessionId = savedInstanceState.getInt(STATE_SESSION, -1)
            waitingForSetting = savedInstanceState.getBoolean(STATE_SETTING, false)
        }
        if (sessionId == -1) proceed()
    }

    override fun onSaveInstanceState(outState: Bundle) {
        super.onSaveInstanceState(outState)
        outState.putInt(STATE_SESSION, sessionId)
        outState.putBoolean(STATE_SETTING, waitingForSetting)
    }

    override fun onResume() {
        super.onResume()
        // Back from the system's "install unknown apps" screen.
        if (waitingForSetting) {
            waitingForSetting = false
            proceed()
        }
    }

    private fun proceed() {
        val file = UpdateFiles.file(this, version)
        if (!file.isFile) {
            finish()
            return
        }
        val refusal = UpdateFiles.inspect(this, file)
        if (refusal != null) {
            file.delete()
            UpdateNotices.clear(this)
            say(UpdateDownloader.refusalText(this, refusal))
            return
        }
        if (!packageManager.canRequestPackageInstalls()) {
            dialog = AlertDialog.Builder(this)
                .setTitle(R.string.update_unknown_title)
                .setMessage(R.string.update_unknown_text)
                .setPositiveButton(R.string.update_unknown_open) { _, _ ->
                    waitingForSetting = true
                    startActivity(Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, Uri.parse("package:$packageName")))
                }
                .setNegativeButton(R.string.update_close) { _, _ -> finish() }
                .setOnCancelListener { finish() }
                .show()
            return
        }
        commit(file)
    }

    private fun commit(file: File) {
        val installer = packageManager.packageInstaller
        try {
            val params = PackageInstaller.SessionParams(PackageInstaller.SessionParams.MODE_FULL_INSTALL).apply {
                setAppPackageName(packageName)
                setSize(file.length())
            }
            sessionId = installer.createSession(params)
            installer.openSession(sessionId).use { session ->
                file.inputStream().use { input ->
                    session.openWrite("update", 0, file.length()).use { out ->
                        input.copyTo(out)
                        session.fsync(out)
                    }
                }
                listen()
                val status = PendingIntent.getBroadcast(
                    this,
                    sessionId,
                    Intent(ACTION_STATUS).setPackage(packageName),
                    PendingIntent.FLAG_MUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
                )
                session.commit(status.intentSender)
            }
        } catch (e: Exception) {
            Log.w(Background.TAG, "the update install could not start: ${e.javaClass.simpleName}")
            if (sessionId != -1) try { installer.abandonSession(sessionId) } catch (_: Exception) {}
            say(getString(R.string.update_install_failed))
        }
    }

    private fun listen() {
        if (receiver != null) return
        receiver = object : BroadcastReceiver() {
            override fun onReceive(context: Context, intent: Intent) {
                when (intent.getIntExtra(PackageInstaller.EXTRA_STATUS, PackageInstaller.STATUS_FAILURE)) {
                    PackageInstaller.STATUS_PENDING_USER_ACTION -> {
                        // The system's own prompt: the user's tap there is the only way on.
                        @Suppress("DEPRECATION")
                        val confirm = intent.getParcelableExtra<Intent>(Intent.EXTRA_INTENT)
                        if (confirm != null) startActivity(confirm) else finish()
                    }
                    PackageInstaller.STATUS_SUCCESS -> finish()
                    PackageInstaller.STATUS_FAILURE_ABORTED -> say(getString(R.string.update_install_cancelled))
                    else -> say(getString(R.string.update_install_failed))
                }
            }
        }
        ContextCompat.registerReceiver(this, receiver, IntentFilter(ACTION_STATUS), ContextCompat.RECEIVER_NOT_EXPORTED)
    }

    private fun say(text: String) {
        dialog = AlertDialog.Builder(this)
            .setMessage(text)
            .setPositiveButton(R.string.update_close) { _, _ -> finish() }
            .setOnCancelListener { finish() }
            .show()
    }

    override fun onDestroy() {
        receiver?.let { try { unregisterReceiver(it) } catch (_: Exception) {} }
        dialog?.dismiss()
        super.onDestroy()
    }

    companion object {
        const val EXTRA_VERSION = "version"
        private const val STATE_SESSION = "session"
        private const val STATE_SETTING = "setting"
        private const val ACTION_STATUS = "ovh.kweb.mail.UPDATE_INSTALL_STATUS"

        fun intent(context: Context, version: String): Intent = Intent(context, UpdateInstallActivity::class.java).putExtra(EXTRA_VERSION, version)
    }
}
