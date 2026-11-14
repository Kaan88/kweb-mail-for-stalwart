package ovh.kweb.mail.ui.contacts

import android.app.Application
import android.util.Log
import androidx.compose.foundation.lazy.LazyListState
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import ovh.kweb.mail.background.Background
import ovh.kweb.mail.ui.SessionState
import uniffi.kweb_core.ContactRow

data class ContactsState(
    /** What is typed in the search field. */
    val query: String = "",
    /** The query the rows are the answer to; the empty-state wording depends on it. */
    val answeredQuery: String = "",
    val rows: List<ContactRow> = emptyList(),
    /** An answer has come (from the core or from its cache) for the first time. */
    val loaded: Boolean = false,
    val refreshing: Boolean = false,
    /** The last try failed. What was loaded stays on screen. */
    val failed: Boolean = false,
    /** The server could not be reached: the rows are what the phone has. */
    val fromCache: Boolean = false,
)

/**
 * The contact list and its search. It lives as long as the activity, so that a visit to a contact or to the
 * other roots keeps the query and the place in the list. Only the newest question counts.
 */
class ContactsViewModel(app: Application) : AndroidViewModel(app) {
    private val stateFlow = MutableStateFlow(ContactsState())
    private var job: Job? = null

    /** Counts questions: an answer that is not from the newest one is dropped. */
    private var generation = 0

    val state: StateFlow<ContactsState> get() = stateFlow

    /** Kept here so that the place in the list survives a visit to another screen. */
    val listState = LazyListState()

    init {
        viewModelScope.launch {
            SessionState.signedIn.collect { signedIn ->
                if (signedIn == false) {
                    job?.cancel()
                    generation++
                    stateFlow.value = ContactsState()
                    listState.requestScrollToItem(0)
                }
            }
        }
    }

    /** Reads the list again for the query that is typed; called whenever the screen comes up. */
    fun load() = fetch(refreshing = false, debounce = false)

    /** Pull to refresh, and Retry. */
    fun refresh() {
        fetch(refreshing = true, debounce = false)
    }

    /** The text of the search field changed: ask 300 ms after the last key (at once when it was cleared). */
    fun onQueryChange(query: String) {
        stateFlow.update { it.copy(query = query) }
        fetch(refreshing = false, debounce = query.isNotBlank())
    }

    private fun fetch(refreshing: Boolean, debounce: Boolean) {
        val asked = ++generation
        val query = stateFlow.value.query
        job?.cancel()
        stateFlow.update { it.copy(refreshing = refreshing) }
        job = viewModelScope.launch {
            try {
                if (debounce) delay(ContactRules.DEBOUNCE_MILLIS)
                val list = withContext(Dispatchers.IO) { Background.core(getApplication()).contacts(query) }
                if (asked != generation) return@launch
                stateFlow.update {
                    it.copy(
                        answeredQuery = query,
                        rows = list.rows,
                        loaded = true,
                        refreshing = false,
                        failed = false,
                        fromCache = list.fromCache,
                    )
                }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "reading the contacts failed: ${e.javaClass.simpleName}")
                if (asked != generation) return@launch
                stateFlow.update { it.copy(refreshing = false, failed = true) }
            }
        }
    }
}
