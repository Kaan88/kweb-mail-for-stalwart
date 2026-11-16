package ovh.kweb.mail.ui.filters

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
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
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Close
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.AssistChip
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TextField
import androidx.compose.material3.TextFieldDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.unit.dp
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.calendar.GroupColumn
import ovh.kweb.mail.ui.list.Groups
import ovh.kweb.mail.ui.theme.KwebSwitch

/** The form for one rule. Its state is in the controller, so a rotation keeps what was typed. */
@Composable
internal fun RuleEditor(state: FiltersUi, editor: EditorState, c: FiltersController) {
    val rule = editor.rule
    val core = state.core ?: return
    val caps = core.capabilities
    val res = LocalResources.current
    val tree = remember(state.folders) { RuleModel.folderTree(state.folders) }
    var pickingFolder by rememberSaveable { mutableStateOf<Int?>(null) }
    val problem = c.problem()

    Column(Modifier.fillMaxSize().imePadding().verticalScroll(rememberScrollState()).padding(bottom = 32.dp)) {
        GroupColumn {
            FieldRow(Groups.shape(0, 2), stringResource(R.string.filters_name), rule.name) { v -> c.edit { it.copy(name = v.take(RuleModel.MAX_NAME + 1)) } }
            RowFrame(Groups.shape(1, 2)) {
                MatchChip(rule.matchAll, stringResource(R.string.filters_match_all)) { c.edit { it.copy(matchAll = true) } }
                MatchChip(!rule.matchAll, stringResource(R.string.filters_match_any)) { c.edit { it.copy(matchAll = false) } }
            }
        }

        Heading(stringResource(R.string.filters_conditions))
        GroupColumn {
            rule.conditions.forEachIndexed { index, condition ->
                ConditionRow(
                    shape = Groups.shape(index, rule.conditions.size),
                    condition = condition,
                    caps = caps,
                    canRemove = rule.conditions.size > 1,
                    onChange = { change -> c.edit { r -> r.copy(conditions = r.conditions.mapIndexed { i, x -> if (i == index) change(x) else x }) } },
                    onRemove = { c.edit { r -> r.copy(conditions = r.conditions.filterIndexed { i, _ -> i != index }) } },
                )
            }
        }
        TextButton(onClick = { c.edit { it.copy(conditions = it.conditions + RuleModel.blankCondition()) } }, modifier = Modifier.padding(horizontal = 12.dp)) {
            Text(stringResource(R.string.filters_add_condition))
        }

        Heading(stringResource(R.string.filters_actions))
        GroupColumn {
            rule.actions.forEachIndexed { index, action ->
                ActionRow(
                    shape = Groups.shape(index, rule.actions.size),
                    action = action,
                    caps = caps,
                    labels = state.labels.map { it.name },
                    folderGone = action.mailboxId != null && tree.none { it.id == action.mailboxId },
                    canRemove = rule.actions.size > 1,
                    onChange = { change -> c.edit { r -> r.copy(actions = r.actions.mapIndexed { i, x -> if (i == index) change(x) else x }) } },
                    onPickFolder = { pickingFolder = index },
                    onRemove = { c.edit { r -> r.copy(actions = r.actions.filterIndexed { i, _ -> i != index }) } },
                )
            }
        }
        TextButton(onClick = { c.edit { it.copy(actions = it.actions + ActDraft("move")) } }, modifier = Modifier.padding(horizontal = 12.dp)) {
            Text(stringResource(R.string.filters_add_action))
        }

        val spam = RuleModel.spamOptionApplies(rule, caps)
        val period = caps.period || rule.hasPeriod
        val rows = 1 + (if (spam) 1 else 0) + (if (period) 1 else 0)
        GroupColumn(Modifier.padding(top = 8.dp)) {
            var n = 0
            SwitchRow(Groups.shape(n++, rows), stringResource(R.string.filters_stop), stringResource(R.string.filters_stop_detail), rule.stop) { v -> c.edit { it.copy(stop = v) } }
            if (spam) {
                SwitchRow(Groups.shape(n++, rows), stringResource(R.string.filters_include_spam), stringResource(R.string.filters_include_spam_detail), rule.includeSpam) { v ->
                    c.edit { it.copy(includeSpam = v) }
                }
            }
            if (period) {
                SwitchRow(Groups.shape(n, rows), stringResource(R.string.filters_period), stringResource(R.string.filters_period_detail), rule.periodOn) { v ->
                    c.edit { it.copy(periodOn = v) }
                }
            }
        }
        if (rule.periodOn && period) {
            GroupColumn(Modifier.padding(top = 8.dp)) {
                FieldRow(Groups.shape(0, 2), stringResource(R.string.filters_period_from), rule.from) { v -> c.edit { it.copy(from = v) } }
                FieldRow(Groups.shape(1, 2), stringResource(R.string.filters_period_until), rule.until) { v -> c.edit { it.copy(until = v) } }
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
                Text(problem?.let { FilterLabels.problem(res, it) } ?: stringResource(R.string.filters_save), style = MaterialTheme.typography.labelLarge)
            }
        }
        if (!editor.isNew) {
            TextButton(
                onClick = c::deleteEditedRule,
                enabled = !state.busy,
                modifier = Modifier.align(Alignment.CenterHorizontally).padding(top = 8.dp),
            ) { Text(stringResource(R.string.filters_delete_rule), color = MaterialTheme.colorScheme.error) }
        }
    }

    pickingFolder?.let { index ->
        val chosen = rule.actions.getOrNull(index)?.mailboxId
        FolderPicker(tree, chosen, onDismiss = { pickingFolder = null }) { row ->
            pickingFolder = null
            c.edit { r -> r.copy(actions = r.actions.mapIndexed { i, a -> if (i == index) a.copy(value = row.path, mailboxId = row.id) else a }) }
        }
    }
}

