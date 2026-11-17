package ovh.kweb.mail.ui.files

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.FilterChip
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Shape
import ovh.kweb.mail.ui.list.Groups
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.platform.LocalResources
import ovh.kweb.mail.ui.transfer.DownloadRules
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.UiMessage
import uniffi.kweb_core.FileEntry
import ovh.kweb.mail.ui.calendar.rememberLocale
import uniffi.kweb_core.FileKind
import uniffi.kweb_core.PublicLink
import uniffi.kweb_core.PublicLinkLimits
import java.time.ZoneId

/** The dialog of [FilesState.dialog], if there is one. The typed text lives in the dialog, saved across a rotation. */
@Composable
internal fun FilesDialogHost(state: FilesState, c: FilesController, onCopyLink: (String) -> Unit, onShareLink: (String) -> Unit) {
    when (val dialog = state.dialog) {
        null -> Unit
        FilesDialog.NewFolder -> NameDialog(
            title = stringResource(R.string.files_new_folder),
            label = stringResource(R.string.files_folder_name),
            initial = TextFieldValue(""),
            confirm = stringResource(R.string.files_create),
            busy = state.busy,
            error = state.dialogError,
            onConfirm = c::createFolder,
            onDismiss = c::dismissDialog,
        )
        is FilesDialog.Rename -> NameDialog(
            // The key keeps what was typed for one entry from reaching the dialog of another.
            key = dialog.entry.id,
            title = stringResource(R.string.files_rename_title),
            label = stringResource(R.string.files_name),
            initial = TextFieldValue(dialog.entry.name, TextRange(0, FileRules.renameSelectionEnd(dialog.entry.name, dialog.entry.kind))),
            confirm = stringResource(R.string.files_rename_action),
            busy = state.busy,
            error = state.dialogError,
            onConfirm = { c.rename(dialog.entry, it) },
            onDismiss = c::dismissDialog,
        )
        is FilesDialog.Delete -> DeleteDialog(dialog.entry, state.busy, state.dialogError, onConfirm = { c.delete(dialog.entry) }, onDismiss = c::dismissDialog)
        is FilesDialog.Move -> MoveDialog(dialog, state, c)
        is FilesDialog.PublicLinks -> PublicLinksDialog(dialog, state.publicLinkLimits, c, onCopyLink, onShareLink)
    }
    state.upload?.conflict?.let { ConflictDialog(it, c) }
    state.opening?.let { name ->
        AlertDialog(
            onDismissRequest = c::cancelOpening,
            title = { Text(stringResource(if (state.openingToSave) R.string.files_saving else R.string.files_opening, name), maxLines = 2, overflow = TextOverflow.Ellipsis) },
            text = {
                val progress = state.openingProgress
                val fraction = progress?.let { DownloadRules.fraction(it) }
                val resources = LocalResources.current
                val locale = remember { java.util.Locale.getDefault() }
                Column {
                    // Filled where the size is known, else a bar that only shows that something is going on.
                    if (fraction != null) LinearProgressIndicator(progress = { fraction }, modifier = Modifier.fillMaxWidth(), drawStopIndicator = {})
                    else LinearProgressIndicator(Modifier.fillMaxWidth())
                    if (progress != null && progress.done > 0) {
                        Text(
                            DownloadRules.line(progress, locale) { done, total -> resources.getString(R.string.download_of, done, total) },
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            modifier = Modifier.padding(top = 8.dp),
                        )
                    }
                }
            },
            confirmButton = { TextButton(onClick = c::cancelOpening) { Text(stringResource(R.string.cancel)) } },
        )
    }
}

@Composable
private fun ErrorLine(error: UiMessage?) {
    if (error == null) return
    Text(
        error.resolve(LocalResources.current),
        color = MaterialTheme.colorScheme.error,
        style = MaterialTheme.typography.bodyMedium,
        modifier = Modifier.padding(top = 8.dp),
    )
}

