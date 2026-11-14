package ovh.kweb.mail.ui.shared

import android.app.Application
import android.util.Log
import androidx.compose.foundation.lazy.LazyListState
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import ovh.kweb.mail.R
import ovh.kweb.mail.background.Background
import ovh.kweb.mail.ui.SessionState
import ovh.kweb.mail.ui.UiMessage
import ovh.kweb.mail.ui.compose.DraftLauncher
import ovh.kweb.mail.ui.calendar.DeleteScope
import ovh.kweb.mail.ui.contacts.ContactRules
import uniffi.kweb_core.Address
import uniffi.kweb_core.AddressBook
import uniffi.kweb_core.Attachment
import uniffi.kweb_core.ContactDetail
import uniffi.kweb_core.DraftKind
import uniffi.kweb_core.SharedAccount
import uniffi.kweb_core.SharedMailbox

/*
 * The view models of the "Shared with me" screens. Each belongs to one route (see ScreenStores), so it lives as
 * long as its screen is on the back stack: a rotation keeps it, a new visit starts clean. The logic is in the
 * controllers, which the tests drive with a fake backend.
 */

private fun backendOf(app: Application): SharedBackend = CoreSharedBackend { Background.core(app) }

private fun writeBackendOf(app: Application): SharedWriteBackend = CoreSharedWriteBackend { Background.core(app) }

/** Signing out under an open screen leaves nothing to show. */
private fun CoroutineScope.onSignedOut(reset: () -> Unit) {
    launch { SessionState.signedIn.collect { if (it == false) reset() } }
}

class SharedAccountsViewModel(app: Application) : AndroidViewModel(app) {
    private val backend = backendOf(app)
    val controller = LoadController<List<SharedAccount>>(viewModelScope, Dispatchers.IO, goneOnInvalid = false) {
        SharedRules.listed(backend.accounts())
    }
    val listState = LazyListState()

    init {
        viewModelScope.onSignedOut(controller::reset)
    }
}

class SharedMailboxesViewModel(app: Application, account: String) : AndroidViewModel(app) {
    private val backend = backendOf(app)
    val controller = LoadController<List<SharedMailbox>>(viewModelScope, Dispatchers.IO, goneOnInvalid = true) { backend.mailboxes(account) }
    val listState = LazyListState()

    init {
        viewModelScope.onSignedOut(controller::reset)
    }
}

class SharedFolderViewModel(app: Application, account: String, mailboxId: String) : AndroidViewModel(app) {
    val controller = FolderController(viewModelScope, Dispatchers.IO, backendOf(app), account, mailboxId)

    /** Kept here so that the place in the list survives a visit to a message and back, and a rotation. */
    val listState = LazyListState()

    init {
        viewModelScope.onSignedOut(controller::reset)
    }
}

class SharedMessageViewModel(app: Application, account: String, mailboxId: String, messageId: String) : AndroidViewModel(app) {
    val controller = SharedMessageController(viewModelScope, Dispatchers.IO, backendOf(app), account, mailboxId, messageId)

    /** The attachment the system file picker is open for. Lives here so a rotation does not lose it. */
    var pendingSave: Attachment? = null

    /** Starts the user's own reply, reply to all or forward of this message. */
    val drafts = SharedDraftStarter(viewModelScope, Dispatchers.IO, writeBackendOf(app), account)

    init {
        viewModelScope.onSignedOut(controller::reset)
    }
}

class SharedAgendaViewModel(app: Application, account: String) : AndroidViewModel(app) {
    val controller = SharedAgendaController(viewModelScope, Dispatchers.IO, backendOf(app), account)
    val listState = LazyListState()

    init {
        viewModelScope.onSignedOut(controller::reset)
    }
}

class SharedEventViewModel(app: Application, account: String, eventId: String, recurrenceId: String?) : AndroidViewModel(app) {
    private val backend = backendOf(app)
    val controller = LoadController<SharedEventData>(viewModelScope, Dispatchers.IO, goneOnInvalid = false) {
        readSharedEvent(backend, account, eventId, recurrenceId)
    }

    /** Deletes the event (or one occurrence of it) where the calendar may be written to. */
    val deleter = SharedDeleter<DeleteScope>(viewModelScope, Dispatchers.IO, R.string.cal_delete_failed) { scope ->
        writeBackendOf(app).deleteEvent(account, eventId, recurrenceId.takeIf { scope == DeleteScope.THIS_ONE })
    }

    init {
        viewModelScope.onSignedOut(controller::reset)
        viewModelScope.onSignedOut(deleter::reset)
    }
}

class SharedContactsViewModel(app: Application, account: String) : AndroidViewModel(app) {
    val controller = SharedContactsController(viewModelScope, Dispatchers.IO, backendOf(app), account)
    val listState = LazyListState()

    /** The address books the account shares, for knowing whether a new contact can be added. */
    val books = LoadController<List<AddressBook>>(viewModelScope, Dispatchers.IO, goneOnInvalid = false) {
        writeBackendOf(app).addressBooks(account)
    }

    init {
        viewModelScope.onSignedOut(controller::reset)
        viewModelScope.onSignedOut(books::reset)
    }
}

/** A blank draft the core made for a message to [to]; the compose screen puts the recipient in. */
data class StartedSharedDraft(val draftId: String, val to: Address)

class SharedContactViewModel(app: Application, account: String, contactId: String) : AndroidViewModel(app) {
    private val backend = backendOf(app)
    val controller = LoadController<ContactDetail>(viewModelScope, Dispatchers.IO, goneOnInvalid = false) { backend.contact(account, contactId) }

    /** Deletes the contact where its address book may be written to. */
    val deleter = SharedDeleter<Unit>(viewModelScope, Dispatchers.IO, R.string.contact_delete_failed) {
        writeBackendOf(app).deleteContact(account, contactId)
    }
    private val draftChannel = Channel<StartedSharedDraft>(Channel.BUFFERED)
    private val problems = MutableStateFlow<UiMessage?>(null)
    private var startingDraft = false

    /** Sent with the id of a draft that was started: the screen opens it. */
    val draftStarted: Flow<StartedSharedDraft> get() = draftChannel.receiveAsFlow()

    val problem: StateFlow<UiMessage?> get() = problems

    init {
        viewModelScope.onSignedOut(controller::reset)
        viewModelScope.onSignedOut(deleter::reset)
    }

    /**
     * A tap on a mail address: starts a blank draft and has the screen open it with the address as recipient.
     * One at a time. A draft that nobody gets to open (the screen was left meanwhile) is thrown away again.
     * A shared contact is only shown; the message is the user's own and goes from the user's own account.
     */
    fun write(email: String) {
        if (startingDraft) return
        startingDraft = true
        val contact = controller.state.value.value?.contact
        val name = ContactRules.recipientName(contact?.name.orEmpty(), contact?.emails.orEmpty(), email)
        viewModelScope.launch {
            var draftId: String? = null
            var handedOver = false
            try {
                draftId = withContext(NonCancellable) { DraftLauncher.create(getApplication(), DraftKind.New) }
                ensureActive()
                draftChannel.send(StartedSharedDraft(draftId, Address(name = name, email = email)))
                handedOver = true
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "starting a message to a shared contact failed: ${e.javaClass.simpleName}")
                problems.update { UiMessage.Res(R.string.compose_start_failed) }
            } finally {
                val orphan = draftId
                if (orphan != null && !handedOver) withContext(NonCancellable) { DraftLauncher.discard(getApplication(), orphan) }
                startingDraft = false
            }
        }
    }

    fun dismissProblem() = problems.update { null }
}
