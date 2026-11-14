package ovh.kweb.mail.ui.account

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
import uniffi.kweb_core.Exception as CoreException
import uniffi.kweb_core.MaskedAddress
import uniffi.kweb_core.MaskedAddresses

enum class MaskedPhase { LOADING, READY, FAILED, NONE }

/** A question on top of the list. */
sealed interface MaskedDialog {
    data class Delete(val address: MaskedAddress) : MaskedDialog

    data class Note(val address: MaskedAddress) : MaskedDialog
}

/** The sheet behind "New masked address": the form, then the address that was made. */
sealed interface MaskedSheet {
    data object Make : MaskedSheet

    data class Created(val address: MaskedAddress) : MaskedSheet
}

data class MaskedState(
    val phase: MaskedPhase = MaskedPhase.LOADING,
    val data: MaskedAddresses? = null,
    /** The core's words when the server does not offer masked addresses. */
    val unsupported: UiMessage? = null,
    /** A make, delete or note change is on its way: the other buttons wait. */
    val busy: Boolean = false,
    /** Addresses whose switch is on its way to the server. */
    val pending: Set<String> = emptySet(),
    val error: UiMessage? = null,
    val dialog: MaskedDialog? = null,
    val sheet: MaskedSheet? = null,
    /** The core's refusal of the sheet's form (the limit reached from another device, say). */
    val sheetError: UiMessage? = null,
) {
    val addresses: List<MaskedAddress> get() = data?.addresses.orEmpty()
    val limit: Int? get() = data?.limit?.toInt()
    val atLimit: Boolean get() = MaskedRules.atLimit(addresses.size, limit)
}

/**
 * The state and actions of the masked-address screen. The core is asked off the main thread ([io]); an answer
 * a newer read has overtaken is dropped; a change is refused while another one is on its way (a switch only
 * waits for its own earlier change).
 */
