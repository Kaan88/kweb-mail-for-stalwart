package ovh.kweb.mail.ui.inbox

import android.app.Application
import android.util.Log
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.delay
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
import ovh.kweb.mail.ui.SessionState
import uniffi.kweb_core.Action
import uniffi.kweb_core.ConnectionStatus
import uniffi.kweb_core.MessageHeader
import uniffi.kweb_core.Exception as CoreException

/** One line of the list, ready to draw. */
data class MailRow(
    val id: String,
    /** Null when the message has no sender at all; the screen then writes "unknown sender". */
    val sender: String?,
    val initial: String,
    val subject: String,
    val preview: String,
    val receivedAt: Long,
    val unread: Boolean,
)

data class InboxState(
    val rows: List<MailRow> = emptyList(),
    val unread: Int = 0,
    /** The cache has been read at least once since the screen came up. */
    val loaded: Boolean = false,
    /** The cache knows the mailboxes, so a first sync has happened. */
    val synced: Boolean = false,
    val refreshing: Boolean = false,
)

/** The newest messages of the Inbox from the local cache, and the few things a long press can do. */
class InboxViewModel(app: Application) : AndroidViewModel(app) {
    private val stateFlow = MutableStateFlow(InboxState())
    private val reloads = Channel<Unit>(Channel.CONFLATED)
    private val messageChannel = Channel<Int>(Channel.BUFFERED)

    val state: StateFlow<InboxState> get() = stateFlow

    /** Short texts for a snackbar, as string resource ids. */
    val messages: Flow<Int> get() = messageChannel.receiveAsFlow()

    init {
        // One loader, so that a burst of changes costs one read and loads never overlap.
        viewModelScope.launch { reloads.receiveAsFlow().collect { load() } }
        viewModelScope.launch { Background.cacheChanges.collect { reload() } }
        viewModelScope.launch {
            SessionState.signedIn.collect { signedIn ->
                when (signedIn) {
                    true -> reload()
                    false -> stateFlow.value = InboxState()
                    null -> Unit
                }
            }
        }
    }

    fun reload() {
        reloads.trySend(Unit)
    }

    /** Pull to refresh: ask the live connection for a pass, or queue a check when it is not up. */
    fun refresh() {
        if (stateFlow.value.refreshing) return
        stateFlow.update { it.copy(refreshing = true) }
        viewModelScope.launch {
            withContext(Dispatchers.IO) { pokeOrCheck() }
            reload()
            // The core says nothing when a pass is done; the cache ticks if it found something.
            delay(REFRESH_INDICATOR_MILLIS)
            stateFlow.update { it.copy(refreshing = false) }
        }
    }

    fun toggleRead(row: MailRow) = submit(if (row.unread) Action.MarkRead(row.id) else Action.MarkUnread(row.id))

    fun archive(row: MailRow) = submit(Action.Archive(row.id))

    private fun submit(action: Action) {
        viewModelScope.launch {
            try {
                withContext(Dispatchers.IO) {
                    val core = Background.core(getApplication())
                    core.submit(action)
                    if (core.status() !is ConnectionStatus.Live) CheckScheduler.checkSoon(getApplication())
                }
                reload()
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "action failed", e)
                messageChannel.send(R.string.inbox_action_failed)
            }
        }
    }

    private fun pokeOrCheck() {
        try {
            val core = Background.core(getApplication())
            core.poke()
            if (core.status() !is ConnectionStatus.Live) CheckScheduler.checkSoon(getApplication())
        } catch (e: Exception) {
            Log.w(Background.TAG, "refresh failed", e)
        }
    }

    private suspend fun load() {
        try {
            val next = withContext(Dispatchers.IO) { read() }
            stateFlow.update { it.copy(rows = next.rows, unread = next.unread, loaded = true, synced = next.synced) }
        } catch (e: CancellationException) {
            throw e
        } catch (e: CoreException.SignInNeeded) {
            // Signed out under our feet; the session state moves the app to the sign-in screen.
            stateFlow.update { it.copy(loaded = true) }
        } catch (e: Exception) {
            Log.w(Background.TAG, "reading the cache failed", e)
            stateFlow.update { it.copy(loaded = true) }
            messageChannel.send(R.string.inbox_read_failed)
        }
    }

    private fun read(): InboxState {
        val core = Background.core(getApplication())
        val mailboxes = core.mailboxes()
        val inbox = mailboxes.firstOrNull { it.role == "inbox" }
        val headers = inbox?.let { core.messages(it.id, ROW_LIMIT, 0u) }.orEmpty()
        return InboxState(
            rows = headers.map(::toRow),
            unread = inbox?.unreadEmails?.toInt() ?: 0,
            synced = mailboxes.isNotEmpty(),
        )
    }

    private fun toRow(h: MessageHeader) = MailRow(
        id = h.id,
        sender = Sender.label(h.fromName, h.fromEmail),
        initial = Sender.initial(h.fromName, h.fromEmail),
        subject = h.subject.trim(),
        preview = h.preview.trim(),
        receivedAt = h.receivedAt,
        unread = h.isUnread,
    )

    private companion object {
        const val ROW_LIMIT = 100u
        const val REFRESH_INDICATOR_MILLIS = 1_200L
    }
}
