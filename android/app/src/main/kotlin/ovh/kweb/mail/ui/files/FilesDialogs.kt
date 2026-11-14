package ovh.kweb.mail.ui.files

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.CircularProgressIndicator
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
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.UiMessage
import uniffi.kweb_core.FileEntry
import uniffi.kweb_core.FileKind

/** The dialog of [FilesState.dialog], if there is one. The typed text lives in the dialog, saved across a rotation. */
@Composable
internal fun FilesDialogHost(state: FilesState, c: FilesController) {
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
    }
    state.upload?.conflict?.let { ConflictDialog(it, c) }
    state.opening?.let { name ->
        AlertDialog(
            onDismissRequest = c::cancelOpening,
            title = { Text(stringResource(if (state.openingToSave) R.string.files_saving else R.string.files_opening, name), maxLines = 2, overflow = TextOverflow.Ellipsis) },
            text = { LinearProgressIndicator(Modifier.fillMaxWidth()) },
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
