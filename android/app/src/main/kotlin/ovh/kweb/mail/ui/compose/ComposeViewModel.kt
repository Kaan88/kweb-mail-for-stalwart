package ovh.kweb.mail.ui.compose

import ovh.kweb.mail.accounts.AccountSession
import android.app.Application
import android.net.Uri
import android.util.Log
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.joinAll
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import java.util.concurrent.ConcurrentHashMap
import ovh.kweb.mail.R
import ovh.kweb.mail.background.Background
import ovh.kweb.mail.ui.sending.ScheduledNotices
import ovh.kweb.mail.ui.sending.SendLaterRules
import uniffi.kweb_core.Address
import uniffi.kweb_core.Attachment
import uniffi.kweb_core.Draft
import uniffi.kweb_core.Identity
import uniffi.kweb_core.Exception as CoreException

/** The three recipient fields. */
enum class Field { To, Cc, Bcc }

/** [Missing]: the core has no such draft. [Failed]: it could not be read, which says nothing about the draft. */
enum class Phase { Loading, Ready, Missing, Failed }

/** What the compose screen shows. The text of a field is what the user is typing; the addresses before it are chips. */
data class ComposeState(
    val phase: Phase = Phase.Loading,
    val kind: ComposeRules.Kind = ComposeRules.Kind.New,
    val identities: List<Identity> = emptyList(),
    val identityId: String = "",
    val to: List<Address> = emptyList(),
    val cc: List<Address> = emptyList(),
    val bcc: List<Address> = emptyList(),
    val toText: String = "",
    val ccText: String = "",
    val bccText: String = "",
    /** Fields whose leftover text is drawn as wrong. */
    val marked: Set<Field> = emptySet(),
    val ccBccOpen: Boolean = false,
    val subject: String = "",
    val body: String = "",
    val attachments: List<Attachment> = emptyList(),
    /** Names of the files being copied and uploaded. */
    val attaching: List<String> = emptyList(),
    val attachError: String? = null,
    /** Why the last send or save-and-leave did not work, for the error bar. */
    val error: String? = null,
    /** Leaving without saving is offered, because saving failed. */
    val canLeaveAnyway: Boolean = false,
    val sending: Boolean = false,
    /** Wall-clock time of the last save, null before the first. */
    val savedAtMillis: Long? = null,
    val saveFailed: Boolean = false,
    val suggestionsFor: Field? = null,
    val suggestions: List<Address> = emptyList(),
) {
    fun text(field: Field) = when (field) {
        Field.To -> toText
        Field.Cc -> ccText
        Field.Bcc -> bccText
    }

    fun addresses(field: Field) = when (field) {
        Field.To -> to
        Field.Cc -> cc
        Field.Bcc -> bcc
    }

    fun withText(field: Field, value: String) = when (field) {
        Field.To -> copy(toText = value)
        Field.Cc -> copy(ccText = value)
        Field.Bcc -> copy(bccText = value)
    }

    fun withAddresses(field: Field, value: List<Address>) = when (field) {
        Field.To -> copy(to = value)
        Field.Cc -> copy(cc = value)
        Field.Bcc -> copy(bcc = value)
    }

    val ready: Boolean get() = phase == Phase.Ready
    val canSend: Boolean get() = ready && !sending && attaching.isEmpty()
}

/** What the screen has to do once. */
sealed interface ComposeEvent {
    /** Close the screen and go back. */
    data object Leave : ComposeEvent

    /** The message is with the core: close the screen and show the list, where Undo is offered. */
    data object Sent : ComposeEvent
}

/**
 * One message being written. The state lives here so a rotation loses nothing; the draft itself
 * is saved to the core 2 s after the last change, when the screen goes to the background and when
 * it is left. [start] loads it, [close] saves what is left and lets go of it.
 */
class ComposeViewModel(app: Application, private val acct: AccountSession, private val draftId: String, private val prefillTo: Address? = null) : AndroidViewModel(app) {
    private val eventChannel = Channel<ComposeEvent>(Channel.BUFFERED)

    /** Keeps saves, uploads and the send from running into each other on the same draft. */
    private val lock = Mutex()
    private var started = false
    private var done = false

