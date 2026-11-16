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
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.KeyboardArrowDown
import androidx.compose.material.icons.filled.KeyboardArrowUp
import androidx.compose.material.icons.filled.Lock
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TextField
import androidx.compose.material3.TextFieldDefaults
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
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.Route
import ovh.kweb.mail.ui.calendar.GroupColumn
import ovh.kweb.mail.ui.calendar.NoticeBar
import ovh.kweb.mail.ui.list.Groups
import ovh.kweb.mail.ui.theme.KwebSwitch
import uniffi.kweb_core.FiltersState

/**
 * The mail filters: the switch, the rules in order, and the editor for one rule or for the script's text (shown in
 * place of the list while it is open). [onBack] leaves the screen.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun FiltersScreen(route: Route.Filters, onBack: () -> Unit, vm: FiltersViewModel = viewModel()) {
    val c = vm.controller
    val state by c.state.collectAsStateWithLifecycle()
    LaunchedEffect(vm) { c.start(route.fromMessageId) }

    val leave = {
        when {
            state.busy -> Unit
            state.editor != null -> c.leaveEditor()
            state.raw != null -> c.leaveRaw()
            else -> onBack()
        }
    }
    BackHandler(onBack = leave)

    Scaffold(
        topBar = {
            TopAppBar(
                title = {
                    val title = when {
                        state.editor?.isNew == true -> R.string.filters_editor_new
                        state.editor != null -> R.string.filters_editor_edit
                        state.raw != null -> R.string.filters_script_title
                        else -> R.string.filters_title
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
            val res = LocalResources.current
            state.error?.let { NoticeBar(it.resolve(res), error = true, action = stringResource(R.string.contacts_dismiss) to c::dismissError) }
            state.info?.let { NoticeBar(it.resolve(res), error = false, action = stringResource(R.string.contacts_dismiss) to c::dismissInfo) }
            state.undo?.let { u ->
                NoticeBar(
                    stringResource(R.string.filters_deleted, u.ruleName.ifBlank { stringResource(R.string.filters_title) }),
                    error = false,
                    action = stringResource(R.string.filters_undo) to c::undoDelete,
                )
            }
            val editor = state.editor
            when {
                editor != null -> RuleEditor(state, editor, c)
                state.raw != null -> RawEditor(state, state.raw!!, c)
                state.phase == FiltersPhase.LOADING -> Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) { CircularProgressIndicator() }
                state.phase == FiltersPhase.FAILED -> Message(stringResource(R.string.filters_load_failed), retry = c::load)
                state.phase == FiltersPhase.NONE -> Message(state.unsupported?.resolve(res).orEmpty(), retry = c::load)
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
private fun Quiet(text: String, bottom: Int = 16) {
    Text(
        text,
        modifier = Modifier.padding(start = 28.dp, end = 28.dp, top = 8.dp, bottom = bottom.dp),
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        style = MaterialTheme.typography.bodyMedium,
    )
}

@Composable
private fun RuleList(state: FiltersUi, c: FiltersController) {
    val core = state.core ?: return
    val res = LocalResources.current
    val words = remember(res) { FilterLabels.words(res) }
    val preview = core.migrationPending
    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(bottom = 32.dp)) {
        GroupColumn {
            RowFrame(
                Groups.shape(0, 1),
                Modifier.toggleable(value = core.active, enabled = !state.busy && !preview, role = Role.Switch, onValueChange = c::setEnabled),
            ) {
                Column(Modifier.weight(1f)) {
                    Text(stringResource(R.string.filters_switch), style = MaterialTheme.typography.bodyLarge)
                    Text(
                        stringResource(R.string.filters_switch_detail),
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        style = MaterialTheme.typography.bodyMedium,
                    )
                }
                KwebSwitch(checked = core.active, onCheckedChange = null, enabled = !state.busy && !preview)
            }
        }
        Banners(state, core, c)
        if (state.textOnly) {
            TextOnly(core, state, c)
        } else {
            if (state.rules.isEmpty() && !preview) Quiet(stringResource(R.string.filters_empty))
            if (state.rules.isNotEmpty() && !core.active && !preview) Quiet(stringResource(R.string.filters_off_hint), bottom = 8)
            if (state.rules.isNotEmpty()) {
                GroupColumn(Modifier.padding(top = 8.dp)) {
                    state.rules.forEachIndexed { index, rule ->
                        val name = rule.name.ifBlank { stringResource(R.string.filters_rule_unnamed, index + 1) }
                        RuleRow(
                            shape = Groups.shape(index, state.rules.size),
                            name = name,
                            summary = remember(rule, words) { RuleModel.summary(rule, words) },
                            rule = rule,
                            first = index == 0,
                            last = index == state.rules.lastIndex,
                            enabled = !state.busy && !preview,
                            onOpen = { c.openRule(rule.id) },
                            onUp = { c.moveRule(rule.id, up = true) },
                            onDown = { c.moveRule(rule.id, up = false) },
                            onDelete = { c.deleteRule(rule.id) },
                            onEnabled = { c.setRuleEnabled(rule.id, it) },
                        )
                    }
                }
            }
            if (!preview) {
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
        }
        if (core.externalBlocks.isNotEmpty()) {
            Heading(stringResource(R.string.filters_external_title))
            GroupColumn {
                core.externalBlocks.forEachIndexed { index, block ->
                    ExternalRow(Groups.shape(index, core.externalBlocks.size), block.originLabel, block.raw)
                }
            }
            Quiet(stringResource(R.string.filters_external_note), bottom = 0)
        }
        if (!state.textOnly && !preview) {
            GroupColumn(Modifier.padding(top = 16.dp)) {
                RowFrame(Groups.shape(0, 1), Modifier.clickable(enabled = !state.busy, onClick = c::openRaw)) {
                    Column(Modifier.weight(1f)) {
                        Text(stringResource(R.string.filters_edit_text), style = MaterialTheme.typography.bodyLarge)
                        Text(
                            stringResource(R.string.filters_edit_text_detail),
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            style = MaterialTheme.typography.bodyMedium,
                        )
                    }
                }
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

/** A script the editor cannot show as rules: its text, and the two ways out. */
@Composable
private fun TextOnly(core: FiltersState, state: FiltersUi, c: FiltersController) {
    GroupColumn(Modifier.padding(top = 8.dp)) {
        ExternalRow(Groups.shape(0, 1), core.scriptName.ifBlank { stringResource(R.string.filters_script_title) }, core.rawText)
    }
    GroupColumn(Modifier.padding(top = 16.dp)) {
        RowFrame(Groups.shape(0, 2), Modifier.clickable(enabled = !state.busy, onClick = c::openRaw)) {
            Text(stringResource(R.string.filters_edit_text), modifier = Modifier.weight(1f), style = MaterialTheme.typography.bodyLarge)
        }
        RowFrame(Groups.shape(1, 2), Modifier.clickable(enabled = !state.busy, onClick = c::askReset)) {
            Text(
                stringResource(R.string.filters_reset),
                modifier = Modifier.weight(1f),
                style = MaterialTheme.typography.bodyLarge,
                color = MaterialTheme.colorScheme.error,
            )
        }
    }
}

