package ovh.kweb.mail.ui.transfer

import android.app.Notification
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.IBinder
import android.util.Log
import androidx.core.app.NotificationCompat
import androidx.core.content.ContextCompat
import java.io.File
import java.util.Locale
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import ovh.kweb.mail.AppVisibility
import ovh.kweb.mail.R
import ovh.kweb.mail.accounts.AccountLabels
import ovh.kweb.mail.accounts.Accounts
import ovh.kweb.mail.background.Background
import ovh.kweb.mail.background.Channels
import ovh.kweb.mail.background.NotificationIntents

/**
 * The downloads that are running in this process, and the foreground service that keeps the process alive while
 * there are any. Every download of an attachment or a file ([CoreDownloads]) is entered here with the account it
 * belongs to; the service starts with the first and ends with the last, and its notification shows how far they
 * are, with Cancel. The screens keep their own progress ([ActiveTransfer]); this is the same transfer seen from
 * outside the app.
 *
 * What happens to a download when the process dies (the system needs the memory, or the service is killed): it
 * is gone, with its notification, and its part file is deleted at the next start ([sweepParts]). There is no
 * resume: the next tap starts the file again from nothing.
 *
 * Nothing of a file (its name, its content) is logged, and the notification names no file: only the kind of
 * transfer, the account when several are shown and the sizes.
 */
object TransferHub {
    enum class Kind { Attachment, File }

    /** One running download. [done] and [total] are the core's last report. */
    class Entry internal constructor(val accountId: String, val kind: Kind, val stop: DownloadStop) {
        @Volatile var done: Long = 0

        @Volatile var total: Long? = null
    }

    private val lock = Any()
    private val running = LinkedHashSet<Entry>()
    private var lastPost = 0L
    private val count = MutableStateFlow(0)

    /** How many downloads run now. */
    val active: StateFlow<Int> get() = count

    const val ID_PROGRESS = 5
    const val ID_DONE = 6
    private const val POST_EVERY_MS = 500L

    /** Enters a download of [accountId]; starts the service when it is the first. Null when there is no app context (a test). */
    fun begin(accountId: String, kind: Kind, stop: DownloadStop): Entry? {
        val context = Accounts.appContext() ?: return null
        val entry = Entry(accountId, kind, stop)
        val first = synchronized(lock) {
            running.add(entry)
            count.value = running.size
            running.size == 1
        }
        context.getSystemService(NotificationManager::class.java).cancel(ID_DONE)
        if (first) start(context) else post(context, force = true)
        return entry
    }

    fun report(entry: Entry, done: Long, total: Long?) {
        entry.done = done
        if (total != null) entry.total = total
        val context = Accounts.appContext() ?: return
        post(context, force = false)
    }

    /** The download ended: [complete] when the file is whole. A finished file while the app is out of sight is announced once. */
    fun end(entry: Entry, complete: Boolean) {
        val context = Accounts.appContext() ?: return
        val last = synchronized(lock) {
            running.remove(entry)
            count.value = running.size
            running.isEmpty()
        }
        if (last) {
            context.stopService(Intent(context, DownloadService::class.java))
            context.getSystemService(NotificationManager::class.java).cancel(ID_PROGRESS)
            if (complete && !AppVisibility.visible) announceDone(context)
        } else {
            post(context, force = true)
        }
    }

    /** The user pressed Cancel in the notification: every running download is stopped (each ends itself). */
    fun cancelAll() {
        val all = synchronized(lock) { running.toList() }
        all.forEach { it.stop.stop() }
    }

    private fun start(context: Context) {
        try {
            ContextCompat.startForegroundService(context, Intent(context, DownloadService::class.java))
        } catch (e: Exception) {
            // The download goes on without the service; it only does not survive leaving the app.
            Log.w(Background.TAG, "the download service could not be started: ${e.javaClass.simpleName}")
        }
    }

    private fun post(context: Context, force: Boolean) {
        val now = System.currentTimeMillis()
        val due = synchronized(lock) {
            if (running.isEmpty() || (!force && now - lastPost < POST_EVERY_MS)) {
                false
            } else {
                lastPost = now
                true
            }
        }
        if (!due) return
        try {
            context.getSystemService(NotificationManager::class.java).notify(ID_PROGRESS, notification(context))
        } catch (e: RuntimeException) {
            Log.w(Background.TAG, "posting the download progress failed: ${e.javaClass.simpleName}")
        }
    }

