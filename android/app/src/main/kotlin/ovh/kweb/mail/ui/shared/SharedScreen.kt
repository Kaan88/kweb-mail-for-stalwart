package ovh.kweb.mail.ui.shared

import android.app.Application
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
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
import ovh.kweb.mail.ui.list.Groups
import uniffi.kweb_core.SharedAccount

/**
 * What other users share with this one: each account as a group headed by the owner's address, with a row for
 * each kind it shares. Read from the server every time it comes up; nothing here is kept on the phone.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SharedScreen(
    onBack: () -> Unit,
    onOpenMail: (SharedAccount) -> Unit,
    onOpenCalendar: (SharedAccount) -> Unit,
    onOpenContacts: (SharedAccount) -> Unit,
    vm: SharedAccountsViewModel = viewModel(),
) {
    val c = vm.controller
    val state by c.state.collectAsStateWithLifecycle()
    val snackbar = remember { SnackbarHostState() }
    CollectSharedEvents(c.events, snackbar, onGone = {})
    LifecycleResumeEffect(vm) {
        c.show()
        onPauseOrDispose { }
    }

    Scaffold(
        topBar = { SharedTopBar(stringResource(R.string.shared_title), onBack) },
        snackbarHost = { SnackbarHost(snackbar) },
        containerColor = MaterialTheme.colorScheme.background,
    ) { padding ->
        PullToRefreshBox(
            isRefreshing = state.refreshing,
            onRefresh = c::refresh,
            modifier = Modifier.padding(padding).fillMaxSize(),
        ) {
            val accounts = state.value
            LazyColumn(Modifier.fillMaxSize(), state = vm.listState, contentPadding = PaddingValues(bottom = 24.dp)) {
                when {
                    accounts == null && state.error != null -> item(key = "failed") {
                        SharedCentered { Note(stringResource(R.string.shared_load_failed) + "\n" + state.error!!.text(), onRetry = c::refresh) }
                    }
                    accounts == null -> item(key = "loading") { SharedCentered { Loading() } }
                    accounts.isEmpty() -> item(key = "empty") {
                        SharedCentered {
                            Note(stringResource(R.string.shared_empty) + "\n" + stringResource(R.string.shared_empty_hint))
                        }
                    }
                    else -> accounts.forEach { account ->
                        val kinds = SharedRules.kinds(account)
                        item(key = "head/${account.accountId}") { GroupHeading(account.name) }
                        items(kinds, key = { kind -> SharedRules.key(account.accountId, kind.name) }) { kind ->
                            val index = kinds.indexOf(kind)
                            val shape = Groups.shape(index, kinds.size)
                            when (kind) {
                                SharedRules.Kind.MAIL -> SharedRow(
                                    shape, stringResource(R.string.shared_kind_mail), onClick = { onOpenMail(account) }, icon = R.drawable.ic_mail,
                                )
                                SharedRules.Kind.CALENDAR -> SharedRow(
                                    shape, stringResource(R.string.shared_kind_calendar), onClick = { onOpenCalendar(account) }, icon = R.drawable.ic_calendar,
                                )
                                SharedRules.Kind.CONTACTS -> SharedRow(
                                    shape, stringResource(R.string.shared_kind_contacts), onClick = { onOpenContacts(account) }, icon = R.drawable.ic_person,
                                )
                            }
                        }
                    }
                }
            }
        }
    }
}

/** The folders one account shares, as a tree where a parent is shared too, with the unread count of each. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SharedFoldersScreen(route: Route.SharedFolders, onBack: () -> Unit, onGone: () -> Unit, onOpenFolder: (mailboxId: String) -> Unit) {
    val app = LocalContext.current.applicationContext as Application
    val vm: SharedMailboxesViewModel = viewModel(factory = viewModelFactory { initializer { SharedMailboxesViewModel(app, route.account) } })
    val c = vm.controller
    val state by c.state.collectAsStateWithLifecycle()
    val snackbar = remember { SnackbarHostState() }
    CollectSharedEvents(c.events, snackbar, onGone = onGone)
    LifecycleResumeEffect(vm) {
        c.show()
        onPauseOrDispose { }
    }
    val folders = remember(state.value) { state.value?.let { SharedRules.tree(it) } }

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
            LazyColumn(Modifier.fillMaxSize(), state = vm.listState, contentPadding = PaddingValues(top = 4.dp, bottom = 24.dp)) {
                when {
                    folders == null && state.error != null -> item(key = "failed") {
                        SharedCentered { Note(stringResource(R.string.shared_folders_failed) + "\n" + state.error!!.text(), onRetry = c::refresh) }
                    }
                    folders == null -> item(key = "loading") { SharedCentered { Loading() } }
                    folders.isEmpty() -> item(key = "empty") { SharedCentered { Note(stringResource(R.string.shared_folders_empty)) } }
                    else -> items(folders, key = { SharedRules.key(route.account, it.mailbox.id) }) { entry ->
                        val unread = entry.mailbox.unread.toInt()
                        SharedRow(
                            shape = Groups.shape(folders.indexOf(entry), folders.size),
                            title = entry.mailbox.name,
                            onClick = { onOpenFolder(entry.mailbox.id) },
                            icon = R.drawable.ic_folder,
                            count = if (unread > 0) unread.toString() else null,
                            countDescription = if (unread > 0) pluralStringResource(R.plurals.unread_count, unread, unread) else null,
                            indentDp = 16 * entry.depth,
                        )
                    }
                }
            }
        }
    }
}
