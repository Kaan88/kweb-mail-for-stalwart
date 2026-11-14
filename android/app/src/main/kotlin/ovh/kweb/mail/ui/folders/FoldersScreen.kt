package ovh.kweb.mail.ui.folders

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.Notifications
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.Route
import ovh.kweb.mail.ui.calendar.GroupColumn
import ovh.kweb.mail.ui.calendar.NoticeBar
import ovh.kweb.mail.ui.list.FolderEntry
import ovh.kweb.mail.ui.list.Folders
import ovh.kweb.mail.ui.list.Groups
import ovh.kweb.mail.ui.list.folderName
import uniffi.kweb_core.Mailbox

/**
 * The folders as a tree, each with its bell and a menu, and a button for a new top-level folder. [route] may carry
 * a folder and what to do with it (a long press in the drawer); [onBack] leaves the screen.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun FoldersScreen(route: Route.Folders, onBack: () -> Unit, vm: FoldersViewModel = viewModel()) {
    val c = vm.controller
    val state by c.state.collectAsStateWithLifecycle()
    LaunchedEffect(vm) { c.start(route.folderId, route.action) }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(stringResource(R.string.folders_title)) },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = stringResource(R.string.back))
                    }
                },
                colors = TopAppBarDefaults.topAppBarColors(containerColor = MaterialTheme.colorScheme.background),
            )
        },
        containerColor = MaterialTheme.colorScheme.background,
    ) { padding ->
        Column(Modifier.padding(padding).fillMaxSize()) {
            // A refusal of a name shows in its own dialog; everything else is a bar here.
            val error = state.error
            if (error != null && state.dialog !is FolderDialog.Name) {
                NoticeBar(error.resolve(LocalResources.current), error = true, action = stringResource(R.string.contacts_dismiss) to c::dismissError)
            }
            when {
                state.phase == FoldersPhase.LOADING -> Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) { CircularProgressIndicator() }
                state.phase == FoldersPhase.FAILED -> Column(
                    Modifier.fillMaxSize().padding(32.dp),
                    verticalArrangement = Arrangement.Center,
                    horizontalAlignment = Alignment.CenterHorizontally,
                ) {
                    Text(stringResource(R.string.folders_load_failed), color = MaterialTheme.colorScheme.onSurfaceVariant, textAlign = TextAlign.Center)
                    TextButton(onClick = c::load) { Text(stringResource(R.string.retry)) }
                }
                else -> FolderTree(state, c)
            }
        }
    }
    FolderDialogs(state, c)
}

@Composable
private fun FolderTree(state: FoldersState, c: FoldersController) {
    val entries = remember(state.mailboxes) { Folders.ordered(state.mailboxes) }
    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(bottom = 32.dp)) {
        if (entries.isEmpty()) {
            Text(
                stringResource(R.string.folders_empty),
                modifier = Modifier.padding(horizontal = 28.dp, vertical = 16.dp),
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                style = MaterialTheme.typography.bodyMedium,
            )
        }
        GroupColumn {
            entries.forEachIndexed { index, entry ->
                FolderRow(Groups.shape(index, entries.size), entry, state.busy, c)
            }
        }
        FilledTonalButton(
            onClick = { c.askNew(null) },
            enabled = !state.busy,
            shape = CircleShape,
            modifier = Modifier.fillMaxWidth().padding(horizontal = 12.dp).padding(top = 16.dp).heightIn(min = 52.dp),
        ) { Text(stringResource(R.string.folders_new), style = MaterialTheme.typography.labelLarge) }
        Text(
            stringResource(R.string.folders_notify_note),
            modifier = Modifier.padding(horizontal = 28.dp, vertical = 16.dp),
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            style = MaterialTheme.typography.bodySmall,
        )
    }
}

@Composable
private fun FolderRow(shape: Shape, entry: FolderEntry, busy: Boolean, c: FoldersController) {
    val folder = entry.mailbox
    val name = folderName(folder)
    Row(
        Modifier
            .fillMaxWidth()
            .clip(shape)
            .background(MaterialTheme.colorScheme.surfaceContainerLow)
            .heightIn(min = 56.dp)
            .padding(start = (20 + 16 * entry.depth).dp, end = 4.dp, top = 4.dp, bottom = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        val tint = MaterialTheme.colorScheme.onSurfaceVariant
        if (folder.role == "trash") {
            Icon(Icons.Filled.Delete, contentDescription = null, tint = tint)
        } else {
            Icon(painterResource(folderIcon(folder.role)), contentDescription = null, tint = tint)
        }
        Text(name, modifier = Modifier.weight(1f), maxLines = 1, overflow = TextOverflow.Ellipsis, style = MaterialTheme.typography.bodyLarge)
        BellButton(folder, name, busy, c)
        val items = FolderRules.menu(folder, withBell = false)
        if (items.isNotEmpty()) {
            var open by remember { mutableStateOf(false) }
            Box {
                IconButton(onClick = { open = true }, enabled = !busy) {
                    Icon(Icons.Filled.MoreVert, contentDescription = stringResource(R.string.folders_options_for, name))
                }
                FolderMenu(folder, withBell = false, expanded = open, onDismiss = { open = false }) { item ->
                    when (FolderRules.action(item)) {
                        FolderAction.RENAME -> c.askRename(folder.id)
                        FolderAction.MOVE -> c.askMove(folder.id)
                        FolderAction.NEW_INSIDE -> c.askNew(folder.id)
                        FolderAction.DELETE -> c.askDelete(folder.id)
                        null -> Unit
                    }
                }
            }
        }
    }
}

/** The bell: shows whether the folder notifies and, where the user may, switches it. Folders that never notify have none. */
@Composable
private fun BellButton(folder: Mailbox, name: String, busy: Boolean, c: FoldersController) {
    val colors = MaterialTheme.colorScheme
    when (FolderRules.bell(folder)) {
        Bell.NONE -> Unit
        Bell.FIXED_ON -> Icon(
            Icons.Filled.Notifications,
            contentDescription = stringResource(R.string.folders_bell_fixed, name),
            tint = colors.outline,
            modifier = Modifier.padding(12.dp),
        )
        Bell.SWITCH -> IconButton(onClick = { c.setNotify(folder.id, !folder.notify) }, enabled = !busy) {
            if (folder.notify) {
                Icon(Icons.Filled.Notifications, contentDescription = stringResource(R.string.folders_bell_is_on, name), tint = colors.primary)
            } else {
                Icon(painterResource(R.drawable.ic_bell_off), contentDescription = stringResource(R.string.folders_bell_is_off, name), tint = colors.onSurfaceVariant)
            }
        }
    }
}

/** The picture of a folder, as in the drawer. */
internal fun folderIcon(role: String?): Int = when (role) {
    "inbox" -> R.drawable.ic_inbox
    "drafts" -> R.drawable.ic_drafts
    "sent" -> R.drawable.ic_sent
    "archive" -> R.drawable.ic_archive
    "junk" -> R.drawable.ic_spam
    else -> R.drawable.ic_folder
}
