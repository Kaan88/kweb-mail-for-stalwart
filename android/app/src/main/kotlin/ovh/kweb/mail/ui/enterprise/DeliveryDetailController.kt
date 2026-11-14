package ovh.kweb.mail.ui.enterprise

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
import uniffi.kweb_core.DeliveryDetail

data class DeliveryDetailState(
    val phase: Phase = Phase.LOADING,
    val detail: DeliveryDetail? = null,
    /** The link under the timeline: the server's own event lines instead of the sentences. */
    val showRaw: Boolean = false,
    val error: UiMessage? = null,
)

/** One delivery followed through the server. A late answer for an id that was left is dropped. */
class DeliveryDetailController(
    private val scope: CoroutineScope,
    private val io: CoroutineDispatcher,
    private val backend: EnterpriseBackend,
) {
    private val stateFlow = MutableStateFlow(DeliveryDetailState())
    private var id: String? = null
    private var ticket = 0

    val state: StateFlow<DeliveryDetailState> get() = stateFlow

    /** Reads [deliveryId] once; the same id again (the screen came back) does nothing. */
    fun start(deliveryId: String) {
        if (id == deliveryId) return
        id = deliveryId
        stateFlow.value = DeliveryDetailState()
        load()
    }

    fun reset() {
        ticket++
        id = null
        stateFlow.value = DeliveryDetailState()
    }

    fun load() {
        val wanted = id ?: return
        val mine = ++ticket
        stateFlow.update { it.copy(phase = if (it.detail == null) Phase.LOADING else Phase.READY, error = null) }
        scope.launch {
            try {
                val detail = withContext(io) { backend.delivery(wanted) }
                if (mine == ticket) stateFlow.update { it.copy(phase = Phase.READY, detail = detail) }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                if (mine != ticket) return@launch
                val message = failure(e, R.string.ent_delivery_failed)
                stateFlow.update { it.copy(phase = if (it.detail == null) Phase.FAILED else Phase.READY, error = message) }
            }
        }
    }

    fun toggleRaw() = stateFlow.update { it.copy(showRaw = !it.showRaw) }

    fun dismissError() = stateFlow.update { it.copy(error = null) }
}
