package ovh.kweb.mail.ui.inbox

import android.os.Build
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.animation.animateColorAsState
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Notifications
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.onLongClick
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.core.content.ContextCompat
import androidx.lifecycle.compose.LifecycleResumeEffect
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import java.time.ZoneId
import java.util.Locale
import kotlinx.coroutines.delay
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.FocusRequest
import ovh.kweb.mail.ui.UiPrefs
import ovh.kweb.mail.ui.delivery.NotificationFix

/**
 * The Inbox of milestone M2: the newest messages from the cache, with the actions a long press
 * offers. Rows do not open anything yet; a notification tap scrolls to its message instead.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun InboxScreen(
    focus: FocusRequest?,
    onFocusHandled: (FocusRequest) -> Unit,
    onOpenDeliveryCheck: () -> Unit,
    vm: InboxViewModel = viewModel(),
) {
    val state by vm.state.collectAsStateWithLifecycle()
    val listState = rememberLazyListState()
    val snackbar = remember { SnackbarHostState() }
    val resources = LocalResources.current
    var highlighted by remember { mutableStateOf<String?>(null) }

    LifecycleResumeEffect(vm) {
        vm.reload()
        onPauseOrDispose { }
    }
    AskForNotificationsOnce()
    LaunchedEffect(vm) { vm.messages.collect { snackbar.showSnackbar(resources.getString(it)) } }

    // Scroll to the mail of a tapped notification as soon as the cache has it. It is not marked read.
    LaunchedEffect(focus, state.rows, state.loaded) {
        if (focus == null) return@LaunchedEffect
        val index = state.rows.indexOfFirst { it.id == focus.messageId }
        if (index >= 0) {
            listState.animateScrollToItem(index)
            highlighted = focus.messageId
            onFocusHandled(focus)
        } else if (state.loaded) {
            onFocusHandled(focus)
        }
    }
    LaunchedEffect(highlighted) {
        if (highlighted != null) {
            delay(HIGHLIGHT_MILLIS)
            highlighted = null
        }
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Title(state.unread) },
                actions = {
                    IconButton(onClick = onOpenDeliveryCheck) {
                        Icon(Icons.Filled.Notifications, contentDescription = stringResource(R.string.delivery_check_title))
                    }
                },
                colors = TopAppBarDefaults.topAppBarColors(containerColor = MaterialTheme.colorScheme.background),
            )
        },
        snackbarHost = { SnackbarHost(snackbar) },
    ) { padding ->
        PullToRefreshBox(
            isRefreshing = state.refreshing,
            onRefresh = vm::refresh,
            modifier = Modifier.padding(padding).fillMaxSize(),
        ) {
            LazyColumn(Modifier.fillMaxSize(), state = listState) {
                if (state.rows.isEmpty()) {
                    item { EmptyState(state, Modifier.fillParentMaxSize()) }
                }
                items(state.rows, key = { it.id }) { row ->
                    MailRowItem(
                        row = row,
                        highlighted = row.id == highlighted,
                        onToggleRead = { vm.toggleRead(row) },
                        onArchive = { vm.archive(row) },
                    )
                    HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
                }
            }
        }
    }
}

@Composable
private fun Title(unread: Int) {
    Row(verticalAlignment = Alignment.CenterVertically) {
        Text(stringResource(R.string.inbox_title))
        if (unread > 0) {
            val description = pluralStringResource(R.plurals.unread_count, unread, unread)
            Text(
                unread.toString(),
                modifier = Modifier
                    .padding(start = 10.dp)
                    .clip(CircleShape)
                    .background(MaterialTheme.colorScheme.primary)
                    .padding(horizontal = 8.dp, vertical = 2.dp)
                    .semantics { contentDescription = description },
                color = MaterialTheme.colorScheme.onPrimary,
                style = MaterialTheme.typography.labelMedium,
                fontWeight = FontWeight.Bold,
            )
        }
    }
}

@Composable
private fun EmptyState(state: InboxState, modifier: Modifier) {
    Column(
        modifier.padding(32.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        Text(stringResource(R.string.inbox_empty), style = MaterialTheme.typography.titleMedium)
        // Until the cache has seen the server once, "empty" only means "not here yet".
        if (state.loaded && !state.synced) {
            Spacer(Modifier.size(8.dp))
            Text(
                stringResource(R.string.inbox_first_sync),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

/** One row of the mock-up. A long press opens the menu; there is no tap action yet. */
@Composable
private fun MailRowItem(row: MailRow, highlighted: Boolean, onToggleRead: () -> Unit, onArchive: () -> Unit) {
    var menuOpen by remember { mutableStateOf(false) }
    val background by animateColorAsState(
        if (highlighted) MaterialTheme.colorScheme.primaryContainer else Color.Transparent,
        label = "highlight",
    )
    val menuLabel = stringResource(R.string.inbox_row_actions)
    Box {
        Row(
            Modifier
                .fillMaxWidth()
                .background(background)
                .pointerInput(Unit) { detectTapGestures(onLongPress = { menuOpen = true }) }
                .semantics(mergeDescendants = true) { onLongClick(label = menuLabel) { menuOpen = true; true } }
                .padding(horizontal = 14.dp, vertical = 10.dp),
            horizontalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Avatar(row.initial)
            RowText(row, Modifier.weight(1f))
        }
        RowMenu(menuOpen, row, onDismiss = { menuOpen = false }, onToggleRead = onToggleRead, onArchive = onArchive)
    }
}

