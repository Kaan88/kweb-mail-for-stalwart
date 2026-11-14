package ovh.kweb.mail.ui.assistant

import android.app.Application
import android.util.Log
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import ovh.kweb.mail.R
import ovh.kweb.mail.background.Background
import ovh.kweb.mail.ui.SessionState
import ovh.kweb.mail.ui.UiMessage
import ovh.kweb.mail.ui.assistant.AssistantRules.Kind
import ovh.kweb.mail.ui.compose.DraftLauncher
import uniffi.kweb_core.AiListener
import uniffi.kweb_core.AiPreview
import uniffi.kweb_core.AiSettings
import uniffi.kweb_core.AiTask
import uniffi.kweb_core.DraftKind

/** Where the text of a rewrite came from, to put the result back exactly there. */
data class RewriteTarget(val span: Span, val original: String)

sealed interface AssistPhase {
    /** A reply draft waits for what the user wants it to say. */
    data object Input : AssistPhase

    /** Reading the settings (and what would be sent). */
    data object Checking : AssistPhase

    /** The first task for this service and model: the user is asked before anything is sent. */
    data class Consent(val preview: AiPreview, val settings: AiSettings) : AssistPhase

    data object Running : AssistPhase

    data object Done : AssistPhase

    /** The user stopped it; what came so far is kept to read or copy, not to use. */
    data object Stopped : AssistPhase

    data class Failed(val message: UiMessage) : AssistPhase
}

/**
 * One task and its sheet. [owner] is the screen it belongs to (a sheet shows only the session of its own screen),
 * [id] tells this task from the ones before it: whatever arrives for an older id is dropped.
 */
data class AssistSession(
    val id: Long,
    /** The name this task's run goes by in the core (a UUID, new for every start); a Stop names it. */
    val runId: String,
    val owner: String,
    val kind: Kind,
    val task: AiTask,
    val model: String,
    val phase: AssistPhase,
    val text: String = "",
    val rewrite: RewriteTarget? = null,
    /** A reply draft is being made from the answer. */
    val busy: Boolean = false,
    /** Something went wrong after the answer was complete (the draft could not be started). */
    val notice: UiMessage? = null,
)

data class AssistantState(
    /** As the core has them; null until read, or when they could not be read. */
    val settings: AiSettings? = null,
    val session: AssistSession? = null,
)

sealed interface AssistantEvent {
    /** A reply draft with the answer in it is ready: the screen of [owner] opens it. */
    data class OpenDraft(val owner: String, val draftId: String) : AssistantEvent
}

/**
 * The AI assistant's tasks. One task at a time for the whole app (the core runs one), so this lives in the
 * activity's store and the screens only show the session that is theirs. A rotation keeps it; a screen that is left
 * for good calls [release]. The key never comes here: it is typed on the settings screen and goes straight to the core.
 */
class AssistantViewModel(app: Application) : AndroidViewModel(app) {
    private val stateFlow = MutableStateFlow(AssistantState())
    private val eventChannel = Channel<AssistantEvent>(Channel.BUFFERED)

    /** The core's call blocks until the answer is complete: the next task waits here for the last one to end. */
    private val runLock = Mutex()
    private var counter = 0L

    val state: StateFlow<AssistantState> get() = stateFlow

    val events: Flow<AssistantEvent> get() = eventChannel.receiveAsFlow()

    init {
        viewModelScope.launch {
            SessionState.signedIn.collect { signedIn ->
                if (signedIn == false) {
                    cancelCore(stateFlow.value.session?.runId)
                    stateFlow.value = AssistantState()
                }
            }
        }
    }

