package ovh.kweb.mail.ui.admin

import androidx.compose.foundation.background
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.LifecycleResumeEffect
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.account.rememberCopier
import ovh.kweb.mail.ui.account.systemConfirmsCopy
import ovh.kweb.mail.ui.list.EmptyNote
import ovh.kweb.mail.ui.list.Groups
import uniffi.kweb_core.DkimKey

private fun now() = System.currentTimeMillis() / 1000

/** The server's domains, with a plus to add one. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun DomainsScreen(onBack: () -> Unit, onOpen: (AdminScreen) -> Unit, vm: AdminDomainsViewModel = viewModel()) {
    val c = vm.domains
    val list by c.list.state.collectAsStateWithLifecycle()
    val state by c.state.collectAsStateWithLifecycle()
    LifecycleResumeEffect(vm) {
        c.refresh()
        onPauseOrDispose { }
    }
    AdminScaffold(
        title = stringResource(R.string.admin_area_domains),
        onBack = onBack,
        actions = {
            IconButton(onClick = c::openAdd, enabled = !state.busy) { Icon(Icons.Filled.Add, contentDescription = stringResource(R.string.admin_domain_add)) }
        },
    ) { padding ->
        Column(Modifier.padding(padding).fillMaxSize()) {
            Notices(state.error ?: list.error, state.info, { c.dismissError(); c.list.dismissError() }, c::dismissInfo)
            when (list.phase) {
                ListPhase.LOADING -> LoadingBox()
                ListPhase.FAILED -> FailureBox(stringResource(R.string.admin_domains_load_failed), onRetry = { c.list.reload(quiet = false) })
                ListPhase.READY -> PullToRefreshBox(isRefreshing = list.refreshing, onRefresh = c::refresh, modifier = Modifier.fillMaxSize()) {
                    if (list.items.isEmpty()) {
                        Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.Center) {
                            EmptyNote(R.drawable.ic_admin_domain, stringResource(R.string.admin_domains_none), modifier = Modifier.fillMaxWidth())
                        }
                    } else {
                        LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(start = 12.dp, end = 12.dp, bottom = 32.dp), verticalArrangement = Arrangement.spacedBy(3.dp)) {
                            itemsIndexed(list.items, key = { _, d -> d.id }) { index, domain ->
                                AdminRow(
                                    Groups.shape(index, list.items.size),
                                    domain.name,
                                    detail = domain.aliases.takeIf { it.isNotEmpty() }?.let { stringResource(R.string.admin_domain_aliases, it.joinToString(", ")) },
                                    trailing = { if (!domain.enabled) Pill(stringResource(R.string.admin_domain_off)) },
                                    onClick = { onOpen(AdminScreen.Domain(domain.id)) },
                                )
                            }
                        }
                    }
                }
            }
        }
    }
    state.adding?.let { name ->
        ConfirmDialog(
            title = stringResource(R.string.admin_domain_add),
            text = stringResource(R.string.admin_domain_add_text),
            confirm = stringResource(R.string.admin_domain_add_confirm),
            onConfirm = c::confirmAdd,
            onDismiss = c::dismissDialog,
            enabled = state.addValid && !state.busy,
        ) {
            AdminField(
                name,
                c::typeName,
                stringResource(R.string.admin_domain_name),
                isError = name.isNotBlank() && !state.addValid,
                supporting = if (name.isNotBlank() && !state.addValid) stringResource(R.string.admin_domain_name_bad) else null,
            )
        }
    }
}

/** One domain: the DNS records to publish (with Copy), the DKIM keys, mail on and off, Delete. */
@Composable
fun DomainScreen(id: String, onBack: () -> Unit, vm: AdminDomainViewModel = viewModel()) {
    val c = vm.forDomain(id)
    val state by c.state.collectAsStateWithLifecycle()
    LifecycleResumeEffect(vm) {
        c.refresh()
        onPauseOrDispose { }
    }
    LaunchedEffect(state.gone) { if (state.gone) onBack() }
    AdminScaffold(title = state.domain?.name ?: stringResource(R.string.admin_area_domains), onBack = onBack) { padding ->
        Column(Modifier.padding(padding).fillMaxSize()) {
            Notices(state.error, state.info, c::dismissError, c::dismissInfo)
            val domain = state.domain
            when {
                domain != null -> DomainBody(domain, state.busy, c)
                state.phase == DomainPhase.FAILED -> FailureBox(stringResource(R.string.admin_load_failed), onRetry = c::refresh)
                else -> LoadingBox()
            }
        }
    }
    val domain = state.domain
    when (val dialog = state.dialog) {
        DomainDialog.Disable -> ConfirmDialog(
            title = stringResource(R.string.admin_domain_disable_title, domain?.name.orEmpty()),
            text = stringResource(R.string.admin_domain_disable_text),
            confirm = stringResource(R.string.admin_domain_disable),
            onConfirm = c::confirmDisable,
            onDismiss = c::dismissDialog,
            danger = true,
        )
        is DomainDialog.Delete -> ConfirmDialog(
            title = stringResource(R.string.admin_domain_delete_title, domain?.name.orEmpty()),
            text = stringResource(R.string.admin_domain_delete_text),
            confirm = stringResource(R.string.admin_delete),
            onConfirm = c::confirmDelete,
            onDismiss = c::dismissDialog,
            danger = true,
            enabled = domain != null && AdminRules.deleteConfirmed(dialog.typed, domain.name) && !state.busy,
        ) {
            AdminField(dialog.typed, c::typeDelete, stringResource(R.string.admin_type_name))
        }
        null -> Unit
    }
}

