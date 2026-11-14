package ovh.kweb.mail.ui.folders

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.unit.dp
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.list.FolderEntry
import ovh.kweb.mail.ui.list.Folders
import ovh.kweb.mail.ui.list.folderName

/** The question on top of the folder screen, if there is one. */
@Composable
fun FolderDialogs(state: FoldersState, c: FoldersController) {
    when (val dialog = state.dialog) {
        null -> Unit
        is FolderDialog.Name -> NameDialog(dialog, state, c)
        is FolderDialog.Move -> MovePicker(dialog, state, c)
        is FolderDialog.Delete -> DeleteDialog(dialog, state, c)
    }
}

@Composable
private fun NameDialog(dialog: FolderDialog.Name, state: FoldersState, c: FoldersController) {
    val parent = dialog.targetId?.takeIf { dialog.mode == NameMode.NEW }?.let { id -> state.mailboxes.firstOrNull { it.id == id } }
    val title = when {
        dialog.mode == NameMode.RENAME -> stringResource(R.string.folders_rename_title)
        parent != null -> stringResource(R.string.folders_new_in_title, folderName(parent))
        else -> stringResource(R.string.folders_new_title)
    }
    val focus = remember { FocusRequester() }
    LaunchedEffect(Unit) { focus.requestFocus() }
    val send = { if (!state.busy) c.confirmName() }
    AlertDialog(
        onDismissRequest = { if (!state.busy) c.dismissDialog() },
        title = { Text(title) },
        text = {
            OutlinedTextField(
                value = dialog.text,
                onValueChange = c::setName,
                label = { Text(stringResource(R.string.folders_name_label)) },
                singleLine = true,
                isError = state.error != null,
                supportingText = state.error?.let { { Text(it.resolve(LocalResources.current)) } },
                keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Sentences, imeAction = ImeAction.Done),
                keyboardActions = KeyboardActions(onDone = { send() }),
                modifier = Modifier.fillMaxWidth().focusRequester(focus),
            )
        },
        confirmButton = {
            TextButton(onClick = send, enabled = !state.busy && FolderRules.nameCanBeSent(dialog.text)) {
                Text(stringResource(if (dialog.mode == NameMode.NEW) R.string.folders_create else R.string.folders_save))
            }
        },
        dismissButton = { TextButton(onClick = c::dismissDialog, enabled = !state.busy) { Text(stringResource(R.string.cancel)) } },
    )
}

/** The folders [Move] may go to, as a plain list under the top level. The folder itself and the ones inside it are not in it. */
@Composable
private fun MovePicker(dialog: FolderDialog.Move, state: FoldersState, c: FoldersController) {
    val folder = state.mailboxes.firstOrNull { it.id == dialog.id } ?: return
    val entries = remember(state.mailboxes, dialog.id) {
        val allowed = FolderRules.moveTargets(state.mailboxes, dialog.id).map { it.id }.toSet()
        Folders.ordered(state.mailboxes).filter { it.mailbox.id in allowed }
    }
    val top = FolderRules.canMoveToTopLevel(state.mailboxes, dialog.id)
    AlertDialog(
        onDismissRequest = { if (!state.busy) c.dismissDialog() },
        title = { Text(stringResource(R.string.folders_move_title, folderName(folder))) },
        text = {
            if (entries.isEmpty() && !top) {
                Text(stringResource(R.string.folders_move_nowhere), color = MaterialTheme.colorScheme.onSurfaceVariant)
            } else {
                LazyColumn(Modifier.heightIn(max = 360.dp)) {
                    if (top) item { PickRow(stringResource(R.string.folders_top_level), 0, !state.busy) { c.confirmMove(null) } }
                    items(entries, key = { it.mailbox.id }) { entry: FolderEntry ->
                        PickRow(folderName(entry.mailbox), entry.depth + 1, !state.busy) { c.confirmMove(entry.mailbox.id) }
                    }
                }
            }
        },
        confirmButton = {},
        dismissButton = { TextButton(onClick = c::dismissDialog, enabled = !state.busy) { Text(stringResource(R.string.cancel)) } },
    )
}

@Composable
private fun PickRow(text: String, depth: Int, enabled: Boolean, onClick: () -> Unit) {
    Text(
        text,
        modifier = Modifier
            .fillMaxWidth()
            .clickable(enabled = enabled, onClick = onClick)
            .padding(start = (16 * (depth - 1).coerceAtLeast(0)).dp, top = 12.dp, bottom = 12.dp),
        style = MaterialTheme.typography.bodyLarge,
    )
}

@Composable
private fun DeleteDialog(dialog: FolderDialog.Delete, state: FoldersState, c: FoldersController) {
    val folder = state.mailboxes.firstOrNull { it.id == dialog.id } ?: return
    val name = folderName(folder)
    when (val plan = FolderRules.deletePlan(state.mailboxes, dialog.id)) {
        null -> Unit
        DeletePlan.HasFolders -> AlertDialog(
            onDismissRequest = c::dismissDialog,
            title = { Text(stringResource(R.string.folders_delete_blocked_title)) },
            text = { Text(stringResource(R.string.folders_delete_blocked_text, name)) },
            confirmButton = { TextButton(onClick = c::dismissDialog) { Text(stringResource(R.string.folders_got_it)) } },
        )
        else -> AlertDialog(
            onDismissRequest = { if (!state.busy) c.dismissDialog() },
            title = { Text(stringResource(R.string.folders_delete_title, name)) },
            text = {
                Column {
                    Text(
                        if (plan is DeletePlan.WithMail) {
                            pluralStringResource(R.plurals.folders_delete_mail_text, plan.count, plan.count)
                        } else {
                            stringResource(R.string.folders_delete_empty_text)
                        },
                    )
                }
            },
            confirmButton = {
                TextButton(onClick = { if (!state.busy) c.confirmDelete() }, enabled = !state.busy) {
                    Text(stringResource(R.string.folders_delete), color = MaterialTheme.colorScheme.error)
                }
            },
            dismissButton = { TextButton(onClick = c::dismissDialog, enabled = !state.busy) { Text(stringResource(R.string.cancel)) } },
        )
    }
}
