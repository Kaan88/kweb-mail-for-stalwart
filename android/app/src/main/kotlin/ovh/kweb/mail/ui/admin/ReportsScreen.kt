package ovh.kweb.mail.ui.admin

import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.LifecycleResumeEffect
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.list.EmptyNote
import ovh.kweb.mail.ui.list.Groups
import uniffi.kweb_core.ReportFact
import uniffi.kweb_core.ReportKind
import uniffi.kweb_core.ReportRow

private fun now() = System.currentTimeMillis() / 1000

@Composable
internal fun kindWord(kind: ReportKind): String = stringResource(
    when (kind) {
        ReportKind.DMARC -> R.string.admin_report_dmarc
        ReportKind.TLS -> R.string.admin_report_tls
        ReportKind.ARF -> R.string.admin_report_arf
    },
)

/** Reports other servers sent about this one: a chip per kind, newest first. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ReportsScreen(onBack: () -> Unit, onOpen: (AdminScreen) -> Unit, vm: AdminReportsViewModel = viewModel()) {
    val c = vm.reports
    val list by c.list.state.collectAsStateWithLifecycle()
    val state by c.state.collectAsStateWithLifecycle()
    LifecycleResumeEffect(vm) {
        c.refresh()
        onPauseOrDispose { }
    }
    AdminScaffold(title = stringResource(R.string.admin_area_reports), onBack = onBack) { padding ->
        Column(Modifier.padding(padding).fillMaxSize()) {
            Row(
                Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()).padding(horizontal = 12.dp, vertical = 4.dp),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                for (kind in state.kinds) {
                    FilterChip(selected = state.kind == kind, onClick = { c.select(kind) }, label = { Text(kindWord(kind)) })
                }
            }
            Notices(list.error, null, c.list::dismissError) {}
            when (list.phase) {
                ListPhase.LOADING -> LoadingBox()
                ListPhase.FAILED -> FailureBox(stringResource(R.string.admin_reports_load_failed), onRetry = { c.refresh() })
                ListPhase.READY -> PullToRefreshBox(isRefreshing = list.refreshing, onRefresh = c::refresh, modifier = Modifier.fillMaxSize()) {
                    if (list.items.isEmpty()) {
                        Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.Center) {
                            EmptyNote(R.drawable.ic_admin_report, stringResource(R.string.admin_reports_none), modifier = Modifier.fillMaxWidth())
                        }
                    } else {
                        LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(start = 12.dp, end = 12.dp, bottom = 32.dp), verticalArrangement = Arrangement.spacedBy(3.dp)) {
                            itemsIndexed(list.items, key = { _, r -> r.id }) { index, report ->
                                AdminRow(
                                    Groups.shape(index, list.items.size),
                                    reportTitle(report),
                                    detail = reportLine(report),
                                    trailing = { if ((report.failed ?: 0u) > 0u) Pill(stringResource(R.string.admin_report_failed_pill), error = true) },
                                    onClick = { onOpen(AdminScreen.Report(report.kind, report.id)) },
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
}

private fun reportTitle(r: ReportRow): String = r.organization.ifBlank { r.from.ifBlank { r.subject } }

@Composable
private fun reportLine(r: ReportRow): String {
    val parts = mutableListOf<String>()
    if (r.domain.isNotBlank()) parts += r.domain
    val passed = r.passed
    val failed = r.failed
    if (passed != null || failed != null) parts += stringResource(R.string.admin_report_counts, (passed ?: 0u).toInt(), (failed ?: 0u).toInt())
    if (r.note.isNotBlank()) parts += r.note
    r.receivedAt?.let { parts += whenText(it, now()) }
    return parts.joinToString(" · ")
}

/** One report: who sent it, the main figures, and a group of figures per source. */
@Composable
fun ReportScreen(kind: ReportKind, id: String, onBack: () -> Unit, vm: AdminReportViewModel = viewModel()) {
    val c = vm.forReport(kind, id)
    val state by c.state.collectAsStateWithLifecycle()
    LifecycleResumeEffect(vm) {
        c.refresh()
        onPauseOrDispose { }
    }
    LaunchedEffect(state.gone) { if (state.gone) onBack() }
    AdminScaffold(
        title = kindWord(kind),
        onBack = onBack,
        actions = {
            IconButton(onClick = c::askDelete, enabled = state.report != null && !state.busy) {
                Icon(Icons.Filled.Delete, contentDescription = stringResource(R.string.admin_delete))
            }
        },
    ) { padding ->
        Column(Modifier.padding(padding).fillMaxSize()) {
            Notices(state.error, null, c::dismissError) {}
            val report = state.report
            when {
                report != null -> Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(bottom = 32.dp)) {
                    val fromLabel = stringResource(R.string.admin_report_from)
                    val toLabel = stringResource(R.string.admin_report_to)
                    val receivedLabel = stringResource(R.string.admin_report_received)
                    val periodLabel = stringResource(R.string.admin_report_period)
                    val start = report.periodStart
                    val end = report.periodEnd
                    val periodText = if (start != null && end != null) whenText(start, now()) + " - " + whenText(end, now()) else null
                    val receivedText = report.receivedAt?.let { whenText(it, now()) }
                    val head = buildList {
                        if (report.from.isNotBlank()) add(ReportFact(fromLabel, report.from))
                        if (report.to.isNotEmpty()) add(ReportFact(toLabel, report.to.joinToString(", ")))
                        if (receivedText != null) add(ReportFact(receivedLabel, receivedText))
                        if (periodText != null) add(ReportFact(periodLabel, periodText))
                    } + report.summary
                    Facts(head)
                    for (group in report.groups) {
                        SectionLabel(group.title)
                        Facts(group.facts)
                    }
                }
                state.phase == ReportPhase.FAILED -> FailureBox(stringResource(R.string.admin_load_failed), onRetry = c::refresh)
                else -> LoadingBox()
            }
        }
    }
    if (state.askDelete) {
        ConfirmDialog(
            title = stringResource(R.string.admin_report_delete_title),
            text = stringResource(R.string.admin_report_delete_text),
            confirm = stringResource(R.string.admin_delete),
            onConfirm = c::confirmDelete,
            onDismiss = c::dismissDialog,
            danger = true,
        )
    }
}

@Composable
private fun Facts(facts: List<ReportFact>) {
    AdminGroup(Modifier.padding(top = 4.dp)) {
        facts.forEachIndexed { i, f -> AdminRow(Groups.shape(i, facts.size), f.value.ifBlank { "-" }, detail = f.label) }
    }
}
