package ovh.kweb.mail.ui.catchup

import android.app.Application
import android.util.Log
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import java.time.LocalDate
import java.time.ZoneId
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import ovh.kweb.mail.R
import ovh.kweb.mail.accounts.AccountSession
import ovh.kweb.mail.background.Background
import ovh.kweb.mail.ui.SessionState
import ovh.kweb.mail.ui.UiMessage
import ovh.kweb.mail.ui.assistant.AssistantMessages
import ovh.kweb.mail.ui.assistant.AssistantRules
import ovh.kweb.mail.ui.assistant.ConsentRules
import ovh.kweb.mail.ui.assistant.RunRules
import ovh.kweb.mail.ui.assistant.serviceName
import ovh.kweb.mail.ui.attention.SparkleRules
import uniffi.kweb_core.AiCatchUp
import uniffi.kweb_core.AiSettings
import uniffi.kweb_core.AiSparklePreview
import uniffi.kweb_core.AttentionFamily
import uniffi.kweb_core.AttentionRule
import uniffi.kweb_core.Finding
import uniffi.kweb_core.HomeTile
import uniffi.kweb_core.ServiceAction

/** The question before a run: what will be sent, and whether it is the agreement itself or the per-run ask. */
data class CatchUpAsk(
    val preview: AiSparklePreview,
    val settings: AiSettings,
    val ask: SparkleRules.Ask,
    val service: String,
    val showCost: Boolean,
)

sealed interface CatchUpState {
    data object Idle : CatchUpState

    data class Asking(val ask: CatchUpAsk) : CatchUpState

    /** The request is out; Stop ends it. */
    data class Running(val runId: String) : CatchUpState

    data class Done(val result: AiCatchUp, val service: String, val showCost: Boolean, val at: Long, val canFind: Boolean) : CatchUpState

    /** Nothing to catch up on, or the run failed: a sentence for the user. */
    data class Message(val text: UiMessage, val canRetry: Boolean) : CatchUpState
}

/**
 * "Catch me up" for one account: the question before the run, the run (the core has one slot), and the result.
 * Nothing is changed by it: its result is lines and links to open. The first use for a service and model asks for
 * the agreement itself, and "Ask before each run" asks again because the run sends mail.
 */
class CatchUpViewModel(app: Application, private val session: AccountSession) : AndroidViewModel(app) {
    private val stateFlow = MutableStateFlow<CatchUpState>(CatchUpState.Idle)
    private val offeredFlow = MutableStateFlow(false)

    val state: StateFlow<CatchUpState> get() = stateFlow

    /** The assistant is on and summaries are allowed: the entry is shown. */
    val offered: StateFlow<Boolean> get() = offeredFlow

    init {
        viewModelScope.launch {
            SessionState.signedIn.collect { signedIn ->
                if (signedIn == false) {
                    val running = stateFlow.value as? CatchUpState.Running
                    running?.let { cancelCore(it.runId) }
                    stateFlow.value = CatchUpState.Idle
                    offeredFlow.value = false
                }
            }
        }
    }

    private fun app() = getApplication<Application>()