@Composable
private fun NameDialog(
    title: String,
    label: String,
    initial: TextFieldValue,
    confirm: String,
    busy: Boolean,
    error: UiMessage?,
    onConfirm: (String) -> Unit,
    onDismiss: () -> Unit,
    key: String = "",
) {
    var value by rememberSaveable(key, stateSaver = TextFieldValue.Saver) { mutableStateOf(initial) }
    val focus = remember { FocusRequester() }
    LaunchedEffect(Unit) { focus.requestFocus() }
    AlertDialog(
        onDismissRequest = { if (!busy) onDismiss() },
        title = { Text(title) },
        text = {
            Column {
                ovh.kweb.mail.ui.list.DialogField(
                    value = value,
                    onValueChange = { value = it },
                    label = label,
                    isError = error != null,
                    enabled = !busy,
                    modifier = Modifier.fillMaxWidth().focusRequester(focus),
                )
                ErrorLine(error)
            }
        },
        confirmButton = {
            TextButton(onClick = { onConfirm(value.text) }, enabled = !busy && FileRules.cleanName(value.text).isNotEmpty()) { Text(confirm) }
        },
        dismissButton = { TextButton(onClick = onDismiss, enabled = !busy) { Text(stringResource(R.string.cancel)) } },
    )
}

@Composable
private fun DeleteDialog(entry: FileEntry, busy: Boolean, error: UiMessage?, onConfirm: () -> Unit, onDismiss: () -> Unit) {
    AlertDialog(
        onDismissRequest = { if (!busy) onDismiss() },
        title = { Text(stringResource(R.string.files_delete_title, entry.name), maxLines = 3, overflow = TextOverflow.Ellipsis) },
        text = {
            Column {
                Text(stringResource(if (entry.kind == FileKind.FOLDER) R.string.files_delete_folder_text else R.string.files_delete_file_text))
                ErrorLine(error)
            }
        },
        confirmButton = {
            TextButton(onClick = onConfirm, enabled = !busy) {
                Text(stringResource(R.string.files_delete), color = MaterialTheme.colorScheme.error)
            }
        },
        dismissButton = { TextButton(onClick = onDismiss, enabled = !busy) { Text(stringResource(R.string.cancel)) } },
    )
}

@Composable
private fun ConflictDialog(conflict: UploadConflict, c: FilesController) {
    val isFolder = conflict.existing.kind == FileKind.FOLDER
    val canReplace = conflict.existing.kind == FileKind.FILE && conflict.existing.mayReplace
    AlertDialog(
        // Leaving the dialog any other way is "Skip", so the queue is never stuck behind it.
        onDismissRequest = { c.resolveConflict(ConflictChoice.Skip) },
        title = { Text(stringResource(R.string.files_conflict_title, conflict.name), maxLines = 3, overflow = TextOverflow.Ellipsis) },
        text = { Text(stringResource(if (isFolder) R.string.files_conflict_folder_text else R.string.files_conflict_file_text)) },
        confirmButton = {
            Row {
                if (canReplace) TextButton(onClick = { c.resolveConflict(ConflictChoice.Replace) }) { Text(stringResource(R.string.files_replace)) }
                TextButton(onClick = { c.resolveConflict(ConflictChoice.KeepBoth) }) { Text(stringResource(R.string.files_keep_both)) }
            }
        },
        dismissButton = { TextButton(onClick = { c.resolveConflict(ConflictChoice.Skip) }) { Text(stringResource(R.string.files_skip)) } },
    )
}