@Composable
private fun Banners(state: FiltersUi, core: FiltersState, c: FiltersController) {
    val caps = core.capabilities
    if (core.opaque) BannerBar(stringResource(R.string.filters_banner_opaque), emptyList())
    if (core.foreign) BannerBar(stringResource(R.string.filters_banner_foreign), emptyList())
    if (core.migrationPending) {
        BannerBar(
            stringResource(R.string.filters_banner_migrate),
            listOf(stringResource(R.string.filters_migrate) to c::askMigrate),
        )
    } else if (core.oldScriptPresent && !state.textOnly) {
        BannerBar(stringResource(R.string.filters_banner_old), listOf(stringResource(R.string.filters_remove_old) to c::askRemoveOld))
    }
    if (core.displaced && !core.active) {
        BannerBar(stringResource(R.string.filters_banner_displaced), listOf(stringResource(R.string.filters_switch_on) to { c.setEnabled(true) }))
    }
    if (core.differs && !state.textOnly && !core.migrationPending) {
        BannerBar(stringResource(R.string.filters_banner_differs), listOf(stringResource(R.string.filters_update) to c::update))
    }
    val limit = caps.redirectLimit
    if (limit != null && limit == 0u && state.rules.any { r -> r.actions.any { it.type == "forward" } }) {
        BannerBar(stringResource(R.string.filters_banner_forward_off), emptyList())
    } else if (limit != null && limit > 0u && caps.maxForwardsNow > limit) {
        BannerBar(stringResource(R.string.filters_banner_forward_limit, caps.maxForwardsNow.toInt(), limit.toInt()), emptyList())
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
internal fun Heading(text: String) {
    Text(
        text.uppercase(androidx.compose.ui.text.intl.Locale.current.platformLocale),
        modifier = Modifier.padding(start = 32.dp, end = 20.dp, top = 20.dp, bottom = 8.dp),
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        style = MaterialTheme.typography.labelMedium,
        fontWeight = androidx.compose.ui.text.font.FontWeight.Bold,
        letterSpacing = 0.8.sp,
    )
}

@Composable
private fun RuleRow(
    shape: Shape,
    name: String,
    summary: String,
    rule: RuleDraft,
    first: Boolean,
    last: Boolean,
    enabled: Boolean,
    onOpen: () -> Unit,
    onUp: () -> Unit,
    onDown: () -> Unit,
    onDelete: () -> Unit,
    onEnabled: (Boolean) -> Unit,
) {
    val colors = MaterialTheme.colorScheme
    Column(Modifier.fillMaxWidth().clip(shape).background(colors.surfaceContainerLow).padding(start = 20.dp, end = 8.dp, top = 8.dp, bottom = 2.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f).clickable(enabled = enabled, onClick = onOpen).alpha(if (rule.enabled) 1f else 0.5f).padding(vertical = 4.dp)) {
                Text(name, style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(summary, color = colors.onSurfaceVariant, style = MaterialTheme.typography.bodyMedium, maxLines = 2, overflow = TextOverflow.Ellipsis)
                if (rule.hasPeriod) {
                    Text(
                        stringResource(R.string.filters_period_badge),
                        modifier = Modifier.padding(top = 4.dp).clip(RoundedCornerShape(8.dp)).background(colors.secondaryContainer).padding(horizontal = 8.dp, vertical = 2.dp),
                        color = colors.onSecondaryContainer,
                        style = MaterialTheme.typography.labelSmall,
                    )
                }
            }
            val switchLabel = stringResource(R.string.filters_rule_switch, name)
            KwebSwitch(
                checked = rule.enabled,
                onCheckedChange = onEnabled,
                enabled = enabled,
                modifier = Modifier.padding(start = 8.dp, end = 8.dp).semantics { contentDescription = switchLabel },
            )
        }
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
            IconButton(onClick = onUp, enabled = !first && enabled, modifier = Modifier.size(40.dp)) {
                Icon(Icons.Filled.KeyboardArrowUp, contentDescription = stringResource(R.string.filters_move_up, name))
            }
            IconButton(onClick = onDown, enabled = !last && enabled, modifier = Modifier.size(40.dp)) {
                Icon(Icons.Filled.KeyboardArrowDown, contentDescription = stringResource(R.string.filters_move_down, name))
            }
            IconButton(onClick = onDelete, enabled = enabled, modifier = Modifier.size(40.dp)) {
                Icon(Icons.Filled.Delete, contentDescription = stringResource(R.string.filters_delete_named, name))
            }
        }
    }
}

