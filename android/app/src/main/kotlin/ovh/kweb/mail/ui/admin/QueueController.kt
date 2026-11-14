package ovh.kweb.mail.ui.admin

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.UiMessage
import uniffi.kweb_core.Maintenance
import uniffi.kweb_core.Exception as CoreException
import uniffi.kweb_core.QueuedMessage

sealed interface QueueDialog {
    data class Cancel(val id: String, val from: String) : QueueDialog

    data object Pause : QueueDialog
}

data class QueueState(
    /** This screen paused the queue (the server cannot be asked; Resume is offered whatever this says). */
    val paused: Boolean = false,
    val dialog: QueueDialog? = null,
    val busy: Boolean = false,
    val error: UiMessage? = null,
    val info: UiMessage? = null,
)

/** The mail queue: the list in pages, Retry and Cancel on each message, Pause and Resume for all. */
class QueueController(
    scope: CoroutineScope,
    io: CoroutineDispatcher,
    private val backend: AdminBackend,
) {
    private val gate = ActionGate(scope, io)
    private val stateFlow = MutableStateFlow(QueueState())

    val list = PagedLoader<QueuedMessage>(scope, io, AdminRules.PAGE, R.string.admin_queue_load_failed, { it.id }) { offset, limit, _ ->
        backend.queue(offset, limit).let { Page(it.messages, it.total.toInt()) }
    }
    val state: StateFlow<QueueState> get() = stateFlow

    fun refresh() = list.refresh()

    fun retry(id: String) {
        gate.run({ backend.queueRetry(id) }, ::started, { fail(it, R.string.admin_retry_failed) }) {
            stateFlow.update { it.copy(busy = false, info = UiMessage.Res(R.string.admin_retry_done)) }
            list.reload(quiet = true)
        }
    }

    fun askCancel(message: QueuedMessage) = stateFlow.update { it.copy(dialog = QueueDialog.Cancel(message.id, message.from)) }

    fun confirmCancel() {
        val dialog = stateFlow.value.dialog as? QueueDialog.Cancel ?: return
        stateFlow.update { it.copy(dialog = null) }
        gate.run({ backend.queueCancel(dialog.id) }, ::started, { fail(it, R.string.admin_cancel_failed) }) {
            list.change { items -> items.filter { it.id != dialog.id } }
            stateFlow.update { it.copy(busy = false, info = UiMessage.Res(R.string.admin_cancel_done)) }
            list.reload(quiet = true)
        }
    }

    fun askPause() = stateFlow.update { it.copy(dialog = QueueDialog.Pause) }

    fun confirmPause() {
        stateFlow.update { it.copy(dialog = null) }
        maintenance(Maintenance.PAUSE_QUEUE, paused = true)
    }

    /** Resume does not ask: it only gives back what was there. */
    fun resume() = maintenance(Maintenance.RESUME_QUEUE, paused = false)

    private fun maintenance(what: Maintenance, paused: Boolean) {
        gate.run({ backend.maintenance(what) }, ::started, { fail(it, R.string.admin_pause_failed) }) {
            stateFlow.update { it.copy(busy = false, paused = paused, info = UiMessage.Res(if (paused) R.string.admin_paused_done else R.string.admin_resumed_done)) }
        }
    }

    private fun started() = stateFlow.update { it.copy(busy = true, error = null, info = null) }

    private fun fail(e: Exception, fallback: Int) = stateFlow.update { it.copy(busy = false, error = UiMessage.of(e, fallback)) }

    fun dismissDialog() = stateFlow.update { it.copy(dialog = null) }

    fun dismissError() = stateFlow.update { it.copy(error = null) }

    fun dismissInfo() = stateFlow.update { it.copy(info = null) }
}

enum class MessagePhase { LOADING, READY, FAILED }

data class QueuedMessageState(
    val phase: MessagePhase = MessagePhase.LOADING,
    val message: QueuedMessage? = null,
    val askCancel: Boolean = false,
    val busy: Boolean = false,
    /** The message left the queue (cancelled, or delivered meanwhile): the screen closes. */
    val gone: Boolean = false,
    val error: UiMessage? = null,
    val info: UiMessage? = null,
)

/** One message of the queue, read again after Retry. */
class QueuedMessageController(
    private val scope: CoroutineScope,
    private val io: CoroutineDispatcher,
    private val backend: AdminBackend,
    private val id: String,
) {
    private val gate = ActionGate(scope, io)
    private val stateFlow = MutableStateFlow(QueuedMessageState())
    private var ticket = 0

    val state: StateFlow<QueuedMessageState> get() = stateFlow

    fun refresh() {
        val mine = ++ticket
        scope.launch {
            try {
                val message = withContext(io) { backend.queuedMessage(id) }
                if (mine != ticket) return@launch
                stateFlow.update { it.copy(phase = MessagePhase.READY, message = message, error = null) }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                if (mine != ticket) return@launch
                // Delivered (or cancelled elsewhere) meanwhile: nothing is left to show, so the screen closes.
                if (isGone(e)) {
                    stateFlow.update { it.copy(gone = true, busy = false, error = null) }
                    return@launch
                }
                stateFlow.update {
                    if (it.message != null) it.copy(error = UiMessage.of(e, R.string.admin_load_failed)) else it.copy(phase = MessagePhase.FAILED, error = UiMessage.of(e, R.string.admin_load_failed))
                }
            }
        }
    }

    fun retry() {
        gate.run({ backend.queueRetry(id) }, ::started, { if (isGone(it)) stateFlow.update { s -> s.copy(gone = true, busy = false) } else fail(it, R.string.admin_retry_failed) }) {
            stateFlow.update { it.copy(busy = false, info = UiMessage.Res(R.string.admin_retry_done)) }
            refresh()
        }
    }

    fun askCancel() = stateFlow.update { it.copy(askCancel = true) }

    fun dismissDialog() = stateFlow.update { it.copy(askCancel = false) }

    fun confirmCancel() {
        stateFlow.update { it.copy(askCancel = false) }
        gate.run({ backend.queueCancel(id) }, ::started, { fail(it, R.string.admin_cancel_failed) }) {
            stateFlow.update { it.copy(busy = false, gone = true) }
        }
    }

    private fun isGone(e: Exception) = e is CoreException.Invalid && e.msg == QUEUE_GONE

    private fun started() = stateFlow.update { it.copy(busy = true, error = null, info = null) }

    private fun fail(e: Exception, fallback: Int) = stateFlow.update { it.copy(busy = false, error = UiMessage.of(e, fallback)) }

    fun dismissError() = stateFlow.update { it.copy(error = null) }

    fun dismissInfo() = stateFlow.update { it.copy(info = null) }
}
