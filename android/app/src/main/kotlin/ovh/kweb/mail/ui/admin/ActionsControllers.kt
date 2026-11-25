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
import uniffi.kweb_core.DmarcCheckResult
import uniffi.kweb_core.Maintenance
import uniffi.kweb_core.SpamCheckResult

enum class ActionsPhase { LOADING, READY, FAILED }

data class ActionsState(
    val phase: ActionsPhase = ActionsPhase.LOADING,
    /** The actions the key may start (from the core's reading of its permissions). */
    val allowed: List<Maintenance> = emptyList(),
    val mayDmarc: Boolean = false,
    val maySpam: Boolean = false,
    /** The action the user is asked to confirm. */
    val asking: Maintenance? = null,
    /** The action that is running. */
    val running: Maintenance? = null,
    val error: UiMessage? = null,
    val info: UiMessage? = null,
    /** What the server warned about after the last run (the settings reload says licence problems here). */
    val warnings: List<String> = emptyList(),
)

/** The Actions screen: each simple action runs after a confirmation, one at a time, and shows what the server returned. */
class ActionsController(
    private val scope: CoroutineScope,
    private val io: CoroutineDispatcher,
    private val backend: AdminBackend,
) {
    private val gate = ActionGate(scope, io)
    private val stateFlow = MutableStateFlow(ActionsState())

    val state: StateFlow<ActionsState> get() = stateFlow

    /** Reads what the key may do. A screen that has it keeps it (a later call reads again quietly). */
    fun start() {
        scope.launch {
            try {
                val access = withContext(io) { backend.access() }
                stateFlow.update {
                    it.copy(
                        phase = ActionsPhase.READY,
                        allowed = access.allowedActions,
                        mayDmarc = access.mayDmarcCheck,
                        maySpam = access.maySpamCheck,
                    )
                }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                stateFlow.update {
                    if (it.phase == ActionsPhase.READY) it.copy(error = UiMessage.of(e, R.string.admin_maintenance_failed))
                    else it.copy(phase = ActionsPhase.FAILED, error = UiMessage.of(e, R.string.admin_maintenance_failed))
                }
            }
        }
    }

    /** Asks about an action the key may start; one that is running, or one the key may not start, is not asked about. */
    fun ask(what: Maintenance) = stateFlow.update {
        if (it.running == null && what in it.allowed) it.copy(asking = what, error = null, info = null, warnings = emptyList()) else it
    }

    fun dismissDialog() = stateFlow.update { it.copy(asking = null) }

    fun confirm() {
        val what = stateFlow.value.asking ?: return
        stateFlow.update { it.copy(asking = null) }
        gate.run(
            { backend.action(what) },
            { stateFlow.update { it.copy(running = what, error = null, info = null, warnings = emptyList()) } },
            { e -> stateFlow.update { it.copy(running = null, error = UiMessage.of(e, R.string.admin_maintenance_failed)) } },
        ) { outcome ->
            stateFlow.update {
                it.copy(running = null, info = UiMessage.Res(ServerActionRules.words(what).done), warnings = outcome.warnings)
            }
        }
    }

    fun dismissError() = stateFlow.update { it.copy(error = null) }

    fun dismissInfo() = stateFlow.update { it.copy(info = null, warnings = emptyList()) }
}

data class DmarcCheckState(
    val form: DmarcForm = DmarcForm(),
    val running: Boolean = false,
    val result: DmarcCheckResult? = null,
    val error: UiMessage? = null,
)

/** The DMARC check: a form, one run at a time, and the server's result. */
class DmarcCheckController(
    scope: CoroutineScope,
    io: CoroutineDispatcher,
    private val backend: AdminBackend,
) {
    private val gate = ActionGate(scope, io)
    private val stateFlow = MutableStateFlow(DmarcCheckState())

    val state: StateFlow<DmarcCheckState> get() = stateFlow

    fun edit(change: (DmarcForm) -> DmarcForm) = stateFlow.update { it.copy(form = change(it.form)) }

    fun run() {
        val form = stateFlow.value.form
        if (!form.canRun) return
        gate.run(
            { backend.dmarcCheck(form.toCheck()) },
            { stateFlow.update { it.copy(running = true, error = null, result = null) } },
            { e -> stateFlow.update { it.copy(running = false, error = UiMessage.of(e, R.string.admin_check_failed)) } },
        ) { result -> stateFlow.update { it.copy(running = false, result = result) } }
    }

    fun dismissError() = stateFlow.update { it.copy(error = null) }
}

data class SpamCheckState(
    val form: SpamForm = SpamForm(),
    val running: Boolean = false,
    val result: SpamCheckResult? = null,
    val error: UiMessage? = null,
)

/** The spam check: a pasted message and its session, one run at a time, and the server's verdict. */
class SpamCheckController(
    scope: CoroutineScope,
    io: CoroutineDispatcher,
    private val backend: AdminBackend,
) {
    private val gate = ActionGate(scope, io)
    private val stateFlow = MutableStateFlow(SpamCheckState())

    val state: StateFlow<SpamCheckState> get() = stateFlow

    fun edit(change: (SpamForm) -> SpamForm) = stateFlow.update { it.copy(form = change(it.form)) }

    fun run() {
        val form = stateFlow.value.form
        if (!form.canRun) return
        gate.run(
            { backend.spamCheck(form.toCheck()) },
            { stateFlow.update { it.copy(running = true, error = null, result = null) } },
            { e -> stateFlow.update { it.copy(running = false, error = UiMessage.of(e, R.string.admin_check_failed)) } },
        ) { result -> stateFlow.update { it.copy(running = false, result = result) } }
    }

    fun dismissError() = stateFlow.update { it.copy(error = null) }
}