    /**
     * Counts openings and closings of the screen. A leave, send or upload remembers the number it
     * started under and touches nothing of the session once it has changed.
     */
    @Volatile
    private var epoch = 0

    /** A leave is waiting for the uploads and the final save: [close] must still save the draft. */
    private var leaving = false

    /** The uploads in progress. They are kept so that leaving can wait for them and not lose their files. */
    private val uploads = ConcurrentHashMap.newKeySet<Job>()

    /**
     * The draft as the core last returned it: the bookkeeping fields (server copy, update time) come
     * from here. Written under [lock] on a background thread, read on others, hence volatile.
     */
    @Volatile
    private var base: Draft? = null

    /** The draft as it was when the screen opened, to tell an untouched new message. */
    private var opened: Draft? = null

    /** What was last written to the core. */
    private var saved: Draft? = null
    private var saveJob: Job? = null
    private var suggestJob: Job? = null
    private var suggestSeq = 0

    /**
     * Snapshot state and not a flow: a text field fed through a flow's extra hop to the UI thread
     * loses keystrokes when the user types fast.
     */
    var state by mutableStateOf(ComposeState())
        private set

    val events: Flow<ComposeEvent> get() = eventChannel.receiveAsFlow()

    /** Loads the draft; does nothing when it was loaded already. */
    fun start() {
        if (started) return
        started = true
        done = false
        leaving = false
        // An event of an earlier opening (a Leave nobody collected) must not close this one.
        while (eventChannel.tryReceive().isSuccess) {
            // dropped
        }
        val mine = ++epoch
        viewModelScope.launch {
            val loaded = withContext(Dispatchers.IO) {
                try {
                    val c = acct.core
                    val draft = c.draft(draftId)
                    val identities = try {
                        c.identities()
                    } catch (e: CoreException) {
                        // Without the list the From line is not shown and the draft keeps its identity.
                        Log.w(Background.TAG, "reading the identities failed", e)
                        emptyList()
                    }
                    draft to identities
                } catch (e: CoreException) {
                    Log.w(Background.TAG, "reading the draft failed", e)
                    null
                }
            }
            // The screen was closed while the draft was being read: this answer is for nobody.
            if (mine != epoch || !started) return@launch
            if (loaded == null) {
                update { it.copy(phase = Phase.Failed) }
                return@launch
            }
            val (draft, identities) = loaded
            if (draft == null) {
                update { it.copy(phase = Phase.Missing) }
                return@launch
            }
            base = draft
            opened = draft
            saved = draft
            // A recipient handed in by the caller goes into a blank new message only; it is not saved until something is.
            val to = if (prefillTo != null && ComposeRules.isBlankNew(draft) && prefillTo !in draft.to) draft.to + prefillTo else draft.to
            state = ComposeState(
                phase = Phase.Ready,
                kind = ComposeRules.kind(draft),
                identities = identities,
                identityId = draft.identityId,
                to = to,
                cc = draft.cc,
                bcc = draft.bcc,
                ccBccOpen = draft.cc.isNotEmpty() || draft.bcc.isNotEmpty(),
                subject = draft.subject,
                body = draft.body,
                attachments = draft.attachments,
                error = draft.sendError?.let { reason -> getApplication<Application>().getString(R.string.compose_send_failed_before, reason) },
            )
        }
    }

    /** Reading the draft failed and the user asked again. */
    fun retry() {
        if (state.phase != Phase.Failed) return
        started = false
        state = ComposeState()
        start()
    }

    // ---- editing ----

    fun setSubject(value: String) = edit { it.copy(subject = value) }

    fun setBody(value: String) = edit { it.copy(body = value) }

    fun setIdentity(id: String) = edit { it.copy(identityId = id) }

    fun openCcBcc() = update { it.copy(ccBccOpen = true) }

    /** The text of a recipient field changed: finished addresses become chips, the rest is looked up for completion. */
    fun onText(field: Field, value: String) {
        val s = state
        if (!s.ready || done || s.sending) return
        takeAddresses(field, value, Recipients.consume(value))
        suggest(field, state.text(field))
    }

