package ovh.kweb.mail.ui.attention

import android.app.Application
import android.util.Log
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import java.time.LocalDate
import java.time.ZoneId
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import ovh.kweb.mail.R
import ovh.kweb.mail.accounts.AccountSession
import ovh.kweb.mail.background.Background
import ovh.kweb.mail.ui.SessionState
import ovh.kweb.mail.ui.assistant.AssistantMessages
import ovh.kweb.mail.ui.assistant.AssistantRules
import ovh.kweb.mail.ui.assistant.ConsentRules
import ovh.kweb.mail.ui.assistant.DraftText
import ovh.kweb.mail.ui.assistant.RunRules
import ovh.kweb.mail.ui.assistant.serviceName
import ovh.kweb.mail.ui.compose.DraftLauncher
import uniffi.kweb_core.AiProposal
import uniffi.kweb_core.AiSettings
import uniffi.kweb_core.AiSparkle
import uniffi.kweb_core.AiSparklePreview
import uniffi.kweb_core.DraftKind
import uniffi.kweb_core.Finding

/** The question before a run: what will be sent, and whether it is the agreement itself or the per-run ask. */
data class PendingAsk(
    val finding: Finding,
    val task: AiSparkle,
    val preview: AiSparklePreview,
    val settings: AiSettings,
    val ask: SparkleRules.Ask,
    /** The chat's own yes is still missing (Look in mail and Check the mail read mail by themselves). */
    val chatYesMissing: Boolean,
    val service: String,
    val showCost: Boolean,
)

sealed interface SparkleEvent {
    /** A line for a snackbar; [undo] is given when the change can be taken back (offered for 10 seconds). */
    data class Notice(val text: String, val undo: (() -> Unit)? = null) : SparkleEvent

    /** Every card of this run was added: the finding is done and stays away. */
    data class Completed(val finding: Finding) : SparkleEvent

    /** A reply draft with a reply idea in it is ready to be opened (nothing is sent). */
    data class OpenDraft(val draftId: String) : SparkleEvent
}

/**
 * The runs of the buttons with a sparkle, for one account: the question before a run, the run (one at a time: the core
 * has one slot), the result per finding, and what the user does with the cards. It lives as long as the account is
 * shown, so a result stays when the screen changes. A run makes no change: a card is applied only through the core's
 * apply path, on the user's tap.
 */
class SparkleRunViewModel(app: Application, private val session: AccountSession) : AndroidViewModel(app) {
    private val runsFlow = MutableStateFlow<Map<String, SparkleRun>>(emptyMap())
    private val pendingFlow = MutableStateFlow<PendingAsk?>(null)
    private val eventChannel = Channel<SparkleEvent>(Channel.BUFFERED)

    val runs: StateFlow<Map<String, SparkleRun>> get() = runsFlow
    val pending: StateFlow<PendingAsk?> get() = pendingFlow
    val events: Flow<SparkleEvent> get() = eventChannel.receiveAsFlow()

    init {
        viewModelScope.launch {
            SessionState.signedIn.collect { signedIn ->
                if (signedIn == false) {
                    cancelAll()
                    runsFlow.value = emptyMap()
                    pendingFlow.value = null
                }
            }
        }
    }

    private fun app() = getApplication<Application>()

    private fun now(): Long = System.currentTimeMillis() / 1000

    // ---- starting ----

