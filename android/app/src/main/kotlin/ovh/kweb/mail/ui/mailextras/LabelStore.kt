package ovh.kweb.mail.ui.mailextras

import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.atomic.AtomicLong
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.UiMessage
import uniffi.kweb_core.Label

data class LabelsState(
    /** The user's labels, as the core lists them. */
    val labels: List<Label> = emptyList(),
    /** The labels (ids) of the messages the screen has asked about. */
    val byMessage: Map<String, List<String>> = emptyMap(),
    /** True once the core has answered for the list of labels (an empty list before that is "not read", not "none"). */
    val labelsRead: Boolean = false,
    /** Why the last change, new label or delete was refused; shown inside the sheet, which hides the screen's snackbar. */
    val error: UiMessage? = null,
)

/**
 * The labels one screen shows: the user's list and the labels of the messages on screen. Answers come
 * from the core off the main thread ([io]); an answer that a newer one has overtaken is dropped, per
 * message, so a slow answer never undoes a newer change. Changes are applied to the picture at once and
 * go to the core in order; a change the core refused is taken back by asking again.
 */
class LabelStore(
    private val scope: CoroutineScope,
    private val io: CoroutineDispatcher,
    private val backend: MailExtrasBackend,
    private val onError: (UiMessage) -> Unit,
) {
    private val stateFlow = MutableStateFlow(LabelsState())
    private val order = Mutex()
    private val ticket = AtomicLong()
    private val labelsTicket = AtomicLong()

    /** Answers asked for before [reset] are not used. */
    @Volatile private var floor = 0L
    @Volatile private var labelsFloor = 0L
    private val labelsApplied = AtomicLong()
    private val applied = ConcurrentHashMap<String, Long>()

    /** Changes per message that are waiting or running; while there are any, read answers for it are not shown. Guarded by [applied]. */
    private val pending = HashMap<String, Int>()

    val state: StateFlow<LabelsState> get() = stateFlow

    /** Reads the user's labels. */
    fun refreshLabels() {
        val mine = labelsTicket.incrementAndGet()
        scope.launch {
            val labels = try {
                withContext(io) { backend.labels() }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                return@launch
            }
            if (mine <= labelsFloor) return@launch
            // Only the newest answer counts; the one that ends the race writes its labels last.
            synchronized(labelsApplied) {
                if (mine < labelsApplied.get()) return@launch
                labelsApplied.set(mine)
                stateFlow.update { it.copy(labels = labels, labelsRead = true) }
            }
        }
    }

    /** Reads the labels of [messageIds], in one call. Does nothing for an empty list. */
    fun load(messageIds: List<String>) {
        val ids = messageIds.distinct()
        if (ids.isEmpty()) return
        val mine = ticket.incrementAndGet()
        scope.launch {
            val answer = try {
                withContext(io) { backend.messageLabels(ids) }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                // The chips stay as they were; there is nothing to tell the user about a missing decoration.
                return@launch
            }
            if (mine <= floor) return@launch
            val index = LabelRules.index(ids, answer)
            synchronized(applied) {
                stateFlow.update { state ->
                    val out = HashMap(state.byMessage)
                    for ((id, labelIds) in index) {
                        // A change of the user is still on its way: its own read (after the last one) tells the truth.
                        if (pending.containsKey(id)) continue
                        if (mine >= (applied[id] ?: 0L)) {
                            applied[id] = mine
                            out[id] = labelIds
                        }
                    }
                    state.copy(byMessage = out)
                }
            }
        }
    }

    /** Puts [labelId] on [messageIds] or takes it off, at once on screen and then in the core. */
    fun set(messageIds: List<String>, labelId: String, on: Boolean) {
        val ids = messageIds.distinct()
        if (ids.isEmpty()) return
        begin(ids)
        stateFlow.update { it.copy(byMessage = LabelRules.applied(it.byMessage, ids, labelId, on), error = null) }
        scope.launch { order.withLock { change(ids, labelId, on) } }
    }

    /** Runs a change that [begin] counted; counts it off when it is over, then reads (the read counts only after the last one). */
    private suspend fun change(ids: List<String>, labelId: String, on: Boolean) {
        try {
            withContext(io) { backend.setLabel(ids, labelId, on) }
            finish(ids)
            // What the core has now is the truth; a read that raced the change may have shown the old one.
            load(ids)
        } catch (e: CancellationException) {
            finish(ids)
            throw e
        } catch (e: Exception) {
            finish(ids)
            load(ids)
            fail(UiMessage.of(e, R.string.labels_change_failed))
        }
    }

    private fun fail(message: UiMessage) {
        stateFlow.update { it.copy(error = message) }
        onError(message)
    }

    /** The message about a refusal was seen (or the sheet closed). */
    fun clearError() {
        stateFlow.update { if (it.error == null) it else it.copy(error = null) }
    }

    /** Makes a label and, when [applyTo] is not empty, puts it on those messages. [onDone] gets true when the label was made. */
    fun create(name: String, applyTo: List<String>, onDone: (created: Boolean) -> Unit = {}) {
        clearError()
        scope.launch {
            order.withLock {
                val label = try {
                    withContext(io) { backend.createLabel(name) }
                } catch (e: CancellationException) {
                    throw e
                } catch (e: Exception) {
                    fail(UiMessage.of(e, R.string.labels_create_failed))
                    onDone(false)
                    return@withLock
                }
                stateFlow.update { s -> s.copy(labels = if (s.labels.any { it.id == label.id }) s.labels else s.labels + label) }
                onDone(true)
                if (applyTo.isNotEmpty()) {
                    val ids = applyTo.distinct()
                    begin(ids)
                    stateFlow.update { it.copy(byMessage = LabelRules.applied(it.byMessage, ids, label.id, true)) }
                    change(ids, label.id, true)
                }
            }
            refreshLabels()
        }
    }

    /** Removes the label from the list (messages keep the keyword on the server). [onDone] gets true when it worked. */
    fun delete(labelId: String, onDone: (deleted: Boolean) -> Unit = {}) {
        scope.launch {
            order.withLock {
                try {
                    withContext(io) { backend.deleteLabel(labelId) }
                } catch (e: CancellationException) {
                    throw e
                } catch (e: Exception) {
                    fail(UiMessage.of(e, R.string.labels_delete_failed))
                    onDone(false)
                    return@withLock
                }
                stateFlow.update { s -> s.copy(labels = s.labels.filter { it.id != labelId }) }
                onDone(true)
            }
            refreshLabels()
        }
    }

    fun reset() {
        floor = ticket.incrementAndGet()
        labelsFloor = labelsTicket.incrementAndGet()
        synchronized(applied) {
            applied.clear()
            pending.clear()
            stateFlow.value = LabelsState()
        }
    }

    /** A change of the user starts: it is the newest word about these messages and holds back read answers until it is over. */
    private fun begin(ids: List<String>) {
        mark(ids)
        synchronized(applied) { ids.forEach { pending[it] = (pending[it] ?: 0) + 1 } }
    }

    private fun finish(ids: List<String>) {
        synchronized(applied) {
            ids.forEach { id ->
                val left = (pending[id] ?: 0) - 1
                if (left > 0) pending[id] = left else pending.remove(id)
            }
        }
    }

    /** A change on screen counts as the newest word about these messages: older answers are dropped. */
    private fun mark(ids: List<String>) {
        val mine = ticket.incrementAndGet()
        synchronized(applied) { ids.forEach { applied[it] = mine } }
    }
}
