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
import uniffi.kweb_core.DomainDetail
import uniffi.kweb_core.DomainRow

data class DomainsState(
    /** The text of the "add a domain" dialog; null while the dialog is closed. */
    val adding: String? = null,
    val busy: Boolean = false,
    val error: UiMessage? = null,
    val info: UiMessage? = null,
) {
    val addValid: Boolean get() = adding?.let { AdminRules.domainNameValid(it) } == true
}

/** The server's domains: the list, and a new domain by name. */
class DomainsController(
    scope: CoroutineScope,
    io: CoroutineDispatcher,
    private val backend: AdminBackend,
) {
    private val gate = ActionGate(scope, io)
    private val stateFlow = MutableStateFlow(DomainsState())

    /** The server gives every domain at once, so the list is one page. */
    val list = PagedLoader<DomainRow>(scope, io, AdminRules.PAGE, R.string.admin_domains_load_failed, { it.id }) { _, _, _ ->
        backend.domainRows().let { Page(it, it.size) }
    }
    val state: StateFlow<DomainsState> get() = stateFlow

    fun refresh() = list.refresh()

    fun openAdd() = stateFlow.update { it.copy(adding = "", error = null) }

    fun typeName(text: String) = stateFlow.update { if (it.adding != null) it.copy(adding = text, error = null) else it }

    fun dismissDialog() = stateFlow.update { it.copy(adding = null) }

    fun confirmAdd() {
        val name = stateFlow.value.adding ?: return
        if (!AdminRules.domainNameValid(name)) return
        gate.run(
            { backend.domainAdd(name.trim()) },
            { stateFlow.update { it.copy(busy = true, error = null, info = null) } },
            { e -> stateFlow.update { it.copy(busy = false, error = UiMessage.of(e, R.string.admin_domain_add_failed)) } },
        ) { _ ->
            stateFlow.update { it.copy(busy = false, adding = null, info = UiMessage.Res(R.string.admin_domain_added)) }
            list.reload(quiet = true)
        }
    }

    fun dismissError() = stateFlow.update { it.copy(error = null) }

    fun dismissInfo() = stateFlow.update { it.copy(info = null) }
}

enum class DomainPhase { LOADING, READY, FAILED }

sealed interface DomainDialog {
    /** Switching mail off asks first: the domain stops taking mail. */
    data object Disable : DomainDialog

    data class Delete(val typed: String = "") : DomainDialog
}

data class DomainState(
    val phase: DomainPhase = DomainPhase.LOADING,
    val domain: DomainDetail? = null,
    val dialog: DomainDialog? = null,
    val busy: Boolean = false,
    /** The domain is deleted (or was already gone): the screen closes. */
    val gone: Boolean = false,
    val error: UiMessage? = null,
    val info: UiMessage? = null,
)

/** One domain: its DNS records and DKIM keys, mail on and off, and Delete (after the name is typed). */
class DomainController(
    private val scope: CoroutineScope,
    private val io: CoroutineDispatcher,
    private val backend: AdminBackend,
    private val id: String,
) {
    private val gate = ActionGate(scope, io)
    private val stateFlow = MutableStateFlow(DomainState())
    private var ticket = 0

    val state: StateFlow<DomainState> get() = stateFlow

    fun refresh() {
        val mine = ++ticket
        scope.launch {
            try {
                val domain = withContext(io) { backend.domain(id) }
                if (mine != ticket) return@launch
                stateFlow.update { it.copy(phase = DomainPhase.READY, domain = domain, error = null) }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                if (mine != ticket) return@launch
                stateFlow.update {
                    if (it.domain != null) it.copy(error = UiMessage.of(e, R.string.admin_load_failed)) else it.copy(phase = DomainPhase.FAILED, error = UiMessage.of(e, R.string.admin_load_failed))
                }
            }
        }
    }

    /** The switch: on again at once, off after asking. */
    fun toggleEnabled() {
        val domain = stateFlow.value.domain ?: return
        if (domain.enabled) stateFlow.update { it.copy(dialog = DomainDialog.Disable) } else setEnabled(true)
    }

    fun confirmDisable() {
        stateFlow.update { it.copy(dialog = null) }
        setEnabled(false)
    }

    private fun setEnabled(on: Boolean) {
        gate.run({ backend.domainSetEnabled(id, on) }, ::started, { fail(it, R.string.admin_domain_change_failed) }) {
            stateFlow.update { it.copy(busy = false, info = UiMessage.Res(if (on) R.string.admin_domain_on_done else R.string.admin_domain_off_done)) }
            refresh()
        }
    }

    fun askDelete() = stateFlow.update { it.copy(dialog = DomainDialog.Delete()) }

    fun typeDelete(text: String) = stateFlow.update { if (it.dialog is DomainDialog.Delete) it.copy(dialog = DomainDialog.Delete(text)) else it }

    /** Delete asks for the domain's name; the core also refuses a domain that accounts still use. */
    fun confirmDelete() {
        val s = stateFlow.value
        val dialog = s.dialog as? DomainDialog.Delete ?: return
        val domain = s.domain ?: return
        if (!AdminRules.deleteConfirmed(dialog.typed, domain.name)) return
        stateFlow.update { it.copy(dialog = null) }
        gate.run({ backend.domainDelete(id) }, ::started, { fail(it, R.string.admin_domain_delete_failed) }) {
            stateFlow.update { it.copy(busy = false, gone = true) }
        }
    }

    fun copied() = stateFlow.update { it.copy(info = UiMessage.Res(R.string.admin_copied)) }

    private fun started() = stateFlow.update { it.copy(busy = true, error = null, info = null) }

    private fun fail(e: Exception, fallback: Int) = stateFlow.update { it.copy(busy = false, error = UiMessage.of(e, fallback)) }

    fun dismissDialog() = stateFlow.update { it.copy(dialog = null) }

    fun dismissError() = stateFlow.update { it.copy(error = null) }

    fun dismissInfo() = stateFlow.update { it.copy(info = null) }
}
