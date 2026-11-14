package ovh.kweb.mail.ui.list

import android.app.Application
import android.util.Log
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
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
import ovh.kweb.mail.ui.MailActions
import ovh.kweb.mail.ui.SessionState
import ovh.kweb.mail.ui.UiMessage
import ovh.kweb.mail.ui.folders.SpamRules
import ovh.kweb.mail.ui.compose.DraftLauncher
import ovh.kweb.mail.ui.compose.Outbox
import ovh.kweb.mail.ui.compose.SendRules
import ovh.kweb.mail.ui.mailextras.CoreMailExtrasBackend
import ovh.kweb.mail.ui.mailextras.LabelStore
import uniffi.kweb_core.Action
import uniffi.kweb_core.ConnectionStatus
import uniffi.kweb_core.DraftKind
import uniffi.kweb_core.ListFilter
import uniffi.kweb_core.Mailbox
import uniffi.kweb_core.PendingSend
import uniffi.kweb_core.ThreadRow
import uniffi.kweb_core.Exception as CoreException

data class MailListState(
    val username: String = "",
    val mailboxes: List<Mailbox> = emptyList(),
    /** The folder the user picked; null means the Inbox. */
    val selectedId: String? = null,
    /** The folder shown: [selectedId] if it exists, else the Inbox. Null before the first sync. */
    val current: Mailbox? = null,
    val filter: ListFilter = ListFilter.ALL,
    val rows: List<ThreadRow> = emptyList(),
    /** How many rows the cache is asked for. Grows by a page when the user nears the end. */
    val limit: Int = MailListViewModel.PAGE,
    /** The cache has been read at least once for the current folder and filter. */
    val loaded: Boolean = false,
    /** The cache knows the mailboxes, so a first sync has happened. */
    val synced: Boolean = false,
    val refreshing: Boolean = false,
    /** Older mail is being fetched from the server. */
    val loadingOlder: Boolean = false,
    /** Fetching older mail failed or found nothing more; no more tries until the folder changes or a refresh. */
    val olderStopped: Boolean = false,
    /** Consecutive pages of older mail that added no row to this list (a filter that matches little). */
    val emptyPages: Int = 0,
    /** [MailListViewModel.MAX_EMPTY_PAGES] empty pages in a row: the screen offers "Load older mail" instead of going on. */
    val olderPaused: Boolean = false,
    /** Counts folder and filter changes, so an answer to an older question can be told from a current one. */
    val generation: Int = 0,
    /** Bumped when an action failed, so a swiped row comes back. */
    val swipeEpoch: Int = 0,
    /** A draft the server refused to send; the list offers to open it. */
    val failedDraftId: String? = null,
    /** Messages that are held back or wait for a connection to be sent. */
    val waiting: Int = 0,
    /** The message just sent whose Undo is still on offer; cleared by a timer when the core lets it go. */
    val holding: PendingSend? = null,
)

/** What the screen shows once, as a snackbar. */
sealed interface ListEvent {
    data class Message(val textRes: Int) : ListEvent

    /** [messageIds] were taken out of [mailboxId]; undoing moves them back. */
    data class Undo(val textRes: Int, val messageIds: List<String>, val mailboxId: String) : ListEvent

    /** A refusal in the core's own words, or any text that is not one of the app's strings. */
    data class Text(val message: UiMessage) : ListEvent

    /** [messageIds] were reported as spam ([wasSpam] true) or taken back; undoing does the opposite. */
    data class UndoSpam(val textRes: Int, val messageIds: List<String>, val wasSpam: Boolean, val fromId: String?, val fromRole: String?) : ListEvent

    /** Open the compose screen on this draft (a new one, an answer, or one taken back by Undo). */
    data class OpenDraft(val draftId: String) : ListEvent
}

/** The conversations of one folder from the local cache, and the things a row can do. */
class MailListViewModel(app: Application) : AndroidViewModel(app) {
    private val stateFlow = MutableStateFlow(MailListState())
    private val reloads = Channel<Unit>(Channel.CONFLATED)
    private val eventChannel = Channel<ListEvent>(Channel.BUFFERED)
    private var startingDraft = false
    private var holdJob: Job? = null

    val state: StateFlow<MailListState> get() = stateFlow

    val events: Flow<ListEvent> get() = eventChannel.receiveAsFlow()

    /** The user's labels (for the drawer) and the labels of the rows on screen. */
    val labels = LabelStore(
        viewModelScope,
        Dispatchers.IO,
        CoreMailExtrasBackend { Background.core(app) },
        onError = { eventChannel.trySend(ListEvent.Text(it)) },
    )

    /** Kept here so that the place in the list survives a visit to the calendar or to a thread. */
    val listState = androidx.compose.foundation.lazy.LazyListState()

