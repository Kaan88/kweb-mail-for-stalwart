package ovh.kweb.mail.ui.thread

import ovh.kweb.mail.accounts.AccountSession
import android.app.Application
import android.util.Log
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.Job
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
import ovh.kweb.mail.background.Background
import ovh.kweb.mail.background.CheckScheduler
import ovh.kweb.mail.ui.MailActions
import ovh.kweb.mail.ui.Route
import ovh.kweb.mail.ui.SessionState
import ovh.kweb.mail.ui.compose.DraftLauncher
import ovh.kweb.mail.ui.UiMessage
import ovh.kweb.mail.ui.folders.SpamRules
import ovh.kweb.mail.ui.mailextras.CoreMailExtrasBackend
import ovh.kweb.mail.ui.mailextras.LabelStore
import ovh.kweb.mail.ui.mailextras.UnsubscribeController
import uniffi.kweb_core.Action
import uniffi.kweb_core.Attachment
import uniffi.kweb_core.ConnectionStatus
import uniffi.kweb_core.DraftKind
import uniffi.kweb_core.Mailbox
import uniffi.kweb_core.MessageBody
import uniffi.kweb_core.MessageHeader
import uniffi.kweb_core.Exception as CoreException

/** The body of one message as the card shows it. */
sealed interface BodyState {
    data object Loading : BodyState

    /** Not in the cache and the server could not be reached, or the message is gone. */
    data object Failed : BodyState

    data class Loaded(val body: MessageBody) : BodyState
}

data class ThreadState(
    /** Oldest first. */
    val messages: List<MessageHeader> = emptyList(),
    val expanded: Set<String> = emptySet(),
    val bodies: Map<String, BodyState> = emptyMap(),
    /** Messages whose remote content the user allowed. Not remembered beyond this screen. */
    val remoteAllowed: Set<String> = emptySet(),
    val mailboxes: List<Mailbox> = emptyList(),
    val myAddress: String? = null,
    /** The cache has been read at least once. */
    val loaded: Boolean = false,
    /** The server has been asked which messages the thread has, and answered. */
    val refreshed: Boolean = false,
    /** Nothing is cached and the server could not be asked. */
    val offline: Boolean = false,
) {
    /** The subject of the oldest message that has one (the newest is often "Re: …"). */
    /** What Archive, Delete, Move and Spam act on, for a thread opened from [mailboxId]. */
    fun targets(mailboxId: String?): List<String> = ThreadRules.targets(messages, mailboxId, mailboxes)

    val subject: String get() = messages.firstOrNull { it.subject.isNotBlank() }?.subject?.trim().orEmpty()
}

sealed interface ThreadEvent {
    data class Message(val textRes: Int) : ThreadEvent

    /** A refusal in the core's own words (spam). */
    data class Text(val message: UiMessage) : ThreadEvent

    /** [address] is blocked: their mail goes to the Trash from now on. */
    data class Blocked(val address: String) : ThreadEvent

    /** The core refused to block; [text] is its reason. The screen offers the filters screen next to it. */
    data class BlockRefused(val text: String) : ThreadEvent

    /** Leave the screen: the thread was acted on or is gone. */
    data object Close : ThreadEvent

    /** A draft was started for a reply or a forward: open the compose screen on it. */
    data class OpenDraft(val draftId: String) : ThreadEvent
}

/**
 * One open conversation. It is shared with the activity and so outlives the screen (a rotation
 * must not lose the open cards), but it only works between [start] and [reset]: everything that
 * watches the cache or the server lives in a scope that [reset] cancels. A closed thread does nothing.
 */
class ThreadViewModel(app: Application, private val acct: AccountSession, private val route: Route.Thread) : AndroidViewModel(app) {
    private val stateFlow = MutableStateFlow(ThreadState())
    private val reloads = Channel<Unit>(Channel.CONFLATED)
    private val eventChannel = Channel<ThreadEvent>(Channel.BUFFERED)

    /** Alive while the screen is shown; null when the thread is closed. */
    private var session: CoroutineScope? = null
    private var markedRead = false
    private var closeSent = false
    private var startingDraft = false
    private var blocking = false

    private val extras = CoreMailExtrasBackend { acct.core }

    /** The labels of the thread's messages and the user's list. */
    val labels = LabelStore(viewModelScope, Dispatchers.IO, extras, onError = { eventChannel.trySend(ThreadEvent.Text(it)) })

    /** The Unsubscribe chips of the opened messages. */
    val unsubscribe = UnsubscribeController(viewModelScope, Dispatchers.IO, extras, onError = { eventChannel.trySend(ThreadEvent.Text(it)) })

    /** The attachment the system file picker is open for. Lives here so a rotation does not lose it. */
    var pendingSave: Attachment? = null

    val state: StateFlow<ThreadState> get() = stateFlow

    val events: Flow<ThreadEvent> get() = eventChannel.receiveAsFlow()

