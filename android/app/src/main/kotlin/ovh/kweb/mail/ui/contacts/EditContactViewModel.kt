package ovh.kweb.mail.ui.contacts

import android.app.Application
import android.util.Log
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
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
import uniffi.kweb_core.AddressBook
import uniffi.kweb_core.Exception as CoreException

enum class EditContactPhase { LOADING, READY, FAILED, MISSING, READ_ONLY, NO_BOOK }

data class EditContactState(
    val phase: EditContactPhase = EditContactPhase.LOADING,
    val form: ContactForm? = null,
    /** The form as it was when it was opened, to tell whether the user changed anything. */
    val initial: ContactForm? = null,
    /** The address books the user may write to: the menu. */
    val books: List<AddressBook> = emptyList(),
    val creating: Boolean = true,
    val saving: Boolean = false,
    val error: UiMessage? = null,
) {
    val dirty: Boolean get() = form != null && initial != null && ContactForms.isChanged(form, initial)
}

/** The form for a new contact ([contactId] null) or for changing one. It lives as long as its route (see ScreenStores). */
class EditContactViewModel(
    app: Application,
    private val contactId: String?,
    /** The owner's account when the contact is in an address book shared with the user; null for the user's own. */
    account: String? = null,
) : AndroidViewModel(app) {
    private val backend = contactFormBackend({ Background.core(app) }, account)
    private val stateFlow = MutableStateFlow(EditContactState(creating = contactId == null))
    private val doneChannel = Channel<Unit>(Channel.BUFFERED)
    private var loadJob: Job? = null

    val state: StateFlow<EditContactState> get() = stateFlow

    /** Sent once the contact is saved: the screen goes back without asking about changes. */
    val saved: Flow<Unit> get() = doneChannel.receiveAsFlow()

    /** Reads what the form needs. Does nothing while a read runs, and never replaces a form that is open. */
    fun load() {
        if (loadJob?.isActive == true) return
        if (stateFlow.value.let { it.phase == EditContactPhase.READY || it.phase == EditContactPhase.READ_ONLY }) return
        stateFlow.update { it.copy(phase = EditContactPhase.LOADING) }
        loadJob = viewModelScope.launch {
            try {
                val (books, detail) = withContext(Dispatchers.IO) {
                    val detail = contactId?.let { backend.contact(it) }
                    // For a change the books only fill the menu: a failure leaves the contact where it is.
                    val books = try {
                        backend.addressBooks()
                    } catch (e: CancellationException) {
                        throw e
                    } catch (e: Exception) {
                        if (detail == null) throw e
                        emptyList()
                    }
                    books to detail
                }
                val writable = books.filter { it.mayWrite }
                if (detail != null) {
                    val form = ContactForms.fromContact(detail.contact)
                    stateFlow.update {
                        it.copy(
                            phase = if (detail.mayWrite) EditContactPhase.READY else EditContactPhase.READ_ONLY,
                            form = form,
                            initial = form,
                            books = writable,
                        )
                    }
                } else {
                    val book = writable.firstOrNull { it.isDefault } ?: writable.firstOrNull()
                    if (book == null) {
                        stateFlow.update { it.copy(phase = EditContactPhase.NO_BOOK) }
                        return@launch
                    }
                    val form = ContactForms.blank(book.id)
                    stateFlow.update { it.copy(phase = EditContactPhase.READY, form = form, initial = form, books = writable) }
                }
            } catch (e: CancellationException) {
                throw e
            } catch (e: CoreException.Invalid) {
                Log.w(Background.TAG, "the contact to edit is gone: ${e.msg}")
                stateFlow.update { it.copy(phase = EditContactPhase.MISSING) }
            } catch (e: Exception) {
                Log.w(Background.TAG, "opening the contact form failed: ${e.javaClass.simpleName}")
                stateFlow.update { it.copy(phase = EditContactPhase.FAILED) }
            }
        }
    }

    /** Changes the form; the error shown is about the form as it was, so it goes. */
    fun edit(change: (ContactForm) -> ContactForm) = stateFlow.update { state ->
        state.form?.let { state.copy(form = change(it), error = null) } ?: state
    }

    fun dismissError() = stateFlow.update { it.copy(error = null) }

    fun save() {
        val state = stateFlow.value
        val form = state.form ?: return
        if (state.saving || state.phase != EditContactPhase.READY) return
        stateFlow.update { it.copy(saving = true, error = null) }
        viewModelScope.launch {
            try {
                val wanted = ContactForms.toContact(form)
                val base = state.initial?.let(ContactForms::toContact)
                withContext(Dispatchers.IO) {
                    if (contactId == null || base == null) {
                        backend.save(contactId, wanted)
                    } else {
                        // The form may show an old copy, or the card may have changed while it was open: the changes
                        // go onto the card as it is now, so nothing added elsewhere is deleted.
                        val current = ContactForms.normalized(backend.contact(contactId).contact)
                        backend.save(contactId, ContactForms.merge(base, wanted, current))
                    }
                }
                doneChannel.send(Unit)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "saving a contact failed: ${e.javaClass.simpleName}")
                stateFlow.update { it.copy(saving = false, error = UiMessage.of(e, R.string.contact_save_failed)) }
            }
        }
    }
}
