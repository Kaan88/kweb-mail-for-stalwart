package ovh.kweb.mail.ui.enterprise

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.KeyboardArrowDown
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.calendar.GroupColumn
import ovh.kweb.mail.ui.calendar.NoticeBar
import ovh.kweb.mail.ui.list.Groups
import uniffi.kweb_core.DeletedItem

/** "I deleted it by mistake": the account to look in, what the server still keeps by kind, and Restore behind a question. */
@Composable
fun RecoveryScreen(onBack: () -> Unit, vm: RecoveryViewModel = viewModel()) {
    val c = vm.controller
    val state by c.state.collectAsStateWithLifecycle()
    LaunchedEffect(vm) { c.start() }

    EnterpriseScaffold(stringResource(R.string.ent_recovery_title), onBack) { padding ->
        Column(Modifier.padding(padding).fillMaxSize()) {
            state.notice?.let { NoticeBar(errorText(it), error = false, action = stringResource(R.string.ent_dismiss) to c::dismissNotice) }
            val error = state.error
            if (error != null && state.recovery != null) {
                NoticeBar(errorText(error), error = true, action = stringResource(R.string.ent_dismiss) to c::dismissError)
            }
            val recovery = state.recovery
            when {
                recovery == null && state.phase == Phase.FAILED -> FailedBox(error, R.string.ent_recovery_failed, c::load)
                recovery == null -> LoadingBox()
                !recovery.isOn -> EmptyState(
                    R.drawable.ic_archive,
                    stringResource(R.string.ent_recovery_off),
                    stringResource(R.string.ent_recovery_off_body),
                    extra = stringResource(R.string.ent_recovery_off_note),
                )
                else -> Items(state, c)
            }
        }
    }
    RecoveryDialogs(state, c)
}

@Composable
private fun Items(state: RecoveryScreenState, c: RecoveryController) {
    val keeps = state.recovery?.keepsDays
    val now = System.currentTimeMillis()
    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(bottom = 32.dp)) {
        GroupColumn {
            GroupRow(Groups.shape(0, 1), onClick = if (state.canPick) c::askAccount else null) {
                Icon(painterResource(R.drawable.ic_person), contentDescription = null, tint = MaterialTheme.colorScheme.onSurfaceVariant)
                Column(Modifier.weight(1f)) {
                    Text(
                        if (state.account.other) state.account.display else stringResource(R.string.ent_account_own),
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        style = MaterialTheme.typography.bodyLarge,
                    )
                    Text(stringResource(R.string.ent_account_caption), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
                if (state.canPick) Icon(Icons.Filled.KeyboardArrowDown, contentDescription = stringResource(R.string.ent_account_choose))
            }
        }
        if (keeps != null) SectionTitle(pluralStringResource(R.plurals.ent_kept_for, keeps.toInt(), keeps.toInt()))
        when {
            state.listing -> LoadingBox(Modifier.fillMaxWidth().padding(32.dp))
            state.items.isEmpty() -> QuietNote(stringResource(R.string.ent_recovery_none))
            else -> RecoveryRules.byKind(state.items).forEach { (kind, items) ->
                SectionTitle(stringResource(RecoveryRules.kindTitle(kind)))
                GroupColumn {
                    items.forEachIndexed { index, item ->
                        ItemRow(Groups.shape(index, items.size), item, RecoveryRules.kindIcon(kind), keeps, now, enabled = !state.busy) { c.askRestore(item.id) }
                    }
                }
            }
        }
        if (state.loadingMore) {
            LoadingBox(Modifier.fillMaxWidth().padding(16.dp))
        } else if (state.hasMore && !state.listing) {
            TextButton(onClick = c::loadMore, modifier = Modifier.padding(start = 20.dp)) { Text(stringResource(R.string.ent_load_more)) }
        }
    }
}

@Composable
private fun ItemRow(shape: androidx.compose.ui.graphics.Shape, item: DeletedItem, icon: Int, keeps: UInt?, now: Long, enabled: Boolean, onRestore: () -> Unit) {
    GroupRow(shape) {
        Icon(painterResource(icon), contentDescription = null, tint = MaterialTheme.colorScheme.onSurfaceVariant)
        Column(Modifier.weight(1f)) {
            Text(item.title.ifBlank { stringResource(R.string.ent_untitled) }, maxLines = 2, overflow = TextOverflow.Ellipsis, style = MaterialTheme.typography.bodyLarge)
            val at = item.deletedAt
            if (at != null) {
                Text(stringResource(R.string.ent_deleted_on, RecoveryRules.deletedDay(at)), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
            val left = RecoveryRules.daysLeft(at, keeps, now)
            if (left != null) {
                Text(pluralStringResource(R.plurals.ent_days_left, left, left), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
        }
        TextButton(onClick = onRestore, enabled = enabled) { Text(stringResource(R.string.ent_restore)) }
    }
}

@Composable
private fun RecoveryDialogs(state: RecoveryScreenState, c: RecoveryController) {
    when (val d = state.dialog) {
        null -> Unit
        is RecoveryDialog.Restore -> AlertDialog(
            onDismissRequest = c::dismissDialog,
            title = { Text(stringResource(R.string.ent_restore_title)) },
            text = { Text(stringResource(R.string.ent_restore_body, d.item.title.ifBlank { stringResource(R.string.ent_untitled) })) },
            confirmButton = { TextButton(onClick = c::confirmRestore) { Text(stringResource(R.string.ent_restore)) } },
            dismissButton = { TextButton(onClick = c::dismissDialog) { Text(stringResource(R.string.cancel)) } },
        )
        is RecoveryDialog.Account -> AlertDialog(
            onDismissRequest = c::dismissDialog,
            title = { Text(stringResource(R.string.ent_account_choose)) },
            text = {
                Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                    ChoiceRow(stringResource(R.string.ent_account_own), !d.choice.other) { c.setAccountChoice(d.choice.copy(other = false)) }
                    ChoiceRow(stringResource(R.string.ent_account_other), d.choice.other) { c.setAccountChoice(d.choice.copy(other = true)) }
                    if (d.choice.other) {
                        OutlinedTextField(
                            value = d.query,
                            onValueChange = c::searchAccounts,
                            modifier = Modifier.fillMaxWidth(),
                            singleLine = true,
                            label = { Text(stringResource(R.string.ent_account_search)) },
                            isError = d.error != null,
                            supportingText = { Text(d.error?.let { errorText(it) } ?: stringResource(R.string.ent_account_search_help)) },
                            keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search),
                        )
                        if (d.choice.id.isNotBlank()) Text(d.choice.display, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.primary)
                        if (d.searching) LoadingBox(Modifier.fillMaxWidth().padding(8.dp))
                        d.matches.forEach { m ->
                            ChoiceRow(m.label + if (m.detail.isBlank()) "" else " (${m.detail})", d.choice.id == m.id) { c.pickAccount(m) }
                        }
                    }
                }
            },
            confirmButton = { TextButton(onClick = c::applyAccount) { Text(stringResource(R.string.ent_account_show)) } },
            dismissButton = { TextButton(onClick = c::dismissDialog) { Text(stringResource(R.string.cancel)) } },
        )
    }
}

@Composable
private fun ChoiceRow(text: String, selected: Boolean, onSelect: () -> Unit) {
    androidx.compose.foundation.layout.Row(
        Modifier.fillMaxWidth().selectable(selected, onClick = onSelect, role = Role.RadioButton),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        RadioButton(selected = selected, onClick = null, modifier = Modifier.padding(end = 12.dp).size(24.dp))
        Text(text, style = MaterialTheme.typography.bodyLarge)
    }
}