    /** Reads how the assistant is set up, for the entries that depend on it. Called when a screen comes up. */
    fun refreshSettings() {
        viewModelScope.launch {
            val read = try {
                withContext(Dispatchers.IO) { Background.core(getApplication()).aiSettings() }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "reading the assistant settings failed: ${e.javaClass.simpleName}")
                null
            }
            stateFlow.update { it.copy(settings = read) }
        }
    }

    // ---- starting ----

    /**
     * Opens the sheet of [owner] for [task]. A task that is running is stopped first. A summary or a rewrite starts at
     * once; a reply draft waits for [start] (the user may say what it should say). Does nothing when this kind is
     * not allowed.
     */
    fun open(owner: String, kind: Kind, task: AiTask, rewrite: RewriteTarget? = null) {
        val settings = stateFlow.value.settings
        if (!AssistantRules.isAllowed(settings, kind)) return
        stopRunning()
        val id = ++counter
        val phase = if (kind == Kind.DRAFT) AssistPhase.Input else AssistPhase.Checking
        val session = AssistSession(id, RunRules.newRunId(), owner, kind, task, model = settings?.model.orEmpty(), phase = phase, rewrite = rewrite)
        stateFlow.update { it.copy(session = session) }
        if (phase == AssistPhase.Checking) check(id)
    }

    /** A reply draft: start with what the user wants it to say (may be empty). Also "Try again" after a stop or a failure. */
    fun start(instruction: String) {
        val s = stateFlow.value.session ?: return
        if (s.phase != AssistPhase.Input && s.phase != AssistPhase.Stopped && s.phase !is AssistPhase.Failed) return
        val task = (s.task as? AiTask.DraftReply)?.copy(instruction = instruction.trim()) ?: s.task
        // A new id: whatever the stopped or failed run still delivers belongs to the old one and is dropped.
        val id = ++counter
        stateFlow.update { state ->
            if (state.session?.id != s.id) state else state.copy(session = s.copy(id = id, runId = RunRules.newRunId(), task = task, phase = AssistPhase.Checking, text = "", notice = null))
        }
        check(id)
    }

    /** Runs the same task again after a failure or a stop. */
    fun retry() {
        val s = stateFlow.value.session ?: return
        start((s.task as? AiTask.DraftReply)?.instruction.orEmpty())
    }

    /** Reads the settings and what would be sent; asks for consent if it was not given, otherwise runs. */
    private fun check(id: Long) {
        viewModelScope.launch {
            try {
                val task = sessionOf(id)?.task ?: return@launch
                val (settings, preview) = withContext(Dispatchers.IO) {
                    val core = Background.core(getApplication())
                    val read = core.aiSettings()
                    read to if (ConsentRules.isNeeded(read)) core.aiPreview(task) else null
                }
                val s = sessionOf(id) ?: return@launch
                stateFlow.update { it.copy(settings = settings) }
                when {
                    !AssistantRules.isAllowed(settings, s.kind) -> fail(id, UiMessage.Res(R.string.assistant_not_available))
                    preview != null -> updateSession(id) { it.copy(phase = AssistPhase.Consent(preview, settings), model = settings.model) }
                    else -> run(id, task, settings.model)
                }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "preparing an assistant task failed: ${e.javaClass.simpleName}")
                fail(id, AssistantMessages.of(e, R.string.assistant_failed))
            }
        }
    }

    /** "Send" in the consent dialog: stores the consent for this service and model, then runs. */
    fun confirmConsent() {
        val s = stateFlow.value.session ?: return
        val phase = s.phase as? AssistPhase.Consent ?: return
        updateSession(s.id) { it.copy(phase = AssistPhase.Checking) }
        viewModelScope.launch {
            try {
                val stored = withContext(Dispatchers.IO) { Background.core(getApplication()).setAiSettings(ConsentRules.granted(phase.settings), null) }
                stateFlow.update { it.copy(settings = stored) }
                if (sessionOf(s.id) == null) return@launch
                run(s.id, s.task, stored.model)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "storing the consent failed: ${e.javaClass.simpleName}")
                fail(s.id, AssistantMessages.of(e, R.string.assistant_failed))
            }
        }
    }

    /** "Cancel" in the consent dialog: a reply draft goes back to its question, anything else closes. */
    fun declineConsent() {
        val s = stateFlow.value.session ?: return
        if (s.phase !is AssistPhase.Consent) return
        if (s.kind == Kind.DRAFT) updateSession(s.id) { it.copy(phase = AssistPhase.Input) } else close()
    }

    private suspend fun run(id: Long, task: AiTask, model: String) {
        updateSession(id) { it.copy(phase = AssistPhase.Running, text = "", model = model) }
        val answer = try {
            runLock.withLock {
                // Superseded or stopped while it waited for the last task to end.
                if (sessionOf(id)?.phase != AssistPhase.Running) return
                val runId = sessionOf(id)?.runId ?: return
                withContext(Dispatchers.IO) { Background.core(getApplication()).aiRun(task, runId, Pieces(id)) }
            }
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            Log.w(Background.TAG, "an assistant task failed: ${e.javaClass.simpleName}")
            // A stop already moved the phase on; only a task that is still running fails.
            if (sessionOf(id)?.phase == AssistPhase.Running) fail(id, AssistantMessages.of(e, R.string.assistant_failed))
            return
        }
        // The whole answer replaces what was streamed, so it is complete even if a piece was lost.
        updateSession(id) { if (it.phase == AssistPhase.Running) it.copy(phase = AssistPhase.Done, text = answer.ifBlank { it.text }) else it }
    }

    /** Receives the answer on a core thread. It only appends to the session it was made for, if that is still running. */
    private inner class Pieces(private val id: Long) : AiListener {
        override fun text(piece: String) {
            stateFlow.update { state ->
                val s = state.session
                if (s == null || s.id != id || s.phase != AssistPhase.Running) state else state.copy(session = s.copy(text = s.text + piece))
            }
        }
    }

    // ---- stopping and closing ----

    /** "Stop": keeps what came so far and asks the core to end the task. */
    fun stop() {
        val s = stateFlow.value.session ?: return
        if (s.phase != AssistPhase.Running) return
        updateSession(s.id) { it.copy(phase = AssistPhase.Stopped) }
        cancelCore(s.runId)
    }

    /** The sheet was left or dismissed: a running task is cancelled and the session is gone. */
    fun close() {
        stopRunning()
        stateFlow.update { it.copy(session = null) }
    }

    /** A screen was left for good: its session goes, and with it any task still running for it. */
    fun release(owner: String) {
        if (stateFlow.value.session?.owner == owner) close()
    }

    private fun stopRunning() {
        val session = stateFlow.value.session
        if (RunRules.cancelsWhenClosed(session?.phase)) cancelCore(session?.runId)
    }

    /** The core's cancel does not block, so it is called here and now: a launched coroutine could be cancelled before it ran. */
    private fun cancelCore(runId: String?) {
        if (runId == null) return
        try {
            Background.core(getApplication()).aiCancel(runId)
        } catch (e: Exception) {
            Log.w(Background.TAG, "cancelling the assistant failed: ${e.javaClass.simpleName}")
        }
    }

    override fun onCleared() {
        // The activity is gone for good: a request still out must not keep the core's one slot.
        val session = stateFlow.value.session
        if (RunRules.cancelsWhenClosed(session?.phase)) cancelCore(session?.runId)
    }

    // ---- using the answer ----

    /**
     * Makes a reply draft with the answer above the quoted original, saves it and tells the owner's screen to open it.
     * Nothing is sent from here.
     */
    fun useAsReply() {
        val s = stateFlow.value.session ?: return
        val task = s.task as? AiTask.DraftReply ?: return
        if (s.phase != AssistPhase.Done || s.busy) return
        updateSession(s.id) { it.copy(busy = true, notice = null) }
        viewModelScope.launch {
            var draftId: String? = null
            try {
                val id = DraftLauncher.create(getApplication(), DraftKind.Reply(task.messageId))
                draftId = id
                withContext(Dispatchers.IO) {
                    val core = Background.core(getApplication())
                    val draft = checkNotNull(core.draft(id)) { "the new draft is gone" }
                    core.saveDraft(draft.copy(body = DraftText.withAnswer(draft.body, s.text)))
                }
                if (sessionOf(s.id) == null) {
                    // The sheet was closed meanwhile: do not open a screen nobody asked for; the draft stays in Drafts.
                    return@launch
                }
                eventChannel.send(AssistantEvent.OpenDraft(s.owner, id))
                stateFlow.update { if (it.session?.id == s.id) it.copy(session = null) else it }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "starting a reply from the assistant failed: ${e.javaClass.simpleName}")
                draftId?.let { DraftLauncher.discard(getApplication(), it) }
                updateSession(s.id) { it.copy(busy = false, notice = UiMessage.Res(R.string.assistant_draft_failed)) }
            }
        }
    }

    // ---- helpers ----

    private fun sessionOf(id: Long): AssistSession? = stateFlow.value.session?.takeIf { it.id == id }

    private fun fail(id: Long, message: UiMessage) = updateSession(id) { it.copy(phase = AssistPhase.Failed(message)) }

    /** Changes the session only if it is still the one with [id]. */
    private fun updateSession(id: Long, change: (AssistSession) -> AssistSession) {
        stateFlow.update { state ->
            val s = state.session
            if (s == null || s.id != id) state else state.copy(session = change(s))
        }
    }
}
