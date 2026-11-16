package ovh.kweb.mail.ui.sender

import android.util.Log
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import ovh.kweb.mail.background.Background
import uniffi.kweb_core.Core
import uniffi.kweb_core.SenderCheck

/**
 * Asks the core, once per message and visit, what the user's server found out about the sender of an opened message.
 * A failed question shows nothing and is asked again the next time the card opens. Nothing it holds is written
 * anywhere: the answers live in memory and go with [reset]. Never used for a shared account's mail.
 */
class SenderController(
    private val scope: CoroutineScope,
    private val io: CoroutineDispatcher,
    private val core: () -> Core,
) {
    private val stateFlow = MutableStateFlow<Map<String, SenderCheck>>(emptyMap())
    private val asked = HashSet<String>()
    private var generation = 0

    val state: StateFlow<Map<String, SenderCheck>> get() = stateFlow

    /** Main thread. */
    fun check(messageId: String) {
        if (!asked.add(messageId)) return
        val mine = generation
        scope.launch {
            val result = try {
                withContext(io) { core().senderCheck(messageId) }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                // The class only: a message of the core may name the mail.
                Log.w(Background.TAG, "checking a sender failed: ${e.javaClass.simpleName}")
                if (mine == generation) asked.remove(messageId)
                return@launch
            }
            if (mine == generation) stateFlow.update { it + (messageId to result) }
        }
    }

    fun reset() {
        generation++
        asked.clear()
        stateFlow.value = emptyMap()
    }
}
