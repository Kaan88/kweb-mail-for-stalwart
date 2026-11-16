package ovh.kweb.mail.ui.crypto

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
import uniffi.kweb_core.CryptoKind
import uniffi.kweb_core.CryptoView

/** What the status chip of one message stands for. A message without an entry has no chip. */
sealed interface CryptoUi {
    data object Checking : CryptoUi

    data class Ready(val view: CryptoView) : CryptoUi

    /** The check could not be made (offline, a failed fetch). Tapping the chip tries again. */
    data object Failed : CryptoUi
}

/**
 * Asks the core, once per message and visit, whether an opened message is signed or encrypted and, if so,
 * what that means. While the feature is off nothing is asked of the server: the first call is a local switch.
 * Nothing it holds is written anywhere: the views live in memory and go with [reset].
 */
class CryptoController(
    private val scope: CoroutineScope,
    private val io: CoroutineDispatcher,
    private val core: () -> Core,
) {
    private val stateFlow = MutableStateFlow<Map<String, CryptoUi>>(emptyMap())
    private val asked = HashSet<String>()
    private var generation = 0

    val state: StateFlow<Map<String, CryptoUi>> get() = stateFlow

    /** Main thread. Quiet when the feature is off or the message is plain mail; a failed question shows a chip that retries. */
    fun check(messageId: String) {
        if (!asked.add(messageId)) return
        run(messageId, quiet = true)
    }

    /** The user tapped "try again" on a failed chip, or a key was added. */
    fun recheck(messageId: String) {
        asked.add(messageId)
        run(messageId, quiet = false)
    }

    private fun run(messageId: String, quiet: Boolean) {
        val mine = generation
        scope.launch {
            val result = try {
                withContext(io) {
                    val c = core()
                    if (!c.cryptoEnabled()) {
                        null
                    } else if (c.cryptoProbe(messageId) == CryptoKind.NONE) {
                        null
                    } else {
                        stateFlow.update { it + (messageId to CryptoUi.Checking) }
                        CryptoUi.Ready(c.cryptoOpen(messageId))
                    }
                }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                // The class only: a message of the core may name the mail.
                Log.w(Background.TAG, "checking a message failed: ${e.javaClass.simpleName}")
                if (quiet && stateFlow.value[messageId] == null) {
                    // Nothing was known yet: stay quiet, ask again the next time the card opens.
                    if (mine == generation) asked.remove(messageId)
                    return@launch
                }
                CryptoUi.Failed
            }
            if (mine != generation) return@launch
            stateFlow.update { if (result == null) it - messageId else it + (messageId to result) }
        }
    }

    /** Adds a key the message carries, then checks the message again. */
    fun takeOffer(messageId: String, offerId: String, onFailed: () -> Unit) {
        val mine = generation
        scope.launch {
            try {
                withContext(io) { core().cryptoImportOffer(messageId, offerId) }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "adding a key failed: ${e.javaClass.simpleName}")
                if (mine == generation) onFailed()
                return@launch
            }
            if (mine == generation) recheck(messageId)
        }
    }

    fun reset() {
        generation++
        asked.clear()
        stateFlow.value = emptyMap()
    }
}