    /** Enter, or the field lost focus: whatever is typed is finished if it is an address. */
    fun commit(field: Field) {
        val s = state
        if (!s.ready || done) return
        takeAddresses(field, s.text(field), Recipients.consume(s.text(field), enter = true))
        clearSuggestions()
    }

    fun remove(field: Field, address: Address) = edit { it.withAddresses(field, it.addresses(field) - address) }

    /** A suggestion was picked: it becomes a chip and the typed text goes. */
    fun pick(field: Field, address: Address) {
        edit { it.withAddresses(field, Recipients.merge(it.addresses(field), listOf(address))).withText(field, "") }
        update { it.copy(marked = it.marked - field) }
        clearSuggestions()
    }

    fun clearSuggestions() {
        suggestSeq++
        suggestJob?.cancel()
        if (state.suggestions.isNotEmpty() || state.suggestionsFor != null) {
            update { it.copy(suggestions = emptyList(), suggestionsFor = null) }
        }
    }

    private fun takeAddresses(field: Field, typed: String, consumed: Recipients.Consumed) {
        if (consumed.accepted.isEmpty() && consumed.rest == typed && !consumed.rejected) {
            update { it.withText(field, typed).copy(marked = unmark(it.marked, field, typed, false)) }
            return
        }
        val changed = consumed.accepted.isNotEmpty()
        update {
            val merged = Recipients.merge(it.addresses(field), consumed.accepted)
            it.withAddresses(field, merged).withText(field, consumed.rest)
                .copy(marked = unmark(it.marked, field, consumed.rest, consumed.rejected))
        }
        if (changed) scheduleSave()
    }

    /** Updates the set of fields drawn as wrong for the text now in [field]. */
    private fun unmark(marked: Set<Field>, field: Field, rest: String, rejected: Boolean): Set<Field> =
        if (Recipients.isMarked(rest, rejected || field in marked)) marked + field else marked - field

    private fun suggest(field: Field, text: String) {
        suggestJob?.cancel()
        val query = text.trim()
        val seq = ++suggestSeq
        if (query.isEmpty()) {
            update { it.copy(suggestions = emptyList(), suggestionsFor = null) }
            return
        }
        suggestJob = viewModelScope.launch {
            delay(ComposeRules.SUGGEST_MILLIS)
            val found = withContext(Dispatchers.IO) {
                try {
                    acct.core.suggestRecipients(query, SUGGESTIONS.toUInt())
                } catch (e: CoreException) {
                    Log.d(Background.TAG, "completion failed: ${e.javaClass.simpleName}")
                    emptyList()
                }
            }
            // A newer query has been started since: its answer is the one to show.
            if (seq != suggestSeq) return@launch
            update {
                val taken = it.addresses(field).map { a -> a.email.lowercase() }.toSet()
                val shown = found.filter { a -> a.email.lowercase() !in taken }
                it.copy(suggestions = shown, suggestionsFor = if (shown.isEmpty()) null else field)
            }
        }
    }

    // ---- attachments ----

    /** Copies each picked file to the cache and hands it to the core, one after the other. */
    fun attach(uris: List<Uri>) {
        if (!state.ready || done || state.sending) return
        val mine = epoch
        for (uri in uris) {
            val job = viewModelScope.launch { attachOne(uri, mine) }
            uploads.add(job)
            job.invokeOnCompletion { uploads.remove(job) }
        }
    }

    /** Changes the state only while the screen is still the opening [session]. */
    private fun updateIn(session: Int, change: (ComposeState) -> ComposeState) {
        if (session == epoch) update(change)
    }

