package ovh.kweb.mail.ui.assistant

import android.app.Application
import android.util.Log
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import ovh.kweb.mail.R
import ovh.kweb.mail.background.Background
import ovh.kweb.mail.ui.UiMessage
import uniffi.kweb_core.AiProvider
import uniffi.kweb_core.AiSettings

enum class AssistantPhase { LOADING, READY, FAILED }

data class AssistantSettingsState(
    val phase: AssistantPhase = AssistantPhase.LOADING,
    val form: AssistantRules.Form? = null,
    /** The form as it was read, to tell whether the user changed anything. */
    val initial: AssistantRules.Form? = null,
    /** A key is stored (the key itself never comes here). */
    val hasKey: Boolean = false,
    val saving: Boolean = false,
    val error: UiMessage? = null,
)

/**
 * The settings screen of the assistant. It lives as long as its route. The key the user types is held here only
 * until it is saved (or the screen is left), never in a data class that could be printed, never in saved state;
 * after the core has it, it is blanked.
 */
class AssistantSettingsViewModel(app: Application) : AndroidViewModel(app) {
    private val stateFlow = MutableStateFlow(AssistantSettingsState())
    private val doneChannel = Channel<Unit>(Channel.BUFFERED)
    private var loadJob: Job? = null

    /** What was read from the core: the form is applied on top of it when saving (hasKey and consent stay the core's). */
    private var stored: AiSettings? = null

    /** The typed key. A snapshot state of its own, so that the text field is not fed through a flow. */
    var keyText by mutableStateOf("")
        private set

    val state: StateFlow<AssistantSettingsState> get() = stateFlow

    /** Sent once the settings are saved: the screen goes back without asking about changes. */
    val saved: Flow<Unit> get() = doneChannel.receiveAsFlow()

    val isChanged: Boolean
        get() {
            val s = stateFlow.value
            val form = s.form ?: return false
            return s.phase == AssistantPhase.READY && AssistantRules.isChanged(form, s.initial ?: form, keyText.isNotEmpty())
        }

    /** Reads the settings. Does nothing while a read runs, and never replaces a form that is open. */
    fun load() {
        if (loadJob?.isActive == true || stateFlow.value.phase == AssistantPhase.READY) return
        stateFlow.update { it.copy(phase = AssistantPhase.LOADING) }
        loadJob = viewModelScope.launch {
            try {
                val read = withContext(Dispatchers.IO) { Background.core(getApplication()).aiSettings() }
                take(read, error = null)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "reading the assistant settings failed: ${e.javaClass.simpleName}")
                stateFlow.update { it.copy(phase = AssistantPhase.FAILED) }
            }
        }
    }

    private fun take(settings: AiSettings, error: UiMessage?) {
        stored = settings
        val form = AssistantRules.formOf(settings)
        stateFlow.update { AssistantSettingsState(phase = AssistantPhase.READY, form = form, initial = form, hasKey = settings.hasKey, error = error) }
    }

    fun edit(change: (AssistantRules.Form) -> AssistantRules.Form) =
        stateFlow.update { s -> s.form?.let { s.copy(form = change(it), error = null) } ?: s }

    fun setProvider(provider: AiProvider) = edit { it.withProvider(provider) }

    fun setKey(text: String) {
        keyText = text
        stateFlow.update { it.copy(error = null) }
    }

    fun removeKey(remove: Boolean) = edit { it.copy(removeKey = remove) }

    fun dismissError() = stateFlow.update { it.copy(error = null) }

    fun save() {
        val s = stateFlow.value
        val form = s.form ?: return
        val base = stored ?: return
        if (s.saving || s.phase != AssistantPhase.READY) return
        stateFlow.update { it.copy(saving = true, error = null) }
        val key = AssistantRules.keyToSend(keyText, form.removeKey)
        viewModelScope.launch {
            try {
                val after = withContext(Dispatchers.IO) { Background.core(getApplication()).setAiSettings(form.toSettings(base), key) }
                keyText = ""
                stored = after
                doneChannel.send(Unit)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "saving the assistant settings failed: ${e.javaClass.simpleName}")
                stateFlow.update { it.copy(saving = false, error = AssistantMessages.of(e, R.string.assistant_save_failed)) }
            }
        }
    }

    override fun onCleared() {
        keyText = ""
        super.onCleared()
    }
}