    /** A tap on the sparkle button of [finding]: works out what it would send, asks if the rules say so, then runs. */
    fun start(finding: Finding) {
        val action = finding.service ?: return
        if (runsFlow.value[finding.id]?.phase == RunPhase.RUNNING || pendingFlow.value != null) return
        val zone = ZoneId.systemDefault()
        val task = SparkleRules.task(finding, action, LocalDate.now(zone), zone)
        if (task == null) {
            notice(app().getString(R.string.att_run_no_task))
            return
        }
        val runId = RunRules.newRunId()
        setRun(SparkleRun(finding, task, runId, RunPhase.RUNNING))
        viewModelScope.launch {
            try {
                val (ai, attention, preview) = withContext(Dispatchers.IO) {
                    val core = session.core
                    Triple(core.aiSettings(), core.attentionSettings(), core.aiSparklePreview(task))
                }
                val label = serviceLabel(ai.baseUrl)
                updateRun(finding.id) { if (it.runId == runId) it.copy(service = label, showCost = attention.showCost) else it }
                val chatMissing = SparkleRules.needsChatYes(task) && !ai.chatConsented
                val ask = SparkleRules.ask(ai.consented, chatMissing, attention.askEachRun, preview)
                if (ask == SparkleRules.Ask.NONE) {
                    run(finding, task, runId, ai, grant = false, chatYes = false)
                } else {
                    // Nothing runs until the user says yes; the finding shows its card meanwhile.
                    removeRun(finding.id, runId)
                    pendingFlow.value = PendingAsk(finding, task, preview, ai, ask, chatMissing, label, attention.showCost)
                }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "preparing a sparkle run failed: ${e.javaClass.simpleName}")
                failRun(finding.id, runId, AssistantMessages.of(e, R.string.att_run_start_failed).resolve(app().resources))
            }
        }
    }

    /** "Send" in the dialog: the agreement is stored when it was the first use, then the run goes. */
    fun confirm() {
        val p = pendingFlow.value ?: return
        pendingFlow.value = null
        val runId = RunRules.newRunId()
        setRun(SparkleRun(p.finding, p.task, runId, RunPhase.RUNNING, service = p.service, showCost = p.showCost))
        viewModelScope.launch {
            run(p.finding, p.task, runId, p.settings, grant = !p.settings.consented, chatYes = p.chatYesMissing)
        }
    }

    /** "Cancel" in the dialog: nothing was sent and nothing is stored. */
    fun declinePending() {
        pendingFlow.value = null
    }

    private suspend fun run(finding: Finding, task: AiSparkle, runId: String, settings: AiSettings, grant: Boolean, chatYes: Boolean) {
        try {
            val result = withContext(Dispatchers.IO) {
                val core = session.core
                if (grant) core.setAiSettings(ConsentRules.granted(settings), null)
                if (chatYes) core.aiChatConsent(true)
                core.aiSparkleRun(task, runId)
            }
            val current = runsFlow.value[finding.id]
            if (current == null || current.runId != runId) {
                // Stopped or dismissed while it ran: what came back is dropped.
                withContext(Dispatchers.IO) { runCatching { session.core.aiRunForget(runId) } }
                return
            }
            setRun(current.copy(phase = RunPhase.DONE, at = now(), result = result, open = result.proposals))
            afterChange(finding.id)
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            Log.w(Background.TAG, "a sparkle run failed: ${e.javaClass.simpleName}")
            failRun(finding.id, runId, AssistantMessages.of(e, R.string.att_run_failed).resolve(app().resources))
        }
    }

    /** The service as the user knows it: "OpenAI", or the host for an address of his own. */
    private fun serviceLabel(baseUrl: String): String {
        val service = AssistantRules.serviceOf(baseUrl)
        return if (service == AssistantRules.Service.OTHER) {
            AssistantRules.hostOf(baseUrl).ifEmpty { app().getString(R.string.assistant_svc_other) }
        } else {
            app().getString(serviceName(service))
        }
    }

    /** Retry on a failed run: the same finding starts again. */
    fun retry(finding: Finding) {
        runsFlow.update { it - finding.id }
        start(finding)
    }

    /** "Stop" while a run goes: the core is told, the card comes back. */
    fun stop(findingId: String) {
        val run = runsFlow.value[findingId] ?: return
        if (run.phase != RunPhase.RUNNING) return
        runsFlow.update { it - findingId }
        cancelCore(run.runId)
    }

    // ---- what the user does with the cards ----

    /** Add: the card is done through the core's apply path, once. */
    fun add(findingId: String, proposal: AiProposal) {
        val run = runsFlow.value[findingId] ?: return
        if (proposal.id in run.busy || run.open.none { it.id == proposal.id }) return
        setRun(run.adding(proposal.id))
        viewModelScope.launch {
            try {
                val applied = withContext(Dispatchers.IO) { session.core.aiApply(proposal.id) }
                updateRun(findingId) { it.added(listOf(proposal.id)) }
                val title = SparkleRules.inputOf(proposal)?.title.orEmpty()
                val text = app().getString(if (SparkleRules.isTask(proposal)) R.string.att_run_added_task else R.string.att_run_added_event, title)
                notice(text, if (applied.canUndo) ({ undo(proposal.id) }) else null)
                afterChange(findingId)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "adding a card failed: ${e.javaClass.simpleName}")
                val why = AssistantMessages.of(e, R.string.att_run_add_failed).resolve(app().resources)
                updateRun(findingId) { it.failed(mapOf(proposal.id to why)) }
            }
        }
    }

    /** "Add both" or "Add N events": one batch with one Undo. The ones that failed stay as cards with the reason. */
    fun addAll(findingId: String, ids: List<String>) {
        val run = runsFlow.value[findingId] ?: return
        val wanted = ids.filter { id -> run.open.any { it.id == id } && id !in run.busy }.distinct().take(MOST_AT_ONCE)
        if (wanted.isEmpty()) return
        if (wanted.size == 1) {
            run.open.firstOrNull { it.id == wanted[0] }?.let { add(findingId, it) }
            return
        }
        val tasks = run.open.filter { it.id in wanted }.associate { it.id to SparkleRules.isTask(it) }
        setRun(wanted.fold(run) { r, id -> r.adding(id) })
        viewModelScope.launch {
            try {
                val done = withContext(Dispatchers.IO) { session.core.aiApplyAll(wanted) }
                val appliedIds = done.applied.map { it.proposalId }
                val reasons = done.failed.associate { it.proposalId to it.message }
                updateRun(findingId) { it.added(appliedIds).failed(reasons) }
                val line = SparkleRules.batchLine(done, { tasks[it] }, ResourceSparkleWords(app().resources))
                val text = listOfNotNull(line.added, line.failed).joinToString(" ")
                if (text.isNotEmpty()) {
                    val undoId = done.undoId
                    notice(text, if (undoId != null) ({ undoBatch(undoId) }) else null)
                }
                afterChange(findingId)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "adding the cards failed: ${e.javaClass.simpleName}")
                val why = AssistantMessages.of(e, R.string.att_run_add_failed).resolve(app().resources)
                updateRun(findingId) { it.failed(wanted.associateWith { why }) }
            }
        }
    }

    /** The plan's "Edit" list: the lines left ticked are added, the others are dismissed. */
    fun addChosen(findingId: String, keep: Set<String>) {
        val run = runsFlow.value[findingId] ?: return
        val drop = run.open.filter { it.id !in keep }
        for (card in drop) dismissCard(findingId, card.id)
        addAll(findingId, run.open.filter { it.id in keep }.map { it.id })
    }

    private fun undo(proposalId: String) {
        viewModelScope.launch {
            try {
                withContext(Dispatchers.IO) { session.core.aiUndo(proposalId) }
                notice(app().getString(R.string.att_run_undone))
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "taking a card back failed: ${e.javaClass.simpleName}")
                notice(AssistantMessages.of(e, R.string.att_run_undo_failed).resolve(app().resources))
            }
        }
    }

    private fun undoBatch(undoId: String) {
        viewModelScope.launch {
            try {
                val done = withContext(Dispatchers.IO) { session.core.aiUndoBatch(undoId) }
                val text = if (done.failed.isEmpty()) app().getString(R.string.att_run_undone) else done.failed.first().message
                notice(text)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "taking the cards back failed: ${e.javaClass.simpleName}")
                notice(AssistantMessages.of(e, R.string.att_run_undo_failed).resolve(app().resources))
            }
        }
    }

    /** Dismiss on one card. */
    fun dismissCard(findingId: String, proposalId: String) {
        updateRun(findingId) { it.dismissedCard(proposalId) }
        viewModelScope.launch { withContext(Dispatchers.IO) { runCatching { session.core.aiDecline(proposalId) } } }
        afterChange(findingId)
    }

    /** Dismiss on the found place, the verdict, the ideas or the plain line. */
    fun dismissExtras(findingId: String) {
        updateRun(findingId) { it.dismissedExtras() }
        afterChange(findingId)
    }

    /** The whole result goes (also a failed run): the finding's own card comes back. */
    fun dismissRun(findingId: String) {
        val run = runsFlow.value[findingId] ?: return
        runsFlow.update { it - findingId }
        forget(run.runId)
    }

    /** A tap on a reply idea: a reply draft with the idea as its text. Nothing is sent from here. */
    fun useIdea(findingId: String, idea: String) {
        val run = runsFlow.value[findingId] ?: return
        val task = run.task as? AiSparkle.ReplyIdeas ?: return
        if (idea.isBlank()) return
        viewModelScope.launch {
            var draftId: String? = null
            try {
                val id = DraftLauncher.create(session.core, DraftKind.Reply(task.messageId))
                draftId = id
                withContext(Dispatchers.IO) {
                    val core = session.core
                    val draft = checkNotNull(core.draft(id)) { "the new draft is gone" }
                    core.saveDraft(draft.copy(body = DraftText.withAnswer(draft.body, idea.trim())))
                }
                eventChannel.send(SparkleEvent.OpenDraft(id))
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "starting a reply from an idea failed: ${e.javaClass.simpleName}")
                draftId?.let { DraftLauncher.discard(session.core, it) }
                notice(app().getString(R.string.att_run_draft_failed))
            }
        }
    }

    // ---- helpers ----

    /** After a change: a used-up run goes; one with at least one addition finishes its finding. */
    private fun afterChange(findingId: String) {
        val run = runsFlow.value[findingId] ?: return
        if (!run.isUsedUp) return
        runsFlow.update { it - findingId }
        forget(run.runId)
        if (run.isCompleted) eventChannel.trySend(SparkleEvent.Completed(run.finding))
    }

    private fun forget(runId: String) {
        viewModelScope.launch { withContext(Dispatchers.IO) { runCatching { session.core.aiRunForget(runId) } } }
    }

    private fun setRun(run: SparkleRun) = runsFlow.update { it + (run.id to run) }

    private fun updateRun(id: String, change: (SparkleRun) -> SparkleRun) {
        runsFlow.update { map -> map[id]?.let { map + (id to change(it)) } ?: map }
    }

    private fun removeRun(id: String, runId: String) = runsFlow.update { if (it[id]?.runId == runId) it - id else it }

    private fun failRun(id: String, runId: String, why: String) {
        runsFlow.update { map ->
            val run = map[id]
            if (run == null || run.runId != runId) map else map + (id to run.copy(phase = RunPhase.FAILED, error = why.replaceFirstChar { it.uppercase() }))
        }
    }

    private fun notice(text: String, undo: (() -> Unit)? = null) {
        eventChannel.trySend(SparkleEvent.Notice(text, undo))
    }

    private fun cancelCore(runId: String) {
        try {
            session.core.aiCancel(runId)
        } catch (e: Exception) {
            Log.w(Background.TAG, "cancelling a sparkle run failed: ${e.javaClass.simpleName}")
        }
    }

    private fun cancelAll() {
        for (run in runsFlow.value.values) if (run.phase == RunPhase.RUNNING) cancelCore(run.runId)
    }

    override fun onCleared() {
        cancelAll()
    }

    private companion object {
        /** The core takes at most 20 cards in one batch. */
        const val MOST_AT_ONCE = 20
    }
}
