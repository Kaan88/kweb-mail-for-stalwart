package ovh.kweb.mail.ui.sending

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.UiMessage
import uniffi.kweb_core.ScheduledSend
import uniffi.kweb_core.Exception as CoreException

enum class ScheduledPhase { LOADING, READY, FAILED }

data class ScheduledState(
    val phase: ScheduledPhase = ScheduledPhase.LOADING,
    val items: List<ScheduledSend> = emptyList(),
    /** A pull to refresh is running. */
    val refreshing: Boolean = false,
    /** The message the user is asked about: take it back? */
    val asking: ScheduledSend? = null,
    /** The message being taken back. */
    val busyId: String? = null,
    val error: UiMessage? = null,
)

/**
 * The list of messages the server holds back, and taking one back. The core is asked off the main thread ([io]);
 * an answer that a newer read has overtaken is dropped; only one message is taken back at a time.
 */
class ScheduledController(
    private val scope: CoroutineScope,
    private val io: CoroutineDispatcher,
    private val backend: ScheduledBackend,
) {
    private val stateFlow = MutableStateFlow(ScheduledState())
    private val openChannel = Channel<String>(Channel.BUFFERED)
    private var started = false

    /** Counts reads; an answer is used only when no newer read was started. */
    private var ticket = 0

    /**
     * A message whose cancel went through but could not be finished (the core said "Temporary" and where the message is).
     * The server no longer lists it as waiting, so a read would drop the row: it stays here until a retry has returned a draft.
     */
    private var stuck: ScheduledSend? = null

    val state: StateFlow<ScheduledState> get() = stateFlow

    /** The draft a taken-back message became: the screen opens it for editing. */
    val openDraft: Flow<String> get() = openChannel.receiveAsFlow()

    /** Reads the list once. */
    fun start() {
        if (started) return
        started = true
        scope.launch { read(pull = false) }
    }

    fun reset() {
        ticket++
        started = false
        stuck = null
        stateFlow.value = ScheduledState()
    }

    /** Reads again: the screen came back, Retry, or a pull. What is on screen stays until the answer is there. */
    fun refresh(pull: Boolean = false, keepError: Boolean = false) {
        if (!started) return
        scope.launch { read(pull, keepError) }
    }

    private suspend fun read(pull: Boolean, keepError: Boolean = false) {
        val mine = ++ticket
        stateFlow.update { it.copy(refreshing = pull, phase = if (it.phase == ScheduledPhase.FAILED) ScheduledPhase.LOADING else it.phase) }
        try {
            val read = withContext(io) { backend.scheduledSends() }
            if (mine != ticket) return
            val kept = stuck
            val items = ScheduledRules.ordered(if (kept != null && read.none { it.id == kept.id }) read + kept else read)
            stateFlow.update { it.copy(phase = ScheduledPhase.READY, items = items, refreshing = false, error = it.error.takeIf { keepError }) }
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            if (mine != ticket) return
            // A list that is on screen stays; the failure is said above it.
            stateFlow.update {
                if (it.phase == ScheduledPhase.READY) it.copy(refreshing = false, error = UiMessage.of(e, R.string.scheduled_load_failed))
                else it.copy(phase = ScheduledPhase.FAILED, refreshing = false)
            }
        }
    }

    fun dismissError() = stateFlow.update { it.copy(error = null) }

    /** "Cancel sending" was tapped: ask first. */
    fun ask(item: ScheduledSend) = stateFlow.update { if (it.busyId == null) it.copy(asking = item) else it }

    fun dismissAsk() = stateFlow.update { it.copy(asking = null) }

    /** The question was answered yes: the core takes the message back and the draft it returns is opened. */
    fun confirmCancel() {
        val item = stateFlow.value.asking ?: return
        if (stateFlow.value.busyId != null) return
        stateFlow.update { it.copy(asking = null, busyId = item.id, error = null) }
        scope.launch {
            try {
                val draft = withContext(io) { backend.cancelScheduled(item.id) }
                // It is a draft again, so it is not on the list any more, and a read that started before must not bring it back.
                ticket++
                stuck = null
                stateFlow.update { s -> s.copy(items = s.items.filter { it.id != item.id }, busyId = null, refreshing = false) }
                openChannel.send(draft.id)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                // "Temporary" can also mean the send is cancelled and only the move back to Drafts is left: the core says
                // where the message is, so its words are shown, and the row stays for another try.
                val message = when {
                    e is CoreException.Temporary && e.msg.isNotBlank() -> UiMessage.Text(e.msg)
                    else -> UiMessage.of(e, R.string.scheduled_cancel_failed)
                }
                if (e is CoreException.Temporary) stuck = item
                if (e is CoreException.Invalid) stuck = null
                stateFlow.update { it.copy(busyId = null, error = message) }
                // Too late means it has left or is gone: the list must say so.
                if (e is CoreException.Invalid) refresh(keepError = true)
            }
        }
    }
}