/** Another tool's rules, or a script as text: a lock, where it came from, and the text in monospace. */
@Composable
private fun ExternalRow(shape: Shape, origin: String, raw: String) {
    val colors = MaterialTheme.colorScheme
    Column(Modifier.fillMaxWidth().clip(shape).background(colors.surfaceContainerLow).padding(horizontal = 20.dp, vertical = 12.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Icon(Icons.Filled.Lock, contentDescription = null, modifier = Modifier.size(18.dp), tint = colors.onSurfaceVariant)
            Text(stringResource(R.string.filters_external_label, origin), style = MaterialTheme.typography.bodyLarge)
        }
        SelectionContainer {
            Text(
                raw,
                modifier = Modifier.padding(top = 8.dp),
                style = MaterialTheme.typography.bodySmall,
                fontFamily = FontFamily.Monospace,
                color = colors.onSurfaceVariant,
            )
        }
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

/** The script as text, to be changed by hand. */
@Composable
private fun RawEditor(state: FiltersUi, raw: RawState, c: FiltersController) {
    val container = MaterialTheme.colorScheme.surfaceContainerLow
    Column(Modifier.fillMaxSize().imePadding().verticalScroll(rememberScrollState()).padding(bottom = 32.dp)) {
        Quiet(stringResource(R.string.filters_raw_note), bottom = 8)
        TextField(
            value = raw.text,
            onValueChange = c::editRaw,
            label = { Text(stringResource(R.string.filters_raw_hint)) },
            minLines = 12,
            textStyle = MaterialTheme.typography.bodySmall.copy(fontFamily = FontFamily.Monospace),
            shape = RoundedCornerShape(20.dp),
            colors = TextFieldDefaults.colors(
                focusedContainerColor = container,
                unfocusedContainerColor = container,
                focusedIndicatorColor = Color.Transparent,
                unfocusedIndicatorColor = Color.Transparent,
            ),
            modifier = Modifier.fillMaxWidth().padding(horizontal = 12.dp),
        )
        Button(
            onClick = c::saveRaw,
            enabled = raw.dirty && !state.busy,
            shape = CircleShape,
            modifier = Modifier.fillMaxWidth().padding(horizontal = 12.dp).padding(top = 24.dp).heightIn(min = 52.dp),
        ) {
            if (state.busy) {
                CircularProgressIndicator(Modifier.size(20.dp), strokeWidth = 2.dp, color = MaterialTheme.colorScheme.onPrimary)
            } else {
                Text(stringResource(R.string.filters_raw_save), style = MaterialTheme.typography.labelLarge)
            }
        }
        TextButton(onClick = c::askReset, enabled = !state.busy, modifier = Modifier.align(Alignment.CenterHorizontally).padding(top = 8.dp)) {
            Text(stringResource(R.string.filters_reset), color = MaterialTheme.colorScheme.error)
        }
    }
}

@Composable
private fun FiltersDialogs(state: FiltersUi, c: FiltersController) {
    when (val d = state.dialog) {
        null -> Unit
        FiltersDialog.SwitchOn -> AlertDialog(
            onDismissRequest = c::dismissDialog,
            title = { Text(stringResource(R.string.filters_switch_on_title)) },
            text = { Text(stringResource(R.string.filters_switch_on_away)) },
            confirmButton = { TextButton(onClick = c::confirmSwitchOn) { Text(stringResource(R.string.filters_switch_on)) } },
            dismissButton = { TextButton(onClick = c::dismissDialog) { Text(stringResource(R.string.cancel)) } },
        )
        FiltersDialog.DiscardEditor -> DiscardDialog(c::discardEditor, c::dismissDialog)
        FiltersDialog.DiscardRaw -> DiscardDialog(c::discardRaw, c::dismissDialog)
        FiltersDialog.ResetToVisual -> AlertDialog(
            onDismissRequest = c::dismissDialog,
            title = { Text(stringResource(R.string.filters_reset_title)) },
            text = { Text(stringResource(R.string.filters_reset_text)) },
            confirmButton = { TextButton(onClick = c::confirmReset) { Text(stringResource(R.string.filters_reset_confirm), color = MaterialTheme.colorScheme.error) } },
            dismissButton = { TextButton(onClick = c::dismissDialog) { Text(stringResource(R.string.cancel)) } },
        )
        is FiltersDialog.Migrate -> AlertDialog(
            onDismissRequest = c::dismissDialog,
            title = { Text(stringResource(R.string.filters_migrate_title)) },
            text = { Text(stringResource(R.string.filters_migrate_text)) },
            confirmButton = { TextButton(onClick = c::confirmMigrate) { Text(stringResource(R.string.filters_migrate)) } },
            dismissButton = { TextButton(onClick = c::dismissDialog) { Text(stringResource(R.string.cancel)) } },
        )
        FiltersDialog.RemoveOld -> AlertDialog(
            onDismissRequest = c::dismissDialog,
            title = { Text(stringResource(R.string.filters_remove_old_title)) },
            text = { Text(stringResource(R.string.filters_remove_old_text)) },
            confirmButton = { TextButton(onClick = c::confirmRemoveOld) { Text(stringResource(R.string.filters_remove_old), color = MaterialTheme.colorScheme.error) } },
            dismissButton = { TextButton(onClick = c::dismissDialog) { Text(stringResource(R.string.cancel)) } },
        )
    }
}

@Composable
private fun DiscardDialog(onDiscard: () -> Unit, onKeep: () -> Unit) {
    AlertDialog(
        onDismissRequest = onKeep,
        title = { Text(stringResource(R.string.filters_discard_title)) },
        text = { Text(stringResource(R.string.filters_discard_text)) },
        confirmButton = { TextButton(onClick = onDiscard) { Text(stringResource(R.string.filters_discard)) } },
        dismissButton = { TextButton(onClick = onKeep) { Text(stringResource(R.string.filters_keep_editing)) } },
    )
}