@Composable
private fun MatchChip(selected: Boolean, label: String, onClick: () -> Unit) {
    FilterChip(
        selected = selected,
        onClick = onClick,
        label = { Text(label) },
        leadingIcon = if (selected) { { Icon(Icons.Filled.Check, contentDescription = null, modifier = Modifier.size(18.dp)) } } else null,
    )
}

@Composable
private fun SwitchRow(shape: Shape, title: String, detail: String?, checked: Boolean, onChange: (Boolean) -> Unit) {
    RowFrame(shape, Modifier.toggleable(value = checked, role = Role.Switch, onValueChange = onChange)) {
        Column(Modifier.weight(1f)) {
            Text(title, style = MaterialTheme.typography.bodyLarge)
            if (detail != null) Text(detail, color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.bodyMedium)
        }
        KwebSwitch(checked = checked, onCheckedChange = null)
    }
}

@Composable
private fun FieldRow(shape: Shape, label: String, value: String, onValue: (String) -> Unit) {
    val container = MaterialTheme.colorScheme.surfaceContainerLow
    TextField(
        value = value,
        onValueChange = onValue,
        label = { Text(label) },
        singleLine = true,
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

/** One condition: which part, how, a remove button, then the header's name (for another header) and the text. */
@Composable
private fun ConditionRow(
    shape: Shape,
    condition: CondDraft,
    caps: uniffi.kweb_core.FilterCapabilities,
    canRemove: Boolean,
    onChange: ((CondDraft) -> CondDraft) -> Unit,
    onRemove: () -> Unit,
) {
    val res = LocalResources.current
    val fields = RuleModel.fieldsFor(caps, condition.field).map { it to FilterLabels.field(res, it) }
    val comparators = RuleModel.comparatorsFor(condition.field, caps, condition.comparator).map { it to FilterLabels.comparator(res, it) }
    Column(Modifier.fillMaxWidth().clip(shape).background(MaterialTheme.colorScheme.surfaceContainerLow).padding(start = 12.dp, end = 4.dp, top = 8.dp, bottom = 4.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Choice(FilterLabels.field(res, condition.field), fields) { picked -> onChange { RuleModel.withField(it, picked, caps) } }
            if (comparators.isNotEmpty()) {
                Choice(FilterLabels.comparator(res, condition.comparator), comparators, Modifier.weight(1f, fill = false)) { picked ->
                    onChange { RuleModel.withComparator(it, picked) }
                }
            }
            Box(Modifier.weight(1f))
            if (canRemove) {
                IconButton(onClick = onRemove) { Icon(Icons.Filled.Close, contentDescription = stringResource(R.string.filters_remove_condition)) }
            }
        }
        if (condition.field == "header") {
            Spacer(Modifier.height(4.dp))
            PlainField(stringResource(R.string.filters_header_name), condition.headerName) { v -> onChange { it.copy(headerName = v) } }
        }
        if (RuleModel.needsValue(condition)) {
            Spacer(Modifier.height(4.dp))
            val hint = when {
                condition.comparator == "regex" -> R.string.filters_value_regex
                condition.field == "size" -> R.string.filters_value_size
                condition.comparator == "has_type" -> R.string.filters_value_type
                else -> R.string.filters_value_hint
            }
            PlainField(stringResource(hint), condition.value) { v -> onChange { it.copy(value = v) } }
        }
        Spacer(Modifier.height(4.dp))
    }
}

/** One action: its kind, a remove button, and what the kind needs (a folder, an address, a text or a label). */
@Composable
private fun ActionRow(
    shape: Shape,
    action: ActDraft,
    caps: uniffi.kweb_core.FilterCapabilities,
    labels: List<String>,
    folderGone: Boolean,
    canRemove: Boolean,
    onChange: ((ActDraft) -> ActDraft) -> Unit,
    onPickFolder: () -> Unit,
    onRemove: () -> Unit,
) {
    val res = LocalResources.current
    val types = RuleModel.actionsFor(caps, action.type).map { it to FilterLabels.action(res, it) }
    Column(Modifier.fillMaxWidth().clip(shape).background(MaterialTheme.colorScheme.surfaceContainerLow).padding(start = 12.dp, end = 4.dp, top = 8.dp, bottom = 4.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Choice(FilterLabels.action(res, action.type), types) { picked -> onChange { RuleModel.withActionType(it, picked) } }
            Box(Modifier.weight(1f))
            if (canRemove) {
                IconButton(onClick = onRemove) { Icon(Icons.Filled.Close, contentDescription = stringResource(R.string.filters_remove_action)) }
            }
        }
        when (action.type) {
            "move", "copy" -> {
                Spacer(Modifier.height(4.dp))
                Surface(
                    onClick = onPickFolder,
                    shape = RoundedCornerShape(12.dp),
                    color = MaterialTheme.colorScheme.surfaceContainerHighest,
                    modifier = Modifier.fillMaxWidth().padding(end = 8.dp, bottom = 4.dp).heightIn(min = 48.dp),
                ) {
                    Box(Modifier.padding(horizontal = 16.dp), contentAlignment = Alignment.CenterStart) {
                        Text(
                            when {
                                action.value.isBlank() -> stringResource(R.string.filters_no_folder)
                                folderGone -> stringResource(R.string.filters_folder_gone)
                                else -> action.value
                            },
                            style = MaterialTheme.typography.bodyLarge,
                            color = if (folderGone) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurface,
                        )
                    }
                }
            }
            "forward" -> {
                Spacer(Modifier.height(4.dp))
                PlainField(stringResource(R.string.filters_forward_address), action.value) { v -> onChange { it.copy(value = v) } }
                Row(
                    Modifier.fillMaxWidth().toggleable(value = action.keepCopy, role = Role.Switch, onValueChange = { v -> onChange { it.copy(keepCopy = v) } }).padding(end = 12.dp, bottom = 4.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(12.dp),
                ) {
                    Text(stringResource(R.string.filters_keep_copy), modifier = Modifier.weight(1f), style = MaterialTheme.typography.bodyLarge)
                    KwebSwitch(checked = action.keepCopy, onCheckedChange = null)
                }
            }
            "reject" -> {
                Spacer(Modifier.height(4.dp))
                PlainField(stringResource(R.string.filters_reject_text), action.value) { v -> onChange { it.copy(value = v) } }
            }
            "add_label" -> {
                Spacer(Modifier.height(4.dp))
                PlainField(stringResource(R.string.filters_label_name), action.value) { v -> onChange { it.copy(value = v) } }
                if (labels.isNotEmpty()) {
                    androidx.compose.foundation.layout.FlowRow(
                        Modifier.padding(end = 8.dp),
                        horizontalArrangement = Arrangement.spacedBy(8.dp),
                    ) {
                        for (name in labels) AssistChip(onClick = { onChange { it.copy(value = name) } }, label = { Text(name) })
                    }
                }
                Text(
                    stringResource(R.string.filters_label_note),
                    modifier = Modifier.padding(end = 8.dp, top = 4.dp, bottom = 4.dp),
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    style = MaterialTheme.typography.bodySmall,
                )
            }
        }
        Spacer(Modifier.height(4.dp))
    }
}

/** A filled field with the chips' own corners and edge, so the card has one left edge. */
@Composable
private fun PlainField(label: String, value: String, onValue: (String) -> Unit) {
    val container = MaterialTheme.colorScheme.surfaceContainerHighest
    TextField(
        value = value,
        onValueChange = onValue,
        label = { Text(label) },
        singleLine = true,
        shape = RoundedCornerShape(12.dp),
        keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.None),
        colors = TextFieldDefaults.colors(
            focusedContainerColor = container,
            unfocusedContainerColor = container,
            focusedIndicatorColor = Color.Transparent,
            unfocusedIndicatorColor = Color.Transparent,
        ),
        modifier = Modifier.fillMaxWidth().padding(end = 8.dp, bottom = 4.dp),
    )
}

/** A button that opens a menu of [options]; [label] is the one chosen now. */
@Composable
private fun <T> Choice(label: String, options: List<Pair<T, String>>, modifier: Modifier = Modifier, onPick: (T) -> Unit) {
    var open by rememberSaveable { mutableStateOf(false) }
    Box(modifier) {
        Surface(
            onClick = { open = true },
            shape = RoundedCornerShape(12.dp),
            color = MaterialTheme.colorScheme.surfaceContainerHighest,
            modifier = Modifier.heightIn(min = 40.dp),
        ) {
            Box(Modifier.padding(horizontal = 12.dp), contentAlignment = Alignment.Center) { Text(label, style = MaterialTheme.typography.bodyLarge, maxLines = 1) }
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
private fun FolderPicker(tree: List<FolderRow>, selected: String?, onDismiss: () -> Unit, onPick: (FolderRow) -> Unit) {
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
                            .clickable { onPick(row) }
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
