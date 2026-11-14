package ovh.kweb.mail.ui.filters

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.KeyboardArrowDown
import androidx.compose.material.icons.filled.KeyboardArrowUp
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.Button
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.theme.KwebSwitch
import ovh.kweb.mail.ui.Route
import ovh.kweb.mail.ui.calendar.GroupColumn
import ovh.kweb.mail.ui.calendar.NoticeBar
import ovh.kweb.mail.ui.list.Groups

/**
 * The mail filters: the switch, the rules in order, and the editor for one rule (shown in place of the list
 * while it is open). [onBack] leaves the screen.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun FiltersScreen(route: Route.Filters, onBack: () -> Unit, vm: FiltersViewModel = viewModel()) {
    val c = vm.controller
    val state by c.state.collectAsStateWithLifecycle()
    LaunchedEffect(vm) { c.start(route.fromMessageId) }

    val editing = state.editor != null
    val leave = { if (state.busy) Unit else if (editing) c.leaveEditor() else onBack() }
    BackHandler(onBack = leave)

    Scaffold(
        topBar = {
            TopAppBar(
                title = {
                    val title = when {
                        state.editor == null -> R.string.filters_title
                        state.editor?.isNew == true -> R.string.filters_editor_new
                        else -> R.string.filters_editor_edit
                    }
                    Text(stringResource(title))
                },
                navigationIcon = {
                    IconButton(onClick = leave, enabled = !state.busy) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = stringResource(R.string.back))
                    }
                },
                colors = TopAppBarDefaults.topAppBarColors(containerColor = MaterialTheme.colorScheme.background),
            )
        },
        containerColor = MaterialTheme.colorScheme.background,
    ) { padding ->
        Column(Modifier.padding(padding).fillMaxSize()) {
            state.error?.let { NoticeBar(it.resolve(LocalResources.current), error = true, action = stringResource(R.string.contacts_dismiss) to c::dismissError) }
            state.info?.let { NoticeBar(it.resolve(LocalResources.current), error = false, action = stringResource(R.string.contacts_dismiss) to c::dismissInfo) }
            val editor = state.editor
            when {
                editor != null -> RuleEditor(state, editor, c)
                state.phase == FiltersPhase.LOADING -> Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) { CircularProgressIndicator() }
                state.phase == FiltersPhase.FAILED -> Message(stringResource(R.string.filters_load_failed), retry = c::load)
                state.phase == FiltersPhase.NONE -> Message(state.unsupported?.resolve(LocalResources.current).orEmpty(), retry = c::load)
                else -> RuleList(state, c)
            }
        }
    }
    FiltersDialogs(state, c)
}

@Composable
private fun Message(text: String, retry: () -> Unit) {
    Column(Modifier.fillMaxSize().padding(32.dp), verticalArrangement = Arrangement.Center, horizontalAlignment = Alignment.CenterHorizontally) {
        Text(text, color = MaterialTheme.colorScheme.onSurfaceVariant, textAlign = TextAlign.Center)
        TextButton(onClick = retry) { Text(stringResource(R.string.retry)) }
    }
}

@Composable
private fun RuleList(state: FiltersState, c: FiltersController) {
    val filters = state.filters ?: return
    val words = rememberFilterWords()
    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(bottom = 32.dp)) {
        GroupColumn {
            RowFrame(
                Groups.shape(0, 1),
                Modifier.toggleable(value = filters.isActive, enabled = !state.busy, role = Role.Switch, onValueChange = c::setActive),
            ) {
                Column(Modifier.weight(1f)) {
                    Text(stringResource(R.string.filters_switch), style = MaterialTheme.typography.bodyLarge)
                    Text(
                        stringResource(R.string.filters_switch_detail),
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        style = MaterialTheme.typography.bodyMedium,
                    )
                }
                KwebSwitch(checked = filters.isActive, onCheckedChange = null, enabled = !state.busy)
            }
        }
        Banners(filters, c)
        if (!filters.isEdited) {
            if (state.rules.isEmpty()) {
                Text(
                    stringResource(R.string.filters_empty),
                    modifier = Modifier.padding(horizontal = 28.dp, vertical = 16.dp),
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    style = MaterialTheme.typography.bodyMedium,
                )
            }
            if (state.rules.isNotEmpty() && !filters.isActive) {
                Text(
                    stringResource(R.string.filters_off_hint),
                    modifier = Modifier.padding(horizontal = 28.dp, vertical = 8.dp),
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    style = MaterialTheme.typography.bodyMedium,
                )
            }
            GroupColumn(Modifier.padding(top = 8.dp)) {
                state.rules.forEachIndexed { index, rule ->
                    RuleRow(
                        shape = Groups.shape(index, state.rules.size),
                        name = rule.name.ifBlank { stringResource(R.string.filters_rule_unnamed, index + 1) },
                        summary = remember(rule, state.folders, words) { FilterRules.summary(rule, { FilterRules.folderName(state.folders, it) }, words) },
                        rule = rule,
                        first = index == 0,
                        last = index == state.rules.lastIndex,
                        busy = state.busy,
                        onOpen = { c.openRule(index) },
                        onUp = { c.moveRule(index, -1) },
                        onDown = { c.moveRule(index, 1) },
                        onEnabled = { c.setRuleEnabled(index, it) },
                    )
                }
            }
            Button(
                onClick = c::addRule,
                enabled = !state.busy,
                shape = CircleShape,
                modifier = Modifier.fillMaxWidth().padding(horizontal = 12.dp).padding(top = 16.dp).heightIn(min = 52.dp),
            ) {
                Icon(Icons.Filled.Add, contentDescription = null)
                Text(stringResource(R.string.filters_add), modifier = Modifier.padding(start = 8.dp), style = MaterialTheme.typography.labelLarge)
            }
        }
        Text(
            stringResource(R.string.filters_inbox_note),
            modifier = Modifier.padding(horizontal = 28.dp, vertical = 16.dp),
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            style = MaterialTheme.typography.bodySmall,
        )
    }
}

@Composable
private fun Banners(filters: uniffi.kweb_core.Filters, c: FiltersController) {
    for (banner in FilterRules.banners(filters)) {
        when (banner) {
            Banner.Edited -> BannerBar(
                stringResource(R.string.filters_banner_edited),
                listOf(stringResource(R.string.filters_show) to c::showOwnScript, stringResource(R.string.filters_start_over) to c::askStartOver),
            )
            Banner.Stale -> BannerBar(stringResource(R.string.filters_banner_stale), listOf(stringResource(R.string.filters_update) to c::update))
            Banner.AwayOn -> BannerBar(stringResource(R.string.filters_banner_away), emptyList())
            is Banner.OtherScript -> BannerBar(
                stringResource(R.string.filters_banner_other, banner.name),
                listOf(stringResource(R.string.filters_show) to c::showOtherScript),
            )
        }
    }
}

/** A notice with its buttons under the text. */
@Composable
private fun BannerBar(text: String, actions: List<Pair<String, () -> Unit>>) {
    val colors = MaterialTheme.colorScheme
    Column(
        Modifier
            .fillMaxWidth()
            .padding(horizontal = 14.dp, vertical = 4.dp)
            .clip(RoundedCornerShape(14.dp))
            .background(colors.tertiaryContainer)
            .padding(start = 14.dp, end = 6.dp, top = 10.dp, bottom = if (actions.isEmpty()) 10.dp else 2.dp),
    ) {
        Text(text, color = colors.onTertiaryContainer, style = MaterialTheme.typography.bodyMedium)
        if (actions.isNotEmpty()) {
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
                for ((label, action) in actions) TextButton(onClick = action) { Text(label, color = colors.onTertiaryContainer) }
            }
        }
    }
}

