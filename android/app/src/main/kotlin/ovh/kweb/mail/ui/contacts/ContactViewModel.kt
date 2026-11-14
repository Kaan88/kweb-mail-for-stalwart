package ovh.kweb.mail.ui.contacts

import ovh.kweb.mail.accounts.AccountSession
import android.app.Application
import android.util.Log
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.ensureActive
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
import ovh.kweb.mail.ui.UiMessage
import ovh.kweb.mail.ui.compose.DraftLauncher
import uniffi.kweb_core.Address
import uniffi.kweb_core.ContactDetail
import uniffi.kweb_core.DraftKind
import uniffi.kweb_core.MessageHeader
import uniffi.kweb_core.Exception as CoreException

enum class ContactPhase { LOADING, READY, FAILED, MISSING }

data class ContactState(
    val phase: ContactPhase = ContactPhase.LOADING,
    val detail: ContactDetail? = null,
    /** Up to five messages with the contact's first mail address; empty when there is none or the search failed. */
    val recent: List<MessageHeader> = emptyList(),
    /** A delete is on its way. */
    val busy: Boolean = false,
    val message: UiMessage? = null,
)

/** A blank draft the core made for a message to [to]; the compose screen puts the recipient in. */
data class StartedDraft(val draftId: String, val to: Address)

/** One contact for its own screen: reads it and its recent mail, deletes it, starts a message to one of its addresses. */
class ContactViewModel(app: Application, private val session: AccountSession, private val contactId: String) : AndroidViewModel(app) {
    private val stateFlow = MutableStateFlow(ContactState())
    private val leaveChannel = Channel<Unit>(Channel.BUFFERED)
    private val draftChannel = Channel<StartedDraft>(Channel.BUFFERED)
    private var loadJob: Job? = null
    private var startingDraft = false

    val state: StateFlow<ContactState> get() = stateFlow

    /** Sent once the contact is gone: the screen goes back. */
    val left: Flow<Unit> get() = leaveChannel.receiveAsFlow()

    /** Sent with the id of a draft that was started: the screen opens it. */
    val draftStarted: Flow<StartedDraft> get() = draftChannel.receiveAsFlow()

    /** Reads the contact, then its recent mail. What is on screen stays while it is read again. */
    fun load() {
        loadJob?.cancel()
        loadJob = viewModelScope.launch {
            val detail = try {
                withContext(Dispatchers.IO) { session.core.contact(contactId) }
            } catch (e: CancellationException) {
                throw e
            } catch (e: CoreException.Invalid) {
                Log.w(Background.TAG, "the contact is gone: ${e.msg}")
                stateFlow.update { if (it.detail == null) it.copy(phase = ContactPhase.MISSING) else it }
                return@launch
            } catch (e: Exception) {
                Log.w(Background.TAG, "reading a contact failed: ${e.javaClass.simpleName}")
                stateFlow.update {
                    if (it.detail == null) it.copy(phase = ContactPhase.FAILED) else it.copy(message = UiMessage.Res(R.string.contact_load_failed))
                }
                return@launch
            }
            stateFlow.update { it.copy(phase = ContactPhase.READY, detail = detail) }
            val address = ContactRules.firstEmail(detail.contact.emails)
            if (address == null) {
                stateFlow.update { it.copy(recent = emptyList()) }
                return@launch
            }
            // A failure here is silent: the contact itself is on screen.
            val recent = try {
                withContext(Dispatchers.IO) { session.core.search(address, null, ContactRules.RECENT_LIMIT.toUInt()).messages }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "reading the recent mail of a contact failed: ${e.javaClass.simpleName}")
                emptyList()
            }
            stateFlow.update { it.copy(recent = recent.take(ContactRules.RECENT_LIMIT)) }
        }
    }

    fun retry() {
        stateFlow.update { it.copy(phase = ContactPhase.LOADING) }
        load()
    }

    fun delete() {
        if (stateFlow.value.busy) return
        stateFlow.update { it.copy(busy = true, message = null) }
        viewModelScope.launch {
            try {
                withContext(Dispatchers.IO) { session.core.deleteContact(contactId) }
                leaveChannel.send(Unit)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "deleting a contact failed: ${e.javaClass.simpleName}")
                stateFlow.update { it.copy(busy = false, message = UiMessage.of(e, R.string.contact_delete_failed)) }
            }
        }
    }

    /**
     * A tap on a mail address: starts a blank draft and has the screen open it with the address as recipient.
     * One at a time, so a double tap makes one draft. A draft that nobody gets to open (the screen was left
     * meanwhile) is thrown away again.
     */
    fun write(email: String) {
        if (startingDraft) return
        startingDraft = true
        val contact = stateFlow.value.detail?.contact
        val name = ContactRules.recipientName(contact?.name.orEmpty(), contact?.emails.orEmpty(), email)
        viewModelScope.launch {
            var draftId: String? = null
            var handedOver = false
            try {
                // Not cancellable: a draft the core made must reach the discard below, or the screen.
                draftId = withContext(NonCancellable) { DraftLauncher.create(session.core, DraftKind.New) }
                ensureActive()
                draftChannel.send(StartedDraft(draftId, Address(name = name, email = email)))
                handedOver = true
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "starting a message to a contact failed: ${e.javaClass.simpleName}")
                stateFlow.update { it.copy(message = UiMessage.Res(R.string.compose_start_failed)) }
            } finally {
                val orphan = draftId
                if (orphan != null && !handedOver) withContext(NonCancellable) { DraftLauncher.discard(session.core, orphan) }
                startingDraft = false
            }
        }
    }

    fun dismissMessage() = stateFlow.update { it.copy(message = null) }
}
