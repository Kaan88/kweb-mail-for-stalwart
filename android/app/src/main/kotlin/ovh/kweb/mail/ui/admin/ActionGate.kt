package ovh.kweb.mail.ui.admin

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/**
 * Lets one change at a time run on the server: a second one asked for while the first runs is refused (never
 * queued, so a double tap cannot do a thing twice). The call runs off the main thread, the answers on it.
 */
class ActionGate(private val scope: CoroutineScope, private val io: CoroutineDispatcher) {
    private val busyFlow = MutableStateFlow(false)

    val busy: StateFlow<Boolean> get() = busyFlow

    /** Starts [call] ([onStart] runs first, on the caller's thread); false when another change is running (nothing was started). */
    fun <T> run(call: () -> T, onStart: () -> Unit, onFailed: (Exception) -> Unit, onDone: (T) -> Unit): Boolean {
        if (!busyFlow.compareAndSet(expect = false, update = true)) return false
        onStart()
        scope.launch {
            try {
                val result = withContext(io) { call() }
                busyFlow.value = false
                onDone(result)
            } catch (e: CancellationException) {
                busyFlow.value = false
                throw e
            } catch (e: Exception) {
                busyFlow.value = false
                onFailed(e)
            }
        }
        return true
    }
}
