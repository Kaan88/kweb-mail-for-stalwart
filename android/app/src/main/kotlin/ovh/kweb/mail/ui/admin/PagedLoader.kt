package ovh.kweb.mail.ui.admin

import androidx.annotation.StringRes
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import ovh.kweb.mail.ui.UiMessage

enum class ListPhase { LOADING, READY, FAILED }

/**
 * One page the core gave: [total] is null when the server does not say how many there are. [tag] is what the page was
 * asked with (a search text, say): the loader keeps the tag of the first page on screen and gives it back to the
 * fetch of the next pages, so a list is never continued with another question.
 */
data class Page<T>(val items: List<T>, val total: Int?, val tag: Any? = null)

data class PagedState<T>(
    val phase: ListPhase = ListPhase.LOADING,
    val items: List<T> = emptyList(),
    val total: Int? = null,
    /** A page is being added at the end. */
    val loadingMore: Boolean = false,
    /** The list is being read again while the old one is shown (pull down, coming back). */
    val refreshing: Boolean = false,
    val more: Boolean = false,
    /** The [Page.tag] of the first page of the list on screen. */
    val tag: Any? = null,
    val error: UiMessage? = null,
)

/**
 * A list the core gives in pages, as all the admin lists are. Calls go off the main thread; an answer that a newer
 * read has overtaken is dropped; "load more" is refused while a read is running. A list on screen stays when a
 * later read fails (the error is shown above it). The server's lists move between two reads (a message is queued, an
 * address is blocked), so a row of a later page that is already on screen is not added again ([key] names a row): a
 * repeated key would close a LazyColumn.
 */
class PagedLoader<T>(
    private val scope: CoroutineScope,
    private val io: CoroutineDispatcher,
    private val pageSize: UInt,
    @StringRes private val failed: Int,
    private val key: (T) -> Any,
    /** [tag] is the tag of the first page on screen (null for the first page itself). */
    private val fetch: (offset: UInt, limit: UInt, tag: Any?) -> Page<T>,
) {
    private val stateFlow = MutableStateFlow(PagedState<T>())
    private var ticket = 0

    /** How many rows the server gave so far: the next page starts there, also when some were repeats. */
    private var fetched = 0

    val state: StateFlow<PagedState<T>> get() = stateFlow

    /** Reads the first page. [quiet]: the list on screen stays until the answer comes. */
    fun reload(quiet: Boolean = true) {
        val mine = ++ticket
        stateFlow.update {
            if (quiet && it.phase == ListPhase.READY) it.copy(refreshing = true, loadingMore = false) else PagedState(phase = ListPhase.LOADING)
        }
        scope.launch {
            try {
                val page = withContext(io) { fetch(0u, pageSize, null) }
                if (mine != ticket) return@launch
                val items = page.items.distinctBy(key)
                fetched = page.items.size
                stateFlow.value = PagedState(ListPhase.READY, items, page.total, more = hasMore(page, fetched), tag = page.tag)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                if (mine != ticket) return@launch
                stateFlow.update {
                    if (it.phase == ListPhase.READY) it.copy(refreshing = false, error = UiMessage.of(e, failed)) else PagedState(phase = ListPhase.FAILED, error = UiMessage.of(e, failed))
                }
            }
        }
    }

    /** First read of a screen, a re-read when it comes back. */
    fun refresh() = reload(quiet = true)

    fun loadMore() {
        val s = stateFlow.value
        if (s.phase != ListPhase.READY || !s.more || s.loadingMore || s.refreshing) return
        val mine = ticket
        val offset = fetched.toUInt()
        val tag = s.tag
        stateFlow.update { it.copy(loadingMore = true) }
        scope.launch {
            try {
                val page = withContext(io) { fetch(offset, pageSize, tag) }
                if (mine != ticket) return@launch
                fetched += page.items.size
                stateFlow.update {
                    val known = it.items.mapTo(HashSet(), key)
                    val items = it.items + page.items.filter { row -> known.add(key(row)) }
                    it.copy(loadingMore = false, items = items, total = page.total ?: it.total, more = hasMore(page, fetched))
                }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                if (mine != ticket) return@launch
                stateFlow.update { it.copy(loadingMore = false, error = UiMessage.of(e, failed)) }
            }
        }
    }

    /** A change made on the server and mirrored here without reading the list again. */
    fun change(edit: (List<T>) -> List<T>) = stateFlow.update {
        val items = edit(it.items)
        fetched = maxOf(0, fetched - (it.items.size - items.size))
        it.copy(items = items, total = it.total?.let { t -> maxOf(0, t - (it.items.size - items.size)) })
    }

    fun dismissError() = stateFlow.update { it.copy(error = null) }

    private fun hasMore(page: Page<T>, count: Int): Boolean = page.total?.let { count < it } ?: (page.items.size.toUInt() >= pageSize)
}