    /** Reads whether the entry is shown. Called when a screen that has the entry comes up. */
    fun refreshOffered() {
        viewModelScope.launch {
            offeredFlow.value = try {
                val settings = withContext(Dispatchers.IO) { session.core.aiSettings() }
                CatchUpRules.offered(settings)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "reading the assistant settings failed: ${e.javaClass.simpleName}")
                false
            }
        }
    }

    /** The user chose "Catch me up": works out what would be sent, asks when the rules say so, then runs. */
    fun start() {
        if (stateFlow.value is CatchUpState.Running || stateFlow.value is CatchUpState.Asking) return
        val zone = ZoneId.systemDefault()
        val today = LocalDate.now(zone).toString()
        stateFlow.value = CatchUpState.Running("")
        viewModelScope.launch {
            try {
                val (ai, attention, preview) = withContext(Dispatchers.IO) {
                    val core = session.core
                    Triple(core.aiSettings(), core.attentionSettings(), core.aiCatchupPreview(today, zone.id))
                }
                val label = serviceLabel(ai.baseUrl)
                val ask = SparkleRules.ask(ai.consented, false, attention.askEachRun, preview)
                if (ask == SparkleRules.Ask.NONE) {
                    run(ai, preview, label, attention.showCost, grant = false)
                } else {
                    stateFlow.value = CatchUpState.Asking(CatchUpAsk(preview, ai, ask, label, attention.showCost))
                }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "preparing a catch-up failed: ${e.javaClass.simpleName}")
                stateFlow.value = CatchUpState.Message(AssistantMessages.of(e, R.string.catchup_failed), canRetry = false)
            }
        }
    }

    /** "Send" in the dialog: the agreement is stored when it was the first use, then the run goes. */
    fun confirm() {
        val asking = stateFlow.value as? CatchUpState.Asking ?: return
        val a = asking.ask
        stateFlow.value = CatchUpState.Running("")
        viewModelScope.launch { run(a.settings, a.preview, a.service, a.showCost, grant = a.ask == SparkleRules.Ask.FIRST) }
    }

    /** "Cancel" in the dialog: nothing was sent and nothing is stored. */
    fun decline() {
        if (stateFlow.value is CatchUpState.Asking) stateFlow.value = CatchUpState.Idle
    }

    private suspend fun run(settings: AiSettings, preview: AiSparklePreview, service: String, showCost: Boolean, grant: Boolean) {
        val runId = RunRules.newRunId()
        stateFlow.value = CatchUpState.Running(runId)
        val zone = ZoneId.systemDefault()
        try {
            val result = withContext(Dispatchers.IO) {
                val core = session.core
                if (grant) core.setAiSettings(ConsentRules.granted(settings), null)
                core.aiCatchupRun(LocalDate.now(zone).toString(), zone.id, runId)
            }
            // Stopped while it ran: what came back is dropped.
            if ((stateFlow.value as? CatchUpState.Running)?.runId != runId) return
            stateFlow.value = CatchUpState.Done(result, service, showCost, System.currentTimeMillis() / 1000, canFind = settings.allowExtract)
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            Log.w(Background.TAG, "a catch-up failed: ${e.javaClass.simpleName}")
            if ((stateFlow.value as? CatchUpState.Running)?.runId != runId) return
            stateFlow.value = CatchUpState.Message(AssistantMessages.of(e, R.string.catchup_failed), canRetry = true)
        }
    }

    /** "Stop" while the run goes. */
    fun stop() {
        val running = stateFlow.value as? CatchUpState.Running ?: return
        stateFlow.value = CatchUpState.Idle
        if (running.runId.isNotEmpty()) cancelCore(running.runId)
    }

    /** The user closed the result or the message. */
    fun dismiss() {
        if (stateFlow.value is CatchUpState.Done || stateFlow.value is CatchUpState.Message) stateFlow.value = CatchUpState.Idle
    }

    /** The core's cancel does not block, so it is called here and now. */
    private fun cancelCore(runId: String) {
        try {
            session.core.aiCancel(runId)
        } catch (e: Exception) {
            Log.w(Background.TAG, "cancelling the assistant failed: ${e.javaClass.simpleName}")
        }
    }

    private fun serviceLabel(baseUrl: String): String {
        val service = AssistantRules.serviceOf(baseUrl)
        return if (service == AssistantRules.Service.OTHER) {
            AssistantRules.hostOf(baseUrl).ifEmpty { app().getString(R.string.assistant_svc_other) }
        } else {
            app().getString(serviceName(service))
        }
    }

    /**
     * The finding the "Find to-dos and dates" button of the result starts: the mails that ask for an answer first,
     * then the newest, at most five. The ordinary run of "Add dates" does the rest.
     */
    fun finderFinding(result: AiCatchUp): Finding? {
        val items = result.groups.flatMap { it.items }
        val ids = (items.filter { it.needsAnswer } + items.filterNot { it.needsAnswer }).map { it.message.id }.take(SparkleRules.MOST_MAILS)
        if (ids.isEmpty()) return null
        return Finding(
            id = CatchUpRules.FINDER_ID,
            rule = AttentionRule.DATE_IN_MAIL,
            family = AttentionFamily.MAIL,
            tile = HomeTile.UNREAD,
            names = emptyList(),
            times = emptyList(),
            count = ids.size.toUInt(),
            at = 0L,
            eventIds = emptyList(),
            taskIds = emptyList(),
            messageIds = ids,
            threadId = null,
            phone = emptyList(),
            service = ServiceAction.ADD_DATES,
            slots = emptyList(),
        )
    }
}
