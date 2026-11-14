package ovh.kweb.mail.ui.shared

import java.time.LocalDate
import java.time.ZoneId
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.UiMessage
import ovh.kweb.mail.ui.calendar.AgendaRules
import ovh.kweb.mail.ui.calendar.AgendaWindow
import ovh.kweb.mail.ui.contacts.ContactRules
import uniffi.kweb_core.Calendar
import uniffi.kweb_core.ContactRow
import uniffi.kweb_core.EventDetail
import uniffi.kweb_core.MessageHeader
import uniffi.kweb_core.SharedAction
import uniffi.kweb_core.SharedMailbox
import uniffi.kweb_core.SharedMessage
import uniffi.kweb_core.SharedMessages

// ---- the messages of one folder

data class FolderState(
    /** The folder, with the rights the user has in it; null until the first answer. */
    val folder: SharedMailbox? = null,
    /** Newest first; more pages are appended. */
    val messages: List<MessageHeader> = emptyList(),
    /** How many messages the folder has in all. */
    val total: Int = 0,
    val loaded: Boolean = false,
    val loading: Boolean = false,
    val refreshing: Boolean = false,
    val loadingMore: Boolean = false,
    val error: UiMessage? = null,
    /** The last "load more" failed; what is shown stays. */
    val moreFailed: Boolean = false,
) {
    val hasMore: Boolean get() = SharedRules.hasMore(messages.size, total)
}

/**
 * The first pages of one shared folder, newest first, with "load more" for the rest. Reading the folder
 * again (the screen came up, pull to refresh) keeps as many messages as were shown, read in as many calls
 * of at most a page of [SharedRules.MAX_PAGE] as it takes, so a message acted on in between is gone from the list and nothing else jumps. Only the newest
 * request counts: a "load more" that is answered after the list was read again is dropped.
 */
class FolderController(
    private val scope: CoroutineScope,
    private val io: CoroutineDispatcher,
    private val backend: SharedBackend,
    private val account: String,
    private val mailboxId: String,
) {
    private val flow = MutableStateFlow(FolderState())
    private val bus = SharedEvents()
    private var generation = 0

    val state: StateFlow<FolderState> get() = flow
    val events: Flow<SharedEvent> get() = bus.flow

    fun show() = read(manual = false)

    fun refresh() = read(manual = true)

    fun reset() {
        generation++
        flow.value = FolderState()
        bus.clear()
    }

    private fun read(manual: Boolean) {
        val gen = ++generation
        val had = flow.value.loaded
        val wanted = if (had) SharedRules.reloadCount(flow.value.messages.size) else SharedRules.PAGE
        flow.update { it.copy(loading = !had, refreshing = manual && had, loadingMore = false, moreFailed = false, error = null) }
        scope.launch {
            val result = io.attempt { backend.mailboxes(account) to readPages(wanted) }
            if (gen != generation) return@launch
            result.onSuccess { (folders, page) ->
                val folder = folders.firstOrNull { it.id == mailboxId }
                if (folder == null) {
                    // The folder is not shared with the user any more.
                    flow.update { it.copy(loading = false, refreshing = false, error = UiMessage.Res(R.string.shared_folder_gone)) }
                    bus.send(SharedEvent.Gone)
                } else {
                    flow.value = FolderState(folder = folder, messages = page.messages, total = page.total.toInt(), loaded = true)
                }
            }.onFailure { e ->
                val gone = SharedRules.isGone(e)
                val message = SharedRules.message(e)
                flow.update { it.copy(loading = false, refreshing = false, error = if (!it.loaded || gone) message else null) }
                if (gone) bus.send(SharedEvent.Gone) else if (manual && flow.value.loaded) bus.say(message)
            }
        }
    }

    /** The first [wanted] messages, in as many calls as the core's page size makes necessary. */
    private fun readPages(wanted: Int): SharedMessages {
        var all = emptyList<MessageHeader>()
        var total = 0
        while (true) {
            val page = backend.messages(account, mailboxId, all.size, minOf(SharedRules.MAX_PAGE, wanted - all.size))
            total = page.total.toInt()
            val joined = SharedRules.append(all, page.messages)
            val grew = joined.size > all.size
            all = joined
            if (!grew || all.size >= wanted || all.size >= total) break
        }
        return SharedMessages(all, total.toUInt())
    }

    /** The next page, at the end of what is shown. One at a time. */
    fun loadMore() {
        val s = flow.value
        if (!s.loaded || s.loadingMore || s.loading || !s.hasMore) return
        val gen = generation
        val offset = s.messages.size
        flow.update { it.copy(loadingMore = true, moreFailed = false) }
        scope.launch {
            val result = io.attempt { backend.messages(account, mailboxId, offset, SharedRules.PAGE) }
            if (gen != generation) return@launch
            result.onSuccess { page ->
                flow.update { it.copy(messages = SharedRules.append(it.messages, page.messages), total = page.total.toInt(), loadingMore = false) }
            }.onFailure { e ->
                flow.update { it.copy(loadingMore = false, moreFailed = true) }
                bus.say(SharedRules.message(e))
            }
        }
    }
}