@Composable
private fun Avatar(initial: String) {
    Box(
        Modifier.size(40.dp).clip(CircleShape).background(MaterialTheme.colorScheme.primaryContainer),
        contentAlignment = Alignment.Center,
    ) {
        Text(
            initial,
            color = MaterialTheme.colorScheme.onPrimaryContainer,
            fontWeight = FontWeight.Bold,
            fontSize = 15.sp,
        )
    }
}

@Composable
private fun RowText(row: MailRow, modifier: Modifier) {
    val weight = if (row.unread) FontWeight.Bold else FontWeight.Normal
    val muted = MaterialTheme.colorScheme.onSurfaceVariant
    val yesterday = stringResource(R.string.inbox_yesterday)
    val time = remember(row.receivedAt, yesterday) {
        RowTime.format(row.receivedAt, System.currentTimeMillis(), ZoneId.systemDefault(), Locale.getDefault(), yesterday)
    }
    Column(modifier) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            if (row.unread) {
                Box(Modifier.size(8.dp).clip(CircleShape).background(MaterialTheme.colorScheme.primary))
                Spacer(Modifier.width(6.dp))
            }
            Text(
                row.sender ?: stringResource(R.string.inbox_unknown_sender),
                modifier = Modifier.weight(1f),
                fontWeight = weight,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                style = MaterialTheme.typography.bodyLarge,
            )
            Spacer(Modifier.width(8.dp))
            Text(
                time,
                color = if (row.unread) MaterialTheme.colorScheme.primary else muted,
                fontWeight = weight,
                style = MaterialTheme.typography.labelMedium,
                maxLines = 1,
            )
        }
        Text(
            row.subject.ifEmpty { stringResource(R.string.inbox_no_subject) },
            fontWeight = weight,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            style = MaterialTheme.typography.bodyMedium,
        )
        if (row.preview.isNotEmpty()) {
            Text(
                row.preview,
                color = muted,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                style = MaterialTheme.typography.bodyMedium,
            )
        }
    }
}

@Composable
private fun RowMenu(open: Boolean, row: MailRow, onDismiss: () -> Unit, onToggleRead: () -> Unit, onArchive: () -> Unit) {
    DropdownMenu(expanded = open, onDismissRequest = onDismiss) {
        DropdownMenuItem(
            text = { Text(stringResource(if (row.unread) R.string.action_mark_read else R.string.action_mark_unread)) },
            onClick = { onDismiss(); onToggleRead() },
        )
        DropdownMenuItem(
            text = { Text(stringResource(R.string.action_archive)) },
            onClick = { onDismiss(); onArchive() },
        )
    }
}

/** Asks for the notification permission the first time the Inbox is shown, and never again by itself. */
@Composable
private fun AskForNotificationsOnce() {
    val context = LocalContext.current
    val launcher = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { }
    LaunchedEffect(Unit) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU) return@LaunchedEffect
        val prefs = UiPrefs(context)
        val granted = ContextCompat.checkSelfPermission(context, NotificationFix.PERMISSION) ==
            android.content.pm.PackageManager.PERMISSION_GRANTED
        if (NotificationFix.shouldAskOnArrival(Build.VERSION.SDK_INT, granted, prefs.notificationPermissionAsked)) {
            prefs.notificationPermissionAsked = true
            launcher.launch(NotificationFix.PERMISSION)
        }
    }
}

private const val HIGHLIGHT_MILLIS = 2_500L