/** Browses the folders of the space; "Move here" is on where [FileRules.canMoveHere] says so. */
@Composable
private fun MoveDialog(dialog: FilesDialog.Move, state: FilesState, c: FilesController) {
    val colors = MaterialTheme.colorScheme
    val listing = dialog.listing
    val folders = remember(listing, dialog.entry) { listing?.let { FileRules.moveChoices(it.entries, dialog.entry) }.orEmpty() }
    val spaceName = state.space?.let { spaceLabel(it) }.orEmpty()
    val canHere = listing != null && !state.busy && FileRules.canMoveHere(dialog.entry, dialog.trail, listing.mayAdd)
    AlertDialog(
        onDismissRequest = { if (!state.busy) c.dismissDialog() },
        title = { Text(stringResource(R.string.files_move_title, dialog.entry.name), maxLines = 2, overflow = TextOverflow.Ellipsis) },
        text = {
            Column {
                Text(
                    FileRules.pathLine(spaceName, dialog.trail),
                    style = MaterialTheme.typography.labelLarge,
                    color = colors.onSurfaceVariant,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                Box(Modifier.padding(top = 8.dp).heightIn(min = 120.dp, max = 320.dp).fillMaxWidth()) {
                    when {
                        dialog.error != null -> Column(Modifier.align(Alignment.Center), horizontalAlignment = Alignment.CenterHorizontally) {
                            Text(dialog.error.resolve(LocalResources.current), color = colors.error)
                            TextButton(onClick = c::moveRetry) { Text(stringResource(R.string.retry)) }
                        }
                        listing == null -> CircularProgressIndicator(Modifier.align(Alignment.Center))
                        else -> LazyColumn(verticalArrangement = Arrangement.spacedBy(3.dp)) {
                            val hasUp = dialog.trail.isNotEmpty()
                            val count = folders.size + if (hasUp) 1 else 0
                            if (hasUp) item(key = "up") {
                                MoveRow(stringResource(R.string.files_up), FileIcon.Folder, Groups.shape(0, count), onClick = c::moveUp)
                            }
                            itemsIndexed(folders, key = { _, it -> it.id }) { i, folder ->
                                MoveRow(folder.name, FileIcon.Folder, Groups.shape(i + if (hasUp) 1 else 0, count), onClick = { c.moveOpen(folder) })
                            }
                            if (folders.isEmpty()) item(key = "none") {
                                Text(
                                    stringResource(R.string.files_move_no_folders),
                                    color = colors.onSurfaceVariant,
                                    modifier = Modifier.padding(12.dp),
                                )
                            }
                        }
                    }
                }
                if (listing != null && !listing.mayAdd) {
                    Text(stringResource(R.string.files_move_cannot_add), color = colors.onSurfaceVariant, style = MaterialTheme.typography.bodySmall)
                }
                ErrorLine(state.dialogError)
            }
        },
        confirmButton = { TextButton(onClick = c::moveHere, enabled = canHere) { Text(stringResource(R.string.files_move_here)) } },
        dismissButton = { TextButton(onClick = c::dismissDialog, enabled = !state.busy) { Text(stringResource(R.string.cancel)) } },
    )
}

@Composable
private fun MoveRow(name: String, icon: FileIcon, shape: Shape, onClick: () -> Unit) {
    Row(
        Modifier
            .fillMaxWidth()
            .clip(shape)
            .background(MaterialTheme.colorScheme.surfaceContainerHigh)
            .clickable(role = Role.Button, onClick = onClick)
            .padding(horizontal = 12.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        FileIconBadge(icon, size = 32)
        Text(name, modifier = Modifier.weight(1f), maxLines = 1, overflow = TextOverflow.Ellipsis, style = MaterialTheme.typography.bodyLarge)
    }
}


private fun validityLabel(validity: LinkValidity): Int = when (validity) {
    LinkValidity.Day -> R.string.files_link_1_day
    LinkValidity.Week -> R.string.files_link_7_days
    LinkValidity.Month -> R.string.files_link_30_days
    LinkValidity.Year -> R.string.files_link_1_year
}

/**
 * The public links of one file: the live ones with Copy, Share and Revoke, and a place to make a new one. A link is
 * a secret: it goes only to the clipboard or the share sheet, never into a log.
 */
@Composable
private fun PublicLinksDialog(
    dialog: FilesDialog.PublicLinks,
    limits: PublicLinkLimits?,
    c: FilesController,
    onCopy: (String) -> Unit,
    onShare: (String) -> Unit,
) {
    val colors = MaterialTheme.colorScheme
    val links = dialog.links
    val choices = remember(limits) { limits?.let { FileRules.linkChoices(it) }.orEmpty() }
    val atLimit = links != null && limits != null && FileRules.atLinkLimit(links, limits)
    val zone = remember { ZoneId.systemDefault() }
    val locale = rememberLocale()
    AlertDialog(
        onDismissRequest = { if (!dialog.working) c.dismissDialog() },
        title = { Text(stringResource(R.string.files_public_links), maxLines = 2, overflow = TextOverflow.Ellipsis) },
        text = {
            Column {
                Text(
                    stringResource(R.string.files_public_links_hint),
                    style = MaterialTheme.typography.bodyMedium,
                    color = colors.onSurfaceVariant,
                )
                Box(Modifier.padding(top = 12.dp).heightIn(min = 48.dp, max = 240.dp).fillMaxWidth()) {
                    when {
                        links == null && dialog.error != null -> Column(Modifier.align(Alignment.Center), horizontalAlignment = Alignment.CenterHorizontally) {
                            Text(dialog.error.resolve(LocalResources.current), color = colors.error)
                            TextButton(onClick = c::linksRetry) { Text(stringResource(R.string.retry)) }
                        }
                        links == null -> CircularProgressIndicator(Modifier.align(Alignment.Center))
                        links.isEmpty() -> Text(stringResource(R.string.files_no_public_link), color = colors.onSurfaceVariant, modifier = Modifier.padding(vertical = 8.dp))
                        else -> LazyColumn(verticalArrangement = Arrangement.spacedBy(3.dp)) {
                            itemsIndexed(links, key = { _, l -> l.id }) { i, link ->
                                LinkRow(
                                    link = link,
                                    shape = Groups.shape(i, links.size),
                                    expires = FileRules.dateTimeText(link.expires, zone, locale),
                                    busy = dialog.working,
                                    onCopy = {
                                        onCopy(link.url)
                                        c.linkCopied(link.id)
                                    },
                                    onShare = { onShare(link.url) },
                                    onRevoke = { c.askRevoke(link) },
                                )
                            }
                        }
                    }
                }
                if (dialog.copiedId != null) {
                    Text(
                        stringResource(R.string.files_link_copied),
                        color = colors.primary,
                        style = MaterialTheme.typography.bodyMedium,
                        modifier = Modifier.padding(top = 8.dp),
                    )
                }
                if (links != null) {
                    if (atLimit && limits != null) {
                        Text(
                            stringResource(R.string.files_link_limit, limits.maxPerFile.toInt()),
                            color = colors.onSurfaceVariant,
                            style = MaterialTheme.typography.bodySmall,
                            modifier = Modifier.padding(top = 8.dp),
                        )
                    } else {
                        Text(
                            stringResource(R.string.files_link_valid_for),
                            style = MaterialTheme.typography.labelLarge,
                            color = colors.onSurfaceVariant,
                            modifier = Modifier.padding(top = 12.dp, bottom = 4.dp),
                        )
                        Row(Modifier.horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            for (choice in choices) {
                                FilterChip(
                                    selected = dialog.validity == choice,
                                    onClick = { c.chooseValidity(choice) },
                                    enabled = !dialog.working,
                                    label = { Text(stringResource(validityLabel(choice))) },
                                )
                            }
                        }
                    }
                    // A reason for a failed Make or Revoke; a failed load is shown in place of the list.
                    if (dialog.error != null) ErrorLine(dialog.error)
                }
            }
        },
        confirmButton = {
            TextButton(onClick = c::makeLink, enabled = links != null && !atLimit && !dialog.working) { Text(stringResource(R.string.files_link_make)) }
        },
        dismissButton = { TextButton(onClick = c::dismissDialog, enabled = !dialog.working) { Text(stringResource(R.string.files_link_close)) } },
    )
    if (dialog.revoking != null) {
        AlertDialog(
            onDismissRequest = { if (!dialog.working) c.cancelRevoke() },
            title = { Text(stringResource(R.string.files_link_revoke)) },
            text = { Text(stringResource(R.string.files_link_revoke_text)) },
            confirmButton = {
                TextButton(onClick = c::revokeLink, enabled = !dialog.working) {
                    Text(stringResource(R.string.files_link_revoke), color = colors.error)
                }
            },
            dismissButton = { TextButton(onClick = c::cancelRevoke, enabled = !dialog.working) { Text(stringResource(R.string.cancel)) } },
        )
    }
}

@Composable
private fun LinkRow(link: PublicLink, shape: Shape, expires: String, busy: Boolean, onCopy: () -> Unit, onShare: () -> Unit, onRevoke: () -> Unit) {
    val colors = MaterialTheme.colorScheme
    Column(
        Modifier
            .fillMaxWidth()
            .clip(shape)
            .background(colors.surfaceContainerHigh)
            .padding(start = 16.dp, end = 8.dp, top = 10.dp, bottom = 2.dp),
    ) {
        Text(link.url, style = MaterialTheme.typography.bodyMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
        Text(
            stringResource(R.string.files_link_expires, expires),
            style = MaterialTheme.typography.bodySmall,
            color = colors.onSurfaceVariant,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
        Row {
            TextButton(onClick = onCopy, enabled = !busy) { Text(stringResource(R.string.files_link_copy)) }
            TextButton(onClick = onShare, enabled = !busy) { Text(stringResource(R.string.files_link_share)) }
            TextButton(onClick = onRevoke, enabled = !busy) { Text(stringResource(R.string.files_link_revoke), color = colors.error) }
        }
    }
}