    init {
        // One loader, so that a burst of changes costs one read and loads never overlap.
        viewModelScope.launch { reloads.receiveAsFlow().collect { load() } }
        viewModelScope.launch { Background.cacheChanges.collect { reload() } }
        viewModelScope.launch { Outbox.sent.collect { hold(it) } }
        viewModelScope.launch {
            SessionState.signedIn.collect { signedIn ->
                when (signedIn) {
                    true -> reload()
                    false -> {
                        holdJob?.cancel()
                        labels.reset()
                        stateFlow.value = MailListState()
                        listState.requestScrollToItem(0)
                    }
                    null -> Unit
                }
            }
        }
    }

    fun reload() {
        reloads.trySend(Unit)
    }

    /** Shows the Inbox without a filter (a tap on a notification). */
    fun showInbox() = selectFolder(null)

    fun selectFolder(mailboxId: String?) {
        stateFlow.update { it.fresh(selectedId = mailboxId, filter = ListFilter.ALL) }
        listState.requestScrollToItem(0)
        reload()
    }

    fun selectFilter(filter: ListFilter) {
        if (stateFlow.value.filter == filter) return
        stateFlow.update { it.fresh(selectedId = it.selectedId, filter = filter) }
        listState.requestScrollToItem(0)
        reload()
    }

    /** Pull to refresh: ask the live connection for a pass, or queue a check when it is not up. */
    fun refresh() {
        if (stateFlow.value.refreshing) return
        stateFlow.update { it.copy(refreshing = true, olderStopped = false, emptyPages = 0, olderPaused = false) }
        viewModelScope.launch {
            withContext(Dispatchers.IO) { pokeOrCheck() }
            reload()
            // The core says nothing when a pass is done; the cache ticks if it found something.
            delay(REFRESH_INDICATOR_MILLIS)
            stateFlow.update { it.copy(refreshing = false) }
        }
    }

    /**
     * The list is near its end: show more of the cache, or fetch older mail from the server. After
     * [MAX_EMPTY_PAGES] fetched pages in a row that add no row it stops until [loadOlderNow] or [userScrolled].
     */
    fun loadMore() {
        val s = stateFlow.value
        if (s.current == null || !s.loaded || s.loadingOlder) return
        if (s.rows.size >= s.limit) {
            stateFlow.update { it.copy(limit = it.limit + PAGE) }
            reload()
            return
        }
        if (s.olderStopped || s.olderPaused) return
        fetchOlder(s)
    }

    /** The user tapped "Load older mail". */
    fun loadOlderNow() {
        stateFlow.update { it.copy(emptyPages = 0, olderPaused = false) }
        val s = stateFlow.value
        if (s.current != null && s.loaded && !s.loadingOlder && !s.olderStopped) fetchOlder(s)
    }

    /** The user moved the list: automatic fetching may go on. */
    fun userScrolled() {
        if (stateFlow.value.olderPaused || stateFlow.value.emptyPages > 0) {
            stateFlow.update { it.copy(emptyPages = 0, olderPaused = false) }
        }
    }