class MaskedController(
    private val scope: CoroutineScope,
    private val io: CoroutineDispatcher,
    private val backend: AccountBackend,
) {
    private val stateFlow = MutableStateFlow(MaskedState())
    private var started = false
    private var ticket = 0
    private var epoch = 0

    val state: StateFlow<MaskedState> get() = stateFlow

    fun start() {
        if (started) return
        started = true
        load()
    }

    fun reset() {
        ticket++
        epoch++
        started = false
        stateFlow.value = MaskedState()
    }

    /** Reads the list (Retry, and the screen coming back). What is on screen stays while it is read. */
    fun load() {
        scope.launch { readNow() }
    }

    private suspend fun readNow() {
        val mine = ++ticket
        stateFlow.update { if (it.data == null) it.copy(phase = MaskedPhase.LOADING) else it }
        try {
            val data = withContext(io) { backend.maskedAddresses() }
            if (mine != ticket) return
            stateFlow.update { it.copy(phase = MaskedPhase.READY, data = keepPending(it, data), unsupported = null, error = null) }
        } catch (e: CancellationException) {
            throw e
        } catch (e: CoreException.Invalid) {
            if (mine != ticket) return
            stateFlow.update {
                if (it.data != null) it.copy(error = UiMessage.of(e, R.string.masked_load_failed))
                else it.copy(phase = MaskedPhase.NONE, unsupported = UiMessage.of(e, R.string.masked_none))
            }
        } catch (e: Exception) {
            if (mine != ticket) return
            stateFlow.update {
                if (it.data != null) it.copy(error = UiMessage.Res(R.string.masked_load_failed)) else it.copy(phase = MaskedPhase.FAILED)
            }
        }
    }

    /**
     * A read that started while a switch was on its way may have been answered before the server applied it:
     * for those addresses the switch the user chose (what is on screen now) wins over the answer.
     */
    private fun keepPending(now: MaskedState, read: MaskedAddresses): MaskedAddresses {
        if (now.pending.isEmpty()) return read
        val chosen = now.addresses.associate { it.id to it.enabled }
        return read.copy(addresses = read.addresses.map { a -> if (a.id in now.pending && chosen[a.id] != null) a.copy(enabled = chosen.getValue(a.id)) else a })
    }

    fun dismissError() = stateFlow.update { it.copy(error = null) }

    // ---- the switch ----

    /** Switches an address at once on screen and on the server; a refusal puts the switch back. */
    fun setEnabled(id: String, enabled: Boolean) {
        val current = stateFlow.value
        if (id in current.pending || current.addresses.none { it.id == id }) return
        val mine = epoch
        // A read that started before this change would bring the old switch back.
        ticket++
        stateFlow.update { s -> s.copy(pending = s.pending + id, data = s.data?.let { it.copy(addresses = MaskedRules.withEnabled(it.addresses, id, enabled)) }) }
        scope.launch {
            try {
                withContext(io) { backend.updateMaskedAddress(id, enabled, null) }
                if (mine != epoch) return@launch
                stateFlow.update { it.copy(pending = it.pending - id) }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                if (mine != epoch) return@launch
                stateFlow.update { s ->
                    s.copy(
                        pending = s.pending - id,
                        data = s.data?.let { it.copy(addresses = MaskedRules.withEnabled(it.addresses, id, !enabled)) },
                        error = UiMessage.of(e, R.string.masked_change_failed),
                    )
                }
            }
        }
    }

    // ---- delete and note ----

    fun askDelete(address: MaskedAddress) = stateFlow.update { if (it.busy) it else it.copy(dialog = MaskedDialog.Delete(address)) }

    fun askNote(address: MaskedAddress) = stateFlow.update { if (it.busy) it else it.copy(dialog = MaskedDialog.Note(address)) }

    fun dismissDialog() = stateFlow.update { if (it.busy) it else it.copy(dialog = null) }

    fun confirmDelete() {
        val dialog = stateFlow.value.dialog as? MaskedDialog.Delete ?: return
        change(R.string.masked_delete_failed, { backend.deleteMaskedAddress(dialog.address.id) }) {
            it.copy(dialog = null, data = it.data?.let { d -> d.copy(addresses = d.addresses.filter { a -> a.id != dialog.address.id }) })
        }
    }

    fun saveNote(text: String) {
        val dialog = stateFlow.value.dialog as? MaskedDialog.Note ?: return
        val note = text.trim()
        val now = stateFlow.value
        // The switch is written with the note: take it from the row as it is now, not as it was when the dialog opened.
        val row = now.addresses.firstOrNull { it.id == dialog.address.id }
        if (row == null) {
            dismissDialog()
            return
        }
        if (dialog.address.id in now.pending) return
        if (note == row.description) {
            dismissDialog()
            return
        }
        change(R.string.masked_change_failed, { backend.updateMaskedAddress(row.id, row.enabled, note) }) {
            it.copy(dialog = null, data = it.data?.let { d -> d.copy(addresses = d.addresses.map { a -> if (a.id == dialog.address.id) a.copy(description = note) else a }) })
        }
    }

    /** One change of the list that waits for the server; the others wait for it. A failure closes the dialog and says why. */
    private fun change(fallback: Int, call: () -> Unit, onDone: (MaskedState) -> MaskedState) {
        if (stateFlow.value.busy) return
        val mine = epoch
        stateFlow.update { it.copy(busy = true) }
        scope.launch {
            try {
                withContext(io) { call() }
                if (mine != epoch) return@launch
                stateFlow.update { onDone(it).copy(busy = false) }
                readNow()
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                if (mine != epoch) return@launch
                stateFlow.update { it.copy(busy = false, dialog = null, error = UiMessage.of(e, fallback)) }
            }
        }
    }

    // ---- the sheet ----

    fun openSheet() = stateFlow.update { if (it.busy || it.atLimit) it else it.copy(sheet = MaskedSheet.Make, sheetError = null) }

    fun closeSheet() = stateFlow.update { if (it.busy) it else it.copy(sheet = null, sheetError = null) }

    /** Makes the address; the sheet turns into the large address. A refusal (the limit, say) stays in the sheet. */
    fun create(description: String, site: String, prefix: String) {
        if (stateFlow.value.busy || stateFlow.value.sheet != MaskedSheet.Make) return
        if (!MaskedRules.prefixIsValid(prefix)) return
        val mine = epoch
        stateFlow.update { it.copy(busy = true, sheetError = null) }
        scope.launch {
            try {
                val made = withContext(io) { backend.createMaskedAddress(description.trim(), MaskedRules.cleanSite(site), prefix.trim()) }
                if (mine != epoch) return@launch
                stateFlow.update { s ->
                    s.copy(busy = false, sheet = MaskedSheet.Created(made), data = s.data?.let { it.copy(addresses = listOf(made) + it.addresses.filter { a -> a.id != made.id }) })
                }
                readNow()
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                if (mine != epoch) return@launch
                stateFlow.update { it.copy(busy = false, sheetError = UiMessage.of(e, R.string.masked_create_failed)) }
                // The limit may have been reached from another device: read the list again so the button says so.
                if (e is CoreException.Invalid) readNow()
            }
        }
    }
}