// ---- one message

data class SharedMessageState(
    val message: SharedMessage? = null,
    /** All the shared folders of the account: for the rights of the folder the message was opened from and the places to move it to. */
    val folders: List<SharedMailbox> = emptyList(),
    /** The folder the message was opened from, as the server describes it now; null when it is not known or not shared any more. */
    val folder: SharedMailbox? = null,
    val loading: Boolean = false,
    val error: UiMessage? = null,
    /** The folders could not be read and none were known before, so no action is offered; the screen says so and offers a retry. */
    val foldersFailed: Boolean = false,
    /** The user allowed remote content for this message. Not remembered beyond this screen. */
    val remoteAllowed: Boolean = false,
    /** An action is on its way. */
    val busy: Boolean = false,
) {
    fun allows(kind: SharedActionKind): Boolean = SharedRules.allows(kind, folder, folders)

    val moveTargets: List<SharedFolderEntry> get() = SharedRules.moveTargets(folders, folder)
}

/**
 * A message of a shared folder, with what can be done to it. Opening it does not mark it read. An action is
 * checked against the rights of the folder it was opened from, sent at once (there is no queue) and, when it
 * worked, either shown by reading the message again (a mark) or ended by [SharedEvent.Done] (a move or a delete).
 */
class SharedMessageController(
    private val scope: CoroutineScope,
    private val io: CoroutineDispatcher,
    private val backend: SharedBackend,
    private val account: String,
    private val mailboxId: String,
    private val messageId: String,
) {
    private val flow = MutableStateFlow(SharedMessageState())
    private val bus = SharedEvents()
    private var generation = 0

    val state: StateFlow<SharedMessageState> get() = flow
    val events: Flow<SharedEvent> get() = bus.flow

    fun show() = read(manual = false)

    fun refresh() = read(manual = true)

    fun reset() {
        generation++
        flow.value = SharedMessageState()
        bus.clear()
    }

    fun allowRemote() = flow.update { it.copy(remoteAllowed = true) }

    private fun read(manual: Boolean) {
        val gen = ++generation
        flow.update { it.copy(loading = it.message == null, error = null) }
        scope.launch {
            val result = io.attempt {
                val message = backend.message(account, messageId)
                // Without the folders no action is offered, but the message can still be read.
                val folders: List<SharedMailbox>? = try {
                    backend.mailboxes(account)
                } catch (e: kotlin.Exception) {
                    null
                }
                message to folders
            }
            if (gen != generation) return@launch
            result.onSuccess { (message, folders) ->
                flow.update {
                    // A failed folder read keeps the folders and the rights already known.
                    val known = folders ?: it.folders
                    it.copy(
                        message = message,
                        folders = known,
                        folder = if (folders != null) folders.firstOrNull { f -> f.id == mailboxId } else it.folder,
                        foldersFailed = folders == null && it.folder == null,
                        loading = false,
                        error = null,
                    )
                }
            }.onFailure { e ->
                val text = SharedRules.message(e)
                val had = flow.value.message != null
                flow.update { it.copy(loading = false, error = if (had) null else text) }
                if (had && manual) bus.say(text)
            }
        }
    }

    /** [message] as it is once [action] worked, so that a failed re-read afterwards shows the truth. */
    private fun applied(message: SharedMessage, action: SharedAction): SharedMessage {
        val h = message.header
        val header = when (action) {
            SharedAction.MarkRead -> h.copy(isUnread = false)
            SharedAction.MarkUnread -> h.copy(isUnread = true)
            SharedAction.Flag -> h.copy(isFlagged = true)
            SharedAction.Unflag -> h.copy(isFlagged = false)
            else -> return message
        }
        return message.copy(header = header)
    }

    /** Does [action] to the message, when the folder's rights allow it. One at a time. */
    fun act(action: SharedAction) {
        val s = flow.value
        if (s.busy || s.message == null) return
        if (!s.allows(SharedRules.kindOf(action))) {
            bus.say(UiMessage.Res(R.string.shared_not_allowed))
            return
        }
        flow.update { it.copy(busy = true) }
        scope.launch {
            val result = io.attempt { backend.act(account, messageId, action) }
            result.onSuccess {
                flow.update { it.copy(busy = false, message = it.message?.let { m -> applied(m, action) }) }
                when (action) {
                    is SharedAction.Move, SharedAction.Delete -> bus.send(SharedEvent.Done)
                    else -> read(manual = false)
                }
            }.onFailure { e ->
                flow.update { it.copy(busy = false) }
                bus.say(SharedRules.message(e))
                // The rights or the message may have changed: look again.
                read(manual = false)
            }
        }
    }
}

