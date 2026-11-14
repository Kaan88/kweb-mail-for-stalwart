package ovh.kweb.mail.ui.filters

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Close
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TextField
import androidx.compose.material3.TextFieldDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.unit.dp
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.calendar.GroupColumn
import ovh.kweb.mail.ui.list.Groups
import uniffi.kweb_core.FilterCondition
import uniffi.kweb_core.FilterMatch

/** The form for one rule. Its state is in the controller, so a rotation keeps what was typed. */
@Composable
internal fun RuleEditor(state: FiltersState, editor: EditorState, c: FiltersController) {
    val rule = editor.rule
    val tree = remember(state.folders) { FilterRules.folderTree(state.folders) }
    var picking by rememberSaveable { mutableStateOf(false) }
    val problem = c.problem()

    Column(Modifier.fillMaxSize().imePadding().verticalScroll(rememberScrollState()).padding(bottom = 32.dp)) {
        GroupColumn {
            FieldRow(Groups.shape(0, 2), stringResource(R.string.filters_name), rule.name, singleLine = true) { v -> c.edit { it.copy(name = v) } }
            RowFrame(Groups.shape(1, 2)) {
                FilterChip(
                    selected = rule.matchAll,
                    onClick = { c.edit { it.copy(matchAll = true) } },
                    label = { Text(stringResource(R.string.filters_match_all)) },
                )
                FilterChip(
                    selected = !rule.matchAll,
                    onClick = { c.edit { it.copy(matchAll = false) } },
                    label = { Text(stringResource(R.string.filters_match_any)) },
                )
            }
        }

        Heading(stringResource(R.string.filters_conditions))
        GroupColumn {
            rule.conditions.forEachIndexed { index, condition ->
                ConditionRow(
                    shape = Groups.shape(index, rule.conditions.size),
                    condition = condition,
                    canRemove = rule.conditions.size > 1,
                    onChange = { change -> c.edit { FilterRules.withCondition(it, index, change) } },
                    onRemove = { c.edit { FilterRules.removeCondition(it, index) } },
                )
            }
        }
        TextButton(onClick = { c.edit(FilterRules::addCondition) }, modifier = Modifier.padding(horizontal = 12.dp)) {
            Text(stringResource(R.string.filters_add_condition))
        }

        Heading(stringResource(R.string.filters_actions))
        GroupColumn {
            val folder = rule.moveTo?.let { FilterRules.folderName(state.folders, it) }
            RowFrame(Groups.shape(0, 5), Modifier.clickable(enabled = !rule.delete) { picking = true }) {
                Column(Modifier.weight(1f)) {
                    Text(
                        stringResource(R.string.filters_move_to),
                        style = MaterialTheme.typography.bodyLarge,
                        color = if (rule.delete) MaterialTheme.colorScheme.onSurfaceVariant else MaterialTheme.colorScheme.onSurface,
                    )
                    Text(
                        when {
                            rule.delete -> stringResource(R.string.filters_delete_detail)
                            rule.moveTo == null -> stringResource(R.string.filters_move_to_none)
                            folder == null -> stringResource(R.string.filters_problem_folder)
                            else -> folder
                        },
                        color = if (rule.moveTo != null && folder == null && !rule.delete) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurfaceVariant,
                        style = MaterialTheme.typography.bodyMedium,
                    )
                }
                if (rule.moveTo != null && !rule.delete) {
                    IconButton(onClick = { c.edit { FilterRules.withFolder(it, null) } }) {
                        Icon(Icons.Filled.Close, contentDescription = stringResource(R.string.filters_move_to_none))
                    }
                }
            }
            SwitchRow(Groups.shape(1, 5), stringResource(R.string.filters_mark_read), null, rule.markRead) { v -> c.edit { it.copy(markRead = v) } }
            SwitchRow(Groups.shape(2, 5), stringResource(R.string.filters_star), null, rule.star) { v -> c.edit { it.copy(star = v) } }
            SwitchRow(Groups.shape(3, 5), stringResource(R.string.filters_delete), stringResource(R.string.filters_delete_detail), rule.delete) { v ->
                c.edit { FilterRules.withDelete(it, v) }
            }
            SwitchRow(Groups.shape(4, 5), stringResource(R.string.filters_stop), stringResource(R.string.filters_stop_detail), rule.stop) { v ->
                c.edit { it.copy(stop = v) }
            }
        }

        // The button says what is missing; it works when nothing is.
        Button(
            onClick = c::saveEditor,
            enabled = problem == null && !state.busy,
            shape = CircleShape,
            modifier = Modifier.fillMaxWidth().padding(horizontal = 12.dp).padding(top = 24.dp).heightIn(min = 52.dp),
        ) {
            if (state.busy) {
                CircularProgressIndicator(Modifier.size(20.dp), strokeWidth = 2.dp, color = MaterialTheme.colorScheme.onPrimary)
            } else {
                Text(problem?.let { problemText(it) } ?: stringResource(R.string.filters_save), style = MaterialTheme.typography.labelLarge)
            }
        }
        if (!editor.isNew) {
            TextButton(
                onClick = c::askDeleteRule,
                enabled = !state.busy,
                modifier = Modifier.align(Alignment.CenterHorizontally).padding(top = 8.dp),
            ) { Text(stringResource(R.string.filters_delete_rule), color = MaterialTheme.colorScheme.error) }
        }
    }

    if (picking) {
        FolderPicker(tree, rule.moveTo, onDismiss = { picking = false }) { id ->
            picking = false
            c.edit { FilterRules.withFolder(it, id) }
        }
    }
}