    private suspend fun attachOne(uri: Uri, session: Int) {
        val context = getApplication<Application>()
        val name = withContext(Dispatchers.IO) { PickedFile.displayName(context, uri) }
        updateIn(session) { it.copy(attaching = it.attaching + name, attachError = null) }
        var picked: PickedFile? = null
        try {
            val added = withContext(Dispatchers.IO) {
                val copy = PickedFile.copy(context, uri, name)
                picked = copy
                lock.withLock {
                    val before = base?.attachments.orEmpty()
                    val after = acct.core.addAttachment(draftId, copy.file.path, copy.name, copy.mimeType)
                    if (session == epoch) base = after
                    // Only what is new: one the user removed meanwhile is in the core's list still and must stay removed.
                    after.attachments.filter { it !in before }
                }
            }
            if (session == epoch) {
                update { it.copy(attachments = it.attachments + added) }
                scheduleSave()
            }
        } catch (e: CancellationException) {
            throw e
        } catch (e: CoreException.Invalid) {
            Log.w(Background.TAG, "the server did not take an attachment: ${e.msg}")
            updateIn(session) { it.copy(attachError = context.getString(R.string.compose_attach_refused, name)) }
        } catch (e: Exception) {
            Log.w(Background.TAG, "adding an attachment failed", e)
            updateIn(session) { it.copy(attachError = context.getString(R.string.compose_attach_failed, name)) }
        } finally {
            withContext(NonCancellable + Dispatchers.IO) { picked?.delete() }
            updateIn(session) { it.copy(attaching = it.attaching - name) }
        }
    }

    fun removeAttachment(attachment: Attachment) = edit { it.copy(attachments = it.attachments - attachment) }

    fun dismissAttachError() = update { it.copy(attachError = null) }

    // ---- saving and leaving ----

    private fun update(change: (ComposeState) -> ComposeState) {
        state = change(state)
    }

    private fun edit(change: (ComposeState) -> ComposeState) {
        if (!state.ready || done || state.sending) return
        update { change(it).copy(error = null, canLeaveAnyway = false) }
        scheduleSave()
    }

    private fun scheduleSave() {
        if (done) return
        saveJob?.cancel()
        saveJob = viewModelScope.launch {
            delay(ComposeRules.AUTOSAVE_MILLIS)
            val draft = assemble() ?: return@launch
            write(draft)
        }
    }

    /** The draft as the screen shows it, on top of the core's own fields. */
    private fun assemble(): Draft? {
        val b = base ?: return null
        val s = state
        return b.copy(identityId = s.identityId, to = s.to, cc = s.cc, bcc = s.bcc, subject = s.subject, body = s.body, attachments = s.attachments)
    }

    /** [draft] with the core's own fields as they are now: a save or an upload may have changed them since it was assembled. */
    private fun withBookkeeping(draft: Draft): Draft {
        val b = base ?: return draft
        return draft.copy(serverId = b.serverId, updatedAt = b.updatedAt, sendError = b.sendError)
    }

    /** Writes [draft] to the core unless it is what was written last. Returns false when the core refused. */
    private suspend fun write(draft: Draft): Boolean {
        val last = saved
        if (last != null && ComposeRules.sameContent(last, draft)) return true
        return try {
            withContext(Dispatchers.IO) {
                lock.withLock { acct.core.saveDraft(withBookkeeping(draft)).also { base = it } }
            }
            saved = draft
            update { it.copy(savedAtMillis = System.currentTimeMillis(), saveFailed = false) }
            true
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            Log.w(Background.TAG, "saving the draft failed", e)
            update { it.copy(saveFailed = true) }
            false
        }
    }

    /** The screen went to the background: save now, and stay. */
    fun flush() {
        val s = state
        if (!s.ready || done || s.sending) return
        saveJob?.cancel()
        commitAll()
        val draft = assemble() ?: return
        viewModelScope.launch { write(draft) }
    }

    /**
     * Close and leave: wait for the uploads in progress, save (or throw an untouched new message
     * away), then tell the screen. The screen shows it is finishing meanwhile.
     */
    fun leave() {
        val s = state
        if (!s.ready || done || s.sending) return
        saveJob?.cancel()
        commitAll()
        if (assemble() == null) return
        update { it.copy(sending = true, error = null, canLeaveAnyway = false) }
        leaving = true
        val mine = epoch
        viewModelScope.launch {
            // Assembled once the uploads have landed, so their files are in the draft that is saved.
            uploads.toList().joinAll()
            if (mine != epoch) return@launch
            val draft = assemble() ?: return@launch
            val discard = opened?.let { StartedFromShare.discards(draftId, it, draft, prefillTo) } == true
            val ok = if (discard) discard() else write(draft)
            // The screen was closed meanwhile: [close] has taken over the draft.
            if (mine != epoch) return@launch
            leaving = false
            if (ok) {
                done = true
                eventChannel.send(ComposeEvent.Leave)
            } else {
                val text = getApplication<Application>().getString(R.string.compose_leave_save_failed)
                update { it.copy(sending = false, error = text, canLeaveAnyway = true) }
            }
        }
    }

