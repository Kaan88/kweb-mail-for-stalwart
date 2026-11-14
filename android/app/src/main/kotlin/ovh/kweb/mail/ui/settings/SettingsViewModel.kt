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
import kotlinx.coroutines.withContext
import ovh.kweb.mail.background.Background
import ovh.kweb.mail.ui.SessionState
import ovh.kweb.mail.ui.delivery.DeliveryViewModel
import uniffi.kweb_core.Quota

/** The storage section: not read yet, read, or not readable. */
sealed interface StorageState {
    data object Loading : StorageState

    data class Loaded(val quotas: List<Quota>) : StorageState

    /** The server could not be asked (offline, or it does not say). */
    data object Unavailable : StorageState
}

data class SettingsState(
    val name: String = "",
    /** The server's host, empty until the account is read. */
    val server: String = "",
    val storage: StorageState = StorageState.Loading,
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
    }
}
