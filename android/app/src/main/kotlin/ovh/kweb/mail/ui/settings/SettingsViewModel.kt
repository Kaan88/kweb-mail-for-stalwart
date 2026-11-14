package ovh.kweb.mail.ui.settings

import android.app.Application
import android.util.Log
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import java.time.ZoneId
import kotlinx.coroutines.withContext
import ovh.kweb.mail.background.Background
import ovh.kweb.mail.ui.SessionState
import ovh.kweb.mail.ui.assistant.AssistantRules
import ovh.kweb.mail.ui.away.AwayLine
import ovh.kweb.mail.ui.away.AwayRules
import ovh.kweb.mail.ui.delivery.DeliveryViewModel
import uniffi.kweb_core.Quota

/** The storage section: not read yet, read, or not readable. */
sealed interface StorageState {
    data object Loading : StorageState

    data class Loaded(val quotas: List<Quota>) : StorageState

    /** The server could not be asked (offline, or it does not say). */
    data object Unavailable : StorageState
}

/** The state line of the away message row: not read yet, read, or not readable (the row then says nothing). */
sealed interface AwayRowState {
    data object Loading : AwayRowState

    data class Known(val line: AwayLine) : AwayRowState

    data object Unknown : AwayRowState
}

/** The state line of the assistant row: not read yet, read, or not readable (the row then says nothing). */
sealed interface AssistantRowState {
    data object Loading : AssistantRowState

    data class Known(val line: AssistantRules.StateLine) : AssistantRowState

    data object Unknown : AssistantRowState
}

data class SettingsState(
    val name: String = "",
    /** The server's host, empty until the account is read. */
    val server: String = "",
    val storage: StorageState = StorageState.Loading,
    val away: AwayRowState = AwayRowState.Loading,
    val assistant: AssistantRowState = AssistantRowState.Loading,
)

/** What the Settings screen shows: the account from the core and the quotas from the server. */
class SettingsViewModel(app: Application) : AndroidViewModel(app) {
    private val stateFlow = MutableStateFlow(SettingsState())

    val state: StateFlow<SettingsState> get() = stateFlow

    /** Counts sign-outs: a read started before one is about the old account and is dropped. */
    @Volatile
    private var generation = 0

    init {
        viewModelScope.launch {
            SessionState.signedIn.collect { signedIn ->
                if (signedIn == false) {
                    generation++
                    stateFlow.value = SettingsState()
                }
            }
        }
    }

    /** Reads the account and asks the server for the quotas; called each time the screen comes up. */
    fun load() {
        val asked = generation
        viewModelScope.launch {
            val account = withContext(Dispatchers.IO) { Background.core(getApplication()).account() }
            if (asked != generation) return@launch
            stateFlow.update {
                it.copy(name = account?.username.orEmpty(), server = DeliveryViewModel.hostOf(account?.serverUrl).orEmpty())
            }
        }
        viewModelScope.launch {
            val result = try {
                StorageState.Loaded(withContext(Dispatchers.IO) { Background.core(getApplication()).quota() })
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "reading the quota failed: ${e.javaClass.simpleName}")
                StorageState.Unavailable
            }
            if (asked != generation) return@launch
            // An answer that is only a failure does not replace numbers that were read earlier.
            stateFlow.update { if (result is StorageState.Unavailable && it.storage is StorageState.Loaded) it else it.copy(storage = result) }
        }
        viewModelScope.launch {
            val result = try {
                val message = withContext(Dispatchers.IO) { Background.core(getApplication()).awayMessage() }
                AwayRowState.Known(AwayRules.line(message, System.currentTimeMillis() / 1000, ZoneId.systemDefault()))
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "reading the away message failed: ${e.javaClass.simpleName}")
                AwayRowState.Unknown
            }
            if (asked != generation) return@launch
            // An answer that is only a failure does not replace a state that was read earlier.
            stateFlow.update { if (result is AwayRowState.Unknown && it.away is AwayRowState.Known) it else it.copy(away = result) }
        }
        viewModelScope.launch {
            val result = try {
                val settings = withContext(Dispatchers.IO) { Background.core(getApplication()).aiSettings() }
                AssistantRules.stateLine(settings)?.let { AssistantRowState.Known(it) } ?: AssistantRowState.Unknown
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "reading the assistant settings failed: ${e.javaClass.simpleName}")
                AssistantRowState.Unknown
            }
            if (asked != generation) return@launch
            stateFlow.update { if (result is AssistantRowState.Unknown && it.assistant is AssistantRowState.Known) it else it.copy(assistant = result) }
        }
    }
}