    private fun fetchOlder(asked: MailListState) {
        val mailbox = asked.current ?: return
        stateFlow.update { it.copy(loadingOlder = true) }
        viewModelScope.launch {
            var failed = false
            var more = false
            var read: Read? = null
            try {
                withContext(Dispatchers.IO) {
                    val core = Background.core(getApplication())
                    more = core.hasOlder(mailbox.id) && core.loadOlder(mailbox.id)
                    read = read(asked)
                }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "loading older mail failed", e)
                failed = true
            }
            // The folder or filter may have changed while the server was asked: that answer is not for this list.
            val current = stateFlow.value
            if (!current.sameQuestion(asked)) return@launch
            val added = (read?.rows?.size ?: asked.rows.size) - asked.rows.size
            val empty = if (added > 0) 0 else current.emptyPages + 1
            read?.let { labels.load(it.rows.flatMap { row -> row.messageIds }) }
            stateFlow.update {
                it.withRead(read).copy(
                    loadingOlder = false,
                    olderStopped = failed || !more,
                    emptyPages = empty,
                    olderPaused = more && !failed && empty >= MAX_EMPTY_PAGES,
                )
            }
            if (failed) eventChannel.send(ListEvent.Message(R.string.list_older_failed))
            reload()
        }
    }

    fun toggleRead(row: ThreadRow) {
        val ids = row.messageIds
        submit(if (row.unreadCount > 0u) MailActions.markRead(ids) else MailActions.markUnread(ids))
    }

    fun toggleFlag(row: ThreadRow) = submit(MailActions.setFlagged(row.messageIds, !row.isFlagged))

    /** Archive, or Delete (permanent in Trash), with an Undo that moves the messages back. */
    fun archive(row: ThreadRow) = removeFromFolder(row, MailActions.archive(row.messageIds), R.string.list_archived)

    fun delete(row: ThreadRow) {
        val inTrash = stateFlow.value.current?.role == "trash"
        removeFromFolder(row, MailActions.delete(row.messageIds), if (inTrash) null else R.string.list_deleted)
    }

    fun move(row: ThreadRow, mailboxId: String) = submit(MailActions.move(row.messageIds, mailboxId))

    /**
     * Report spam (or, in the Junk folder, "Not spam") for the whole conversation, with an Undo that does the
     * opposite. The core's refusal (no Junk folder) is shown as it is.
     */
    fun spam(row: ThreadRow) {
        val from = stateFlow.value.current
        val isSpam = !SpamRules.isNotSpamEntry(from?.role)
        reportSpam(row.messageIds, isSpam) {
            eventChannel.send(
                ListEvent.UndoSpam(if (isSpam) R.string.spam_reported else R.string.spam_cleared, row.messageIds, isSpam, from?.id, from?.role),
            )
        }
    }

    /** Puts the mail back where the report was made from (see [SpamRules.undoGoesTo]). */
    fun undoSpam(event: ListEvent.UndoSpam) {
        val to = SpamRules.undoGoesTo(event.wasSpam, event.fromId, event.fromRole)
        if (to == null) reportSpam(event.messageIds, !event.wasSpam) else submit(MailActions.move(event.messageIds, to))
    }

    private fun reportSpam(ids: List<String>, isSpam: Boolean, onDone: suspend () -> Unit = {}) {
        viewModelScope.launch {
            try {
                MailActions.reportSpam(getApplication(), ids, isSpam)
                reload()
                onDone()
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "reporting spam failed: ${e.javaClass.simpleName}")
                stateFlow.update { it.copy(swipeEpoch = it.swipeEpoch + 1) }
                reload()
                eventChannel.send(ListEvent.Text(SpamRules.failure(e)))
            }
        }
    }

    /** The bell of a folder from the drawer's menu. The list reads again when the core has it. */
    fun setNotify(mailboxId: String, notify: Boolean) {
        viewModelScope.launch {
            try {
                withContext(Dispatchers.IO) {
                    val core = Background.core(getApplication())
                    core.setMailboxNotify(mailboxId, notify)
                    if (core.status() !is ConnectionStatus.Live) CheckScheduler.checkSoon(getApplication())
                }
                reload()
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "switching a folder's notifications failed: ${e.javaClass.simpleName}")
                reload()
                eventChannel.send(ListEvent.Text(UiMessage.of(e, R.string.folders_notify_failed)))
            }
        }
    }

    /** Starts a draft of [kind] and has the screen open it. One at a time: a double tap makes one draft. */
    fun startDraft(kind: DraftKind) {
        if (startingDraft) return
        startingDraft = true
        viewModelScope.launch {
            try {
                eventChannel.send(ListEvent.OpenDraft(DraftLauncher.create(getApplication(), kind)))
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "starting a draft failed", e)
                eventChannel.send(ListEvent.Message(R.string.compose_start_failed))
            } finally {
                startingDraft = false
            }
        }
    }

    /**
     * [pending] is held back: the list shows "Sending…" with Undo from its state until the core lets
     * it go. A timer here clears it, so a rotation or another event does not take the offer away.
     */
    private fun hold(pending: PendingSend) {
        val left = SendRules.remainingMillis(pending.sendAt, System.currentTimeMillis())
        if (left <= 0) return
        if (stateFlow.value.holding == pending) return
        holdJob?.cancel()
        stateFlow.update { it.copy(holding = pending) }
        holdJob = viewModelScope.launch {
            delay(left)
            stateFlow.update { if (it.holding?.draftId == pending.draftId) it.copy(holding = null) else it }
            reload()
        }
    }

    /** Undo on "Sending…": the core hands the message back as a draft and the screen opens it. */
    fun undoSend(draftId: String) {
        if (stateFlow.value.holding?.draftId == draftId) {
            holdJob?.cancel()
            stateFlow.update { it.copy(holding = null) }
        }
        viewModelScope.launch {
            try {
                withContext(Dispatchers.IO) { Background.core(getApplication()).undoSend(draftId) }
                eventChannel.send(ListEvent.OpenDraft(draftId))
            } catch (e: CancellationException) {
                throw e
            } catch (e: CoreException.Invalid) {
                eventChannel.send(ListEvent.Message(R.string.send_undo_too_late))
            } catch (e: Exception) {
                Log.w(Background.TAG, "undoing a send failed", e)
                eventChannel.send(ListEvent.Message(R.string.send_undo_failed))
            }
            reload()
        }
    }

    fun undo(event: ListEvent.Undo) = submit(MailActions.move(event.messageIds, event.mailboxId))

    private fun removeFromFolder(row: ThreadRow, actions: List<Action>, undoText: Int?) {
        val from = stateFlow.value.current ?: return
        submit(actions) {
            if (undoText != null) eventChannel.send(ListEvent.Undo(undoText, row.messageIds, from.id))
        }
    }

    private fun submit(actions: List<Action>, onDone: suspend () -> Unit = {}) {
        viewModelScope.launch {
            try {
                MailActions.submit(getApplication(), actions)
                reload()
                onDone()
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "action failed", e)
                // The core stops at the first error; some of the actions may have been applied.
                stateFlow.update { it.copy(swipeEpoch = it.swipeEpoch + 1) }
                reload()
                eventChannel.send(ListEvent.Message(R.string.list_action_failed))
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
        val asked = stateFlow.value
        readOutbox()
        try {
            val read = withContext(Dispatchers.IO) { read(asked) }
            // The labels of the rows of this page, in one call; the drawer's list with them.
            labels.refreshLabels()
            if (stateFlow.value.sameQuestion(asked)) labels.load(read.rows.flatMap { it.messageIds })
            // A newer choice is already queued to be read; this answer is about an older one.
            stateFlow.update { if (it.sameQuestion(asked)) it.withRead(read) else it.copy(mailboxes = read.mailboxes, username = read.username) }
        } catch (e: CancellationException) {
            throw e
        } catch (e: CoreException.SignInNeeded) {
            // Signed out under our feet; the session state moves the app to the sign-in screen.
            stateFlow.update { it.copy(loaded = true) }
        } catch (e: Exception) {
            Log.w(Background.TAG, "reading the cache failed", e)
            stateFlow.update { it.copy(loaded = true) }
            eventChannel.send(ListEvent.Message(R.string.list_read_failed))
        }
    }

    /** The drafts the server refused and the messages waiting to go out. A failure only means the bars stay as they were. */
    private suspend fun readOutbox() {
        val outbox = withContext(Dispatchers.IO) {
            try {
                val core = Background.core(getApplication())
                val pending = core.pendingSends()
                OutboxRead(ListRules.failedDraft(core.drafts())?.id, ListRules.waiting(pending), ListRules.holding(pending, System.currentTimeMillis()), pending.map { it.draftId }.toSet())
            } catch (e: CoreException) {
                Log.d(Background.TAG, "reading the outbox failed: ${e.javaClass.simpleName}")
                null
            }
        } ?: return
        // A held message the core no longer has (taken back, or gone out) loses its Undo.
        stateFlow.update {
            it.copy(
                failedDraftId = outbox.failedDraftId,
                waiting = outbox.waiting,
                holding = it.holding?.takeIf { held -> held.draftId in outbox.pendingIds },
            )
        }
        // A message sent before the list was on screen (or before a restart of the process) is still held.
        outbox.holding?.let { hold(it) }
    }

    private class OutboxRead(val failedDraftId: String?, val waiting: Int, val holding: PendingSend?, val pendingIds: Set<String>)

    private fun MailListState.sameQuestion(asked: MailListState) =
        selectedId == asked.selectedId && filter == asked.filter && limit == asked.limit && generation == asked.generation

    /** The state with what was read from the cache; unchanged when [read] is null. */
    private fun MailListState.withRead(read: Read?): MailListState = if (read == null) this else copy(
        username = read.username,
        mailboxes = read.mailboxes,
        current = read.current,
        rows = read.rows,
        loaded = true,
        synced = read.mailboxes.isNotEmpty(),
    )

    private class Read(val username: String, val mailboxes: List<Mailbox>, val current: Mailbox?, val rows: List<ThreadRow>)

    private fun read(asked: MailListState): Read {
        val core = Background.core(getApplication())
        val mailboxes = core.mailboxes()
        val current = mailboxes.firstOrNull { it.id == asked.selectedId } ?: Folders.withRole(mailboxes, "inbox")
        val rows = current?.let { core.threads(it.id, asked.filter, asked.limit.toUInt(), 0u) }.orEmpty()
        return Read(core.account()?.username.orEmpty(), mailboxes, current, rows)
    }

    private fun MailListState.fresh(selectedId: String?, filter: ListFilter) = copy(
        selectedId = selectedId,
        filter = filter,
        rows = emptyList(),
        limit = PAGE,
        loaded = false,
        olderStopped = false,
        emptyPages = 0,
        olderPaused = false,
        loadingOlder = false,
        generation = generation + 1,
    )

    companion object {
        const val PAGE = 50
        const val MAX_EMPTY_PAGES = 2
        private const val REFRESH_INDICATOR_MILLIS = 1_200L
    }
}
