package ovh.kweb.mail.ui.shared

import ovh.kweb.mail.ui.accounts.LocalAccountSession
import android.app.Application
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
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
import ovh.kweb.mail.ui.list.MailRow
import ovh.kweb.mail.ui.list.Preview
import ovh.kweb.mail.ui.list.RowContent
import ovh.kweb.mail.ui.list.Sender
import uniffi.kweb_core.MessageHeader

/** What a row of the own mail list draws, for a message of a shared folder. */
@Composable
internal fun sharedRowContent(m: MessageHeader): RowContent {
    val unknown = stringResource(R.string.list_unknown_sender)
    return RowContent(
        sender = Sender.label(m.fromName, m.fromEmail) ?: unknown,
        initial = Sender.initial(m.fromName, m.fromEmail),
        subject = m.subject.trim(),
        preview = Preview.oneLine(m.preview),
        receivedAt = m.receivedAt,
        unread = m.isUnread,
        flagged = m.isFlagged,
        hasAttachment = m.hasAttachment,
    )
}

/**
 * The messages of one shared folder, newest first, drawn with the row of the own mail list. A page of 50 at
 * a time with "Load more" at the end. Under the title, in a folder where marks may be set, one line says the
 * marks are shared with the owner. The list reads itself again whenever the screen comes up, so what was done to a message
 * is shown when the user comes back from it.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SharedFolderScreen(route: Route.SharedFolder, onBack: () -> Unit, onGone: () -> Unit, onOpenMessage: (messageId: String) -> Unit) {
    val app = LocalContext.current.applicationContext as Application
    val accountSession = LocalAccountSession.current
    val vm: SharedFolderViewModel = viewModel(factory = viewModelFactory { initializer { SharedFolderViewModel(app, accountSession, route.account, route.mailboxId) } })
    val c = vm.controller
    val state by c.state.collectAsStateWithLifecycle()
    val snackbar = remember { SnackbarHostState() }
    CollectSharedEvents(c.events, snackbar, onGone = onGone)
    LifecycleResumeEffect(vm) {
        c.show()
        onPauseOrDispose { }
    }
    val folder = state.folder

    Scaffold(
        topBar = { SharedTopBar(folder?.name.orEmpty(), onBack) },
        snackbarHost = { SnackbarHost(snackbar) },
        containerColor = MaterialTheme.colorScheme.background,
    ) { padding ->
        PullToRefreshBox(
            isRefreshing = state.refreshing,
            onRefresh = c::refresh,
            modifier = Modifier.padding(padding).fillMaxSize(),
        ) {
            LazyColumn(Modifier.fillMaxSize(), state = vm.listState, contentPadding = PaddingValues(bottom = 24.dp)) {
                if (folder != null && folder.mayMark) {
                    item(key = "note") {
                        Text(
                            stringResource(R.string.shared_marks_note),
                            modifier = Modifier.fillMaxWidth().padding(start = 20.dp, end = 20.dp, bottom = 8.dp),
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            style = MaterialTheme.typography.bodyMedium,
                        )
                    }
                }
                when {
                    !state.loaded && state.error != null -> item(key = "failed") {
                        SharedCentered { Note(stringResource(R.string.shared_folder_failed) + "\n" + state.error!!.text(), onRetry = c::refresh) }
                    }
                    !state.loaded -> item(key = "loading") { SharedCentered { Loading() } }
                    state.messages.isEmpty() -> item(key = "empty") { SharedCentered { Note(stringResource(R.string.shared_folder_empty)) } }
                    else -> {
                        itemsIndexed(state.messages, key = { _, m -> SharedRules.key(route.account, m.id) }) { index, message ->
                            Box(Modifier.padding(horizontal = 12.dp).padding(top = if (index == 0) 0.dp else 3.dp)) {
                                MailRow(
                                    sharedRowContent(message),
                                    onClick = { onOpenMessage(message.id) },
                                    shape = Groups.shape(index, state.messages.size),
                                )
                            }
                        }
                        if (state.hasMore) {
                            item(key = "more") {
                                Box(Modifier.fillMaxWidth().padding(8.dp), contentAlignment = Alignment.Center) {
                                    if (state.loadingMore) {
                                        CircularProgressIndicator(Modifier.size(28.dp), strokeWidth = 3.dp)
                                    } else {
                                        TextButton(onClick = c::loadMore) {
                                            Text(stringResource(if (state.moreFailed) R.string.shared_load_more_failed else R.string.shared_load_more))
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
}