// ---- the agenda of one account

data class SharedAgendaState(
    val zone: ZoneId,
    val today: LocalDate,
    /** The span asked for: today and the next 30 days, longer after "load 30 more days". */
    val window: AgendaWindow.Window,
    /** Calendar ids in the core's order: a calendar's position gives its tone. */
    val calendarIds: List<String> = emptyList(),
    val rows: List<AgendaRules.Row> = emptyList(),
    val loaded: Boolean = false,
    val loading: Boolean = false,
    val refreshing: Boolean = false,
    val loadingMore: Boolean = false,
    val error: UiMessage? = null,
) {
    val canLoadMore: Boolean get() = SharedRules.canExtend(window)
}

/**
 * The events of the calendars an account shares, grouped by day, as the own agenda groups them. Not cached:
 * without a connection a read fails and what was shown stays. "Load 30 more days" asks for the longer span
 * again as a whole, so an event that changed in between is not shown twice or stale.
 */
class SharedAgendaController(
    private val scope: CoroutineScope,
    private val io: CoroutineDispatcher,
    private val backend: SharedBackend,
    private val account: String,
    private val zoneNow: () -> ZoneId = { ZoneId.systemDefault() },
    private val todayIn: (ZoneId) -> LocalDate = { LocalDate.now(it) },
) {
    private val flow = MutableStateFlow(initial())
    private val bus = SharedEvents()
    private var generation = 0

    val state: StateFlow<SharedAgendaState> get() = flow
    val events: Flow<SharedEvent> get() = bus.flow

    private fun initial(): SharedAgendaState {
        val zone = zoneNow()
        val today = todayIn(zone)
        return SharedAgendaState(zone, today, SharedRules.firstSpan(today))
    }

    fun show() = read(manual = false, window = null)

    fun refresh() = read(manual = true, window = null)

    fun reset() {
        generation++
        flow.value = initial()
        bus.clear()
    }

    /** 30 more days at the end. One at a time. */
    fun loadMore() {
        val s = flow.value
        if (!s.loaded || s.loadingMore || s.loading || !s.canLoadMore) return
        read(manual = false, window = SharedRules.extend(s.window))
    }

    private fun read(manual: Boolean, window: AgendaWindow.Window?) {
        val gen = ++generation
        val zone = zoneNow()
        val today = todayIn(zone)
        val before = flow.value
        val more = window != null
        // Before the first answer the span starts today; after it, it stays so that the list does not jump.
        val span = window ?: if (before.loaded) before.window else SharedRules.firstSpan(today)
        flow.update {
            it.copy(
                zone = zone,
                today = today,
                loading = !it.loaded,
                refreshing = manual && it.loaded,
                loadingMore = more,
                error = null,
            )
        }
        scope.launch {
            val result = io.attempt {
                val (from, to) = AgendaWindow.seconds(span, zone)
                val calendars: List<Calendar>? = try {
                    backend.calendars(account)
                } catch (e: kotlin.Exception) {
                    null
                }
                calendars to backend.agenda(account, from, to, SharedRules.coreZone(zone))
            }
            if (gen != generation) return@launch
            result.onSuccess { (calendars, agenda) ->
                val days = AgendaRules.group(agenda.entries, zone, span.from, span.to)
                flow.update {
                    it.copy(
                        window = span,
                        calendarIds = calendars?.map { c -> c.id } ?: it.calendarIds,
                        rows = AgendaRules.rows(days, zone),
                        loaded = true,
                        loading = false,
                        refreshing = false,
                        loadingMore = false,
                        error = null,
                    )
                }
            }.onFailure { e ->
                val gone = SharedRules.isGone(e)
                val message = SharedRules.message(e)
                flow.update { it.copy(loading = false, refreshing = false, loadingMore = false, error = if (!it.loaded || gone) message else null) }
                if (gone) bus.send(SharedEvent.Gone) else if (flow.value.loaded && (manual || more)) bus.say(message)
            }
        }
    }
}

