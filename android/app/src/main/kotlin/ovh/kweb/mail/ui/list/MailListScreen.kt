package ovh.kweb.mail.ui.list

import android.os.Build
import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Create
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.Menu
import androidx.compose.material.icons.filled.Notifications
import androidx.compose.material.icons.filled.Search
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DrawerValue
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExtendedFloatingActionButton
import androidx.compose.material3.FilterChip
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalDrawerSheet
import androidx.compose.material3.ModalNavigationDrawer
import androidx.compose.material3.NavigationDrawerItem
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarDuration
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.SnackbarResult
import androidx.compose.material3.Surface
import androidx.compose.material3.SwipeToDismissBox
import androidx.compose.material3.SwipeToDismissBoxValue
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.material3.rememberDrawerState
import androidx.compose.material3.rememberSwipeToDismissBoxState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.core.content.ContextCompat
import androidx.lifecycle.compose.LifecycleResumeEffect
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import kotlinx.coroutines.launch
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.UiPrefs
import ovh.kweb.mail.ui.delivery.NotificationFix
import uniffi.kweb_core.DraftKind
import uniffi.kweb_core.ListFilter
import uniffi.kweb_core.ThreadRow

/**
 * The mail list of milestone M3: the conversations of one folder from the cache, with the
 * folders in a drawer, filter chips, swipe actions and a menu on a long press.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun MailListScreen(
    onOpenThread: (threadId: String, mailboxId: String) -> Unit,
    onOpenSearch: () -> Unit,
    onOpenDeliveryCheck: () -> Unit,
    onOpenSettings: () -> Unit,
    onCompose: (draftId: String) -> Unit,
    vm: MailListViewModel = viewModel(),
) {
    val state by vm.state.collectAsStateWithLifecycle()
    val listState = rememberLazyListState()
    val snackbar = remember { SnackbarHostState() }
    val drawerState = rememberDrawerState(DrawerValue.Closed)
    val scope = rememberCoroutineScope()
    val resources = LocalResources.current
    var moving by remember { mutableStateOf<ThreadRow?>(null) }
    var deleting by remember { mutableStateOf<ThreadRow?>(null) }

    // Coming back from a thread does not resume the activity, so ask for a fresh read here.
    LaunchedEffect(vm) { vm.reload() }
    LifecycleResumeEffect(vm) {
        vm.reload()
        onPauseOrDispose { }
    }
    // New mail goes on top. A list keeps its place by row, so without this the reader who was at
    // the top would be left looking at the second row with the new mail hidden above it.
    val topThread = state.rows.firstOrNull()?.newest?.threadId
    LaunchedEffect(topThread) {
        if (listState.firstVisibleItemIndex in 1..2) listState.scrollToItem(0)
    }
    AskForNotificationsOnce()
    BackHandler(enabled = drawerState.isOpen) { scope.launch { drawerState.close() } }
    LaunchedEffect(vm) {
        vm.events.collect { event ->
            // A newer message replaces the one on screen instead of queueing behind it.
            snackbar.currentSnackbarData?.dismiss()
            launch {
                when (event) {
                    is ListEvent.Message -> snackbar.showSnackbar(resources.getString(event.textRes))
                    is ListEvent.OpenDraft -> onCompose(event.draftId)
                    is ListEvent.Undo -> {
                        val result = snackbar.showSnackbar(
                            resources.getString(event.textRes),
                            actionLabel = resources.getString(R.string.action_undo),
                            duration = SnackbarDuration.Long,
                        )
                        if (result == SnackbarResult.ActionPerformed) vm.undo(event)
                    }
                }
            }
        }
    }

    val current = state.current
    val folders = remember(state.mailboxes) { Folders.ordered(state.mailboxes) }
    val junk = remember(state.mailboxes) { Folders.withRole(state.mailboxes, "junk") }
    val inTrash = current?.role == "trash"

    // Near the end of the list: more of the cache, then older mail from the server.
    val atEnd by remember {
        derivedStateOf {
            val info = listState.layoutInfo
            info.totalItemsCount > 0 && (info.visibleItemsInfo.lastOrNull()?.index ?: -1) >= info.totalItemsCount - NEAR_END
        }
    }
    // Every key that can make loadMore() do something new is here, the limit included: with exactly
    // one page cached, raising it changes nothing else.
    LaunchedEffect(atEnd, state.rows.size, state.limit, state.loadingOlder, state.loaded, state.olderPaused, state.olderStopped) {
        if (atEnd) vm.loadMore()
    }
    // A finger on the list lets automatic fetching go on after it paused.
    LaunchedEffect(listState) {
        snapshotFlow { listState.isScrollInProgress }.collect { if (it) vm.userScrolled() }
    }

    ModalNavigationDrawer(
        drawerState = drawerState,
        drawerContent = {
            DrawerContent(
                username = state.username,
                folders = folders,
                currentId = current?.id,
                onFolder = {
                    scope.launch { drawerState.close() }
                    vm.selectFolder(it)
                },
                onDeliveryCheck = {
                    scope.launch { drawerState.close() }
                    onOpenDeliveryCheck()
                },
                onSettings = {
                    scope.launch { drawerState.close() }
                    onOpenSettings()
                },
            )
        },
    ) {
        Scaffold(
            topBar = {
                Column(Modifier.background(MaterialTheme.colorScheme.background)) {
                    SearchPill(
                        initial = Sender.initial("", state.username),
                        onSearch = onOpenSearch,
                        onAccount = { scope.launch { drawerState.open() } },
                    )
                    FilterChips(current?.let { folderName(it) }, state.filter, vm::selectFilter)
                }
            },
            snackbarHost = { SnackbarHost(snackbar) },
            floatingActionButton = {
                // The primary container, a pen and a word, at the bottom right, as in the Inbox figure.
                ExtendedFloatingActionButton(
                    onClick = { vm.startDraft(DraftKind.New) },
                    icon = { Icon(Icons.Filled.Create, contentDescription = null) },
                    text = { Text(stringResource(R.string.compose_button)) },
                    shape = RoundedCornerShape(20.dp),
                    containerColor = MaterialTheme.colorScheme.primaryContainer,
                    contentColor = MaterialTheme.colorScheme.onPrimaryContainer,
                )
            },
        ) { padding ->
            Column(Modifier.padding(padding).fillMaxSize()) {
                // A message the server refused comes first, then a quiet count of what is only waiting.
                state.failedDraftId?.let { id -> OutboxBar(failed = true, count = 0, onClick = { onCompose(id) }) }
                // The message held back for Undo has its own bar; the count is of the others.
                state.holding?.let { SendingBar(onUndo = { vm.undoSend(it.draftId) }) }
                val others = state.waiting - if (state.holding != null) 1 else 0
                if (others > 0) OutboxBar(failed = false, count = others, onClick = null)
                PullToRefreshBox(
                    isRefreshing = state.refreshing,
                    onRefresh = vm::refresh,
                    modifier = Modifier.weight(1f).fillMaxWidth(),
                ) {
                    LazyColumn(
                        Modifier.fillMaxSize(),
                        state = listState,
                        contentPadding = PaddingValues(start = 12.dp, end = 12.dp, top = 4.dp, bottom = 88.dp),
                        verticalArrangement = Arrangement.spacedBy(3.dp),
                    ) {
                        if (state.rows.isEmpty()) {
                            item { EmptyState(state, Modifier.fillParentMaxSize()) }
                        }
                        itemsIndexed(state.rows, key = { _, row -> row.newest.threadId }) { index, row ->
                            key(state.swipeEpoch) {
                                ConversationItem(
                                    row = row,
                                    shape = Groups.shape(index, state.rows.size),
                                    role = current?.role,
                                    junkAvailable = junk != null && current?.id != junk.id,
                                    onOpen = {
                                        current?.let {
                                            if (ListRules.opensCompose(it.role)) {
                                                vm.startDraft(DraftKind.Existing(row.newest.id))
                                            } else {
                                                onOpenThread(row.newest.threadId, it.id)
                                            }
                                        }
                                    },
                                    onArchive = { vm.archive(row) },
                                    onDelete = { if (inTrash) deleting = row else vm.delete(row) },
                                    onToggleRead = { vm.toggleRead(row) },
                                    onToggleFlag = { vm.toggleFlag(row) },
                                    onMove = { moving = row },
                                    onSpam = { vm.spam(row) },
                                )
                            }
                        }
                        if (state.olderPaused && !state.loadingOlder) {
                            item {
                                TextButton(onClick = vm::loadOlderNow, modifier = Modifier.fillMaxWidth()) {
                                    Text(stringResource(R.string.list_load_older))
                                }
                            }
                        }
                        if (state.loadingOlder) {
                            item {
                                Box(Modifier.fillMaxWidth().padding(16.dp), contentAlignment = Alignment.Center) {
                                    CircularProgressIndicator(Modifier.size(24.dp))
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    moving?.let { row ->
        MoveDialog(
            folders = folders,
            exclude = current?.id,
            onPick = {
                moving = null
                vm.move(row, it.id)
            },
            onDismiss = { moving = null },
        )
    }
    deleting?.let { row ->
        DeleteForeverDialog(
            onConfirm = {
                deleting = null
                vm.delete(row)
            },
            onDismiss = { deleting = null },
        )
    }
}

/**
 * A line above the list about mail that is on its way out. [failed]: the server refused one, an
 * error container that opens it. Otherwise a quiet bar that says how many are waiting.
 */
