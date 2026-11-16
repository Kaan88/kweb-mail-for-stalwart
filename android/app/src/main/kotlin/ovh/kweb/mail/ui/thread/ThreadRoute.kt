package ovh.kweb.mail.ui.thread

import android.app.Application
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.pager.HorizontalPager
import androidx.compose.foundation.pager.rememberPagerState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.CustomAccessibilityAction
import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.ViewModelStore
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.filterNotNull
import kotlinx.coroutines.flow.flowOf
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.launch
import ovh.kweb.mail.R
import ovh.kweb.mail.accounts.AccountSession
import ovh.kweb.mail.ui.AppViewModel
import ovh.kweb.mail.ui.Route
import ovh.kweb.mail.ui.accounts.LocalAccountSession
import ovh.kweb.mail.ui.calendar.EventPrefill
import ovh.kweb.mail.ui.list.MailListViewModel
import ovh.kweb.mail.ui.search.SearchResults
import ovh.kweb.mail.ui.search.SearchViewModel
import ovh.kweb.mail.ui.thread.ThreadSwipeRules.Listing
import ovh.kweb.mail.ui.thread.ThreadSwipeRules.Source

/**
 * What the pager of one thread route keeps: the order of the conversations it walked, which one is the shown one, and one view
 * model per conversation that is loaded now or was a short while ago. It lives in the route's own store, so it survives a
 * rotation and goes when the route leaves the back stack; every view model it made is cleared with it. Pages far from the
 * shown one lose theirs ([trim]), so a long walk through a folder does not pile them up.
 */
class ThreadPagerViewModel(private val application: Application, private val acct: AccountSession, first: String) : ViewModel() {
    /** The order walked so far, and the conversations visited (see [ThreadSwipeRules.order]). */
    val order = ThreadOrderMemory()

    /** The conversation the pager has settled on: the only one that marks mail read or closes the screen. */
    var current by mutableStateOf(first)

    private val stores = HashMap<String, ViewModelStore>()

    /** The view model of the conversation [route] is for; the same one for as long as it is kept. */
    fun threadModel(route: Route.Thread): ThreadViewModel {
        val store = stores.getOrPut(route.threadId) { ViewModelStore() }
        val factory = object : ViewModelProvider.Factory {
            @Suppress("UNCHECKED_CAST")
            override fun <T : ViewModel> create(modelClass: Class<T>): T = ThreadViewModel(application, acct, route) as T
        }
        return ViewModelProvider(store, factory)[ThreadViewModel::class.java]
    }

    /** Clears the view models of every conversation but [keep]. */
    fun trim(keep: Set<String>) {
        for (id in stores.keys.filter { it !in keep }) stores.remove(id)?.clear()
    }

    override fun onCleared() {
        stores.values.forEach { it.clear() }
        stores.clear()
    }
}

/**
 * The thread screen as a pager over the conversations of the list it was opened from (the mail list or the search; any other
 * screen gives a single page). The finger drags the neighbouring conversation in; each page is a whole [ThreadScreen] of its own
 * conversation, so its bar slides with it. Only the page the pager settled on counts as opened: its neighbours are loaded but
 * mark nothing read. Back leaves the route (the list under it), not the previous conversation.
 */
