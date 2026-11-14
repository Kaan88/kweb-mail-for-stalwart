package ovh.kweb.mail.ui.mailextras

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.AssistChip
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TriStateCheckbox
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.state.ToggleableState
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import ovh.kweb.mail.R
import uniffi.kweb_core.Label

/** The labels of a row of the list: small chips, at most [LabelRules.ROW_CHIPS] and "+n". */
@Composable
fun RowLabelChips(names: List<String>, modifier: Modifier = Modifier) {
    if (names.isEmpty()) return
    val (shown, more) = LabelRules.shown(names)
    Row(modifier.padding(top = 4.dp), horizontalArrangement = Arrangement.spacedBy(4.dp), verticalAlignment = Alignment.CenterVertically) {
        shown.forEach { SmallChip(it, Modifier.weight(1f, fill = false)) }
        if (more > 0) SmallChip(stringResource(R.string.labels_more, more))
    }
}

@Composable
private fun SmallChip(text: String, modifier: Modifier = Modifier) {
    Text(
        text,
        modifier = modifier
            .clip(RoundedCornerShape(6.dp))
            .background(MaterialTheme.colorScheme.secondaryContainer)
            .padding(horizontal = 6.dp, vertical = 1.dp),
        color = MaterialTheme.colorScheme.onSecondaryContainer,
        style = MaterialTheme.typography.labelSmall,
        maxLines = 1,
        overflow = TextOverflow.Ellipsis,
    )
}

/** The chips under a message's sender line: its labels (a tap opens the labels sheet) and the Unsubscribe chip. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun MessageChips(
    labelNames: List<String>,
    unsubscribe: UnsubEntry?,
    onLabels: (() -> Unit)?,
    onUnsubscribe: () -> Unit,
    modifier: Modifier = Modifier,
) {
    if (labelNames.isEmpty() && unsubscribe == null) return
    FlowRow(modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
        labelNames.forEach { name ->
            AssistChip(
                onClick = { onLabels?.invoke() },
                enabled = onLabels != null,
                label = { Text(name, maxLines = 1, overflow = TextOverflow.Ellipsis) },
                shape = RoundedCornerShape(10.dp),
            )
        }
        when (unsubscribe) {
            null -> Unit
            is UnsubEntry.Available -> AssistChip(
                onClick = onUnsubscribe,
                label = { Text(stringResource(R.string.unsub_chip)) },
                shape = RoundedCornerShape(10.dp),
            )
            UnsubEntry.Working -> AssistChip(onClick = {}, enabled = false, label = { Text(stringResource(R.string.unsub_working)) }, shape = RoundedCornerShape(10.dp))
            UnsubEntry.Requested -> AssistChip(onClick = {}, enabled = false, label = { Text(stringResource(R.string.unsub_requested)) }, shape = RoundedCornerShape(10.dp))
        }
    }
}

/** "Unsubscribe from [list]?" with what will happen, in words. */
@Composable
fun UnsubscribeDialog(list: String, way: UnsubscribeRules.Way, onConfirm: () -> Unit, onDismiss: () -> Unit) {
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.unsub_title, list)) },
        text = { Text(stringResource(UnsubscribeRules.confirmText(way), list)) },
        confirmButton = { TextButton(onClick = onConfirm) { Text(stringResource(UnsubscribeRules.confirmButton(way))) } },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.cancel)) } },
    )
}

