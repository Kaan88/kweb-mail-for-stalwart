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
import uniffi.kweb_core.AdminAccess
import uniffi.kweb_core.Exception as CoreException

enum class AccessPhase { LOADING, READY, FAILED }

/** A question on top of the access screen. The key itself is never in it: only how to show it. */
sealed interface AccessDialog {
    /** A key was read from the clipboard; [shown] is its first eight characters and dots. */
    data class ConfirmKey(val shown: String) : AccessDialog

    data object Forget : AccessDialog
}

data class AccessState(
    val phase: AccessPhase = AccessPhase.LOADING,
    val access: AdminAccess? = null,
    /** The server turned the pasted key down; [refusal] is its reason in the core's words. */
    val refused: Boolean = false,
    val refusal: UiMessage? = null,
    val busy: Boolean = false,
    val error: UiMessage? = null,
    val dialog: AccessDialog? = null,
) {
    val kind: AccessKind get() = AdminRules.accessKind(access, refused)
}

/**
 * The admin key of one account: what it opens, pasting one, forgetting it. A key read from the clipboard waits in
 * [pendingKey] (not in the state, which is saved nowhere) until the user confirms or declines, and is let go of
 * either way, also when the answer of the server has not come yet.
 */
class AccessController(
    private val scope: CoroutineScope,
    private val io: CoroutineDispatcher,
    private val backend: AdminBackend,
    /** Called on the main thread when the access changed (a key taken or forgotten). */
    private val onChanged: (AdminAccess) -> Unit = {},
) {
    private val stateFlow = MutableStateFlow(AccessState())
    private var ticket = 0
    private var pendingKey: String? = null

    val state: StateFlow<AccessState> get() = stateFlow

    /** Asks the server what the key opens. The screen coming back asks again without the loading state. */
    fun load() {
        val mine = ++ticket
        if (stateFlow.value.access == null) stateFlow.update { it.copy(phase = AccessPhase.LOADING) }
        scope.launch {
            try {
                val access = withContext(io) { backend.access() }
                if (mine != ticket) return@launch
                stateFlow.update {
                    it.copy(phase = AccessPhase.READY, access = access, error = null, refused = if (access.connected) false else it.refused, refusal = if (access.connected) null else it.refusal)
                }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                if (mine != ticket) return@launch
                stateFlow.update { if (it.access != null) it.copy(error = UiMessage.of(e, R.string.admin_load_failed)) else it.copy(phase = AccessPhase.FAILED) }
            }
        }
    }

    /** "Paste an API key": [clipboardText] is what the clipboard held when the user tapped. */
    fun paste(clipboardText: CharSequence?) {
        if (stateFlow.value.busy) return
        val key = AdminRules.keyFromClipboard(clipboardText)
        if (key == null) {
            stateFlow.update { it.copy(error = UiMessage.Res(R.string.admin_clipboard_no_key)) }
            return
        }
        pendingKey = key
        stateFlow.update { it.copy(dialog = AccessDialog.ConfirmKey(AdminRules.maskKey(key)), error = null) }
    }

    /** Yes, use this key. */
    fun confirmKey() {
        val key = pendingKey
        pendingKey = null
        stateFlow.update { it.copy(dialog = null) }
        if (key == null || stateFlow.value.busy) return
        ++ticket // an older read of the access must not overwrite the answer
        stateFlow.update { it.copy(busy = true, error = null) }
        scope.launch {
            try {
                val access = withContext(io) { backend.connect(key) }
                stateFlow.update { it.copy(busy = false, phase = AccessPhase.READY, access = access, refused = false, refusal = null) }
                onChanged(access)
            } catch (e: CancellationException) {
                throw e
            } catch (e: CoreException.Invalid) {
                // Nothing was stored. The card says why in the core's words.
                stateFlow.update { it.copy(busy = false, refused = true, refusal = UiMessage.of(e, R.string.admin_key_refused_default)) }
            } catch (e: Exception) {
                stateFlow.update { it.copy(busy = false, error = UiMessage.of(e, R.string.admin_connect_failed)) }
            }
        }
    }

    /** No, not this key. */
    fun declineKey() {
        pendingKey = null
        stateFlow.update { it.copy(dialog = null) }
    }

    // A key the server turned down is still on the phone, so it can be taken off too.
    fun askForget() = stateFlow.update { if (it.access?.connected == true || it.access?.keyRefused == true) it.copy(dialog = AccessDialog.Forget) else it }

    fun forget() {
        stateFlow.update { it.copy(dialog = null, busy = true, error = null) }
        ++ticket
        scope.launch {
            try {
                withContext(io) { backend.disconnect() }
                val access = withContext(io) { backend.access() }
                stateFlow.update { it.copy(busy = false, access = access, refused = false, refusal = null) }
                onChanged(access)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                stateFlow.update { it.copy(busy = false, error = UiMessage.of(e, R.string.admin_forget_failed)) }
            }
        }
    }

    fun dismissDialog() {
        pendingKey = null
        stateFlow.update { it.copy(dialog = null) }
    }

    fun dismissError() = stateFlow.update { it.copy(error = null) }

    /** The screen is gone: let go of anything held. */
    fun clear() {
        pendingKey = null
        stateFlow.update { it.copy(dialog = null) }
    }
}
