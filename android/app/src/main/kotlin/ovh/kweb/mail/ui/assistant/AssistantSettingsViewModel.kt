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
import uniffi.kweb_core.AiService
import uniffi.kweb_core.AiSettings

enum class AssistantPhase { LOADING, READY, FAILED }

/** The model picker: closed (null in the state), asking the service, showing the list, or failed. */
sealed interface ModelsState {
    data object Loading : ModelsState

    data class Loaded(val list: AiModelList) : ModelsState

    data class Failed(val failure: ModelFailure) : ModelsState
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
    /** The reset to the defaults is running (it removes the keys and the consents too). */
    val resetting: Boolean = false,
    /** Another entry's settings are being read: the fields wait. */
    val switching: Boolean = false,
    /** The services the user keeps, as the core last listed them. */
    val services: List<AiService> = emptyList(),
    /** A service is being removed. */
    val removing: Boolean = false,
    /** The picker's sheet, when open. */
    val models: ModelsState? = null,
    /** The last list the service gave, for the effort levels of the model; dropped when the service or address changes. */
    val modelList: AiModelList? = null,
    val consent: ConsentAsk? = null,
    /**
     * The second question, after the service itself is agreed to and the chat is on: in the chat the assistant itself
     * chooses what it reads and sends there.
     */
    val chatConsent: ConsentAsk? = null,
    val error: UiMessage? = null,
    /** A plain line after something went well ("Earlier chats deleted"). */
    val info: UiMessage? = null,
    /** The address and model a chat found to take no tools, for the shown service; null when none is known. */
    val tools: AssistantRules.ToolsFact? = null,
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

    /** The address [keyText] was typed at ([setKey]); see [edit]. */
    private var keyAddress = ""
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

