package ovh.kweb.mail.ui.assistant

import ovh.kweb.mail.accounts.AccountSession
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
import uniffi.kweb_core.AiModel
import uniffi.kweb_core.AiModelList
import uniffi.kweb_core.AiProvider
import uniffi.kweb_core.AiSettings

enum class AssistantPhase { LOADING, READY, FAILED }

/** The model picker: closed (null in the state), asking the service, showing the list, or failed. */
sealed interface ModelsState {
    data object Loading : ModelsState

    data class Loaded(val list: AiModelList) : ModelsState

    data class Failed(val message: UiMessage) : ModelsState
}

/** The question after a save that left the assistant on and not agreed to: what goes where. */
data class ConsentAsk(val host: String, val model: String, val busy: Boolean = false)

data class AssistantSettingsState(
    val phase: AssistantPhase = AssistantPhase.LOADING,
    val form: AssistantRules.Form? = null,
    /** The form as it was read, to tell whether the user changed anything. */
    val initial: AssistantRules.Form? = null,
    /** A key is stored for the shown service (the key itself never comes here). */
    val hasKey: Boolean = false,
    val saving: Boolean = false,
    /** The other service's settings are being read: the fields wait. */
    val switching: Boolean = false,
    /** The picker's sheet, when open. */
    val models: ModelsState? = null,
    /** The last list the service gave, for the effort levels of the model; dropped when the service changes. */
    val modelList: AiModelList? = null,
    val consent: ConsentAsk? = null,
    val error: UiMessage? = null,
)

/**
 * The settings screen of the assistant. It lives as long as its route. The key the user types is held here only
 * until it is saved (or the screen is left), never in a data class that could be printed, never in saved state;
 * after the core has it, it is blanked.
 */
class AssistantSettingsViewModel(app: Application, private val session: AccountSession) : AndroidViewModel(app) {
    private val stateFlow = MutableStateFlow(AssistantSettingsState())
    private val doneChannel = Channel<Unit>(Channel.BUFFERED)
    private var loadJob: Job? = null
    private var switchJob: Job? = null
    private var modelsJob: Job? = null