@Composable
internal fun problemText(problem: RuleProblem): String = when (problem) {
    RuleProblem.NoCondition -> stringResource(R.string.filters_problem_no_condition)
    is RuleProblem.BlankValue -> stringResource(R.string.filters_problem_blank, problem.index + 1)
    is RuleProblem.ManyLines -> stringResource(R.string.filters_problem_lines, problem.index + 1)
    is RuleProblem.BadHeader -> stringResource(R.string.filters_problem_header, problem.index + 1)
    RuleProblem.DeleteAndMove -> stringResource(R.string.filters_problem_delete_move)
    RuleProblem.NoAction -> stringResource(R.string.filters_problem_no_action)
    RuleProblem.FolderGone -> stringResource(R.string.filters_problem_folder)
    RuleProblem.TooMany -> stringResource(R.string.filters_problem_too_many)
}

@Composable
private fun Heading(text: String) {
    Text(
        text,
        modifier = Modifier.padding(start = 28.dp, end = 16.dp, top = 20.dp, bottom = 8.dp),
        color = MaterialTheme.colorScheme.primary,
        style = MaterialTheme.typography.titleSmall,
    )
}

@Composable
private fun SwitchRow(shape: Shape, title: String, detail: String?, checked: Boolean, onChange: (Boolean) -> Unit) {
    RowFrame(shape, Modifier.toggleable(value = checked, role = Role.Switch, onValueChange = onChange)) {
        Column(Modifier.weight(1f)) {
            Text(title, style = MaterialTheme.typography.bodyLarge)
            if (detail != null) Text(detail, color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.bodyMedium)
        }
        Switch(checked = checked, onCheckedChange = null)
    }
}

@Composable
private fun FieldRow(shape: Shape, label: String, value: String, singleLine: Boolean, onValue: (String) -> Unit) {
    val container = MaterialTheme.colorScheme.surfaceContainerLow
    TextField(
        value = value,
        onValueChange = onValue,
        label = { Text(label) },
        singleLine = singleLine,
        shape = shape,
        keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.None),
        colors = TextFieldDefaults.colors(
            focusedContainerColor = container,
            unfocusedContainerColor = container,
            focusedIndicatorColor = Color.Transparent,
            unfocusedIndicatorColor = Color.Transparent,
        ),
        modifier = Modifier.fillMaxWidth(),
    )
}

