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
import uniffi.kweb_core.ServerAlert

data class AlertsState(
    val phase: Phase = Phase.LOADING,
    val alerts: List<ServerAlert> = emptyList(),
    /** The alerts whose switch is on its way to the server; their switch cannot be used meanwhile. */
    val pending: Set<String> = emptySet(),
    val error: UiMessage? = null,
)

/** The server's alerts with a switch each. The switch shows the new position at once and goes back when the server refuses. */
class AlertsController(
    private val scope: CoroutineScope,
    private val io: CoroutineDispatcher,
    private val backend: EnterpriseBackend,
) {
    private val stateFlow = MutableStateFlow(AlertsState())
    private var started = false
    private var ticket = 0

    val state: StateFlow<AlertsState> get() = stateFlow

    fun start() {
        if (started) return
        started = true
        load()
    }

    fun reset() {
        ticket++
        started = false
        stateFlow.value = AlertsState()
    }

    fun load() {
        val mine = ++ticket
        stateFlow.update { it.copy(phase = if (it.alerts.isEmpty() && it.phase != Phase.READY) Phase.LOADING else it.phase, error = null) }
        scope.launch {
            try {
                val alerts = withContext(io) { backend.alerts() }
                if (mine == ticket) stateFlow.update { it.copy(phase = Phase.READY, alerts = alerts) }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                if (mine != ticket) return@launch
                val message = failure(e, R.string.ent_alerts_failed)
                stateFlow.update { it.copy(phase = if (it.phase == Phase.READY) Phase.READY else Phase.FAILED, error = message) }
            }
        }
    }

    fun setEnabled(id: String, enabled: Boolean) {
        val s = stateFlow.value
        val alert = s.alerts.firstOrNull { it.id == id } ?: return
        if (id in s.pending || alert.isEnabled == enabled) return
        stateFlow.update { it.copy(pending = it.pending + id, alerts = it.alerts.withEnabled(id, enabled), error = null) }
        scope.launch {
            try {
                withContext(io) { backend.setAlertEnabled(id, enabled) }
                stateFlow.update { it.copy(pending = it.pending - id) }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                val message = failure(e, R.string.ent_alert_failed)
                stateFlow.update { it.copy(pending = it.pending - id, alerts = it.alerts.withEnabled(id, !enabled), error = message) }
            }
        }
    }

    private fun List<ServerAlert>.withEnabled(id: String, enabled: Boolean) = map { if (it.id == id) it.copy(isEnabled = enabled) else it }

    fun dismissError() = stateFlow.update { it.copy(error = null) }
}
