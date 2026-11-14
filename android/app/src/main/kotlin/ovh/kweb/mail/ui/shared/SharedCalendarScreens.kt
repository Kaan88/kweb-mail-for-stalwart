package ovh.kweb.mail.ui.shared

import android.app.Application
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.LifecycleResumeEffect
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.Route
import ovh.kweb.mail.ui.calendar.AgendaRules
import ovh.kweb.mail.ui.calendar.DayHeading
import ovh.kweb.mail.ui.calendar.EntryCard
import ovh.kweb.mail.ui.calendar.EventDetails
import ovh.kweb.mail.ui.calendar.EventPhase
import ovh.kweb.mail.ui.calendar.EventState
import ovh.kweb.mail.ui.calendar.rememberLocale
import ovh.kweb.mail.ui.calendar.rememberWords

/**
 * The events of the calendars one account shares: the next 30 days drawn with the agenda's rows, grouped by
 * day, with "Load 30 more days" at the end. Read from the server every time; without a connection it fails.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SharedCalendarScreen(
    route: Route.SharedCalendar,
    onBack: () -> Unit,
    onGone: () -> Unit,
    onOpenEvent: (eventId: String, recurrenceId: String?) -> Unit,
) {
    val app = LocalContext.current.applicationContext as Application
    val vm: SharedAgendaViewModel = viewModel(factory = viewModelFactory { initializer { SharedAgendaViewModel(app, route.account) } })
    val c = vm.controller
    val state by c.state.collectAsStateWithLifecycle()
    val snackbar = remember { SnackbarHostState() }
    val locale = rememberLocale()
    val words = rememberWords()
    CollectSharedEvents(c.events, snackbar, onGone = onGone)
    LifecycleResumeEffect(vm) {
        c.show()
        onPauseOrDispose { }
    }

    Scaffold(
        topBar = { SharedTopBar(route.owner, onBack) },
        snackbarHost = { SnackbarHost(snackbar) },
        containerColor = MaterialTheme.colorScheme.background,
    ) { padding ->
        PullToRefreshBox(
            isRefreshing = state.refreshing,
            onRefresh = c::refresh,
            modifier = Modifier.padding(padding).fillMaxSize(),
        ) {
            LazyColumn(Modifier.fillMaxSize(), state = vm.listState, contentPadding = PaddingValues(bottom = 24.dp)) {
                when {
                    !state.loaded && state.error != null -> item(key = "failed") {
                        SharedCentered { Note(stringResource(R.string.shared_agenda_failed) + "\n" + state.error!!.text(), onRetry = c::refresh) }
                    }
                    !state.loaded -> item(key = "loading") { SharedCentered { Loading() } }
                    else -> {
                        if (state.rows.isEmpty()) {
                            item(key = "empty") {
                                Box(Modifier.fillMaxWidth().padding(32.dp), contentAlignment = Alignment.Center) {
                                    Column(horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(4.dp)) {
                                        Note(pluralStringResource(R.plurals.shared_agenda_empty, state.window.days.toInt(), state.window.days.toInt()))
                                    }
                                }
                            }
                        }
                        items(state.rows, key = { row -> rowKey(route.account, row) }) { row ->
                            when (row) {
                                is AgendaRules.Row.Header -> DayHeading(
                                    AgendaRules.heading(row.date, state.today, locale, stringResource(R.string.cal_today)).uppercase(locale),
                                )
                                is AgendaRules.Row.Entry -> EntryCard(
                                    entry = row.occurrence,
                                    continues = row.continues,
                                    tone = AgendaRules.tone(row.occurrence.calendarId, state.calendarIds),
                                    zone = state.zone,
                                    locale = locale,
                                    words = words,
                                    onOpen = { onOpenEvent(row.occurrence.eventId, row.occurrence.recurrenceId) },
                                    onToggleDone = null,
                                )
                            }
                        }
                        if (state.canLoadMore) {
                            item(key = "more") {
                                Box(Modifier.fillMaxWidth().padding(8.dp), contentAlignment = Alignment.Center) {
                                    if (state.loadingMore) {
                                        CircularProgressIndicator(Modifier.size(28.dp), strokeWidth = 3.dp)
                                    } else {
                                        TextButton(onClick = c::loadMore) { Text(stringResource(R.string.shared_load_more_days)) }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/** A row's key: the account is part of it, since event ids are only unique within one. */
private fun rowKey(account: String, row: AgendaRules.Row): String = when (row) {
    is AgendaRules.Row.Header -> SharedRules.key(account, "day/${row.date}")
    is AgendaRules.Row.Entry -> SharedRules.key(account, "entry/${row.key}")
}

/** One event of a shared calendar: the content of the event screen, with nothing to edit, delete or answer. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SharedEventScreen(route: Route.SharedEventView, onBack: () -> Unit) {
    val app = LocalContext.current.applicationContext as Application
    val vm: SharedEventViewModel = viewModel(
        factory = viewModelFactory { initializer { SharedEventViewModel(app, route.account, route.eventId, route.recurrenceId) } },
    )
    val c = vm.controller
    val state by c.state.collectAsStateWithLifecycle()
    val snackbar = remember { SnackbarHostState() }
    CollectSharedEvents(c.events, snackbar, onGone = {})
    LifecycleResumeEffect(vm) {
        c.show()
        onPauseOrDispose { }
    }
    val data = state.value

    Scaffold(
        topBar = { SharedTopBar("", onBack) },
        snackbarHost = { SnackbarHost(snackbar) },
        containerColor = MaterialTheme.colorScheme.background,
    ) { padding ->
        Column(Modifier.padding(padding).fillMaxSize()) {
            when {
                data != null -> EventDetails(
                    detail = data.detail,
                    recurrenceId = route.recurrenceId,
                    state = EventState(phase = EventPhase.READY, detail = data.detail, calendarName = data.calendarName),
                    onAnswer = {},
                    readOnly = true,
                )
                state.error != null -> SharedCenteredBox {
                    Note(stringResource(R.string.shared_event_failed) + "\n" + state.error!!.text(), onRetry = c::refresh)
                }
                else -> SharedCenteredBox { Loading() }
            }
        }
    }
}
