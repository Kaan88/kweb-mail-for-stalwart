package ovh.kweb.mail.ui.sending

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.UiMessage
import uniffi.kweb_core.IdentityDetail

enum class SendersPhase { LOADING, READY, FAILED }

/** A question on top of the screen. */
enum class SendersDialog { Delete, Discard }

data class SendersState(
    val phase: SendersPhase = SendersPhase.LOADING,
    val items: List<IdentityDetail> = emptyList(),
    /** The form that is open, with what it was when it was opened. */
    val editor: SenderForm? = null,
    val original: SenderForm? = null,
    val busy: Boolean = false,
    val error: UiMessage? = null,
    val info: UiMessage? = null,
    val dialog: SendersDialog? = null,
    /** The form's problem shown under the field after a try to save. */
    val problem: SendersRules.Problem? = null,
) {
    val dirty: Boolean get() = editor != null && original != null && SendersRules.dirty(original, editor)
}

/**
 * The state and actions of the senders screens. The state is snapshot state and not a flow: a text field fed
 * through a flow's extra hop loses keystrokes when the user types fast. Everything changes on [scope]'s thread;
 * the core is asked on [io]; a read that a newer one has overtaken is dropped; a change is refused while another is running.
 */
class SendersController(
    private val scope: CoroutineScope,
    private val io: CoroutineDispatcher,
    private val backend: SendersBackend,
) {
    var state by mutableStateOf(SendersState())
        private set

    private var started = false

    /** Counts reads; an answer is used only when no newer read was started. */
    private var ticket = 0

    fun start() {
        if (started) return
        started = true
        scope.launch { read() }
    }

    fun reset() {
        ticket++
        started = false
        state = SendersState()
    }

    /** Reads again (Retry, the screen coming back). A form that is open stays open. */
    fun load() {
        if (state.busy) return
        scope.launch { read() }
    }

    private suspend fun read() {
        val mine = ++ticket
        if (state.items.isEmpty()) state = state.copy(phase = SendersPhase.LOADING)
        try {
            val items = withContext(io) { backend.identityDetails() }
            if (mine != ticket) return
            state = state.copy(phase = SendersPhase.READY, items = items, error = null)
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            if (mine != ticket) return
            state = if (state.phase == SendersPhase.READY) state.copy(error = UiMessage.of(e, R.string.senders_load_failed)) else state.copy(phase = SendersPhase.FAILED)
        }
    }

    fun dismissError() {
        state = state.copy(error = null)
    }

    fun dismissInfo() {
        state = state.copy(info = null)
    }

    fun dismissDialog() {
        state = state.copy(dialog = null)
    }

    // ---- the form ----

    fun openNew() {
        if (state.busy) return
        val form = SendersRules.blank()
        state = state.copy(editor = form, original = form, error = null, info = null, problem = null)
    }

    fun open(id: String) {
        if (state.busy) return
        val item = state.items.firstOrNull { it.id == id } ?: return
        val form = SendersRules.form(item)
        state = state.copy(editor = form, original = form, error = null, info = null, problem = null)
    }

    private fun change(edit: (SenderForm) -> SenderForm) {
        val form = state.editor ?: return
        if (state.busy) return
        state = state.copy(editor = edit(form), error = null, problem = null)
    }

    fun setName(value: String) = change { it.copy(name = value) }

    fun setEmail(value: String) = change { if (it.isNew) it.copy(email = value) else it }

    fun setReplyTo(value: String) = change { it.copy(replyTo = value) }

    fun setSignature(value: String) = change { it.copy(signature = value) }

    fun setUseForNew(value: Boolean) = change { if (it.isDefault) it else it.copy(useForNew = value) }

    /** Back from the form: asks first when something was changed. */
    fun leaveEditor() {
        if (state.busy) return
        if (state.dirty) state = state.copy(dialog = SendersDialog.Discard) else closeEditor()
    }

    fun discardEditor() {
        closeEditor()
    }

    private fun closeEditor() {
        state = state.copy(editor = null, original = null, dialog = null, error = null, problem = null)
    }

    // ---- changes ----

    /** Saves the form: the sender, and then (when asked) the default. The core's refusal is shown and the form stays. */
    fun save() {
        val form = state.editor ?: return
        if (state.busy) return
        val problem = SendersRules.problem(form)
        if (problem != null) {
            state = state.copy(problem = problem)
            return
        }
        state = state.copy(busy = true, error = null, problem = null)
        scope.launch {
            var saved: IdentityDetail? = null
            try {
                val done = withContext(io) {
                    backend.saveIdentity(form.id, form.name.trim(), form.email.trim(), form.replyTo.trim(), SendersRules.cleanSignature(form.signature))
                }
                saved = done
                if (SendersRules.makesDefault(form)) withContext(io) { backend.setDefaultIdentity(done.id) }
                finish(info = R.string.senders_saved)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                if (saved == null) {
                    state = state.copy(busy = false, error = UiMessage.of(e, R.string.senders_save_failed))
                } else {
                    // The sender is stored; only the default failed. The form is done, the list says what happened.
                    finish(error = UiMessage.of(e, R.string.senders_default_failed))
                }
            }
        }
    }

    fun askDelete() {
        val form = state.editor ?: return
        if (state.busy || form.isNew || !form.mayDelete) return
        state = state.copy(dialog = SendersDialog.Delete)
    }

    fun confirmDelete() {
        val form = state.editor ?: return
        val id = form.id ?: return
        if (state.busy) return
        state = state.copy(busy = true, dialog = null, error = null)
        scope.launch {
            try {
                withContext(io) { backend.deleteIdentity(id) }
                finish(info = R.string.senders_deleted, removed = id)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                state = state.copy(busy = false, error = UiMessage.of(e, R.string.senders_delete_failed))
            }
        }
    }

    /** A change went through: the form closes and the list is read again (the default may have moved). */
    private suspend fun finish(info: Int? = null, error: UiMessage? = null, removed: String? = null) {
        val mine = ++ticket
        val items = try {
            withContext(io) { backend.identityDetails() }
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            null
        }
        // A newer read has started: its answer is the one to show.
        if (mine == ticket && items != null) state = state.copy(items = items, phase = SendersPhase.READY)
        // A deleted sender is gone whatever the re-read did: it is taken off the list here, and that list is not stale.
        if (items == null && removed != null) state = state.copy(items = state.items.filter { it.id != removed })
        state = state.copy(
            busy = false, editor = null, original = null, dialog = null, problem = null,
            info = info?.let { UiMessage.Res(it) }, error = error ?: if (items == null && removed == null) UiMessage.Res(R.string.senders_stale) else null,
        )
    }
}