    private fun announceDone(context: Context) {
        val notification = NotificationCompat.Builder(context, Channels.DOWNLOADS)
            .setSmallIcon(R.drawable.ic_download)
            .setContentTitle(context.getString(R.string.download_done_title))
            .setContentText(context.getString(R.string.download_done_text))
            .setCategory(NotificationCompat.CATEGORY_STATUS)
            .setVisibility(NotificationCompat.VISIBILITY_PRIVATE)
            .setAutoCancel(true)
            .setTimeoutAfter(10 * 60 * 1000L)
            .setContentIntent(NotificationIntents.openAppPlain(context, ID_DONE))
            .build()
        try {
            context.getSystemService(NotificationManager::class.java).notify(ID_DONE, notification)
        } catch (e: RuntimeException) {
            Log.w(Background.TAG, "posting the finished download failed: ${e.javaClass.simpleName}")
        }
    }

    /** The progress notification, from what runs now. */
    internal fun notification(context: Context): Notification {
        val now = synchronized(lock) { running.toList() }
        val records = Accounts.records.value
        val first = now.firstOrNull()
        val label = first?.takeIf { now.size == 1 }
            ?.let { e -> records.firstOrNull { it.id == e.accountId } }
            ?.let { AccountLabels.forNotice(it, records, Accounts.names.value) }
        val title = when {
            now.size > 1 -> context.resources.getQuantityString(R.plurals.download_title_many, now.size, now.size)
            first?.kind == Kind.File -> context.getString(R.string.download_title_file)
            else -> context.getString(R.string.download_title_attachment)
        }
        val done = now.sumOf { it.done }
        val total = if (now.isNotEmpty() && now.all { (it.total ?: 0) > 0 }) now.sumOf { it.total ?: 0 } else null
        val cancel = PendingIntent.getService(
            context,
            0,
            Intent(context, DownloadService::class.java).setAction(DownloadService.ACTION_CANCEL),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        val of = { a: String, b: String -> context.getString(R.string.download_of, a, b) }
        return NotificationCompat.Builder(context, Channels.DOWNLOADS)
            .setSmallIcon(R.drawable.ic_download)
            .setContentTitle(title)
            .setContentText(DownloadRules.line(DownloadProgress(done, total), Locale.getDefault(), of))
            .apply { label?.let { setSubText(it) } }
            .setProgress(1000, if (total != null) (done * 1000 / total).toInt().coerceIn(0, 1000) else 0, total == null)
            .setCategory(NotificationCompat.CATEGORY_PROGRESS)
            .setVisibility(NotificationCompat.VISIBILITY_PRIVATE)
            .setOngoing(true)
            .setSilent(true)
            .setOnlyAlertOnce(true)
            .setShowWhen(false)
            .setForegroundServiceBehavior(NotificationCompat.FOREGROUND_SERVICE_IMMEDIATE)
            .setContentIntent(NotificationIntents.openAppPlain(context, ID_PROGRESS))
            .addAction(0, context.getString(R.string.cancel), cancel)
            .build()
    }

    /**
     * Deletes the part files a process that died left (the core names them `file.pid-n.part`). The ones of this
     * process are not touched. Cheap; called at start.
     */
    fun sweepParts(cacheDir: File, ownPid: Int = android.os.Process.myPid()) {
        for (top in listOf("blobs", "attachments")) {
            File(cacheDir, top).walkTopDown()
                .filter { it.isFile && it.name.endsWith(".part") && !it.name.contains(".$ownPid-") }
                .forEach { it.delete() }
        }
    }
}

/** The foreground service of [TransferHub]: it exists to keep the process alive while a download runs, and shows its notification. */
class DownloadService : Service() {
    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (intent?.action == ACTION_CANCEL) TransferHub.cancelAll()
        try {
            startForeground(TransferHub.ID_PROGRESS, TransferHub.notification(this), ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC)
        } catch (e: Exception) {
            Log.w(Background.TAG, "the download service could not go to the foreground: ${e.javaClass.simpleName}")
            stopSelf()
            return START_NOT_STICKY
        }
        // The last download may have ended before this ran: the service has nothing to keep alive then.
        if (TransferHub.active.value == 0) {
            stopForeground(STOP_FOREGROUND_REMOVE)
            stopSelf()
        }
        return START_NOT_STICKY
    }

    /** Android ends a data-sync service after hours (Android 15): the downloads stop rather than run unprotected. */
    override fun onTimeout(startId: Int, fgsType: Int) {
        TransferHub.cancelAll()
        stopSelf()
    }

    companion object {
        const val ACTION_CANCEL = "ovh.kweb.mail.action.CANCEL_DOWNLOADS"
    }
}