    /** Saving did not work and the user chose to go on leaving. */
    fun leaveAnyway() {
        done = true
        viewModelScope.launch { eventChannel.send(ComposeEvent.Leave) }
    }

    private suspend fun discard(): Boolean = try {
        withContext(Dispatchers.IO) { lock.withLock { acct.core.discardDraft(draftId) } }
        true
    } catch (e: CancellationException) {
        throw e
    } catch (e: Exception) {
        Log.w(Background.TAG, "discarding the draft failed", e)
        false
    }

    fun send() {
        val s = state
        if (!s.canSend || done) return
        saveJob?.cancel()
        val context = getApplication<Application>()
        if (!commitAll()) {
            update { it.copy(error = context.getString(R.string.compose_error_bad_address)) }
            return
        }
        if (assemble() == null) return
        update { it.copy(sending = true, error = null, canLeaveAnyway = false) }
        val mine = epoch
        viewModelScope.launch {
            try {
                val pending = withContext(Dispatchers.IO) {
                    // Assembled once the save or upload in front of it has finished, so it carries what they changed.
                    lock.withLock {
                        val draft = withBookkeeping(checkNotNull(assemble()))
                        acct.core.send(draft, ComposeRules.UNDO_SECONDS)
                    }
                }
                // The message is with the core whatever became of the screen: the list must hear of it.
                Outbox.announce(acct.record.id, pending)
                if (mine != epoch) return@launch
                done = true
                eventChannel.send(ComposeEvent.Sent)
            } catch (e: CancellationException) {
                throw e
            } catch (e: CoreException.Invalid) {
                updateIn(mine) { it.copy(sending = false, error = e.msg) }
            } catch (e: Exception) {
                Log.w(Background.TAG, "sending failed", e)
                updateIn(mine) { it.copy(sending = false, error = context.getString(R.string.compose_send_failed)) }
            }
        }
    }

    /**
     * "Send later": hands the message to the core to be sent at [sendAt] (Unix seconds). The same checks as
     * [send]; the core's refusal is shown and the draft stays open. Once the core has it the confirmation goes
     * out through [ScheduledNotices], whatever has become of the screen, and the screen closes.
     */
    fun scheduleSend(sendAt: Long) {
        val s = state
        if (!s.canSend || done) return
        val context = getApplication<Application>()
        when (SendLaterRules.check(sendAt, System.currentTimeMillis() / 1000)) {
            SendLaterRules.Check.Ok -> Unit
            SendLaterRules.Check.TooSoon -> return update { it.copy(error = context.getString(R.string.send_later_too_soon)) }
            SendLaterRules.Check.TooFar -> return update { it.copy(error = context.getString(R.string.send_later_too_far)) }
        }
        saveJob?.cancel()
        if (!commitAll()) {
            update { it.copy(error = context.getString(R.string.compose_error_bad_address)) }
            return
        }
        if (assemble() == null) return
        update { it.copy(sending = true, error = null, canLeaveAnyway = false) }
        val mine = epoch
        viewModelScope.launch {
            try {
                withContext(Dispatchers.IO) {
                    lock.withLock {
                        val draft = withBookkeeping(checkNotNull(assemble()))
                        acct.core.scheduleSend(draft, sendAt)
                    }
                }
                // The message is with the core whatever became of the screen: the confirmation must still be shown.
                ScheduledNotices.announce(sendAt)
                if (mine != epoch) return@launch
                done = true
                eventChannel.send(ComposeEvent.Leave)
            } catch (e: CancellationException) {
                throw e
            } catch (e: CoreException.Invalid) {
                updateIn(mine) { it.copy(sending = false, error = e.msg.ifBlank { context.getString(R.string.send_later_failed) }) }
            } catch (e: CoreException.Temporary) {
                Log.w(Background.TAG, "scheduling failed: no answer")
                afterUnsureSchedule(mine, R.string.send_later_offline)
            } catch (e: Exception) {
                Log.w(Background.TAG, "scheduling failed", e)
                afterUnsureSchedule(mine, R.string.send_later_unsure)
            }
        }
    }

