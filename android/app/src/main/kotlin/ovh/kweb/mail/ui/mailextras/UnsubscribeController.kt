package ovh.kweb.mail.ui.mailextras

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

/** What the Unsubscribe chip of one message shows. A message without an entry has no chip. */
sealed interface UnsubEntry {
    /** There is a way to leave the list. */
    data class Available(val way: UnsubscribeRules.Way, val webUrl: String?) : UnsubEntry

    /** The request is on its way. */
    data object Working : UnsubEntry

    /** Asked for; the chip says so until the screen closes. */
    data object Requested : UnsubEntry
}

/**
 * Asks, quietly, whether an opened message can be unsubscribed from, and does it. A failed question shows
 * nothing; a failed unsubscribe is reported through [onError] and the chip comes back. [reset] forgets
 * everything, answers that are still on their way included.
 */
class UnsubscribeController(
    private val scope: CoroutineScope,
    private val io: CoroutineDispatcher,
    private val backend: MailExtrasBackend,
    private val onError: (UiMessage) -> Unit,
) {
    private val stateFlow = MutableStateFlow<Map<String, UnsubEntry>>(emptyMap())
    private val asked = HashSet<String>()
    private var generation = 0

    val state: StateFlow<Map<String, UnsubEntry>> get() = stateFlow

    /** Asks once per message and visit; a question that failed may be asked again the next time. Main thread. */
    fun check(messageId: String) {
        if (!asked.add(messageId)) return
        val mine = generation
        scope.launch {
            val info = try {
                withContext(io) { backend.unsubscribeInfo(messageId) }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                if (mine == generation) asked.remove(messageId)
                return@launch
            }
            if (mine != generation) return@launch
            val usable = UnsubscribeRules.way(info) ?: return@launch
            stateFlow.update { if (messageId in it) it else it + (messageId to UnsubEntry.Available(usable, info?.webUrl)) }
        }
    }

    /** The user said yes. For the browser nothing is done here: the screen opens the page ([UnsubEntry.Available.webUrl]). */
    fun confirm(messageId: String) {
        val entry = stateFlow.value[messageId] as? UnsubEntry.Available ?: return
        if (entry.way == UnsubscribeRules.Way.BROWSER) return
        val mine = generation
        stateFlow.update { it + (messageId to UnsubEntry.Working) }
        scope.launch {
            try {
                withContext(io) { backend.unsubscribe(messageId) }
                if (mine == generation) stateFlow.update { it + (messageId to UnsubEntry.Requested) }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                if (mine != generation) return@launch
                stateFlow.update { it + (messageId to entry) }
                onError(UiMessage.of(e, R.string.unsub_failed))
            }
        }
    }

    fun reset() {
        generation++
        asked.clear()
        stateFlow.value = emptyMap()
    }
}
