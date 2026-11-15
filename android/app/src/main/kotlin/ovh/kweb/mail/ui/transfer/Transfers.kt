package ovh.kweb.mail.ui.transfer

import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.update

/**
 * A way to stop one download from the main thread while another thread is blocked in the core.
 * [attach] is given what stops the core call; it is called at once when the stop came first.
 */
class DownloadStop {
    private val lock = Any()
    private var hook: (() -> Unit)? = null
    var stopped = false
        private set

    fun stop() {
        synchronized(lock) {
            stopped = true
            hook?.invoke()
        }
    }

    /** Takes what ends the core call (null when the call is over). A hook is never called after it was taken away. */
    fun attach(newHook: (() -> Unit)?) {
        synchronized(lock) {
            hook = newHook
            if (stopped) newHook?.invoke()
        }
    }
}

/** What is being downloaded under [key] (a blob id) and how far it is. */
data class ActiveDownload(val key: String, val progress: DownloadProgress)

/**
 * The one download a screen has under way. A second [begin] while one runs is refused, so a second tap
 * on the same file does not start a second transfer from zero. Leaving the screen [cancel]s it.
 */
class ActiveTransfer {
    /** One download's run; only its own [finish] ends it. */
    class Run internal constructor(val key: String, val stop: DownloadStop)

    private val flow = MutableStateFlow<ActiveDownload?>(null)
    private val lock = Any()
    private var current: Run? = null

    val state: StateFlow<ActiveDownload?> get() = flow

    /** Starts a download of [key], whose size is [expected] when known; null when one is under way already. */
    fun begin(key: String, expected: Long?): Run? = synchronized(lock) {
        if (current != null) return null
        val run = Run(key, DownloadStop())
        current = run
        flow.value = ActiveDownload(key, DownloadProgress(0, expected))
        run
    }

    /** Progress from the core; a total the core does not know keeps the size that was expected. */
    fun report(run: Run, done: Long, total: Long?) {
        flow.update { now ->
            if (current === run && now != null) now.copy(progress = DownloadProgress(done, total ?: now.progress.total)) else now
        }
    }

    fun finish(run: Run) = synchronized(lock) {
        if (current === run) {
            current = null
            flow.value = null
        }
    }

    /** Stops the download under way; it ends itself and calls [finish]. */
    fun cancel() {
        val run = synchronized(lock) { current }
        run?.stop?.stop()
    }
}