    /**
     * The core did not say that the message is scheduled. "Temporary" covers a request that never went out and one
     * whose answer was lost, so the exception does not tell which: the core is asked whether the draft is an
     * ordinary draft again. Only then is it said to be still a draft and Send offered. Otherwise the server may
     * already hold the message: Send stays off (it would look sent and do nothing) and the user is pointed to
     * Scheduled; leaving is still possible.
     */
    private suspend fun afterUnsureSchedule(mine: Int, stillDraftText: Int) {
        val context = getApplication<Application>()
        val ordinary = try {
            withContext(Dispatchers.IO) { acct.core.drafts().any { it.id == draftId } }
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            false
        }
        if (ordinary) {
            updateIn(mine) { it.copy(sending = false, error = context.getString(stillDraftText)) }
        } else {
            updateIn(mine) { it.copy(sending = true, error = context.getString(R.string.send_later_unsure), canLeaveAnyway = true) }
        }
    }

    /**
     * Finishes the addresses still being typed in the three fields. False when something is left in
     * one of them that is not an address: it is marked and the message must not go.
     */
    private fun commitAll(): Boolean {
        for (field in Field.entries) {
            val text = state.text(field)
            if (text.isNotBlank()) takeAddresses(field, text, Recipients.consume(text, enter = true))
        }
        return Field.entries.none { Recipients.blocksSend(state.text(it)) }
    }

    /**
     * The screen is gone for good. What is unsaved is written, off the screen's lifetime, and the
     * state is dropped so that opening the same draft again (Undo) starts from the core's copy.
     */
    fun close() {
        if (!started) return
        val s = state
        saveJob?.cancel()
        suggestJob?.cancel()
        if (s.ready && !done && (!s.sending || leaving)) {
            commitAll()
            val draft = assemble()
            if (draft != null) {
                val jobs = uploads.toList()
                val known = base?.attachments.orEmpty()
                val firstOpened = opened
                val lastSaved = saved
                // Not viewModelScope: when the account is switched this view model is cleared (which cancels that scope)
                // before or while the screen goes, and the text of the message must still reach its own account.
                lastSaveScope.launch { finalSave(draft, firstOpened, lastSaved, known, jobs) }
            }
        }
        leaving = false
        started = false
        done = true
        epoch++
        base = null
        opened = null
        saved = null
        state = ComposeState()
    }

    /**
     * The save of a screen that is gone: waits for the uploads still running, takes in the files they
     * added (the screen can no longer show them) and writes the draft, or throws it away when it is
     * still the blank message it was. [known] are the attachments the screen knew of.
     */
    private suspend fun finalSave(draft: Draft, opened: Draft?, saved: Draft?, known: List<Attachment>, jobs: List<Job>) {
        jobs.joinAll()
        try {
            withContext(Dispatchers.IO) {
                lock.withLock {
                    val core = acct.core
                    val stored = if (jobs.isEmpty()) null else core.draft(draftId)
                    val late = stored?.attachments.orEmpty().filter { it !in known }
                    val last = draft.copy(attachments = draft.attachments + late).let {
                        if (stored == null) it else it.copy(serverId = stored.serverId, updatedAt = stored.updatedAt, sendError = stored.sendError)
                    }
                    when {
                        opened != null && StartedFromShare.discards(draftId, opened, last, prefillTo) -> core.discardDraft(draftId)
                        saved != null && ComposeRules.sameContent(saved, last) -> Unit
                        else -> core.saveDraft(last)
                    }
                }
            }
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            Log.w(Background.TAG, "saving the draft on closing failed", e)
        }
    }

    /** Whatever ends this view model (a switch of account clears it), the draft is saved in the account it was started in. */
    override fun onCleared() {
        close()
    }

    companion object {
        private const val SUGGESTIONS = 6
        private val lastSaveScope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    }
}