@Composable
fun ThreadRoute(
    app: AppViewModel,
    route: Route.Thread,
    stack: List<Route>,
    mailList: MailListViewModel,
    search: SearchViewModel,
    onBack: () -> Unit,
    onCompose: (draftId: String) -> Unit,
    onFilterLike: (messageId: String) -> Unit,
    onOpenFilters: () -> Unit,
    onAddToCalendar: (EventPrefill) -> Unit,
    onAskAssistant: (threadId: String) -> Unit = {},
    /** Where the sparkle menu's service entries are wired (phase 2). */
    onService: (uniffi.kweb_core.Finding) -> Unit = {},
) {
    val acct = LocalAccountSession.current
    val application = LocalContext.current.applicationContext as Application
    val holder: ThreadPagerViewModel = viewModel(
        viewModelStoreOwner = app.stores.owner(route),
        key = "thread-pager",
        factory = viewModelFactory { initializer { ThreadPagerViewModel(application, acct, route.threadId) } },
    )
    val source = ThreadSwipeRules.sourceOf(stack)
    // Only the ids and what can still be loaded are watched, not every change of the list's state.
    val listing by remember(source, mailList, search) { listingFlow(source, mailList, search) }
        .collectAsStateWithLifecycle(remember(source, mailList, search) { listingNow(source, mailList, search) })
    val memory = holder.order
    val current = holder.current
    // What was walked so far brought up to date with the list. The page keys are these ids, so a list that grows or changes
    // under the pager does not move the page the reader is on.
    val shown = remember(listing.ids, current, source) {
        if (source == null) listOf(current) else ThreadSwipeRules.order(memory.snapshot, listing.ids, memory.visited + current)
    }
    val pages = ThreadSwipeRules.pages(shown, current)
    LaunchedEffect(pages, current, source) {
        if (source != null) memory.remember(pages, current)
    }
    val pagesNow by rememberUpdatedState(pages)

    val pagerState = rememberPagerState(initialPage = pages.indexOf(current).coerceAtLeast(0)) { pages.size }
    LaunchedEffect(pagerState) {
        // The key of the page the pager is settled on, read from the layout (an index could be one list change out of date).
        snapshotFlow { pagerState.layoutInfo.visiblePagesInfo.firstOrNull { it.index == pagerState.settledPage }?.key as? String }
            .filterNotNull()
            .distinctUntilChanged()
            .collect { id ->
                holder.current = id
                holder.trim(ThreadSwipeRules.keep(pagesNow, id))
                // Back shows the list with the conversation last read in view.
                if (source == Source.LIST) mailList.reveal(id)
            }
    }

    // At the end of the loaded list: more of the cache, then older mail from the server, so the pager can go on.
    val scope = rememberCoroutineScope()
    val settledIndex = pages.indexOf(current)
    var loading by remember { mutableStateOf(false) }
    var stuckAt by remember { mutableIntStateOf(-1) }
    LaunchedEffect(settledIndex, pages.size, listing.more, loading, source) {
        if (source != Source.LIST || loading || stuckAt == pages.size) return@LaunchedEffect
        if (!ThreadSwipeRules.wantsMore(settledIndex, pages.size, listing.more)) return@LaunchedEffect
        loading = true
        val size = pages.size
        scope.launch {
            try {
                // Not cancelled by the page changing meanwhile: the list's own state is what changes.
                if (!mailList.loadMoreForSwipe()) stuckAt = size
            } finally {
                loading = false
            }
        }
    }

    val previousLabel = stringResource(R.string.thread_previous)
    val nextLabel = stringResource(R.string.thread_next)
    HorizontalPager(
        state = pagerState,
        modifier = Modifier.fillMaxSize(),
        beyondViewportPageCount = 1,
        userScrollEnabled = pages.size > 1,
        key = { index -> pagesNow.getOrElse(index) { "gone-$index" } },
    ) { index ->
        val id = pagesNow.getOrNull(index) ?: return@HorizontalPager
        val pageRoute = if (id == route.threadId) route else Route.Thread(id, listing.folderOf(id) ?: route.mailboxId, null)
        val vm = remember(id, holder) { holder.threadModel(pageRoute) }
        // Stepping for those who cannot drag: the actions TalkBack offers on the subject.
        val steps = buildList {
            ThreadSwipeRules.neighbour(index, pages.size, forward = false)?.let { to ->
                add(CustomAccessibilityAction(previousLabel) { scope.launch { pagerState.animateScrollToPage(to) }; true })
            }
            ThreadSwipeRules.neighbour(index, pages.size, forward = true)?.let { to ->
                add(CustomAccessibilityAction(nextLabel) { scope.launch { pagerState.animateScrollToPage(to) }; true })
            }
        }
        ThreadScreen(
            vm = vm,
            route = pageRoute,
            active = id == current,
            onBack = onBack,
            onClosed = {
                // The user's choice for after Archive or Delete: back to the list, or on to the next or the previous conversation.
                val to = ovh.kweb.mail.ui.ReadingRules.pageAfter(ovh.kweb.mail.ui.UiPrefs(application).afterAction, index, pagesNow.size)
                if (to == null) onBack() else scope.launch { pagerState.animateScrollToPage(to) }
            },
            onCompose = onCompose,
            onFilterLike = onFilterLike,
            onOpenFilters = onOpenFilters,
            onAddToCalendar = onAddToCalendar,
            onAskAssistant = onAskAssistant,
            stepActions = steps,
            onService = onService,
        )
    }
}

private fun listingFlow(source: Source?, mailList: MailListViewModel, search: SearchViewModel): Flow<Listing> = when (source) {
    Source.LIST -> mailList.state.map { ThreadSwipeRules.listing(it) }
    Source.SEARCH -> search.state.map { searchListing(it.results) }
    null -> flowOf(Listing.EMPTY)
}.distinctUntilChanged()

private fun listingNow(source: Source?, mailList: MailListViewModel, search: SearchViewModel): Listing = when (source) {
    Source.LIST -> ThreadSwipeRules.listing(mailList.state.value)
    Source.SEARCH -> searchListing(search.state.value.results)
    null -> Listing.EMPTY
}

private fun searchListing(results: SearchResults): Listing =
    (results as? SearchResults.Found)?.let { found -> ThreadSwipeRules.searchListing(found.messages.map { it.threadId }) } ?: Listing.EMPTY
