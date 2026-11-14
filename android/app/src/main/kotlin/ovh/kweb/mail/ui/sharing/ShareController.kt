package ovh.kweb.mail.ui.sharing

import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import ovh.kweb.mail.ui.UiMessage
import ovh.kweb.mail.ui.shared.SharedEvent
import ovh.kweb.mail.ui.shared.SharedEvents
import ovh.kweb.mail.ui.shared.SharedRules
import ovh.kweb.mail.ui.shared.attempt
import uniffi.kweb_core.Exception as CoreException
import uniffi.kweb_core.Person
import uniffi.kweb_core.Share
import uniffi.kweb_core.ShareKind
import uniffi.kweb_core.ShareLevel

data class ShareState(
    /** Whom the thing is shared with; null until the first answer. */
    val shares: List<Share>? = null,
    val loading: Boolean = false,
    /** Why there is no list of shares. */
    val error: UiMessage? = null,
    /** What is in the search field. */
    val query: String = "",
    /** The trimmed query [people] answers. */
    val answeredQuery: String = "",
    val people: List<Person> = emptyList(),
    val searching: Boolean = false,
    /** A lookup failed (no connection, say); typing again asks again. */
    val searchError: UiMessage? = null,
    /** The server does not let the app look people up: its words, shown in place of the search. Existing shares still work. */
    val lookupRefused: UiMessage? = null,
    /** The level a person that is added gets. */
    val newLevel: ShareLevel = ShareLevel.READ,
    /** The person whose share is being changed; one change at a time. */
    val busyPersonId: String? = null,
    /** The person whose share is about to be taken back: the question is open. */
    val confirmRemove: Person? = null,
)

/**
 * The state of the Share screen for one thing of the user's own: whom it is shared with, a lookup of people to add,
 * levels to change, shares to take back (after a question). Only the newest answer of each kind counts: a late
 * answer of an older lookup or read never replaces a newer one. The tests drive it with a fake backend.
 */
class ShareController(
    private val scope: CoroutineScope,
    private val io: CoroutineDispatcher,
    private val backend: SharingBackend,
    private val kind: ShareKind,
    private val id: String,
    private val debounceMillis: Long = ShareRules.DEBOUNCE_MILLIS,
) {
    private val flow = MutableStateFlow(ShareState())
    private val bus = SharedEvents()
    private var loadGen = 0
    private var searchGen = 0
    private var epoch = 0
    private var typing: Job? = null

    val state: StateFlow<ShareState> get() = flow
    val events: Flow<SharedEvent> get() = bus.flow

    /** The screen came up: the first time it reads the shares, later it reads again quietly. */
    fun show() = load(manual = false)

    /** Retry. */
    fun refresh() = load(manual = true)

    /** Signed out or left for good: forget everything and ignore what is still on its way. */
    fun reset() {
        typing?.cancel()
        loadGen++
        searchGen++
        epoch++
        flow.value = ShareState()
        bus.clear()
    }

    private fun load(manual: Boolean) {
        val gen = ++loadGen
        flow.update { it.copy(loading = it.shares == null, error = null) }
        scope.launch {
            val result = io.attempt { backend.shares(kind, id) }
            if (gen != loadGen) return@launch
            result.onSuccess { list -> flow.update { it.copy(shares = ShareRules.ordered(list), loading = false, error = null) } }
                .onFailure { e ->
                    val message = SharedRules.message(e)
                    val had = flow.value.shares != null
                    flow.update { it.copy(loading = false, error = if (had) null else message) }
                    if (had && manual) bus.say(message)
                }
        }
    }

    // ---- the lookup

    fun onQueryChange(text: String) {
        typing?.cancel()
        val gen = ++searchGen
        flow.update { it.copy(query = text) }
        if (!ShareRules.canSearch(text) || flow.value.lookupRefused != null) {
            flow.update { it.copy(people = emptyList(), answeredQuery = "", searching = false, searchError = null) }
            return
        }
        flow.update { it.copy(searching = true, searchError = null) }
        val wanted = text.trim()
        typing = scope.launch {
            delay(debounceMillis)
            val result = io.attempt { backend.people(wanted) }
            if (gen != searchGen) return@launch
            result.onSuccess { people ->
                flow.update { it.copy(people = people, answeredQuery = wanted, searching = false, searchError = null) }
            }.onFailure { e ->
                val message = SharedRules.message(e)
                if (e is CoreException.Invalid) {
                    // The server's own words, in place of the search.
                    flow.update { it.copy(lookupRefused = message, people = emptyList(), answeredQuery = "", searching = false, searchError = null) }
                } else {
                    flow.update { it.copy(people = emptyList(), answeredQuery = "", searching = false, searchError = message) }
                }
            }
        }
    }

    fun setNewLevel(level: ShareLevel) = flow.update { it.copy(newLevel = level) }

    // ---- changing shares

    /** Shares the thing with [person] at the chosen level; the search field is emptied. */
    fun add(person: Person) {
        val pickedFrom = flow.value.query
        change(person.id, flow.value.newLevel) {
            // Newer input typed while the add was on its way stays.
            if (flow.value.query != pickedFrom) return@change
            typing?.cancel()
            searchGen++
            flow.update { it.copy(query = "", people = emptyList(), answeredQuery = "", searching = false, searchError = null) }
        }
    }

    fun changeLevel(person: Person, level: ShareLevel) {
        if (flow.value.shares?.firstOrNull { it.person.id == person.id }?.level == level) return
        change(person.id, level) {}
    }

    /** Takes a share back, but only after the question: this opens it. */
    fun askRemove(person: Person) = flow.update { it.copy(confirmRemove = person) }

    fun cancelRemove() = flow.update { it.copy(confirmRemove = null) }

    fun confirmRemove() {
        val person = flow.value.confirmRemove ?: return
        flow.update { it.copy(confirmRemove = null) }
        change(person.id, null) {}
    }

    private fun change(personId: String, level: ShareLevel?, onDone: () -> Unit) {
        if (flow.value.busyPersonId != null) return
        val mine = epoch
        flow.update { it.copy(busyPersonId = personId) }
        scope.launch {
            val result = io.attempt { backend.setShare(kind, id, personId, level) }
            if (mine != epoch) return@launch
            result.onSuccess { list ->
                // Newer than any read still on its way.
                loadGen++
                flow.update { it.copy(shares = ShareRules.ordered(list), busyPersonId = null, loading = false, error = null) }
                onDone()
            }.onFailure { e ->
                flow.update { it.copy(busyPersonId = null) }
                bus.say(SharedRules.message(e))
            }
        }
    }
}