@Composable
private fun OutboxBar(failed: Boolean, count: Int, onClick: (() -> Unit)?) {
    val colors = MaterialTheme.colorScheme
    val text = if (failed) stringResource(R.string.outbox_failed) else pluralStringResource(R.plurals.outbox_waiting, count, count)
    Row(
        Modifier
            .fillMaxWidth()
            .padding(horizontal = 14.dp, vertical = 4.dp)
            .clip(RoundedCornerShape(14.dp))
            .background(if (failed) colors.errorContainer else colors.surfaceContainerHigh)
            .then(if (onClick != null) Modifier.clickable(onClick = onClick) else Modifier)
            .padding(horizontal = 14.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            text,
            modifier = Modifier.weight(1f),
            color = if (failed) colors.onErrorContainer else colors.onSurfaceVariant,
            style = MaterialTheme.typography.labelLarge,
        )
        if (failed) {
            Text(
                stringResource(R.string.outbox_open),
                color = colors.onErrorContainer,
                fontWeight = FontWeight.Bold,
                style = MaterialTheme.typography.labelLarge,
            )
        }
    }
}

/** "Sending…" with Undo, for as long as the core holds the message back. */
@Composable
private fun SendingBar(onUndo: () -> Unit) {
    val colors = MaterialTheme.colorScheme
    Row(
        Modifier
            .fillMaxWidth()
            .padding(horizontal = 14.dp, vertical = 4.dp)
            .clip(RoundedCornerShape(14.dp))
            .background(colors.inverseSurface)
            .padding(start = 14.dp, end = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            stringResource(R.string.send_sending),
            modifier = Modifier.weight(1f),
            color = colors.inverseOnSurface,
            style = MaterialTheme.typography.labelLarge,
        )
        TextButton(onClick = onUndo) { Text(stringResource(R.string.action_undo), color = colors.inversePrimary) }
    }
}

