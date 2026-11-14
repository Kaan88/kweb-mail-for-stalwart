package ovh.kweb.mail.ui.shared

import android.app.Application
import android.content.Intent
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.core.net.toUri
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.LifecycleResumeEffect
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.Route
import ovh.kweb.mail.ui.calendar.NoticeBar
import ovh.kweb.mail.ui.calendar.rememberLocale
import ovh.kweb.mail.ui.contacts.ContactDetails
import ovh.kweb.mail.ui.contacts.ContactItem
import ovh.kweb.mail.ui.contacts.ContactRules
import ovh.kweb.mail.ui.contacts.LetterHeading
import ovh.kweb.mail.ui.contacts.SearchPill
import ovh.kweb.mail.ui.contacts.launch
import uniffi.kweb_core.Address

/** The contacts of the address books one account shares, with the search field of the own contacts. Read from the server every time. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SharedContactsScreen(route: Route.SharedContacts, onBack: () -> Unit, onGone: () -> Unit, onOpenContact: (contactId: String) -> Unit) {
    val app = LocalContext.current.applicationContext as Application
    val vm: SharedContactsViewModel = viewModel(factory = viewModelFactory { initializer { SharedContactsViewModel(app, route.account) } })
    val c = vm.controller
    val state by c.state.collectAsStateWithLifecycle()
    val snackbar = remember { SnackbarHostState() }
    val locale = rememberLocale()
    val entries = remember(state.rows, locale) { state.rows?.let { ContactRules.group(it, locale) } }
    CollectSharedEvents(c.events, snackbar, onGone = onGone)
    LifecycleResumeEffect(vm) {
        c.show()
        onPauseOrDispose { }
    }

    Scaffold(
        topBar = {
            Column(Modifier.background(MaterialTheme.colorScheme.background)) {
                SharedTopBar(route.owner, onBack)
                SearchPill(initial = state.query, onChange = c::onQueryChange)
            }
        },
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
                    entries == null && state.error != null -> item(key = "failed") {
                        SharedCentered { Note(stringResource(R.string.shared_contacts_failed) + "\n" + state.error!!.text(), onRetry = c::refresh) }
                    }
                    entries == null -> item(key = "loading") { SharedCentered { Loading() } }
                    entries.isEmpty() -> item(key = "empty") {
                        SharedCentered {
                            Note(stringResource(if (state.answeredQuery.isBlank()) R.string.shared_contacts_empty else R.string.shared_contacts_none))
                        }
                    }
                    else -> items(entries, key = { entryKey(route.account, it) }) { entry ->
                        when (entry) {
                            is ContactRules.Entry.Heading -> LetterHeading(entry.letter)
                            is ContactRules.Entry.Item -> ContactItem(entry, locale, onClick = { onOpenContact(entry.row.id) })
                        }
                    }
                }
            }
        }
    }
}

private fun entryKey(account: String, entry: ContactRules.Entry): String = when (entry) {
    is ContactRules.Entry.Heading -> SharedRules.key(account, "letter/${entry.letter}")
    is ContactRules.Entry.Item -> SharedRules.key(account, "contact/${entry.row.id}")
}

/**
 * One shared contact: the content of the contact screen, with nothing to edit or delete. A tap on a mail
 * address starts a new message of the user's own.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SharedContactScreen(route: Route.SharedContactView, onBack: () -> Unit, onWrite: (draftId: String, to: Address) -> Unit) {
    val context = LocalContext.current
    val app = context.applicationContext as Application
    val vm: SharedContactViewModel = viewModel(factory = viewModelFactory { initializer { SharedContactViewModel(app, route.account, route.contactId) } })
    val c = vm.controller
    val state by c.state.collectAsStateWithLifecycle()
    val problem by vm.problem.collectAsStateWithLifecycle()
    val snackbar = remember { SnackbarHostState() }
    CollectSharedEvents(c.events, snackbar, onGone = {})
    LifecycleResumeEffect(vm) {
        c.show()
        onPauseOrDispose { }
    }
    LaunchedEffect(vm) { vm.draftStarted.collect { onWrite(it.draftId, it.to) } }
    val detail = state.value

    Scaffold(
        topBar = { SharedTopBar("", onBack) },
        snackbarHost = { SnackbarHost(snackbar) },
        containerColor = MaterialTheme.colorScheme.background,
    ) { padding ->
        Column(Modifier.padding(padding).fillMaxSize()) {
            problem?.let { NoticeBar(it.text(), error = true, action = stringResource(R.string.contacts_dismiss) to vm::dismissProblem) }
            when {
                detail != null -> ContactDetails(
                    detail,
                    recent = emptyList(),
                    onWrite = vm::write,
                    onDial = { launch(context, Intent(Intent.ACTION_DIAL, ContactRules.dialUri(it).toUri()), R.string.contact_no_dialler) },
                    onMap = { launch(context, Intent(Intent.ACTION_VIEW, ContactRules.geoUri(it).toUri()), R.string.contact_no_map) },
                    onOpenMail = { _, _ -> },
                )
                state.error != null -> SharedCenteredBox {
                    Note(stringResource(R.string.shared_contact_failed) + "\n" + state.error!!.text(), onRetry = c::refresh)
                }
                else -> SharedCenteredBox { Loading() }
            }
        }
    }
}
