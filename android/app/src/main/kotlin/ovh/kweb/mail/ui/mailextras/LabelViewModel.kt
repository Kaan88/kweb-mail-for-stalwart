package ovh.kweb.mail.ui.mailextras

import android.app.Application
import android.util.Log
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import java.util.concurrent.atomic.AtomicLong
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import ovh.kweb.mail.background.Background
import ovh.kweb.mail.ui.SessionState
import uniffi.kweb_core.MessageHeader

sealed interface LabelResults {
    data object Loading : LabelResults

    data class Found(val messages: List<MessageHeader>, val fromCache: Boolean) : LabelResults

    data object Failed : LabelResults
}

/** The mail that carries one label. It lives as long as its route (see ScreenStores), so a rotation keeps the list. */
class LabelViewModel(app: Application) : AndroidViewModel(app) {
    private val stateFlow = MutableStateFlow<LabelResults>(LabelResults.Loading)
    private val ticket = AtomicLong()
    private var labelId: String? = null
    private var job: Job? = null
    private val extras = CoreMailExtrasBackend { Background.core(app) }

    val state: StateFlow<LabelResults> get() = stateFlow

    /** The labels of the messages shown, and the user's list (for the chip names). */
    val labels = LabelStore(viewModelScope, Dispatchers.IO, extras, onError = {})

    init {
        viewModelScope.launch { Background.cacheChanges.collect { refreshShown() } }
        viewModelScope.launch { SessionState.signedIn.collect { if (it == false) labels.reset() } }
    }

    /** Called each time the screen comes up with its label. */
    fun start(id: String) {
        if (labelId != id) {
            labelId = id
            stateFlow.value = LabelResults.Loading
        }
        labels.refreshLabels()
        reload()
    }

    /**
     * Asks the server (Retry, and when the screen comes up). A cache change does not come here: it only
     * brings the rows on screen up to date from the cache ([refreshShown]). One question runs at a time.
     */
    fun reload() {
        val id = labelId ?: return
        val mine = ticket.incrementAndGet()
        job?.cancel()
        job = viewModelScope.launch {
            val result = try {
                withContext(Dispatchers.IO) { extras.labelMessages(id, LIMIT) }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "reading the mail of a label failed: ${e.javaClass.simpleName}")
                // What is on screen stays; only an empty screen says that it failed.
                if (mine == ticket.get()) stateFlow.value = (stateFlow.value as? LabelResults.Found) ?: LabelResults.Failed
                return@launch
            }
            // A newer question was started while the core was answering this one.
            if (mine != ticket.get()) return@launch
            stateFlow.value = LabelResults.Found(result.messages, result.fromCache)
            labels.load(result.messages.map { it.id })
        }
    }

    /** Re-reads the rows on screen from the cache; mail that no longer carries the label drops out. Never asks the server. */
    private suspend fun refreshShown() {
        val id = labelId ?: return
        val shown = stateFlow.value as? LabelResults.Found ?: return
        val mine = ticket.get()
        try {
            val (fresh, index) = withContext(Dispatchers.IO) {
                val core = Background.core(getApplication())
                val rows = shown.messages.mapNotNull { core.message(it.id) }
                rows to LabelRules.index(rows.map { it.id }, extras.messageLabels(rows.map { it.id }))
            }
            val keep = LabelRules.stillCarrying(fresh.map { it.id }, index, id).toSet()
            val rows = fresh.filter { it.id in keep }
            // A server answer or another screen state may have replaced these rows meanwhile.
            if (mine != ticket.get() || stateFlow.value !== shown) return
            if (rows != shown.messages) stateFlow.value = shown.copy(messages = rows)
            labels.load(rows.map { it.id })
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            Log.w(Background.TAG, "refreshing the mail of a label failed: ${e.javaClass.simpleName}")
        }
    }

    companion object {
        const val LIMIT = 100u
    }
}