    /** What the core gave for the service whose fields are shown: the form is applied on top of it when saving. */
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
                val read = withContext(Dispatchers.IO) { session.core.aiSettings() }
                take(read)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "reading the assistant settings failed: ${e.javaClass.simpleName}")
                stateFlow.update { it.copy(phase = AssistantPhase.FAILED) }
            }
        }
    }

    private fun take(settings: AiSettings) {
        stored = settings
        val form = AssistantRules.formOf(settings)
        stateFlow.update { AssistantSettingsState(phase = AssistantPhase.READY, form = form, initial = form, hasKey = settings.hasKey) }
    }

    /** Changes the form; the effort falls back to Default when the new model does not offer it. */
    fun edit(change: (AssistantRules.Form) -> AssistantRules.Form) = stateFlow.update { s ->
        val form = s.form ?: return@update s
        val next = change(form)
        val effort = EffortRules.normalised(next.effort, next.provider, next.model, s.modelList)
        s.copy(form = next.copy(effort = effort), error = null)
    }

    /**
     * The user picked a service. The other service's own stored address, model, effort, key state and consent are
     * read from the core (off the main thread); until they are there the fields wait. The typed key and unsaved
     * address, model and effort of the service left are dropped; the switches that belong to no service stay.
     */
    fun setProvider(provider: AiProvider) {
        val s = stateFlow.value
        val form = s.form ?: return
        if (s.phase != AssistantPhase.READY || s.saving) return
        val shown = stored?.provider ?: return
        switchJob?.cancel()
        modelsJob?.cancel()
        if (provider == shown) {
            // Back to the service whose fields are on screen (a switch that was still reading is dropped).
            stateFlow.update { it.copy(form = form.copy(provider = provider), switching = false, models = null) }
            return
        }
        stateFlow.update { it.copy(form = form.copy(provider = provider), switching = true, models = null, error = null) }
        switchJob = viewModelScope.launch {
            try {
                val service = withContext(Dispatchers.IO) { session.core.aiService(provider) }
                stored = service
                keyText = "" // dropped only now: a failed read keeps the form of the service left, key included
                stateFlow.update { cur ->
                    cur.copy(form = cur.form?.switchedTo(service), hasKey = service.hasKey, switching = false, modelList = null, models = null)
                }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "reading a service of the assistant failed: ${e.javaClass.simpleName}")
                stateFlow.update { cur ->
                    cur.copy(form = cur.form?.copy(provider = shown), switching = false, error = AssistantMessages.of(e, R.string.assistant_switch_failed))
                }
            }
        }
    }

    fun setKey(text: String) {
        if (stateFlow.value.switching) return
        keyText = text
        stateFlow.update { it.copy(error = null) }
    }

    fun removeKey(remove: Boolean) = edit { it.copy(removeKey = remove) }

    fun dismissError() = stateFlow.update { it.copy(error = null) }

    // ---- the model picker ----

    /** Opens the sheet and asks the service which models it offers, with the fields as shown and the key typed (if any). */
    fun chooseModel() {
        val s = stateFlow.value
        val form = s.form ?: return
        val base = stored ?: return
        if (s.phase != AssistantPhase.READY || s.switching) return
        modelsJob?.cancel()
        stateFlow.update { it.copy(models = ModelsState.Loading) }
        val key = AssistantRules.keyToSend(keyText, form.removeKey)
        modelsJob = viewModelScope.launch {
            try {
                val list = withContext(Dispatchers.IO) { session.core.aiModels(form.toSettings(base), key) }
                stateFlow.update { cur -> cur.copy(models = ModelsState.Loaded(list), modelList = list) }
                edit { it } // the effort may not be offered by the model any more
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "asking for the models failed: ${e.javaClass.simpleName}")
                stateFlow.update { it.copy(models = ModelsState.Failed(AssistantMessages.of(e, R.string.assistant_models_failed))) }
            }
        }
    }

    fun closeModels() {
        modelsJob?.cancel()
        stateFlow.update { it.copy(models = null) }
    }

    fun pickModel(model: AiModel) {
        edit { it.copy(model = model.id) }
        closeModels()
    }

    // ---- saving ----

    /**
     * Stores the shown service and makes it the one in use. The core clears consent itself when address or model
     * changed; when the result is on and not agreed to, the screen asks before it closes.
     */
    fun save() {
        val s = stateFlow.value
        val form = s.form ?: return
        val base = stored ?: return
        if (s.saving || s.switching || s.phase != AssistantPhase.READY) return
        stateFlow.update { it.copy(saving = true, error = null) }
        val key = AssistantRules.keyToSend(keyText, form.removeKey)
        viewModelScope.launch {
            try {
                val after = withContext(Dispatchers.IO) { session.core.setAiSettings(form.toSettings(base), key) }
                keyText = ""
                stored = after
                val shown = AssistantRules.formOf(after)
                if (ConsentRules.isNeededAfterSave(after)) {
                    stateFlow.update {
                        it.copy(form = shown, initial = shown, hasKey = after.hasKey, saving = false, consent = ConsentAsk(ConsentRules.hostOf(after.baseUrl), after.model))
                    }
                } else {
                    doneChannel.send(Unit)
                }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "saving the assistant settings failed: ${e.javaClass.simpleName}")
                stateFlow.update { it.copy(saving = false, error = AssistantMessages.of(e, R.string.assistant_save_failed)) }
            }
        }
    }

    /** "Agree": saves again with the same values and consent given; that is the grant. */
    fun agreeConsent() = settleConsent { ConsentRules.granted(it) }

    /** "Decline" (or leaving the dialog): saves the same values with the assistant switched off. */
    fun declineConsent() = settleConsent { ConsentRules.declined(it) }

    private fun settleConsent(change: (AiSettings) -> AiSettings) {
        val ask = stateFlow.value.consent ?: return
        val base = stored ?: return
        if (ask.busy) return
        stateFlow.update { it.copy(consent = ask.copy(busy = true)) }
        viewModelScope.launch {
            try {
                val after = withContext(Dispatchers.IO) { session.core.setAiSettings(change(base), null) }
                stored = after
                doneChannel.send(Unit)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "settling the consent failed: ${e.javaClass.simpleName}")
                stateFlow.update { it.copy(consent = null, error = AssistantMessages.of(e, R.string.assistant_save_failed)) }
            }
        }
    }

    override fun onCleared() {
        keyText = ""
        super.onCleared()
    }
}
