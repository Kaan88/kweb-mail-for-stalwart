package ovh.kweb.mail.ui.enterprise

import androidx.annotation.StringRes
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

enum class Phase { LOADING, READY, FAILED }

/** What to tell the user about a failed call: the core's own words when it gave some, else [fallback]. */
internal fun failure(error: Throwable, @StringRes fallback: Int): UiMessage = when {
    error is CoreException.Invalid && error.msg.isNotBlank() -> UiMessage.Text(error.msg)
    error is CoreException.Temporary && error.msg.isNotBlank() -> UiMessage.Text(error.msg)
    else -> UiMessage.Res(fallback)
}

data class LoadState<T>(val phase: Phase = Phase.LOADING, val value: T? = null, val error: UiMessage? = null)

/**
 * One value read from the core: loading, ready or failed with a retry. A read that a newer one has overtaken is
 * dropped. Used by the screens that only show something (one delivery, the tenants).
 */
class LoadController<T>(
    private val scope: CoroutineScope,
    private val io: CoroutineDispatcher,
    @StringRes private val failedText: Int,
    private val read: () -> T,
) {
    private val stateFlow = MutableStateFlow(LoadState<T>())
    private var started = false
    private var ticket = 0

    val state: StateFlow<LoadState<T>> get() = stateFlow

    /** Reads once when the screen first shows. */
    fun start() {
        if (started) return
        started = true
        reload()
    }

    fun reset() {
        ticket++
        started = false
        stateFlow.value = LoadState()
    }

    /** Reads again (Retry, pull down). What is on screen stays until the answer comes. */
    fun reload() {
        val mine = ++ticket
        stateFlow.update { if (it.value == null) LoadState() else it.copy(error = null) }
        scope.launch {
            try {
                val value = withContext(io) { read() }
                if (mine == ticket) stateFlow.value = LoadState(Phase.READY, value)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                if (mine != ticket) return@launch
                val message = failure(e, failedText)
                stateFlow.update { if (it.value == null) LoadState(Phase.FAILED, null, message) else it.copy(phase = Phase.READY, error = message) }
            }
        }
    }

    fun dismissError() = stateFlow.update { it.copy(error = null) }
}
