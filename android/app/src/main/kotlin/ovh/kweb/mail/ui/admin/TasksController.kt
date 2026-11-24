package ovh.kweb.mail.ui.admin

import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.update
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.UiMessage
import uniffi.kweb_core.ClusterNode
import uniffi.kweb_core.TaskRow
import uniffi.kweb_core.TaskState

data class TasksState(
    /** The state the list is narrowed to; null for all. */
    val filter: TaskState? = null,
    /** The task Delete asked about. */
    val asking: TaskRow? = null,
    val busy: Boolean = false,
    val error: UiMessage? = null,
    val info: UiMessage? = null,
)

/** The server's background tasks: the list in pages (the one due first at the top), Retry and Delete on each. */
class TasksController(
    scope: CoroutineScope,
    io: CoroutineDispatcher,
    private val backend: AdminBackend,
) {
    private val gate = ActionGate(scope, io)
    private val stateFlow = MutableStateFlow(TasksState())

    /** A page is asked for the filter of the first page on screen (the tag); the tag is a one-element list so that "all" (null) can be one. */
    val list = PagedLoader<TaskRow>(scope, io, AdminRules.PAGE, R.string.admin_tasks_load_failed, { it.id }) { offset, limit, tag ->
        val filter = if (offset == 0u) stateFlow.value.filter else (tag as? List<*>)?.firstOrNull() as? TaskState
        backend.tasks(filter, offset, limit).let { Page(it.tasks, it.total.toInt(), listOf(filter)) }
    }
    val state: StateFlow<TasksState> get() = stateFlow

    fun refresh() = list.refresh()

    fun filter(state: TaskState?) {
        if (stateFlow.value.filter == state) return
        stateFlow.update { it.copy(filter = state) }
        list.reload(quiet = false)
    }

    fun retry(task: TaskRow) {
        gate.run({ backend.taskRetry(task.id) }, ::started, { fail(it, R.string.admin_task_retry_failed) }) {
            stateFlow.update { it.copy(busy = false, info = UiMessage.Res(R.string.admin_task_retry_done)) }
            list.reload(quiet = true)
        }
    }

    fun askDelete(task: TaskRow) = stateFlow.update { it.copy(asking = task) }

    fun dismissDialog() = stateFlow.update { it.copy(asking = null) }

    fun confirmDelete() {
        val task = stateFlow.value.asking ?: return
        stateFlow.update { it.copy(asking = null) }
        gate.run({ backend.taskDelete(task.id) }, ::started, { fail(it, R.string.admin_task_delete_failed) }) {
            list.change { items -> items.filter { it.id != task.id } }
            stateFlow.update { it.copy(busy = false, info = UiMessage.Res(R.string.admin_task_delete_done)) }
            list.reload(quiet = true)
        }
    }

    private fun started() = stateFlow.update { it.copy(busy = true, error = null, info = null) }

    private fun fail(e: Exception, fallback: Int) = stateFlow.update { it.copy(busy = false, error = UiMessage.of(e, fallback)) }

    fun dismissError() = stateFlow.update { it.copy(error = null) }

    fun dismissInfo() = stateFlow.update { it.copy(info = null) }
}

/** The cluster's nodes (read only): the list, read again when the screen comes back. */
class ClusterController(
    scope: CoroutineScope,
    io: CoroutineDispatcher,
    private val backend: AdminBackend,
) {
    val list = PagedLoader<ClusterNode>(scope, io, AdminRules.PAGE, R.string.admin_cluster_load_failed, { it.id }) { _, _, _ ->
        backend.cluster().let { Page(it, it.size) }
    }

    fun refresh() = list.refresh()
}
