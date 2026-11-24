package ovh.kweb.mail.ui.admin

import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
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
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.LifecycleResumeEffect
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.list.EmptyNote
import ovh.kweb.mail.ui.list.Groups
import uniffi.kweb_core.TaskRow
import uniffi.kweb_core.TaskState

private fun now() = System.currentTimeMillis() / 1000

@Composable
private fun stateWord(state: TaskState?): String = stringResource(
    when (state) {
        null -> R.string.admin_task_all
        TaskState.WAITING -> R.string.admin_task_waiting
        TaskState.RETRYING -> R.string.admin_task_retrying
        TaskState.STALLED -> R.string.admin_task_stalled
        TaskState.FAILED -> R.string.admin_task_failed
    },
)

/** The server's background tasks, the one due first at the top, with Retry and Delete on each. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun TasksScreen(onBack: () -> Unit, vm: AdminTasksViewModel = viewModel()) {
    val c = vm.tasks
    val list by c.list.state.collectAsStateWithLifecycle()
    val state by c.state.collectAsStateWithLifecycle()
    LifecycleResumeEffect(vm) {
        c.refresh()
        onPauseOrDispose { }
    }
    AdminScaffold(title = stringResource(R.string.admin_area_tasks), onBack = onBack) { padding ->
        Column(Modifier.padding(padding).fillMaxSize()) {
            Row(
                Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()).padding(horizontal = 12.dp, vertical = 4.dp),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                for (f in AdminRules.taskFilters) {
                    FilterChip(selected = state.filter == f, onClick = { c.filter(f) }, label = { Text(stateWord(f)) })
                }
            }
            Notices(state.error ?: list.error, state.info, { c.dismissError(); c.list.dismissError() }, c::dismissInfo)
            when (list.phase) {
                ListPhase.LOADING -> LoadingBox()
                ListPhase.FAILED -> FailureBox(stringResource(R.string.admin_tasks_load_failed), onRetry = { c.list.reload(quiet = false) })
                ListPhase.READY -> PullToRefreshBox(isRefreshing = list.refreshing, onRefresh = c::refresh, modifier = Modifier.fillMaxSize()) {
                    if (list.items.isEmpty()) {
                        Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.Center) {
                            EmptyNote(R.drawable.ic_admin_task, stringResource(R.string.admin_tasks_none), modifier = Modifier.fillMaxWidth())
                        }
                    } else {
                        LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(start = 12.dp, end = 12.dp, bottom = 32.dp), verticalArrangement = Arrangement.spacedBy(3.dp)) {
                            itemsIndexed(list.items, key = { _, t -> t.id }) { index, task ->
                                TaskItem(Groups.shape(index, list.items.size), task, !state.busy, onRetry = { c.retry(task) }, onDelete = { c.askDelete(task) })
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
    state.asking?.let { task ->
        ConfirmDialog(
            title = stringResource(R.string.admin_task_delete_title),
            text = stringResource(R.string.admin_task_delete_text, task.kind),
            confirm = stringResource(R.string.admin_delete),
            onConfirm = c::confirmDelete,
            onDismiss = c::dismissDialog,
            danger = true,
        )
    }
}

@Composable
private fun TaskItem(shape: Shape, task: TaskRow, enabled: Boolean, onRetry: () -> Unit, onDelete: () -> Unit) {
    var open by remember { mutableStateOf(false) }
    val dueText = task.due?.let { stringResource(R.string.admin_task_due, whenText(it, now())) }
    val attemptsText = if (task.attempts > 0u) stringResource(R.string.admin_task_attempts, task.attempts.toInt()) else null
    val detail = listOfNotNull(
        task.subject.takeIf { it.isNotBlank() },
        dueText,
        attemptsText,
        task.reason.takeIf { it.isNotBlank() },
    ).joinToString(" · ")
    AdminRow(
        shape,
        task.kind,
        detail = detail,
        trailing = {
            Pill(stateWord(task.state), error = AdminRules.taskNeedsLook(task.state))
            Box {
                IconButton(onClick = { open = true }, enabled = enabled) {
                    Icon(Icons.Filled.MoreVert, contentDescription = stringResource(R.string.admin_task_menu))
                }
                DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
                    // A waiting task runs at its time; only one that failed or waits for another try is run again (the core
                    // refuses the rest too).
                    if (task.state != TaskState.WAITING) {
                        DropdownMenuItem(
                            text = { Text(stringResource(R.string.admin_task_retry)) },
                            leadingIcon = { Icon(Icons.Filled.Refresh, contentDescription = null) },
                            onClick = { open = false; onRetry() },
                        )
                    }
                    DropdownMenuItem(
                        text = { Text(stringResource(R.string.admin_delete)) },
                        leadingIcon = { Icon(Icons.Filled.Delete, contentDescription = null) },
                        onClick = { open = false; onDelete() },
                    )
                }
            }
        },
    )
}

/** The cluster's nodes, read only. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ClusterScreen(onBack: () -> Unit, vm: AdminClusterViewModel = viewModel()) {
    val c = vm.cluster
    val list by c.list.state.collectAsStateWithLifecycle()
    LifecycleResumeEffect(vm) {
        c.refresh()
        onPauseOrDispose { }
    }
    AdminScaffold(title = stringResource(R.string.admin_area_cluster), onBack = onBack) { padding ->
        Column(Modifier.padding(padding).fillMaxSize()) {
            Notices(list.error, null, c.list::dismissError) {}
            when (list.phase) {
                ListPhase.LOADING -> LoadingBox()
                ListPhase.FAILED -> FailureBox(stringResource(R.string.admin_cluster_load_failed), onRetry = { c.list.reload(quiet = false) })
                ListPhase.READY -> PullToRefreshBox(isRefreshing = list.refreshing, onRefresh = c::refresh, modifier = Modifier.fillMaxSize()) {
                    if (list.items.isEmpty()) {
                        Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.Center) {
                            EmptyNote(R.drawable.ic_admin_cluster, stringResource(R.string.admin_cluster_none), modifier = Modifier.fillMaxWidth())
                        }
                    } else {
                        LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(start = 12.dp, end = 12.dp, bottom = 32.dp), verticalArrangement = Arrangement.spacedBy(3.dp)) {
                            itemsIndexed(list.items, key = { _, n -> n.id }) { index, node ->
                                val nodeText = stringResource(R.string.admin_cluster_node, node.nodeId.toInt())
                                val renewed = node.lastRenewal?.let { stringResource(R.string.admin_cluster_renewed, whenText(it, now())) }
                                AdminRow(
                                    Groups.shape(index, list.items.size),
                                    node.hostname.ifBlank { nodeText },
                                    detail = listOfNotNull(nodeText, node.status.takeIf { it.isNotBlank() }, renewed).joinToString(" · "),
                                )
                            }
                        }
                    }
                }
            }
        }
    }
}