/** One condition: which line, how, a remove button, then the header's name (for another header) and the text. */
@Composable
private fun ConditionRow(shape: Shape, condition: FilterCondition, canRemove: Boolean, onChange: ((FilterCondition) -> FilterCondition) -> Unit, onRemove: () -> Unit) {
    val kind = FilterRules.kind(condition.field)
    val fieldLabels = mapOf(
        FieldKind.FROM to stringResource(R.string.filters_field_from),
        FieldKind.RECIPIENT to stringResource(R.string.filters_field_to),
        FieldKind.SUBJECT to stringResource(R.string.filters_field_subject),
        FieldKind.HEADER to stringResource(R.string.filters_field_header),
    )
    val howLabels = listOf(
        FilterMatch.CONTAINS to stringResource(R.string.filters_how_contains),
        FilterMatch.IS to stringResource(R.string.filters_how_is),
        FilterMatch.BEGINS_WITH to stringResource(R.string.filters_how_begins),
        FilterMatch.ENDS_WITH to stringResource(R.string.filters_how_ends),
    )
    run {
        Column(Modifier.fillMaxWidth().clip(shape).background(MaterialTheme.colorScheme.surfaceContainerLow).padding(start = 12.dp, end = 4.dp, top = 8.dp, bottom = 4.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Choice(fieldLabels.getValue(kind), FieldKind.entries.map { it to fieldLabels.getValue(it) }) { picked ->
                    onChange { it.copy(field = FilterRules.field(picked, FilterRules.headerName(it.field))) }
                }
                Choice(howLabels.first { it.first == condition.how }.second, howLabels) { picked -> onChange { it.copy(how = picked) } }
                Box(Modifier.weight(1f))
                if (canRemove) {
                    IconButton(onClick = onRemove) { Icon(Icons.Filled.Close, contentDescription = stringResource(R.string.filters_remove_condition)) }
                }
            }
            if (kind == FieldKind.HEADER) {
                PlainField(stringResource(R.string.filters_header_name), FilterRules.headerName(condition.field)) { v ->
                    onChange { it.copy(field = FilterRules.field(FieldKind.HEADER, v.trim())) }
                }
            }
            PlainField(stringResource(R.string.filters_value), condition.value) { v -> onChange { it.copy(value = v) } }
        }
    }
}

@Composable
private fun PlainField(label: String, value: String, onValue: (String) -> Unit) {
    TextField(
        value = value,
        onValueChange = onValue,
        label = { Text(label) },
        singleLine = true,
        colors = TextFieldDefaults.colors(
            focusedContainerColor = Color.Transparent,
            unfocusedContainerColor = Color.Transparent,
            focusedIndicatorColor = Color.Transparent,
            unfocusedIndicatorColor = Color.Transparent,
        ),
        modifier = Modifier.fillMaxWidth(),
    )
}

/** A button that opens a menu of [options]; [label] is the one chosen now. */
@Composable
private fun <T> Choice(label: String, options: List<Pair<T, String>>, onPick: (T) -> Unit) {
    var open by rememberSaveable { mutableStateOf(false) }
    Box {
        Surface(
            onClick = { open = true },
            shape = RoundedCornerShape(12.dp),
            color = MaterialTheme.colorScheme.surfaceContainerHighest,
            modifier = Modifier.heightIn(min = 40.dp),
        ) {
            Box(Modifier.padding(horizontal = 12.dp), contentAlignment = Alignment.Center) { Text(label, style = MaterialTheme.typography.bodyLarge) }
        }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            for ((value, text) in options) {
                DropdownMenuItem(text = { Text(text) }, onClick = { open = false; onPick(value) })
            }
        }
    }
}

/** The user's folders as a tree by name (the Drafts folder is not offered). */
@Composable
private fun FolderPicker(tree: List<FolderRow>, selected: String?, onDismiss: () -> Unit, onPick: (String) -> Unit) {
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.filters_choose_folder)) },
        text = {
            LazyColumn {
                items(tree, key = { it.id }) { row ->
                    Text(
                        row.name,
                        modifier = Modifier
                            .fillMaxWidth()
                            .clickable { onPick(row.id) }
                            .heightIn(min = 48.dp)
                            .padding(start = (row.depth * 20).dp, top = 12.dp, bottom = 12.dp),
                        color = if (row.id == selected) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurface,
                        style = MaterialTheme.typography.bodyLarge,
                    )
                }
            }
        },
        confirmButton = {},
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.cancel)) } },
    )
}