    /** Called each time the screen comes up: reads the cache at once; the first time, asks the server too. */
    fun start() {
        if (session != null) {
            reload()
            return
        }
        val scope = CoroutineScope(viewModelScope.coroutineContext + SupervisorJob(viewModelScope.coroutineContext[Job]))
        session = scope
        scope.launch { reloads.receiveAsFlow().collect { load() } }
        scope.launch { acct.cacheChanges.collect { reload() } }
        // Signing out under an open thread leaves nothing to show.
        scope.launch { SessionState.signedIn.collect { if (it == false) reset() } }
        scope.launch { refreshFromServer() }
        reload()
    }

    /** The screen was left for good: stop everything and forget the cards, the bodies and what was marked read. */
    fun reset() {
        session?.cancel()
        session = null
        markedRead = false
        closeSent = false
        pendingSave = null
        labels.reset()
        unsubscribe.reset()
        stateFlow.value = ThreadState()
        // Nothing queued for the last visit may reach the next one.
        while (reloads.tryReceive().isSuccess) { }
        while (eventChannel.tryReceive().isSuccess) { }
    }

    fun reload() {
        reloads.trySend(Unit)
    }

    fun toggle(messageId: String) {
        stateFlow.update { it.copy(expanded = if (messageId in it.expanded) it.expanded - messageId else it.expanded + messageId) }
        fetchMissingBodies()
        askUnsubscribe()
    }

    /** An opened message of someone else's may come from a list: ask, once, whether it can be left. A failure shows nothing. */
    private fun askUnsubscribe() {
        if (session == null) return
        val s = stateFlow.value
        for (id in s.expanded) {
            val header = s.messages.firstOrNull { it.id == id } ?: continue
            if (s.myAddress != null && header.fromEmail.equals(s.myAddress, ignoreCase = true)) continue
            unsubscribe.check(id)
        }
    }

    fun retryBody(messageId: String) {
        stateFlow.update { it.copy(bodies = it.bodies - messageId) }
        fetchMissingBodies()
    }

    fun allowRemote(messageId: String) = stateFlow.update { it.copy(remoteAllowed = it.remoteAllowed + messageId) }

    // ---- Replying ----