@Composable
private fun RuleRow(
    shape: Shape,
    name: String,
    summary: String,
    rule: uniffi.kweb_core.FilterRule,
    first: Boolean,
    last: Boolean,
    busy: Boolean,
    onOpen: () -> Unit,
    onUp: () -> Unit,
    onDown: () -> Unit,
    onEnabled: (Boolean) -> Unit,
) {
    val colors = MaterialTheme.colorScheme
    Row(
        Modifier.fillMaxWidth().clip(shape).background(colors.surfaceContainerLow).heightIn(min = 64.dp).padding(start = 20.dp, end = 12.dp, top = 6.dp, bottom = 6.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f).clickable(onClick = onOpen).alpha(if (rule.enabled) 1f else 0.5f).padding(vertical = 4.dp)) {
            Text(name, style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(summary, color = colors.onSurfaceVariant, style = MaterialTheme.typography.bodyMedium, maxLines = 2, overflow = TextOverflow.Ellipsis)
            if (rule.folderMissing) {
                Text(stringResource(R.string.filters_folder_missing), color = colors.error, style = MaterialTheme.typography.bodySmall)
            }
        }
        IconButton(onClick = onUp, enabled = !first && !busy) {
            Icon(Icons.Filled.KeyboardArrowUp, contentDescription = stringResource(R.string.filters_move_up, name))
        }
        IconButton(onClick = onDown, enabled = !last && !busy) {
            Icon(Icons.Filled.KeyboardArrowDown, contentDescription = stringResource(R.string.filters_move_down, name))
        }
        val switchLabel = stringResource(R.string.filters_rule_switch, name)
        KwebSwitch(
            checked = rule.enabled,
            onCheckedChange = onEnabled,
            enabled = !busy,
            modifier = Modifier.semantics { contentDescription = switchLabel },
        )
    }
}