/** The rounded "Search mail" field with the account's round avatar at its right end. */
@Composable
private fun SearchPill(initial: String, onSearch: () -> Unit, onAccount: () -> Unit) {
    Surface(
        modifier = Modifier.statusBarsPadding().padding(horizontal = 14.dp, vertical = 8.dp).fillMaxWidth().height(52.dp),
        shape = CircleShape,
        color = MaterialTheme.colorScheme.surfaceContainerHigh,
    ) {
        Row(
            Modifier.padding(start = 4.dp, end = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            IconButton(onClick = onAccount) {
                Icon(
                    Icons.Filled.Menu,
                    contentDescription = stringResource(R.string.list_open_folders),
                    tint = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            Row(
                Modifier.weight(1f).fillMaxHeight().clip(CircleShape).clickable(onClick = onSearch).padding(start = 4.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    stringResource(R.string.search_hint),
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    style = MaterialTheme.typography.bodyLarge,
                )
            }
            Box(
                Modifier
                    .clip(CircleShape)
                    .clickable(onClickLabel = stringResource(R.string.list_open_folders), onClick = onAccount),
            ) { Avatar(initial, size = 36.dp) }
        }
    }
}

@Composable
private fun FilterChips(folderName: String?, filter: ListFilter, onFilter: (ListFilter) -> Unit) {
    Row(
        Modifier.horizontalScroll(rememberScrollState()).padding(horizontal = 14.dp, vertical = 4.dp),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Chip(folderName ?: stringResource(R.string.folder_inbox), filter == ListFilter.ALL) { onFilter(ListFilter.ALL) }
        Chip(stringResource(R.string.filter_unread), filter == ListFilter.UNREAD) { onFilter(ListFilter.UNREAD) }
        Chip(stringResource(R.string.filter_flagged), filter == ListFilter.FLAGGED) { onFilter(ListFilter.FLAGGED) }
        Chip(stringResource(R.string.filter_files), filter == ListFilter.ATTACHMENTS) { onFilter(ListFilter.ATTACHMENTS) }
    }
}

@Composable
private fun Chip(label: String, selected: Boolean, onClick: () -> Unit) {
    FilterChip(
        selected = selected,
        onClick = onClick,
        label = { Text(label) },
        shape = RoundedCornerShape(10.dp),
        leadingIcon = if (selected) {
            { Icon(Icons.Filled.Check, contentDescription = null, modifier = Modifier.size(16.dp)) }
        } else {
            null
        },
    )
}

@Composable
private fun DrawerContent(
    username: String,
    folders: List<FolderEntry>,
    currentId: String?,
    onFolder: (String) -> Unit,
    onDeliveryCheck: () -> Unit,
    onSettings: () -> Unit,
) {
    ModalDrawerSheet {
        Column(Modifier.verticalScroll(rememberScrollState()).padding(horizontal = 12.dp, vertical = 16.dp)) {
            Row(
                Modifier.padding(start = 16.dp, bottom = 12.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                Avatar(Sender.initial(username, ""))
                Text(username, style = MaterialTheme.typography.titleMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
            folders.forEach { entry ->
                val unread = entry.mailbox.unreadEmails.toInt()
                NavigationDrawerItem(
                    label = { Text(folderName(entry.mailbox), maxLines = 1, overflow = TextOverflow.Ellipsis) },
                    selected = entry.mailbox.id == currentId,
                    onClick = { onFolder(entry.mailbox.id) },
                    modifier = Modifier.padding(start = (16 * entry.depth).dp),
                    icon = { FolderIcon(entry.mailbox.role) },
                    badge = if (unread > 0) {
                        {
                            val description = pluralStringResource(R.plurals.unread_count, unread, unread)
                            Text(unread.toString(), modifier = Modifier.semantics { contentDescription = description })
                        }
                    } else {
                        null
                    },
                )
            }
            HorizontalDivider(Modifier.padding(vertical = 8.dp))
            NavigationDrawerItem(
                label = { Text(stringResource(R.string.delivery_check_title)) },
                icon = { Icon(Icons.Filled.Notifications, contentDescription = null) },
                selected = false,
                onClick = onDeliveryCheck,
            )
            NavigationDrawerItem(
                label = { Text(stringResource(R.string.settings_title)) },
                icon = { Icon(Icons.Filled.Settings, contentDescription = null) },
                selected = false,
                onClick = onSettings,
            )
        }
    }
}

/** The picture of a folder in the drawer: its own for the system folders, a plain folder for the rest. */
@Composable
private fun FolderIcon(role: String?) {
    when (role) {
        "trash" -> Icon(Icons.Filled.Delete, contentDescription = null)
        else -> Icon(
            painterResource(
                when (role) {
                    "inbox" -> R.drawable.ic_inbox
                    "drafts" -> R.drawable.ic_drafts
                    "sent" -> R.drawable.ic_sent
                    "archive" -> R.drawable.ic_archive
                    "junk" -> R.drawable.ic_spam
                    else -> R.drawable.ic_folder
                },
            ),
            contentDescription = null,
        )
    }
}

@Composable
private fun EmptyState(state: MailListState, modifier: Modifier) {
    Column(
        modifier.padding(32.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        // Until the first read has finished, "empty" only means "not here yet".
        if (state.loaded) {
            Text(stringResource(R.string.list_empty), style = MaterialTheme.typography.titleMedium)
            if (!state.synced) {
                Spacer(Modifier.size(8.dp))
                Text(
                    stringResource(R.string.inbox_first_sync),
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
}

/** One conversation: the row inside a swipe box (right: archive, left: delete) with its long-press menu. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun ConversationItem(
    row: ThreadRow,
    shape: Shape,
    role: String?,
    junkAvailable: Boolean,
    onOpen: () -> Unit,
    onArchive: () -> Unit,
    onDelete: () -> Unit,
    onToggleRead: () -> Unit,
    onToggleFlag: () -> Unit,
    onMove: () -> Unit,
    onSpam: () -> Unit,
) {
    var menuOpen by remember { mutableStateOf(false) }
    val canArchive = role != "archive"
    val inTrash = role == "trash"
    val scope = rememberCoroutineScope()
    val swipe = rememberSwipeToDismissBoxState()
    SwipeToDismissBox(
        state = swipe,
        modifier = Modifier.clip(shape),
        enableDismissFromStartToEnd = canArchive,
        onDismiss = { value ->
            when (value) {
                SwipeToDismissBoxValue.StartToEnd -> onArchive()
                SwipeToDismissBoxValue.EndToStart -> {
                    onDelete()
                    // In Trash the row stays until the user has said "Delete forever".
                    if (inTrash) scope.launch { swipe.reset() }
                }
                SwipeToDismissBoxValue.Settled -> Unit
            }
        },
        backgroundContent = {
            val toArchive = swipe.dismissDirection == SwipeToDismissBoxValue.StartToEnd
            val colors = MaterialTheme.colorScheme
            // What the swipe will do, in words and in colour, under the row as it moves away.
            Row(
                Modifier
                    .fillMaxSize()
                    .background(if (toArchive) colors.primaryContainer else colors.errorContainer)
                    .padding(horizontal = 20.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(8.dp, if (toArchive) Alignment.Start else Alignment.End),
            ) {
                val tint = if (toArchive) colors.onPrimaryContainer else colors.onErrorContainer
                if (toArchive) {
                    Icon(painterResource(R.drawable.ic_archive), contentDescription = null, tint = tint)
                } else {
                    Icon(Icons.Filled.Delete, contentDescription = null, tint = tint)
                }
                Text(
                    stringResource(if (toArchive) R.string.action_archive else R.string.action_delete),
                    color = tint,
                    style = MaterialTheme.typography.labelLarge,
                )
            }
        },
    ) {
        Box {
            MailRow(rowContent(row, role), onClick = onOpen, shape = shape, onLongClick = { menuOpen = true })
            DropdownMenu(expanded = menuOpen, onDismissRequest = { menuOpen = false }) {
                MenuItem(if (row.unreadCount > 0u) R.string.action_mark_read else R.string.action_mark_unread) {
                    menuOpen = false
                    onToggleRead()
                }
                MenuItem(if (row.isFlagged) R.string.action_unstar else R.string.action_star) {
                    menuOpen = false
                    onToggleFlag()
                }
                if (canArchive) {
                    MenuItem(R.string.action_archive) {
                        menuOpen = false
                        onArchive()
                    }
                }
                MenuItem(if (inTrash) R.string.action_delete_forever else R.string.action_delete) {
                    menuOpen = false
                    onDelete()
                }
                MenuItem(R.string.action_move) {
                    menuOpen = false
                    onMove()
                }
                if (junkAvailable) {
                    MenuItem(R.string.action_spam) {
                        menuOpen = false
                        onSpam()
                    }
                }
            }
        }
    }
}

@Composable
private fun MenuItem(textRes: Int, onClick: () -> Unit) {
    DropdownMenuItem(text = { Text(stringResource(textRes)) }, onClick = onClick)
}

/** A conversation as a row. Sent and Drafts show who it went to instead of who sent it. */
@Composable
private fun rowContent(row: ThreadRow, role: String?): RowContent {
    val newest = row.newest
    val toYou = role == "sent" || role == "drafts"
    val unknown = stringResource(R.string.list_unknown_sender)
    val noRecipients = stringResource(R.string.list_no_recipients)
    val prefix = stringResource(R.string.list_to_prefix)
    val names = if (toYou) {
        SenderLine.recipients(newest.to)?.let { prefix + it } ?: noRecipients
    } else {
        SenderLine.senders(row.senders) ?: unknown
    }
    val initial = if (toYou) {
        newest.to.firstOrNull()?.let { Sender.initial(it.name, it.email) } ?: "?"
    } else {
        Sender.initial(row.senders.firstOrNull().orEmpty(), "")
    }
    return RowContent(
        sender = SenderLine.withCount(names, row.messageIds.size),
        initial = initial,
        subject = newest.subject.trim(),
        preview = Preview.oneLine(newest.preview),
        receivedAt = newest.receivedAt,
        unread = row.unreadCount > 0u,
        flagged = row.isFlagged,
        hasAttachment = row.hasAttachment,
    )
}

/** Asks for the notification permission the first time the list is shown, and never again by itself. */
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

/** How many rows from the end the list starts to fetch more. */
private const val NEAR_END = 5
