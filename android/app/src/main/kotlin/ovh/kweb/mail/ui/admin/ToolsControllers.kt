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
import uniffi.kweb_core.BlockedAddress
import uniffi.kweb_core.LogLine
import uniffi.kweb_core.Maintenance

data class BlockedState(
    /** The address Unblock asked about. */
    val asking: BlockedAddress? = null,
    val busy: Boolean = false,
    val error: UiMessage? = null,
    val info: UiMessage? = null,
)

/** The blocked network addresses: the list in pages and Unblock (after asking). */
class BlockedController(
    scope: CoroutineScope,
    io: CoroutineDispatcher,
    private val backend: AdminBackend,
) {
    private val gate = ActionGate(scope, io)
    private val stateFlow = MutableStateFlow(BlockedState())

    val list = PagedLoader<BlockedAddress>(scope, io, AdminRules.PAGE, R.string.admin_blocked_load_failed, { it.id }) { offset, limit, _ ->
        // The server does not say how many there are: a full page means there may be more.
        Page(backend.blocked(offset, limit), null)
    }
    val state: StateFlow<BlockedState> get() = stateFlow

    fun refresh() = list.refresh()

    fun askUnblock(address: BlockedAddress) = stateFlow.update { it.copy(asking = address) }

    fun dismissDialog() = stateFlow.update { it.copy(asking = null) }

    fun confirmUnblock() {
        val address = stateFlow.value.asking ?: return
        stateFlow.update { it.copy(asking = null) }
        gate.run(
            { backend.unblock(address.id) },
            { stateFlow.update { it.copy(busy = true, error = null, info = null) } },
            { e -> stateFlow.update { it.copy(busy = false, error = UiMessage.of(e, R.string.admin_unblock_failed)) } },
        ) {
            list.change { items -> items.filter { it.id != address.id } }
            stateFlow.update { it.copy(busy = false, info = UiMessage.Res(R.string.admin_unblock_done)) }
        }
    }

    fun dismissError() = stateFlow.update { it.copy(error = null) }

    fun dismissInfo() = stateFlow.update { it.copy(info = null) }
}

enum class LogPhase { IDLE, LOADING, READY, FAILED }

data class LogsState(
    /** What the search field holds. */
    val text: String = "",
    /** The text the lines on screen were searched with. */
    val searched: String = "",
    val level: LevelFilter = LevelFilter.All,
    val phase: LogPhase = LogPhase.IDLE,
    val lines: List<LogLine> = emptyList(),
    val error: UiMessage? = null,
) {
    /** The lines the level pills let through. */
    val shown: List<LogLine> get() = lines.filter { AdminRules.matches(level, it) }
}

/** The log search. The server cannot filter by level, so the pills only narrow what was loaded. */
class LogsController(
    private val scope: CoroutineScope,
    private val io: CoroutineDispatcher,
    private val backend: AdminBackend,
) {
    private val stateFlow = MutableStateFlow(LogsState())
    private var ticket = 0

    val state: StateFlow<LogsState> get() = stateFlow

    fun typeText(text: String) = stateFlow.update { it.copy(text = text) }

    fun setLevel(level: LevelFilter) = stateFlow.update { it.copy(level = level) }

    /** The first look of the screen: the newest lines. A screen that has lines already keeps them (the sync button reads again). */
    fun start() {
        if (stateFlow.value.phase == LogPhase.IDLE) search()
    }

    /** Searches for what the field holds (or the newest lines when it is blank). */
    fun search() {
        val mine = ++ticket
        val text = stateFlow.value.text.trim()
        stateFlow.update { it.copy(phase = if (it.lines.isEmpty()) LogPhase.LOADING else it.phase, error = null) }
        scope.launch {
            try {
                val lines = withContext(io) { backend.logs(text, AdminRules.LOG_LIMIT) }
                if (mine != ticket) return@launch
                stateFlow.update { it.copy(phase = LogPhase.READY, lines = lines, searched = text, error = null) }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                if (mine != ticket) return@launch
                stateFlow.update {
                    val error = UiMessage.of(e, R.string.admin_logs_load_failed)
                    if (it.lines.isNotEmpty()) it.copy(error = error, phase = LogPhase.READY) else it.copy(phase = LogPhase.FAILED, error = error)
                }
            }
        }
    }

    fun dismissError() = stateFlow.update { it.copy(error = null) }
}

data class MaintenanceState(
    /** The action the user is asked to confirm. */
    val asking: Maintenance? = null,
    /** The action that is running. */
    val running: Maintenance? = null,
    val error: UiMessage? = null,
    val info: UiMessage? = null,
)

/** The maintenance actions that reload the server's settings or certificates, each after a confirmation. */
class MaintenanceController(
    scope: CoroutineScope,
    io: CoroutineDispatcher,
    private val backend: AdminBackend,
) {
    private val gate = ActionGate(scope, io)
    private val stateFlow = MutableStateFlow(MaintenanceState())

    val state: StateFlow<MaintenanceState> get() = stateFlow

    fun ask(what: Maintenance) = stateFlow.update { if (it.running == null) it.copy(asking = what, error = null, info = null) else it }

    fun dismissDialog() = stateFlow.update { it.copy(asking = null) }

    fun confirm() {
        val what = stateFlow.value.asking ?: return
        stateFlow.update { it.copy(asking = null) }
        gate.run(
            { backend.maintenance(what) },
            { stateFlow.update { it.copy(running = what, error = null, info = null) } },
            { e -> stateFlow.update { it.copy(running = null, error = UiMessage.of(e, R.string.admin_maintenance_failed)) } },
        ) {
            stateFlow.update { it.copy(running = null, info = UiMessage.Res(if (what == Maintenance.RELOAD_CERTIFICATES) R.string.admin_reload_certs_done else R.string.admin_reload_settings_done)) }
        }
    }

    fun dismissError() = stateFlow.update { it.copy(error = null) }

    fun dismissInfo() = stateFlow.update { it.copy(info = null) }
}
