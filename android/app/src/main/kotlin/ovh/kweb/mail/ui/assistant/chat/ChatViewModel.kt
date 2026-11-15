package ovh.kweb.mail.ui.assistant.chat

import android.app.Application
import android.util.Log
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import java.time.ZoneId
import java.util.UUID
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
import ovh.kweb.mail.accounts.AccountSession
import ovh.kweb.mail.background.Background
import ovh.kweb.mail.background.CheckScheduler
import ovh.kweb.mail.ui.SessionState
import ovh.kweb.mail.ui.UiMessage
import ovh.kweb.mail.ui.assistant.AssistantRules
import ovh.kweb.mail.ui.assistant.AssistantRules.ChatGate
import ovh.kweb.mail.ui.assistant.ConsentRules
import ovh.kweb.mail.ui.assistant.DraftText
import ovh.kweb.mail.ui.assistant.RunRules
import ovh.kweb.mail.ui.compose.DraftLauncher
import uniffi.kweb_core.AiChatListener
import uniffi.kweb_core.AiLink
import uniffi.kweb_core.AiStep
import uniffi.kweb_core.ConnectionStatus
import uniffi.kweb_core.DraftKind

sealed interface ChatEvent {
    /** The user tapped a message the assistant pointed at. */
    data class OpenThread(val threadId: String, val messageId: String) : ChatEvent

    /** A reply draft the user asked for is ready: it opens in the composer. Nothing is sent. */
    data class OpenDraft(val draftId: String) : ChatEvent
}

/**
 * The chat with the assistant, for one account. It lives in the store of its route, so leaving the screen for good
 * ends the conversation: the chat is memory only, here and in the core. It does not share the view model of the
 * assistant's sheets; the core runs one task at a time and answers a clash with its own sentence.
 *
 * Nothing here changes mail or the calendar by itself: a change is a card, done by [approve] on the user's tap, and
 * the core applies exactly the record the card showed. The key never comes here.
 */
class ChatViewModel(app: Application, private val session: AccountSession) : AndroidViewModel(app) {
    private val stateFlow = MutableStateFlow(ChatState())
    private val eventChannel = Channel<ChatEvent>(Channel.BUFFERED)

    /** The core's calls for a conversation go one after the other: a send waits for "new chat" and for the send before it. */
    private val chatLock = Mutex()

    /** The conversation in use; a new one for every "New chat". */
    private var chatId: String = newChatId()

    /** The conversation the core has started, when that worked. */
    private var startedId: String? = null

    val state: StateFlow<ChatState> get() = stateFlow

    val events: Flow<ChatEvent> get() = eventChannel.receiveAsFlow()

    /** What is being typed. The user's own text, kept across a rotation with the view model. */
    var input by mutableStateOf("")
        private set

    init {
        startChat(chatId)
        viewModelScope.launch {
            SessionState.signedIn.collect { signedIn ->
                if (signedIn == false) {
                    cancelCore(stateFlow.value.runId)
                    stateFlow.value = ChatState()
                    input = ""
                }
            }
        }
    }

    fun onInput(text: String) {
        input = text
    }

    fun dismissNotice() = stateFlow.update { it.copy(notice = null) }

    // ---- settings and the conversation ----

