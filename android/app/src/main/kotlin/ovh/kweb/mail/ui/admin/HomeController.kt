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
import uniffi.kweb_core.AdminSummary

data class HomeState(
    val loaded: Boolean = false,
    val refreshing: Boolean = false,
    val summary: AdminSummary? = null,
    val error: UiMessage? = null,
)

/** The admin home: the figures of the areas. The access (what the key opens) is [access]; a key taken reads the figures. */
class HomeController(
    private val scope: CoroutineScope,
    private val io: CoroutineDispatcher,
    private val backend: AdminBackend,
) {
    private val stateFlow = MutableStateFlow(HomeState())
    private var ticket = 0

    val access = AccessController(scope, io, backend, onChanged = { if (it.connected) readSummary() })
    val state: StateFlow<HomeState> get() = stateFlow

    /** The screen opens, comes back or is pulled down: reads the access and, when a key opens something, the figures. */
    fun refresh() {
        access.load()
        readSummary()
    }

    private fun readSummary() {
        val mine = ++ticket
        stateFlow.update { it.copy(refreshing = true) }
        scope.launch {
            try {
                val summary = withContext(io) { backend.summary() }
                if (mine != ticket) return@launch
                stateFlow.update { it.copy(loaded = true, refreshing = false, summary = summary, error = null) }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                if (mine != ticket) return@launch
                // Without a key the summary is refused: the figures stay empty and the locked card says what is wrong.
                stateFlow.update { it.copy(loaded = true, refreshing = false, error = UiMessage.of(e, R.string.admin_load_failed)) }
            }
        }
    }

    fun dismissError() = stateFlow.update { it.copy(error = null) }
}