// ---- one event

/** An event of a shared calendar and the name of the calendar it is in (null when that could not be found out). */
data class SharedEventData(val detail: EventDetail, val calendarName: String?)

/** Reads one event of a shared calendar and its calendar's name; the name is a nicety and its failure is not the event's. */
fun readSharedEvent(backend: SharedBackend, account: String, eventId: String, recurrenceId: String?): SharedEventData {
    val detail = backend.event(account, eventId, recurrenceId)
    val name = try {
        backend.calendars(account).firstOrNull { it.id == detail.input.calendarId }?.name
    } catch (e: kotlin.Exception) {
        null
    }
    return SharedEventData(detail, name)
}

// ---- the contacts of one account

data class SharedContactsState(
    /** What is in the search field. */
    val query: String = "",
    /** The query the rows are the answer to. */
    val answeredQuery: String = "",
    val rows: List<ContactRow>? = null,
    val loading: Boolean = false,
    val refreshing: Boolean = false,
    val error: UiMessage? = null,
)

/**
 * The contacts of the address books an account shares, by name, narrowed by what the user types (asked for
 * [debounceMillis] after the last key). Never cached and never used for suggestions. Only the answer to the
 * newest query counts.
 */
class SharedContactsController(
    private val scope: CoroutineScope,
    private val io: CoroutineDispatcher,
    private val backend: SharedBackend,
    private val account: String,
    private val debounceMillis: Long = ContactRules.DEBOUNCE_MILLIS,
) {
    private val flow = MutableStateFlow(SharedContactsState())
    private val bus = SharedEvents()
    private var generation = 0
    private var typing: Job? = null

    val state: StateFlow<SharedContactsState> get() = flow
    val events: Flow<SharedEvent> get() = bus.flow

    fun show() = read(manual = false)

    fun refresh() = read(manual = true)

    fun reset() {
        typing?.cancel()
        generation++
        flow.value = SharedContactsState()
        bus.clear()
    }

    fun onQueryChange(text: String) {
        flow.update { it.copy(query = text) }
        typing?.cancel()
        typing = scope.launch {
            delay(debounceMillis)
            read(manual = false)
        }
    }

    private fun read(manual: Boolean) {
        val gen = ++generation
        val query = flow.value.query.trim()
        flow.update { it.copy(loading = it.rows == null, refreshing = manual && it.rows != null, error = null) }
        scope.launch {
            val result = io.attempt { backend.contacts(account, query) }
            if (gen != generation) return@launch
            result.onSuccess { rows ->
                flow.update { it.copy(rows = rows, answeredQuery = query, loading = false, refreshing = false, error = null) }
            }.onFailure { e ->
                val gone = SharedRules.isGone(e)
                val message = SharedRules.message(e)
                val had = flow.value.rows != null
                flow.update { it.copy(loading = false, refreshing = false, error = if (!had || gone) message else null) }
                if (gone) bus.send(SharedEvent.Gone) else if (manual && had) bus.say(message)
            }
        }
    }
}