@Composable
private fun DomainBody(domain: uniffi.kweb_core.DomainDetail, busy: Boolean, c: DomainController) {
    val colors = MaterialTheme.colorScheme
    val copy = rememberCopier(sensitive = false) { if (!systemConfirmsCopy) c.copied() }
    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(bottom = 32.dp)) {
        AdminGroup(Modifier.padding(top = 8.dp)) {
            AdminRow(
                Groups.shape(0, 1),
                stringResource(R.string.admin_domain_takes_mail),
                detail = stringResource(if (domain.enabled) R.string.admin_domain_takes_mail_on else R.string.admin_domain_takes_mail_off),
                trailing = { Switch(checked = domain.enabled, onCheckedChange = null, enabled = !busy) },
                onClick = if (busy) null else c::toggleEnabled,
            )
        }
        if (domain.aliases.isNotEmpty()) {
            SectionLabel(stringResource(R.string.admin_domain_other_names))
            AdminGroup { domain.aliases.forEachIndexed { i, alias -> AdminRow(Groups.shape(i, domain.aliases.size), alias) } }
        }
        SectionLabel(stringResource(R.string.admin_domain_dns))
        Note(stringResource(R.string.admin_domain_dns_note))
        AdminGroup {
            Column(
                Modifier.fillMaxWidth().clip(Groups.shape(0, 1)).background(colors.surfaceContainerLow).padding(horizontal = 16.dp, vertical = 12.dp),
                verticalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                if (domain.dnsZone.isBlank()) {
                    Text(stringResource(R.string.admin_domain_dns_none), color = colors.onSurfaceVariant, style = MaterialTheme.typography.bodyMedium)
                } else {
                    SelectionContainer {
                        // A record can be longer than the screen: the text scrolls sideways, as a zone file does.
                        Text(
                            domain.dnsZone.trim(),
                            modifier = Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()),
                            fontFamily = FontFamily.Monospace,
                            style = MaterialTheme.typography.bodySmall,
                            softWrap = false,
                        )
                    }
                    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
                        FilledTonalButton(onClick = { copy(domain.dnsZone.trim()) }) {
                            Icon(painterResource(R.drawable.ic_copy), contentDescription = null, modifier = Modifier.size(18.dp))
                            Text(stringResource(R.string.admin_copy), modifier = Modifier.padding(start = 8.dp))
                        }
                    }
                }
            }
        }
        SectionLabel(stringResource(R.string.admin_domain_dkim))
        when {
            !domain.dkimAllowed -> Note(stringResource(R.string.admin_domain_dkim_not_allowed))
            domain.dkim.isEmpty() -> Note(stringResource(if (domain.dkimAutomatic) R.string.admin_domain_dkim_none_auto else R.string.admin_domain_dkim_none_manual))
            else -> {
                AdminGroup { domain.dkim.forEachIndexed { i, key -> AdminRow(Groups.shape(i, domain.dkim.size), key.selector, detail = keyDetail(key)) } }
                Note(stringResource(if (domain.dkimAutomatic) R.string.admin_domain_dkim_auto else R.string.admin_domain_dkim_manual))
            }
        }
        AdminGroup(Modifier.padding(top = 16.dp)) {
            AdminRow(Groups.shape(0, 1), stringResource(R.string.admin_domain_delete), titleColor = colors.error, onClick = if (busy) null else c::askDelete)
        }
    }
}

@Composable
private fun keyDetail(key: DkimKey): String {
    val next = key.nextChangeAt
    return listOfNotNull(
        key.algorithm.takeIf { it.isNotBlank() },
        stageWord(key.stage),
        next?.let { stringResource(R.string.admin_dkim_next, whenText(it, now())) },
    ).joinToString(" · ")
}

@Composable
private fun stageWord(stage: String): String? = when (stage) {
    "active" -> stringResource(R.string.admin_dkim_stage_active)
    "pending" -> stringResource(R.string.admin_dkim_stage_pending)
    "retiring" -> stringResource(R.string.admin_dkim_stage_retiring)
    "retired" -> stringResource(R.string.admin_dkim_stage_retired)
    else -> stage.takeIf { it.isNotBlank() }
}

/** Alignment helper kept next to the screen that uses it. */
@Suppress("unused")
private val centered = Alignment.CenterVertically
