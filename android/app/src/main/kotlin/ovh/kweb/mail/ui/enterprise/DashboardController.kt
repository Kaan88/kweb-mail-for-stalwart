package ovh.kweb.mail.ui.enterprise

import java.util.UUID
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.UiMessage
import uniffi.kweb_core.DashboardHistory
import uniffi.kweb_core.LiveFigures
import uniffi.kweb_core.LiveListener
import uniffi.kweb_core.Exception as CoreException

/** The pill beside the title: the stream is being opened, runs, or has stopped (a dropped connection). */
enum class LiveStatus { CONNECTING, LIVE, PAUSED }

data class DashboardState(
    val figures: LiveFigures? = null,
    val live: LiveStatus = LiveStatus.CONNECTING,
    val range: ChartRange = ChartRange.DAY,
    val historyPhase: Phase = Phase.LOADING,
    val history: DashboardHistory? = null,
    /** Why the history could not be read; shown with a retry. */
    val error: UiMessage? = null,
)

/**
 * The dashboard: the live figures run while the screen is shown ([startLive] and [stopLive], called from the
 * lifecycle), the history is read for the chosen range. A history answer that a newer request overtook is dropped;
 * figures of a run that was stopped are ignored. A stream that ends by itself shows "Paused" and is opened again
 * after [retryDelayMs] for as long as the screen is shown. A refusal (the core's `Invalid`) ends the loop and shows
 * the core's words; the stream is opened again when the screen starts it again.
 */
class DashboardController(
    private val scope: CoroutineScope,
    private val io: CoroutineDispatcher,
    private val backend: EnterpriseBackend,
    private val newRunId: () -> String = { UUID.randomUUID().toString() },
    private val retryDelayMs: Long = 10_000,
) {
    private val stateFlow = MutableStateFlow(DashboardState())
    private var historyTicket = 0
    private var historyStarted = false

    /** Counts starts and stops; a run whose number is not the current one is over. */
    @Volatile private var generation = 0
    private var liveJob: Job? = null

    @Volatile private var runId: String? = null

    val state: StateFlow<DashboardState> get() = stateFlow

    /** Reads the history once when the screen first shows. */
    fun start() {
        if (historyStarted) return
        historyStarted = true
        loadHistory()
    }

    fun reset() {
        stopLive()
        historyTicket++
        historyStarted = false
        stateFlow.value = DashboardState()
    }

    fun setRange(range: ChartRange) {
        if (stateFlow.value.range == range && stateFlow.value.historyPhase != Phase.FAILED) return
        stateFlow.update { it.copy(range = range) }
        loadHistory()
    }

    /** Reads the history again (Retry). */
    fun loadHistory() {
        val mine = ++historyTicket
        val range = stateFlow.value.range
        // What is on screen stays while another range is read; a failed first read starts over.
        stateFlow.update { it.copy(historyPhase = if (it.history == null) Phase.LOADING else Phase.READY, error = null) }
        scope.launch {
            try {
                val history = withContext(io) { backend.history(range.hours) }
                if (mine == historyTicket) stateFlow.update { it.copy(historyPhase = Phase.READY, history = history, error = null) }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                if (mine != historyTicket) return@launch
                val message = failure(e, R.string.ent_dash_history_failed)
                stateFlow.update { it.copy(historyPhase = if (it.history == null) Phase.FAILED else Phase.READY, error = message) }
            }
        }
    }

    fun dismissError() = stateFlow.update { it.copy(error = null) }

    /** Opens the stream; a second call while it runs does nothing. */
    fun startLive() {
        if (liveJob?.isActive == true) return
        val mine = ++generation
        stateFlow.update { it.copy(live = LiveStatus.CONNECTING) }
        liveJob = scope.launch {
            while (mine == generation) {
                val run = newRunId()
                runId = run
                val listener = object : LiveListener {
                    override fun figures(figures: LiveFigures) {
                        if (mine == generation && runId == run) stateFlow.update { it.copy(figures = figures, live = LiveStatus.LIVE) }
                    }
                }
                try {
                    withContext(io) { backend.live(run, listener) }
                } catch (e: CancellationException) {
                    throw e
                } catch (e: CoreException.Invalid) {
                    // A refusal that no retry cures (the key is dead, may not follow, no licence): the core's words are
                    // shown and the stream is not opened again by itself. Every try is a sign-in the server counts.
                    if (mine == generation) stateFlow.update { it.copy(live = LiveStatus.PAUSED, error = failure(e, R.string.ent_dash_live_failed)) }
                    break
                } catch (e: CoreException.SignInNeeded) {
                    if (mine == generation) stateFlow.update { it.copy(live = LiveStatus.PAUSED, error = failure(e, R.string.ent_dash_live_failed)) }
                    break
                } catch (e: Exception) {
                    // A connection that ended by itself: paused, tried again below.
                }
                if (mine != generation) break
                stateFlow.update { it.copy(live = LiveStatus.PAUSED) }
                delay(retryDelayMs)
                if (mine == generation) stateFlow.update { it.copy(live = LiveStatus.CONNECTING) }
            }
        }
    }

    /** Closes the stream (the screen left the view or the app went to the background). */
    fun stopLive() {
        generation++
        val run = runId
        runId = null
        liveJob?.cancel()
        liveJob = null
        stateFlow.update { it.copy(live = LiveStatus.PAUSED) }
        if (run != null) scope.launch(io) { try { backend.liveStop(run) } catch (e: Exception) { /* nothing to stop */ } }
    }

    /** The view model is gone: the scope is cancelled, so the stop is made here, on the caller's thread. */
    fun shutdown() {
        generation++
        val run = runId
        runId = null
        if (run != null) try { backend.liveStop(run) } catch (e: Exception) { /* nothing to stop */ }
    }
}
