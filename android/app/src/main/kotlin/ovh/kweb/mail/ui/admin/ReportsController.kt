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
import uniffi.kweb_core.ReportDetail
import uniffi.kweb_core.ReportKind
import uniffi.kweb_core.ReportRow

data class ReportsState(
    /** The kinds the key may read; empty until the access is known. */
    val kinds: List<ReportKind> = emptyList(),
    val kind: ReportKind? = null,
)

/** The reports other servers sent: a list per kind (the chips), newest first, in pages. */
class ReportsController(
    private val scope: CoroutineScope,
    private val io: CoroutineDispatcher,
    private val backend: AdminBackend,
) {
    private val stateFlow = MutableStateFlow(ReportsState())
    private var ticket = 0

    /** A page is asked for the kind of the first page on screen (the tag), whatever chip is selected now. */
    val list = PagedLoader<ReportRow>(scope, io, AdminRules.PAGE, R.string.admin_reports_load_failed, { it.id }) { offset, limit, tag ->
        val kind = if (offset == 0u) stateFlow.value.kind else tag as? ReportKind
        if (kind == null) Page(emptyList(), 0) else backend.reports(kind, offset, limit).let { Page(it.reports, it.total.toInt(), kind) }
    }
    val state: StateFlow<ReportsState> get() = stateFlow

    /** The screen opens or comes back: the access says which kinds there are, then the list is read. */
    fun refresh() {
        val mine = ++ticket
        scope.launch {
            val kinds = try {
                withContext(io) { AdminRules.reportKinds(backend.access()) }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                // The access could not be read: try every kind, so the list says why it failed instead of "no reports".
                stateFlow.value.kinds.ifEmpty { ReportKind.entries.toList() }
            }
            if (mine != ticket) return@launch
            stateFlow.update { s ->
                val kind = s.kind?.takeIf { it in kinds } ?: kinds.firstOrNull()
                s.copy(kinds = kinds, kind = kind)
            }
            list.reload(quiet = true)
        }
    }

    fun select(kind: ReportKind) {
        if (stateFlow.value.kind == kind) return
        stateFlow.update { it.copy(kind = kind) }
        list.reload(quiet = false)
    }
}

enum class ReportPhase { LOADING, READY, FAILED }

data class ReportState(
    val phase: ReportPhase = ReportPhase.LOADING,
    val report: ReportDetail? = null,
    val askDelete: Boolean = false,
    val busy: Boolean = false,
    /** The report is deleted (or was already gone): the screen closes. */
    val gone: Boolean = false,
    val error: UiMessage? = null,
)

/** One received report, and Delete. */
class ReportController(
    private val scope: CoroutineScope,
    private val io: CoroutineDispatcher,
    private val backend: AdminBackend,
    private val kind: ReportKind,
    private val id: String,
) {
    private val gate = ActionGate(scope, io)
    private val stateFlow = MutableStateFlow(ReportState())
    private var ticket = 0

    val state: StateFlow<ReportState> get() = stateFlow

    fun refresh() {
        val mine = ++ticket
        scope.launch {
            try {
                val report = withContext(io) { backend.report(kind, id) }
                if (mine != ticket) return@launch
                stateFlow.update { it.copy(phase = ReportPhase.READY, report = report, error = null) }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                if (mine != ticket) return@launch
                stateFlow.update {
                    if (it.report != null) it.copy(error = UiMessage.of(e, R.string.admin_load_failed)) else it.copy(phase = ReportPhase.FAILED, error = UiMessage.of(e, R.string.admin_load_failed))
                }
            }
        }
    }

    fun askDelete() = stateFlow.update { it.copy(askDelete = true) }

    fun dismissDialog() = stateFlow.update { it.copy(askDelete = false) }

    fun confirmDelete() {
        stateFlow.update { it.copy(askDelete = false) }
        gate.run(
            { backend.reportDelete(kind, id) },
            { stateFlow.update { it.copy(busy = true, error = null) } },
            { e -> stateFlow.update { it.copy(busy = false, error = UiMessage.of(e, R.string.admin_report_delete_failed)) } },
        ) { stateFlow.update { it.copy(busy = false, gone = true) } }
    }

    fun dismissError() = stateFlow.update { it.copy(error = null) }
}
