package ovh.kweb.mail.ui.shared

import androidx.annotation.StringRes
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.launch

/**
 * Deletes the one thing a shared screen shows (an event, a contact) through [action]. One at a time. A refusal says
 * the core's words (else [fallback]); success sends [SharedEvent.Done], and the screen goes back. [reset] ignores a
 * result still on its way. [P] is what the action needs to know: which part of a repeating event goes; `Unit` otherwise.
 */
class SharedDeleter<P>(
    private val scope: CoroutineScope,
    private val io: CoroutineDispatcher,
    @StringRes private val fallback: Int,
    private val action: (P) -> Unit,
) {
    private val flow = MutableStateFlow(false)
    private val bus = SharedEvents()
    private var generation = 0

    /** A delete is on its way. */
    val busy: StateFlow<Boolean> get() = flow
    val events: Flow<SharedEvent> get() = bus.flow

    fun delete(part: P) {
        if (flow.value) return
        flow.value = true
        val gen = ++generation
        scope.launch {
            val result = io.attempt { action(part) }
            if (gen != generation) return@launch
            flow.value = false
            result.onSuccess { bus.send(SharedEvent.Done) }.onFailure { e -> bus.say(SharedRules.message(e, fallback)) }
        }
    }

    fun reset() {
        generation++
        flow.value = false
        bus.clear()
    }
}