@Composable
internal fun RowFrame(shape: Shape, modifier: Modifier = Modifier, content: @Composable androidx.compose.foundation.layout.RowScope.() -> Unit) {
    Row(
        modifier.fillMaxWidth().clip(shape).background(MaterialTheme.colorScheme.surfaceContainerLow).heightIn(min = 56.dp).padding(horizontal = 20.dp, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) { content() }
}

@Composable
private fun rememberFilterWords(): FilterWords {
    val res = LocalResources.current
    return remember(res) {
        FilterWords(
            from = res.getString(R.string.filters_sum_from),
            recipient = res.getString(R.string.filters_sum_to),
            subject = res.getString(R.string.filters_sum_subject),
            header = { res.getString(R.string.filters_sum_header, it) },
            contains = res.getString(R.string.filters_sum_contains),
            isExactly = res.getString(R.string.filters_sum_is),
            beginsWith = res.getString(R.string.filters_sum_begins),
            endsWith = res.getString(R.string.filters_sum_ends),
            and = res.getString(R.string.filters_sum_and),
            or = res.getString(R.string.filters_sum_or),
            moveTo = { res.getString(R.string.filters_sum_move_to, it) },
            moveToUnknown = res.getString(R.string.filters_sum_move_unknown),
            markRead = res.getString(R.string.filters_sum_mark_read),
            star = res.getString(R.string.filters_sum_star),
            trash = res.getString(R.string.filters_sum_trash),
            stop = res.getString(R.string.filters_sum_stop),
            noConditions = res.getString(R.string.filters_sum_no_conditions),
            noActions = res.getString(R.string.filters_sum_no_actions),
            arrow = res.getString(R.string.filters_sum_arrow),
            separator = res.getString(R.string.filters_sum_separator),
        )
    }
}

@Composable
private fun FiltersDialogs(state: FiltersState, c: FiltersController) {
    when (val d = state.dialog) {
        null -> Unit
        is FiltersDialog.SwitchOn -> AlertDialog(
            onDismissRequest = c::dismissDialog,
            title = { Text(stringResource(R.string.filters_switch_on_title)) },
            text = {
                Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    for (ask in d.asks) {
                        Text(
                            when (ask) {
                                SwitchOff.Away -> stringResource(R.string.filters_switch_on_away)
                                is SwitchOff.Other -> stringResource(R.string.filters_switch_on_other, ask.name)
                            },
                        )
                    }
                }
            },
            confirmButton = { TextButton(onClick = c::confirmSwitchOn) { Text(stringResource(R.string.filters_switch_on)) } },
            dismissButton = { TextButton(onClick = c::dismissDialog) { Text(stringResource(R.string.cancel)) } },
        )
        FiltersDialog.StartOver -> AlertDialog(
            onDismissRequest = c::dismissDialog,
            title = { Text(stringResource(R.string.filters_start_over_title)) },
            text = { Text(stringResource(R.string.filters_start_over_text)) },
            confirmButton = { TextButton(onClick = c::confirmStartOver) { Text(stringResource(R.string.filters_start_over_confirm)) } },
            dismissButton = { TextButton(onClick = c::dismissDialog) { Text(stringResource(R.string.cancel)) } },
        )
        FiltersDialog.DeleteRule -> AlertDialog(
            onDismissRequest = c::dismissDialog,
            title = { Text(stringResource(R.string.filters_delete_rule_title)) },
            text = { Text(stringResource(R.string.filters_delete_rule_text)) },
            confirmButton = { TextButton(onClick = c::confirmDeleteRule) { Text(stringResource(R.string.filters_delete_rule), color = MaterialTheme.colorScheme.error) } },
            dismissButton = { TextButton(onClick = c::dismissDialog) { Text(stringResource(R.string.cancel)) } },
        )
        FiltersDialog.DiscardEditor -> AlertDialog(
            onDismissRequest = c::dismissDialog,
            title = { Text(stringResource(R.string.filters_discard_title)) },
            text = { Text(stringResource(R.string.filters_discard_text)) },
            confirmButton = { TextButton(onClick = c::discardEditor) { Text(stringResource(R.string.filters_discard)) } },
            dismissButton = { TextButton(onClick = c::dismissDialog) { Text(stringResource(R.string.filters_keep_editing)) } },
        )
        is FiltersDialog.Script -> AlertDialog(
            onDismissRequest = c::dismissDialog,
            title = {
                Text(if (d.name == null) stringResource(R.string.filters_script_title) else stringResource(R.string.filters_script_title_named, d.name))
            },
            text = {
                Box(Modifier.verticalScroll(rememberScrollState())) {
                    androidx.compose.foundation.text.selection.SelectionContainer {
                        Text(d.text, style = MaterialTheme.typography.bodySmall, fontFamily = androidx.compose.ui.text.font.FontFamily.Monospace)
                    }
                }
            },
            confirmButton = { TextButton(onClick = c::dismissDialog) { Text(stringResource(R.string.filters_close)) } },
        )
    }
}