    /** Reads whether the chat may be used. Called when the screen comes up, and again when it comes back from the settings. */
    fun refresh() {
        viewModelScope.launch {
            val read = try {
                withContext(Dispatchers.IO) { session.core.aiSettings() }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "reading the assistant settings failed: ${e.javaClass.simpleName}")
                null
            }
            if (read == null) {
                stateFlow.update { it.copy(settingsFailed = it.gate == null) }
                return@launch
            }
            val gate = AssistantRules.chatReady(read)
            stateFlow.update { it.copy(gate = gate, settingsFailed = false, host = ConsentRules.hostOf(read.baseUrl)) }
            // The first start may have failed, or was made before the chat was allowed: now it can be used.
            if (gate == ChatGate.READY && startedId != chatId) startChat(chatId)
        }
    }

    /** "New chat": a running answer is stopped, the list is emptied and the core starts a new conversation. */
    fun newChat() = restart(restarted = false)

    private fun restart(restarted: Boolean) {
        cancelCore(stateFlow.value.runId)
        val id = newChatId()
        chatId = id
        stateFlow.update { ChatRules.cleared(it, restarted) }
        startChat(id)
    }

    /** Tells the core about the conversation [id]; this drops the one before it with its suggestions. Sends nothing. */
    private fun startChat(id: String) {
        viewModelScope.launch {
            chatLock.withLock {
                if (chatId != id || startedId == id) return@launch
                try {
                    withContext(Dispatchers.IO) { session.core.aiChatNew(id) }
                    startedId = id
                } catch (e: CancellationException) {
                    throw e
                } catch (e: Exception) {
                    Log.w(Background.TAG, "starting a chat failed: ${e.javaClass.simpleName}")
                    // While the chat may not be used anyway the bar on top says why; this would only be noise.
                    if (stateFlow.value.gate == ChatGate.READY) {
                        stateFlow.update { it.copy(notice = ChatRules.message(e, R.string.chat_new_failed)) }
                    }
                }
            }
        }
    }

    // ---- one message ----

    /** Sends what was typed. The answer arrives piece by piece; [stop] ends it. */
    fun send() {
        val text = input.trim()
        val before = stateFlow.value
        if (!ChatRules.canSend(before.gate, before.running, text)) return
        val runId = RunRules.newRunId()
        val chat = chatId
        stateFlow.update { ChatRules.sent(it, text, runId) }
        input = ""
        viewModelScope.launch {
            val turn = try {
                chatLock.withLock {
                    // Stopped, or a new chat began, while it waited for the call before it to end.
                    if (stateFlow.value.runId != runId) return@launch
                    val zone = ZoneId.systemDefault().id
                    withContext(Dispatchers.IO) { session.core.aiChatSend(chat, text, zone, runId, Heard(runId)) }
                }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "a chat message failed: ${e.javaClass.simpleName}")
                sendFailed(runId, text, e)
                return@launch
            }
            stateFlow.update { ChatRules.finished(it, runId, turn) }
        }
    }

    private fun sendFailed(runId: String, text: String, error: Exception) {
        val current = stateFlow.value.running && stateFlow.value.runId == runId
        when (ChatRules.failure(error)) {
            // The user's Stop already moved the state on; a cancel from elsewhere (sign-out) ends it the same way.
            ChatRules.Failure.CANCELLED -> stateFlow.update { if (it.runId == runId) ChatRules.stopped(it) else it }
            ChatRules.Failure.OVER -> if (current) {
                // The run is over: there is nothing left to cancel when the new chat begins.
                stateFlow.update { it.copy(running = false, runId = null) }
                restart(restarted = true)
                giveBack(text)
            }
            ChatRules.Failure.BUSY, ChatRules.Failure.OTHER -> if (current) {
                stateFlow.update { ChatRules.failed(it, runId, ChatRules.message(error)) }
                giveBack(text)
            }
        }
    }

    /** The core did not take the message: it goes back into the input, unless the user already typed something new. */
    private fun giveBack(text: String) {
        if (input.isBlank()) input = text
    }

    /** Hears the answer on a core thread. It only touches the state, and only for the run it was made for. */
    private inner class Heard(private val runId: String) : AiChatListener {
        override fun text(piece: String) = stateFlow.update { ChatRules.piece(it, runId, piece) }

        override fun step(step: AiStep) = stateFlow.update { ChatRules.step(it, runId, step) }
    }

    /** "Stop": keeps what came so far and asks the core to end the run. */
    fun stop() {
        val s = stateFlow.value
        if (!s.running) return
        stateFlow.update { ChatRules.stopped(it) }
        cancelCore(s.runId)
    }

    /** The core's cancel does not block, so it is called here and now: a launched coroutine could be cancelled before it ran. */
    private fun cancelCore(runId: String?) {
        if (runId == null) return
        try {
            session.core.aiCancel(runId)
        } catch (e: Exception) {
            Log.w(Background.TAG, "cancelling the assistant failed: ${e.javaClass.simpleName}")
        }
    }

    override fun onCleared() {
        // The screen is gone for good: a request still out must not keep the core's one slot, and the conversation
        // with its suggestions and undo records goes. Neither call blocks.
        cancelCore(stateFlow.value.runId)
        try {
            session.core.aiChatClose(chatId)
        } catch (e: Exception) {
            Log.w(Background.TAG, "closing the chat failed: ${e.javaClass.simpleName}")
        }
    }

    // ---- suggestions ----

    /** "Approve": the core does exactly what the card of [proposalId] shows, once. */
    fun approve(proposalId: String) {
        val card = ChatRules.card(stateFlow.value, proposalId) ?: return
        val next = ChatRules.approve(card.state) ?: return
        stateFlow.update { ChatRules.withCard(it, proposalId, next) }
        viewModelScope.launch {
            val after = try {
                ChatRules.applied(withContext(Dispatchers.IO) { session.core.aiApply(proposalId) })
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "applying a suggestion failed: ${e.javaClass.simpleName}")
                ChatRules.applyFailed(card.proposal.change, e)
            }
            stateFlow.update { ChatRules.withCard(it, proposalId, after) }
            checkSoonWhenNotLive()
        }
    }

    /** "Don't": the suggestion is closed; nothing was done. */
    fun decline(proposalId: String) {
        val card = ChatRules.card(stateFlow.value, proposalId) ?: return
        val next = ChatRules.decline(card.state) ?: return
        try {
            // Never fails and does not block.
            session.core.aiDecline(proposalId)
        } catch (e: Exception) {
            Log.w(Background.TAG, "declining a suggestion failed: ${e.javaClass.simpleName}")
        }
        stateFlow.update { ChatRules.withCard(it, proposalId, next) }
    }

    /** "Undo": takes back what the card of [proposalId] did. */
    fun undo(proposalId: String) {
        val card = ChatRules.card(stateFlow.value, proposalId) ?: return
        val next = ChatRules.undo(card.state) ?: return
        stateFlow.update { ChatRules.withCard(it, proposalId, next) }
        viewModelScope.launch {
            val after = try {
                withContext(Dispatchers.IO) { session.core.aiUndo(proposalId) }
                CardState.Undone
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "undoing a suggestion failed: ${e.javaClass.simpleName}")
                ChatRules.undoFailed(e)
            }
            stateFlow.update { ChatRules.withCard(it, proposalId, after) }
            checkSoonWhenNotLive()
        }
    }

    /** A mail change waits in the outbox when the live connection is down: a check carries it to the server. */
    private suspend fun checkSoonWhenNotLive() {
        try {
            val live = withContext(Dispatchers.IO) { session.core.status() is ConnectionStatus.Live }
            if (!live) CheckScheduler.checkSoon(getApplication())
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            Log.w(Background.TAG, "scheduling a check failed: ${e.javaClass.simpleName}")
        }
    }

    // ---- things to open ----

    /**
     * The user tapped a chip. A message opens in its conversation. A reply draft is made by the app's own reply rules
     * (the recipients are theirs, never the model's), gets the suggested text above the quoted original, and opens in
     * the composer: nothing is sent from here.
     */
    fun open(link: AiLink) {
        when (link) {
            is AiLink.Message -> eventChannel.trySend(ChatEvent.OpenThread(link.message.threadId, link.message.id))
            is AiLink.ReplyDraft -> makeDraft(link)
        }
    }

    private fun makeDraft(link: AiLink.ReplyDraft) {
        if (stateFlow.value.draftBusy) return
        stateFlow.update { it.copy(draftBusy = true, notice = null) }
        viewModelScope.launch {
            var draftId: String? = null
            try {
                val id = DraftLauncher.create(session.core, DraftKind.Reply(link.message.id))
                draftId = id
                withContext(Dispatchers.IO) {
                    val core = session.core
                    val draft = checkNotNull(core.draft(id)) { "the new draft is gone" }
                    core.saveDraft(draft.copy(body = DraftText.withAnswer(draft.body, link.text)))
                }
                stateFlow.update { it.copy(draftBusy = false) }
                eventChannel.send(ChatEvent.OpenDraft(id))
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "starting a reply from the chat failed: ${e.javaClass.simpleName}")
                draftId?.let { DraftLauncher.discard(session.core, it) }
                stateFlow.update { it.copy(draftBusy = false, notice = UiMessage.Res(R.string.chat_draft_failed)) }
            }
        }
    }

    private companion object {
        fun newChatId(): String = UUID.randomUUID().toString()
    }
}
