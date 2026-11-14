package ovh.kweb.mail.ui.admin

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
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material.icons.filled.Search
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.LifecycleResumeEffect
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.list.EmptyNote
import ovh.kweb.mail.ui.list.Groups
import uniffi.kweb_core.Maintenance

private fun now() = System.currentTimeMillis() / 1000

/** Blocked network addresses, with Unblock (after asking) for "I am locked out". */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun BlockedScreen(onBack: () -> Unit, vm: AdminBlockedViewModel = viewModel()) {
    val c = vm.blocked
    val list by c.list.state.collectAsStateWithLifecycle()
    val state by c.state.collectAsStateWithLifecycle()
    LifecycleResumeEffect(vm) {
        c.refresh()
        onPauseOrDispose { }
    }
    AdminScaffold(title = stringResource(R.string.admin_area_blocked), onBack = onBack) { padding ->
        Column(Modifier.padding(padding).fillMaxSize()) {
            Notices(state.error ?: list.error, state.info, { c.dismissError(); c.list.dismissError() }, c::dismissInfo)
            when (list.phase) {
                ListPhase.LOADING -> LoadingBox()
                ListPhase.FAILED -> FailureBox(stringResource(R.string.admin_blocked_load_failed), onRetry = { c.list.reload(quiet = false) })
                ListPhase.READY -> PullToRefreshBox(isRefreshing = list.refreshing, onRefresh = c::refresh, modifier = Modifier.fillMaxSize()) {
                    if (list.items.isEmpty()) {
                        Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.Center) {
                            EmptyNote(R.drawable.ic_block, stringResource(R.string.admin_blocked_none), modifier = Modifier.fillMaxWidth())
                        }
                    } else {
                        LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(start = 12.dp, end = 12.dp, bottom = 32.dp), verticalArrangement = Arrangement.spacedBy(3.dp)) {
                            item(key = "note") { Note(stringResource(R.string.admin_blocked_note), Modifier.padding(horizontal = 16.dp)) }
                            itemsIndexed(list.items, key = { _, b -> b.id }) { index, blocked ->
                                val at = blocked.blockedAt
                                val detail = listOfNotNull(blocked.reason.takeIf { it.isNotBlank() }, at?.let { whenText(it, now()) }).joinToString(" · ")
                                AdminRow(
                                    Groups.shape(index, list.items.size),
                                    blocked.address,
                                    detail = detail.ifEmpty { null },
                                    trailing = { FilledTonalButton(onClick = { c.askUnblock(blocked) }, enabled = !state.busy) { Text(stringResource(R.string.admin_unblock)) } },
                                )
                            }
                            if (list.more || list.loadingMore) {
                                item(key = "more") {
                                    TextButton(onClick = c.list::loadMore, enabled = !list.loadingMore, modifier = Modifier.fillMaxWidth()) {
                                        Text(stringResource(if (list.loadingMore) R.string.admin_loading else R.string.admin_load_more))
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    state.asking?.let {
        ConfirmDialog(
            title = stringResource(R.string.admin_unblock_title, it.address),
            text = stringResource(R.string.admin_unblock_text),
            confirm = stringResource(R.string.admin_unblock),
            onConfirm = c::confirmUnblock,
            onDismiss = c::dismissDialog,
        )
    }
}

/** The log search: the server's own lines, narrowed by level pills on the phone. */
@Composable
fun LogsScreen(onBack: () -> Unit, vm: AdminLogsViewModel = viewModel()) {
    val c = vm.logs
    val state by c.state.collectAsStateWithLifecycle()
    LifecycleResumeEffect(vm) {
        c.start()
        onPauseOrDispose { }
    }
    AdminScaffold(
        title = stringResource(R.string.admin_area_logs),
        onBack = onBack,
        actions = { IconButton(onClick = c::search) { Icon(Icons.Filled.Refresh, contentDescription = stringResource(R.string.admin_refresh)) } },
    ) { padding ->
        Column(Modifier.padding(padding).fillMaxSize()) {
            AdminField(
                state.text,
                c::typeText,
                stringResource(R.string.admin_logs_search),
                modifier = Modifier.padding(horizontal = 12.dp, vertical = 8.dp),
                leadingIcon = { Icon(Icons.Filled.Search, contentDescription = null) },
                keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search),
                keyboardActions = KeyboardActions(onSearch = { c.search() }),
            )
            Row(Modifier.padding(horizontal = 12.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                for ((level, label) in listOf(LevelFilter.All to R.string.admin_level_all, LevelFilter.Errors to R.string.admin_level_errors, LevelFilter.Warnings to R.string.admin_level_warnings)) {
                    FilterChip(selected = state.level == level, onClick = { c.setLevel(level) }, label = { Text(stringResource(label)) })
                }
            }
            Notices(state.error, null, c::dismissError) {}
            when (state.phase) {
                LogPhase.IDLE, LogPhase.LOADING -> LoadingBox()
                LogPhase.FAILED -> FailureBox(stringResource(R.string.admin_logs_load_failed), onRetry = c::search)
                LogPhase.READY -> LogList(state)
            }
        }
    }
}

@Composable
private fun LogList(state: LogsState) {
    val shown = state.shown
    LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(start = 12.dp, end = 12.dp, bottom = 32.dp), verticalArrangement = Arrangement.spacedBy(3.dp)) {
        item(key = "note") { Note(pluralStringResource(R.plurals.admin_logs_note, state.lines.size, state.lines.size), Modifier.padding(horizontal = 16.dp)) }
        if (shown.isEmpty()) item(key = "none") { Note(stringResource(R.string.admin_logs_none)) }
        itemsIndexed(shown) { index, line ->
            AdminRow(
                Groups.shape(index, shown.size),
                AdminRules.clip(line.text),
                detail = line.at?.let { whenText(it, now()) },
                trailing = {
                    when (line.level) {
                        "error" -> Pill(stringResource(R.string.admin_level_error), error = true)
                        "warning" -> Pill(stringResource(R.string.admin_level_warning))
                        else -> Unit
                    }
                },
            )
        }
    }
}

/** Maintenance: reload the server's settings or certificates, each after a confirmation. */
@Composable
fun MaintenanceScreen(onBack: () -> Unit, vm: AdminMaintenanceViewModel = viewModel()) {
    val c = vm.maintenance
    val state by c.state.collectAsStateWithLifecycle()
    AdminScaffold(title = stringResource(R.string.admin_area_maintenance), onBack = onBack) { padding ->
        Column(Modifier.padding(padding).fillMaxSize().verticalScroll(rememberScrollState())) {
            Notices(state.error, state.info, c::dismissError, c::dismissInfo)
            Note(stringResource(R.string.admin_maintenance_note))
            val actions = listOf(
                Triple(Maintenance.RELOAD_SETTINGS, R.string.admin_reload_settings, R.string.admin_reload_settings_detail),
                Triple(Maintenance.RELOAD_CERTIFICATES, R.string.admin_reload_certs, R.string.admin_reload_certs_detail),
            )
            AdminGroup {
                actions.forEachIndexed { index, (what, title, detail) ->
                    AdminRow(
                        Groups.shape(index, actions.size),
                        stringResource(title),
                        detail = stringResource(detail),
                        trailing = { if (state.running == what) CircularProgressIndicator(Modifier.size(20.dp), strokeWidth = 2.dp) },
                        onClick = if (state.running == null) ({ c.ask(what) }) else null,
                    )
                }
            }
        }
    }
    state.asking?.let { what ->
        val settings = what == Maintenance.RELOAD_SETTINGS
        ConfirmDialog(
            title = stringResource(if (settings) R.string.admin_reload_settings_title else R.string.admin_reload_certs_title),
            text = stringResource(if (settings) R.string.admin_reload_settings_text else R.string.admin_reload_certs_text),
            confirm = stringResource(R.string.admin_reload_confirm),
            onConfirm = c::confirm,
            onDismiss = c::dismissDialog,
        )
    }
}