/**
 * The user's labels as check rows for [messageIds] (one message, or a whole thread: a label some of them carry
 * shows as half checked), and "New label". A tap changes the labels at once.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun LabelSheet(
    labels: List<Label>,
    messageIds: List<String>,
    byMessage: Map<String, List<String>>,
    /** False until the list of labels and the labels of these messages have been read: the rows wait, and "none" is not said. */
    ready: Boolean,
    error: String?,
    onToggle: (labelId: String, on: Boolean) -> Unit,
    onNewLabel: () -> Unit,
    onDismiss: () -> Unit,
) {
    ModalBottomSheet(
        onDismissRequest = onDismiss,
        sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true),
        containerColor = MaterialTheme.colorScheme.surfaceContainerLow,
    ) {
        Column(Modifier.padding(bottom = 16.dp)) {
            Text(
                stringResource(R.string.labels_title),
                modifier = Modifier.padding(horizontal = 24.dp, vertical = 8.dp),
                style = MaterialTheme.typography.titleLarge,
            )
            if (error != null) {
                Text(
                    error,
                    modifier = Modifier.padding(horizontal = 24.dp, vertical = 8.dp),
                    color = MaterialTheme.colorScheme.error,
                )
            }
            if (!ready) {
                Text(
                    stringResource(R.string.labels_reading),
                    modifier = Modifier.padding(horizontal = 24.dp, vertical = 8.dp),
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            } else if (labels.isEmpty()) {
                Text(
                    stringResource(R.string.labels_none),
                    modifier = Modifier.padding(horizontal = 24.dp, vertical = 8.dp),
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            LazyColumn(Modifier.weight(1f, fill = false)) {
                items(labels, key = { it.id }) { label ->
                    val carry = LabelRules.carry(messageIds, byMessage, label.id)
                    Row(
                        Modifier
                            .fillMaxWidth()
                            .clickable(enabled = ready) { onToggle(label.id, LabelRules.turnOn(carry)) }
                            .padding(horizontal = 16.dp, vertical = 2.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        TriStateCheckbox(
                            state = when (carry) {
                                Carry.ALL -> ToggleableState.On
                                Carry.SOME -> ToggleableState.Indeterminate
                                Carry.NONE -> ToggleableState.Off
                            },
                            onClick = null,
                            enabled = ready,
                        )
                        Text(label.name, modifier = Modifier.padding(start = 16.dp), maxLines = 1, overflow = TextOverflow.Ellipsis)
                    }
                }
            }
            TextButton(onClick = onNewLabel, modifier = Modifier.padding(horizontal = 12.dp)) { Text(stringResource(R.string.labels_new)) }
        }
    }
}

/** A name for a new label. The core says if it is not allowed (a name that exists); the app checks what it can before. */
@Composable
fun NewLabelDialog(
    onCreate: (String) -> Unit,
    onDismiss: () -> Unit,
    /** The core's words when it refused the name; the dialog stays open so the name can be changed. */
    error: String? = null,
    /** True while the core is making the label. */
    busy: Boolean = false,
    onEdit: () -> Unit = {},
) {
    var name by rememberSaveable { mutableStateOf("") }
    val problem = LabelRules.nameProblem(name)
    // Blank is not an error to show while the user has typed nothing.
    val shown = if (name.isEmpty()) null else problem
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.labels_new)) },
        text = {
            OutlinedTextField(
                value = name,
                onValueChange = { name = it; onEdit() },
                singleLine = true,
                label = { Text(stringResource(R.string.labels_name)) },
                isError = shown != null || error != null,
                supportingText = when {
                    shown != null -> {
                        {
                            Text(
                                stringResource(
                                    when (shown) {
                                        LabelRules.NameProblem.BLANK -> R.string.labels_name_blank
                                        LabelRules.NameProblem.TOO_LONG -> R.string.labels_name_long
                                        LabelRules.NameProblem.MULTILINE -> R.string.labels_name_lines
                                    },
                                ),
                            )
                        }
                    }
                    error != null -> {
                        { Text(error) }
                    }
                    else -> null
                },
            )
        },
        confirmButton = { TextButton(onClick = { onCreate(name.trim()) }, enabled = problem == null && !busy) { Text(stringResource(R.string.labels_create)) } },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.cancel)) } },
    )
}

/** Asks before a label leaves the list; says that mail keeps it on the server. */
@Composable
fun DeleteLabelDialog(name: String, onConfirm: () -> Unit, onDismiss: () -> Unit) {
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.labels_delete_title, name)) },
        text = { Text(stringResource(R.string.labels_delete_text, name)) },
        confirmButton = { TextButton(onClick = onConfirm) { Text(stringResource(R.string.action_delete), color = MaterialTheme.colorScheme.error) } },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.cancel)) } },
    )
}