    /** Starts a reply, reply all or forward and has the screen open it. One at a time: a double tap makes one draft. */
    fun startDraft(kind: DraftKind) {
        if (startingDraft) return
        startingDraft = true
        viewModelScope.launch {
            try {
                eventChannel.send(ThreadEvent.OpenDraft(DraftLauncher.create(acct.core, kind)))
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "starting a draft failed", e)
                eventChannel.send(ThreadEvent.Message(R.string.compose_start_failed))
            } finally {
                startingDraft = false
            }
        }
    }

    // ---- Actions on the thread ----

    /** The messages Archive, Delete, Move and Spam would act on now. */
    fun targets(): List<String> = stateFlow.value.targets(route.mailboxId)

    fun archive() = onTargets { MailActions.archive(it) }

    fun delete() = onTargets { MailActions.delete(it) }

    fun move(mailboxId: String) = onTargets { MailActions.move(it, mailboxId) }

    /** Report spam for the thread's messages, or "Not spam" for those in Junk; closes the screen like Archive. */
    fun spam() {
        val s = stateFlow.value
        val ids = targets()
        if (ids.isEmpty()) return
        val isSpam = !SpamRules.threadIsNotSpam(s.messages, ids, route.mailboxId, s.mailboxes)
        viewModelScope.launch {
            try {
                MailActions.reportSpam(getApplication(), acct.core, ids, isSpam)
                reload()
                close()
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "reporting spam failed: ${e.javaClass.simpleName}")
                reload()
                eventChannel.send(ThreadEvent.Text(SpamRules.failure(e)))
            }
        }
    }

    /**
     * Blocks the sender of [messageId]. The core decides (and refuses with words for the user); one block at a
     * time. Stays on the screen: the message is in the Trash after it and the cards show that.
     */
    fun blockSender(messageId: String) {
        val address = stateFlow.value.messages.firstOrNull { it.id == messageId }?.fromEmail.orEmpty()
        if (blocking) return
        blocking = true
        viewModelScope.launch {
            try {
                withContext(Dispatchers.IO) {
                    val core = acct.core
                    core.blockSender(messageId)
                    // The delete waits in the outbox; without a live connection nothing else would send it soon.
                    if (core.status() !is ConnectionStatus.Live) CheckScheduler.checkSoon(getApplication())
                }
                reload()
                eventChannel.send(ThreadEvent.Blocked(address))
            } catch (e: CancellationException) {
                throw e
            } catch (e: CoreException.Invalid) {
                eventChannel.send(if (e.msg.isNotBlank()) ThreadEvent.BlockRefused(e.msg) else ThreadEvent.Message(R.string.block_failed))
            } catch (e: Exception) {
                Log.w(Background.TAG, "blocking a sender failed: ${e.javaClass.simpleName}")
                eventChannel.send(ThreadEvent.Message(R.string.block_failed))
            } finally {
                blocking = false
            }
        }
    }

    fun markUnread() {
        val ids = stateFlow.value.messages.map { it.id }
        if (ids.isNotEmpty()) submit(MailActions.markUnread(ids)) { close() }
    }

    /** Star or unstar the whole thread; stays on the screen. */
    fun toggleFlag() {
        val messages = stateFlow.value.messages
        if (messages.isEmpty()) return
        submit(MailActions.setFlagged(messages.map { it.id }, flagged = messages.none { it.isFlagged }))
    }

    /** Does nothing, and does not close the screen, when there is nothing to act on. */
    private fun onTargets(actions: (List<String>) -> List<Action>) {
        val ids = targets()
        if (ids.isNotEmpty()) submit(actions(ids)) { close() }
    }

    /** Tells the screen to go back, once, however many reasons come together. */
    private suspend fun close() {
        if (closeSent) return
        closeSent = true
        eventChannel.send(ThreadEvent.Close)
    }

    /** Actions outlive the screen: the user decided them, so they are not in the session scope. */
    private fun submit(actions: List<Action>, onDone: suspend () -> Unit = {}) {
        viewModelScope.launch {
            try {
                MailActions.submit(getApplication(), acct.core, actions)
                reload()
                onDone()
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "thread action failed", e)
                // The core stops at the first error; some of the actions may have been applied.
                reload()
                eventChannel.send(ThreadEvent.Message(R.string.list_action_failed))
            }
        }
    }

    // ---- Reading ----

    private suspend fun refreshFromServer() {
        try {
            withContext(Dispatchers.IO) { acct.core.refreshThread(route.threadId) }
            stateFlow.update { it.copy(refreshed = true, offline = false) }
        } catch (e: CancellationException) {
            throw e
        } catch (e: CoreException.Temporary) {
            // Offline: what the cache has is what there is.
            stateFlow.update { it.copy(offline = true) }
        } catch (e: Exception) {
            Log.w(Background.TAG, "refreshing the thread failed", e)
            stateFlow.update { it.copy(offline = true) }
        }
        reload()
    }

    private suspend fun load() {
        try {
            val read = withContext(Dispatchers.IO) {
                val core = acct.core
                Triple(core.threadMessages(route.threadId), core.mailboxes(), core.account()?.username)
            }
            val (messages, mailboxes, me) = read
            val before = stateFlow.value.messages
            val knownIds = before.map { it.id }.toSet()
            // Messages that arrive while the thread is open; the first set is the initial one.
            val arrived = if (before.isEmpty()) emptyList() else messages.filter { it.id !in knownIds && it.isUnread }
            stateFlow.update { old ->
                val expanded = if (old.messages.isEmpty()) {
                    ThreadRules.initiallyExpanded(messages, route.messageId)
                } else {
                    old.expanded.filter { id -> messages.any { it.id == id } }.toSet() + arrived.map { it.id }
                }
                old.copy(
                    messages = messages,
                    expanded = expanded,
                    mailboxes = mailboxes,
                    myAddress = me?.takeIf { '@' in it },
                    loaded = true,
                )
            }
            if (messages.isNotEmpty()) markInitialRead(messages)
            if (arrived.isNotEmpty()) markRead(arrived.map { it.id })
            fetchMissingBodies()
            askUnsubscribe()
            labels.refreshLabels()
            labels.load(messages.map { it.id })
            val s = stateFlow.value
            if (ThreadRules.isGone(s.loaded, s.refreshed, messages.size)) close()
        } catch (e: CancellationException) {
            throw e
        } catch (e: CoreException.SignInNeeded) {
            stateFlow.update { it.copy(loaded = true) }
        } catch (e: Exception) {
            Log.w(Background.TAG, "reading the thread failed", e)
            stateFlow.update { it.copy(loaded = true) }
            eventChannel.send(ThreadEvent.Message(R.string.thread_read_failed))
        }
    }

    /** Opening the thread reads it: once per visit, the messages that are unread now. */
    private fun markInitialRead(messages: List<MessageHeader>) {
        if (markedRead) return
        markedRead = true
        markRead(messages.filter { it.isUnread }.map { it.id })
    }

    private fun markRead(ids: List<String>) {
        if (ids.isEmpty()) return
        viewModelScope.launch {
            try {
                MailActions.submit(getApplication(), acct.core, MailActions.markRead(ids))
                reload()
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "marking the thread read failed", e)
            }
        }
    }

    /** Starts loading the body of every open card that has none yet. */
    private fun fetchMissingBodies() {
        val scope = session ?: return
        val missing = stateFlow.value.let { s -> s.expanded.filter { it !in s.bodies && s.messages.any { m -> m.id == it } } }
        if (missing.isEmpty()) return
        stateFlow.update { s -> s.copy(bodies = s.bodies + missing.associateWith { BodyState.Loading }) }
        missing.forEach { id ->
            scope.launch {
                val result = try {
                    BodyState.Loaded(withContext(Dispatchers.IO) { acct.core.messageBody(id) })
                } catch (e: CancellationException) {
                    throw e
                } catch (e: Exception) {
                    Log.w(Background.TAG, "loading a message body failed", e)
                    BodyState.Failed
                }
                stateFlow.update { it.copy(bodies = it.bodies + (id to result)) }
            }
        }
    }
}
