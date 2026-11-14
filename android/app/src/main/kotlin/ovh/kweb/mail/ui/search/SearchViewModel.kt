package ovh.kweb.mail.ui.search

import android.app.Application
import android.util.Log
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
import ovh.kweb.mail.ui.mailextras.CoreMailExtrasBackend
import ovh.kweb.mail.ui.mailextras.LabelStore
import ovh.kweb.mail.ui.mailextras.SnippetController
import uniffi.kweb_core.MessageHeader

/** What the results area shows. */
sealed interface SearchResults {
    /** Nothing to search for yet: the query is empty or too short. */
    data object Idle : SearchResults

    data object Searching : SearchResults

    data class Found(val messages: List<MessageHeader>, val fromCache: Boolean) : SearchResults

    data object Failed : SearchResults
}

data class SearchState(val query: String = "", val results: SearchResults = SearchResults.Idle)

/** Searches the mail 400 ms after the last key, and only the newest query counts. */
class SearchViewModel(app: Application) : AndroidViewModel(app) {
    private val stateFlow = MutableStateFlow(SearchState())
    private var job: Job? = null

    val state: StateFlow<SearchState> get() = stateFlow

    private val extras = CoreMailExtrasBackend { Background.core(app) }

    /** Where the words were found, for the rows on screen. It never holds the list back: it arrives after it, or not at all. */
    val snippets = SnippetController(viewModelScope, Dispatchers.IO, extras)

    /** The labels of the rows on screen. */
    val labels = LabelStore(viewModelScope, Dispatchers.IO, extras, onError = {})

    init {
        viewModelScope.launch { SessionState.signedIn.collect { if (it == false) onQueryChange("") } }
        viewModelScope.launch { Background.cacheChanges.collect { refreshShown() } }
    }

    /** Forget the query and the results (the Search screen was left for good). */
    fun clear() = onQueryChange("")

    /** Brings the rows on screen up to date with the cache: read, flagged or moved mail changes, gone mail drops out. */
    private suspend fun refreshShown() {
        val shown = stateFlow.value.results as? SearchResults.Found ?: return
        try {
            val fresh = withContext(Dispatchers.IO) {
                val core = Background.core(getApplication())
                shown.messages.mapNotNull { core.message(it.id) }
            }
            labels.load(fresh.map { it.id })
            if (fresh == shown.messages) return
            // A new search may have replaced these results meanwhile.
            stateFlow.update { if (it.results === shown) it.copy(results = shown.copy(messages = fresh)) else it }
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            Log.w(Background.TAG, "refreshing the search results failed", e)
        }
    }

    fun onQueryChange(query: String) {
        job?.cancel()
        snippets.clear()
        if (!SearchRules.isSearchable(query)) {
            job = null
            stateFlow.value = SearchState(query)
            if (query.isEmpty()) labels.reset()
            return
        }
        stateFlow.value = SearchState(query, SearchResults.Searching)
        job = viewModelScope.launch {
            delay(SearchRules.DEBOUNCE_MILLIS)
            val results = try {
                // A search that was replaced meanwhile still finishes, but withContext sees the
                // cancellation when it returns, so its answer is never shown.
                val found = withContext(Dispatchers.IO) {
                    Background.core(getApplication()).search(query.trim(), null, SearchRules.LIMIT)
                }
                SearchResults.Found(found.messages, found.fromCache)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Log.w(Background.TAG, "search failed", e)
                SearchResults.Failed
            }
            var shown = false
            stateFlow.update {
                shown = it.query == query
                if (shown) it.copy(results = results) else it
            }
            if (shown && results is SearchResults.Found && results.messages.isNotEmpty()) {
                val ids = results.messages.map { it.id }
                labels.refreshLabels()
                labels.load(ids)
                // After the list is up, and only for the words that were searched.
                snippets.request(query.trim(), ids)
            }
        }
    }
}
