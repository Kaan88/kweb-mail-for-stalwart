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
import uniffi.kweb_core.Delivery

data class DeliveriesState(
    /** What is typed in the field (kept over a rotation). */
    val text: String = "",
    val range: HistoryRange = HistoryRange.TODAY,
    val phase: Phase = Phase.LOADING,
    val deliveries: List<Delivery> = emptyList(),
    val total: UInt = 0u,
    val loadingMore: Boolean = false,
    /** A failed first read shows with a retry; a failed next page is a bar and the list stays. */
    val error: UiMessage? = null,
) {
    val hasMore: Boolean get() = DeliveryRules.hasMore(deliveries.size, total)
}

/**
 * The delivery history: the search text and the range choose what the core is asked; a new search replaces the list,
 * "load more" adds the next page. An answer that a newer search overtook is dropped.
 */
class DeliveriesController(
    private val scope: CoroutineScope,
    private val io: CoroutineDispatcher,
    private val backend: EnterpriseBackend,
    private val nowMillis: () -> Long = System::currentTimeMillis,
    private val pageSize: UInt = 50u,
) {
    private val stateFlow = MutableStateFlow(DeliveriesState())
    private var started = false
    private var ticket = 0

    /** The text the list on screen was asked with (what "load more" continues), apart from the field. */
    private var askedText = ""
    private var askedSince = 0L

    val state: StateFlow<DeliveriesState> get() = stateFlow

    fun start() {
        if (started) return
        started = true
        search()
    }

    fun reset() {
        ticket++
        started = false
        askedText = ""
        stateFlow.value = DeliveriesState()
    }

    fun setText(text: String) = stateFlow.update { it.copy(text = text) }

    fun setRange(range: HistoryRange) {
        if (stateFlow.value.range == range && stateFlow.value.phase == Phase.READY) return
        stateFlow.update { it.copy(range = range) }
        search()
    }

    /** Asks for the first page of what the field and the chips say. */
    fun search() {
        val mine = ++ticket
        val s = stateFlow.value
        askedText = s.text.trim()
        val text = askedText
        val since = DeliveryRules.since(s.range, nowMillis())
        askedSince = since
        stateFlow.update { it.copy(phase = Phase.LOADING, loadingMore = false, error = null) }
        scope.launch {
            try {
                val page = withContext(io) { backend.deliveries(text, since, 0u, pageSize) }
                if (mine == ticket) stateFlow.update { it.copy(phase = Phase.READY, deliveries = page.deliveries, total = page.total) }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                if (mine == ticket) stateFlow.update { it.copy(phase = Phase.FAILED, deliveries = emptyList(), total = 0u, error = failure(e, R.string.ent_deliveries_failed)) }
            }
        }
    }

    /** The next page, from the end of what is held. */
    fun loadMore() {
        val s = stateFlow.value
        if (s.phase != Phase.READY || s.loadingMore || !s.hasMore) return
        val mine = ++ticket
        val offset = s.deliveries.size.toUInt()
        val since = askedSince
        stateFlow.update { it.copy(loadingMore = true, error = null) }
        scope.launch {
            try {
                val page = withContext(io) { backend.deliveries(askedText, since, offset, pageSize) }
                if (mine == ticket) {
                    stateFlow.update { cur ->
                        val known = cur.deliveries.map { it.id }.toSet()
                        cur.copy(deliveries = cur.deliveries + page.deliveries.filter { it.id !in known }, total = page.total, loadingMore = false)
                    }
                }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                if (mine == ticket) stateFlow.update { it.copy(loadingMore = false, error = failure(e, R.string.ent_deliveries_failed)) }
            }
        }
    }

    fun dismissError() = stateFlow.update { it.copy(error = null) }
}