    /**
     * "Reset to default": the core puts the settings back to how they were before anything was set, removes both keys
     * and every consent, and refuses while the assistant is working. The form is then read from what it returns, and
     * what was typed here is dropped. The screen stays open.
     */
    fun reset() {
        val s = stateFlow.value
        if (s.resetting || s.saving || s.switching || s.removing || s.phase != AssistantPhase.READY) return
        switchJob?.cancel()
        modelsJob?.cancel()
        stateFlow.update { it.copy(resetting = true, models = null, error = null) }
        viewModelScope.launch {
            try {
                val after = withContext(Dispatchers.IO) { session.core.aiResetSettings() }
                keyText = ""
                take(after)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "resetting the assistant settings failed: ${e.javaClass.simpleName}")
                stateFlow.update { it.copy(resetting = false, error = AssistantMessages.of(e, R.string.assistant_reset_failed)) }
            }
        }
    }

    /** "Delete earlier chats": every kept chat of this account goes (on the device only), and so does the open conversation. */
    fun deleteChats() {
        viewModelScope.launch {
            try {
                withContext(Dispatchers.IO) { session.core.aiChatsDeleteAll() }
                stateFlow.update { it.copy(info = UiMessage.Res(R.string.assistant_chats_deleted), error = null) }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "deleting the earlier chats failed: ${e.javaClass.simpleName}")
                stateFlow.update { it.copy(error = AssistantMessages.of(e, R.string.assistant_chats_delete_failed)) }
            }
        }
    }

    fun dismissInfo() = stateFlow.update { it.copy(info = null) }

    private fun take(settings: AiSettings) {
        stored = settings
        val form = AssistantRules.formOf(settings)
        stateFlow.update {
            AssistantSettingsState(
                phase = AssistantPhase.READY,
                form = form,
                initial = form,
                hasKey = settings.hasKey,
                services = settings.services,
                tools = AssistantRules.toolsFact(settings),
            )
        }
        learnLevels()
    }

    /**
     * Asks the service for its list in the background, without the sheet, so that the effort row offers the levels it
     * names for the model (the core keeps them, and sends an effort only where they include it). Only where a request
     * can work: a stored key (sent only to the address it was entered for, the core sees to that) or this device or a
     * private network, and only while the assistant is switched on (nothing is asked of a service the user has not
     * turned on). A failure changes nothing; the picker shows it when the user asks. Once the core has the levels, an
     * effort stored before them is sent again, so the form is built again from the stored settings when the user has not
     * changed it yet: the screen then shows the effort that is sent, and a later Save does not write Default over it.
     */
    private fun learnLevels() {
        val s = stateFlow.value
        val form = s.form ?: return
        val base = stored ?: return
        if (s.phase != AssistantPhase.READY || s.switching || s.removing || form.model.isBlank() || !form.enabled) return
        if (!s.hasKey && !AssistantRules.isPrivateAddress(form.baseUrl)) return
        modelsJob?.cancel()
        modelsJob = viewModelScope.launch {
            try {
                val list = withContext(Dispatchers.IO) { session.core.aiModels(form.toSettings(base), null) }
                stateFlow.update { cur -> if (cur.form?.serviceId == form.serviceId && cur.models == null) cur.copy(modelList = list) else cur }
                // A service not stored yet has no stored effort to read again.
                if (form.serviceId.isNotEmpty()) {
                    val again = withContext(Dispatchers.IO) { session.core.aiService(form.serviceId) }
                    val fresh = AssistantRules.formOf(again)
                    stateFlow.update { cur ->
                        if (cur.form == cur.initial && cur.form?.serviceId == fresh.serviceId && !cur.switching && !cur.removing) {
                            stored = again
                            cur.copy(form = fresh, initial = fresh)
                        } else {
                            cur
                        }
                    }
                }
                edit { it }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "asking for the effort levels failed: ${e.javaClass.simpleName}")
            }
        }
    }

    /**
     * Changes the form. A new address drops the loaded list (its levels were another address's); the effort falls back
     * to Default when the model, by the loaded list, does not offer it.
     */
    fun edit(change: (AssistantRules.Form) -> AssistantRules.Form) {
        val before = stateFlow.value.form
        stateFlow.update { s ->
            val form = s.form ?: return@update s
            val next = change(form)
            val list = if (next.baseUrl.trim().trimEnd('/') == form.baseUrl.trim().trimEnd('/')) s.modelList else null
            val effort = EffortRules.normalised(next.effort, next.model, list)
            s.copy(form = next.copy(effort = effort), modelList = list, error = null)
        }
        // A key typed for one address must not go to another origin (the model picker would send it there). Measured
        // from the address the key was typed at, not from each keystroke: a key typed before any address stays while
        // the address is typed.
        val after = stateFlow.value.form
        if (before == null || after == null || keyText.isEmpty()) return
        val now = after.baseUrl
        when {
            // Typing or deleting in the address field, one step at a time: the key goes with the address being typed.
            now.startsWith(keyAddress) || keyAddress.startsWith(now) -> keyAddress = now
            // A jump to another origin (the Service list, a paste): the key typed for the old one is dropped.
            AssistantRules.originChanged(keyAddress, now) -> keyText = ""
        }
    }

    /**
     * The user tapped an entry of the list. That entry's own stored address, model, effort, key state and consent are
     * read from the core (off the main thread); until they are there the fields wait. The typed key and unsaved
     * name, address, model and effort of the entry left are dropped; the switches that belong to no service stay.
     * Tapping the entry already shown changes nothing.
     */
    fun selectService(id: String) = showService(id)

    /** "Add a service": the form of a new, empty entry. Saving it adds it to the list and makes it the one in use. */
    fun addService() {
        if (!AssistantRules.canAddService(stateFlow.value.services.size)) return
        showService("")
    }

    private fun showService(id: String) {
        val s = stateFlow.value
        if (s.phase != AssistantPhase.READY || s.saving || s.removing || s.resetting) return
        if (s.form == null) return
        val shown = stored?.serviceId ?: return
        if (id == shown && !s.switching) return
        switchJob?.cancel()
        modelsJob?.cancel()
        stateFlow.update { it.copy(switching = true, models = null, error = null) }
        switchJob = viewModelScope.launch {
            try {
                val service = withContext(Dispatchers.IO) { session.core.aiService(id) }
                stored = service
                keyText = "" // dropped only now: a failed read keeps the form of the entry left, key included
                stateFlow.update { cur ->
                    cur.copy(
                        form = cur.form?.showing(service),
                        initial = AssistantRules.formOf(service),
                        hasKey = service.hasKey,
                        switching = false,
                        modelList = null,
                        models = null,
                        tools = AssistantRules.toolsFact(service),
                    )
                }
                learnLevels()
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "reading a service of the assistant failed: ${e.javaClass.simpleName}")
                stateFlow.update { cur -> cur.copy(switching = false, error = AssistantMessages.of(e, R.string.assistant_switch_failed)) }
            }
        }
    }

    /**
     * "Remove service": the core removes the stored entry with its key and agreement (and refuses while the assistant
     * works). The screen then shows what the core gives back: the entry that is in use now, or an empty form.
     */
    fun removeService(id: String) {
        val s = stateFlow.value
        if (id.isEmpty() || s.removing || s.saving || s.switching || s.resetting || s.phase != AssistantPhase.READY) return
        switchJob?.cancel()
        modelsJob?.cancel()
        stateFlow.update { it.copy(removing = true, models = null, error = null) }
        viewModelScope.launch {
            try {
                val after = withContext(Dispatchers.IO) { session.core.aiServiceRemove(id) }
                keyText = ""
                take(after)
                stateFlow.update { it.copy(info = UiMessage.Res(R.string.assistant_service_removed)) }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "removing a service of the assistant failed: ${e.javaClass.simpleName}")
                stateFlow.update { it.copy(removing = false, error = AssistantMessages.of(e, R.string.assistant_service_remove_failed)) }
            }
        }
    }

    fun setKey(text: String) {
        if (stateFlow.value.switching) return
        keyText = text
        // The address the key was typed for; empty while there is none yet (then typing the address keeps the key).
        keyAddress = stateFlow.value.form?.baseUrl.orEmpty()
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
        if (s.phase != AssistantPhase.READY || s.switching || s.removing) return
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
                stateFlow.update { it.copy(models = ModelsState.Failed(ModelFailure.of(e))) }
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
     * changed; when the result is on and not agreed to, the screen asks before it closes. When the chat is on and its
     * own agreement is missing, that is asked next. Switching the chat off needs nothing more than the save.
     */
    fun save() {
        val s = stateFlow.value
        val form = s.form ?: return
        val base = stored ?: return
        if (s.saving || s.switching || s.removing || s.phase != AssistantPhase.READY) return
        stateFlow.update { it.copy(saving = true, error = null) }
        val key = AssistantRules.keyToSend(keyText, form.removeKey)
        viewModelScope.launch {
            try {
                val after = withContext(Dispatchers.IO) { session.core.setAiSettings(form.toSettings(base), key) }
                keyText = ""
                stored = after
                val shown = AssistantRules.formOf(after)
                val ask = ConsentAsk(ConsentRules.hostOf(after.baseUrl), after.model)
                when {
                    ConsentRules.isNeededAfterSave(after) -> stateFlow.update {
                        it.copy(form = shown, initial = shown, hasKey = after.hasKey, services = after.services, saving = false, consent = ask, tools = AssistantRules.toolsFact(after))
                    }
                    ConsentRules.isChatNeededAfterSave(after) -> stateFlow.update {
                        it.copy(form = shown, initial = shown, hasKey = after.hasKey, services = after.services, saving = false, chatConsent = ask, tools = AssistantRules.toolsFact(after))
                    }
                    else -> doneChannel.send(Unit)
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
                if (ConsentRules.isChatNeededAfterSave(after)) {
                    // Agreed to the service, and the chat is on: its own question comes next.
                    stateFlow.update { it.copy(consent = null, chatConsent = ConsentAsk(ConsentRules.hostOf(after.baseUrl), after.model)) }
                } else {
                    doneChannel.send(Unit)
                }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "settling the consent failed: ${e.javaClass.simpleName}")
                stateFlow.update { it.copy(consent = null, error = AssistantMessages.of(e, R.string.assistant_save_failed)) }
            }
        }
    }

    /**
     * "Agree" in the chat's dialog: the core stores the yes for the service in use (only this call can; a save never
     * does). Nothing is sent to the service by it.
     */
    fun agreeChatConsent() {
        val ask = stateFlow.value.chatConsent ?: return
        if (ask.busy) return
        stateFlow.update { it.copy(chatConsent = ask.copy(busy = true)) }
        viewModelScope.launch {
            try {
                stored = withContext(Dispatchers.IO) { session.core.aiChatConsent(true) }
                doneChannel.send(Unit)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "storing the chat consent failed: ${e.javaClass.simpleName}")
                stateFlow.update { it.copy(chatConsent = null, error = AssistantMessages.of(e, R.string.assistant_chat_consent_failed)) }
            }
        }
    }

    /**
     * "Decline" (or leaving the dialog): nothing is stored and nothing changes. The settings are saved already, the
     * chat stays switched on, and its screen says that it is not agreed to.
     */
    fun declineChatConsent() {
        val ask = stateFlow.value.chatConsent ?: return
        if (ask.busy) return
        stateFlow.update { it.copy(chatConsent = null) }
        doneChannel.trySend(Unit)
    }

    override fun onCleared() {
        keyText = ""
        super.onCleared()
    }
}
