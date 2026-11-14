package ovh.kweb.mail.ui.folders

import androidx.annotation.StringRes
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.UiMessage
import uniffi.kweb_core.Mailbox

/** What the menu of a folder offers (also what a long press in the drawer asks the folder screen to open). */
enum class FolderAction { RENAME, MOVE, NEW_INSIDE, DELETE }

enum class FoldersPhase { LOADING, READY, FAILED }

enum class NameMode { NEW, RENAME }

/** A question on top of the screen. What was typed is part of it, so it survives a rotation (this state lives in the route's store). */
sealed interface FolderDialog {
    /** [targetId]: the parent of a new folder (null: the top level), or the folder being renamed. */
    data class Name(val mode: NameMode, val targetId: String?, val text: String) : FolderDialog

    data class Move(val id: String) : FolderDialog

    data class Delete(val id: String) : FolderDialog
}

data class FoldersState(
    val phase: FoldersPhase = FoldersPhase.LOADING,
    val mailboxes: List<Mailbox> = emptyList(),
    /** A change is on its way to the server; another is refused meanwhile. */
    val busy: Boolean = false,
    /** The core's words for a refusal, or the app's own for a failure. */
    val error: UiMessage? = null,
    val dialog: FolderDialog? = null,
)

/**
 * The state and the actions of the folder screen. The core is asked off the main thread ([io]); an answer that a
 * newer request has overtaken is dropped; a change is refused while another one is on its way.
 */
class FoldersController(
    private val scope: CoroutineScope,
    private val io: CoroutineDispatcher,
    private val backend: FoldersBackend,
) {
    private val stateFlow = MutableStateFlow(FoldersState())
    private var started = false

    /** What a long press asked for; opened after the first read that works (a failed one can be retried). */
    private var pending: Pair<String, FolderAction>? = null

    /** Counts reads; an answer is used only when no newer read was started. */
    private var ticket = 0

    val state: StateFlow<FoldersState> get() = stateFlow

    /** Reads the folders once; then opens [action] for [folderId] when both are given (a long press in the drawer). */
    fun start(folderId: String?, action: FolderAction?) {
        if (started) return
        started = true
        if (folderId != null && action != null) pending = folderId to action
        scope.launch { readNow() }
    }

    fun reset() {
        ticket++
        started = false
        pending = null
        stateFlow.value = FoldersState()
    }

    /** Reads again (Retry). */
    fun load() {
        scope.launch { readNow() }
    }

    /** A sync changed something: read again quietly, unless a change of ours is on its way (it reads when done). */
    fun refresh() {
        val s = stateFlow.value
        if (s.phase != FoldersPhase.READY || s.busy) return
        scope.launch { readNow(quiet = true) }
    }

    private suspend fun readNow(quiet: Boolean = false) {
        val mine = ++ticket
        if (!quiet) stateFlow.update { if (it.mailboxes.isEmpty()) it.copy(phase = FoldersPhase.LOADING) else it }
        try {
            val folders = withContext(io) { backend.mailboxes() }
            if (mine != ticket) return
            stateFlow.update { it.withFolders(folders) }
            pending?.let { (id, action) ->
                pending = null
                open(id, action)
            }
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            if (mine != ticket) return
            stateFlow.update {
                // Folders that are on screen stay; only a failed first read shows the retry. While a name is being
                // typed the error field belongs to the name (a refusal of the core), so a failed read stays quiet.
                when {
                    it.mailboxes.isEmpty() -> it.copy(phase = FoldersPhase.FAILED)
                    it.dialog is FolderDialog.Name -> it
                    else -> it.copy(error = UiMessage.Res(R.string.folders_load_failed))
                }
            }
        }
    }

    /** New folders in, and a dialog about a folder that is gone closes. */
    private fun FoldersState.withFolders(folders: List<Mailbox>): FoldersState {
        val ids = folders.map { it.id }.toSet()
        val keep = when (val d = dialog) {
            is FolderDialog.Move -> d.id in ids
            is FolderDialog.Delete -> d.id in ids
            is FolderDialog.Name -> d.targetId == null && d.mode == NameMode.NEW || d.targetId in ids
            null -> true
        }
        return copy(phase = FoldersPhase.READY, mailboxes = folders, dialog = if (keep) dialog else null)
    }

    fun dismissError() = stateFlow.update { it.copy(error = null) }

    fun dismissDialog() = stateFlow.update { it.copy(dialog = null, error = null) }

    // ---- the menu ----

    private fun open(id: String, action: FolderAction) {
        when (action) {
            FolderAction.RENAME -> askRename(id)
            FolderAction.MOVE -> askMove(id)
            FolderAction.NEW_INSIDE -> askNew(id)
            FolderAction.DELETE -> askDelete(id)
        }
    }

    /** The button for a new top-level folder ([parentId] null) or "New folder inside". */
    fun askNew(parentId: String?) {
        val s = stateFlow.value
        if (s.busy) return
        if (parentId != null) {
            val parent = s.mailboxes.firstOrNull { it.id == parentId } ?: return
            if (!FolderRules.canHoldFolders(parent)) return
        }
        stateFlow.update { it.copy(dialog = FolderDialog.Name(NameMode.NEW, parentId, ""), error = null) }
    }

    fun askRename(id: String) {
        val folder = userFolder(id) ?: return
        if (stateFlow.value.busy) return
        stateFlow.update { it.copy(dialog = FolderDialog.Name(NameMode.RENAME, id, folder.name), error = null) }
    }

    fun askMove(id: String) {
        userFolder(id) ?: return
        if (stateFlow.value.busy) return
        stateFlow.update { it.copy(dialog = FolderDialog.Move(id), error = null) }
    }

    fun askDelete(id: String) {
        userFolder(id) ?: return
        if (stateFlow.value.busy) return
        stateFlow.update { it.copy(dialog = FolderDialog.Delete(id), error = null) }
    }

    private fun userFolder(id: String): Mailbox? = stateFlow.value.mailboxes.firstOrNull { it.id == id }?.takeIf { FolderRules.isUserFolder(it) }

    /** Typing: an old refusal no longer applies to what is in the field. */
    fun setName(text: String) = stateFlow.update {
        val d = it.dialog
        if (d is FolderDialog.Name) it.copy(dialog = d.copy(text = text), error = null) else it
    }

    // ---- changes ----

    /** The bell. Only a folder with a switch can be changed here. */
    fun setNotify(id: String, notify: Boolean) {
        val folder = stateFlow.value.mailboxes.firstOrNull { it.id == id } ?: return
        if (FolderRules.bell(folder) != Bell.SWITCH) return
        mutate(R.string.folders_notify_failed) { backend.setNotify(id, notify) }
    }

    fun confirmName() {
        val d = stateFlow.value.dialog as? FolderDialog.Name ?: return
        if (!FolderRules.nameCanBeSent(d.text)) return
        val name = d.text.trim()
        when (d.mode) {
            NameMode.NEW -> mutate(R.string.folders_create_failed, keepDialogOnError = true) { backend.create(name, d.targetId) }
            NameMode.RENAME -> mutate(R.string.folders_rename_failed, keepDialogOnError = true) { backend.rename(d.targetId ?: return@mutate, name) }
        }
    }

    /** [parentId] null: the top level. */
    fun confirmMove(parentId: String?) {
        val d = stateFlow.value.dialog as? FolderDialog.Move ?: return
        mutate(R.string.folders_move_failed) { backend.move(d.id, parentId) }
    }

    /** Takes the mail along to the Trash when the folder holds any; a folder with folders inside is refused here, before the core. */
    fun confirmDelete() {
        val d = stateFlow.value.dialog as? FolderDialog.Delete ?: return
        val plan = FolderRules.deletePlan(stateFlow.value.mailboxes, d.id)
        if (plan == null || plan == DeletePlan.HasFolders) {
            dismissDialog()
            return
        }
        mutate(R.string.folders_delete_failed) { backend.delete(d.id, withMail = plan is DeletePlan.WithMail) }
    }

    /**
     * Runs [call] off the main thread. On success the dialog closes and the folders are read again; on a failure the
     * core's words (or [failure]) are shown, in the dialog when [keepDialogOnError] and on the screen otherwise.
     */
    private fun mutate(@StringRes failure: Int, keepDialogOnError: Boolean = false, call: () -> Unit) {
        if (stateFlow.value.busy) return
        stateFlow.update { it.copy(busy = true, error = null) }
        scope.launch {
            val error = try {
                withContext(io) { call() }
                null
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                UiMessage.of(e, failure)
            }
            if (error == null) {
                stateFlow.update { it.copy(dialog = null) }
            } else {
                stateFlow.update { it.copy(error = error, dialog = if (keepDialogOnError) it.dialog else null) }
            }
            // Read after the change, with a fresh ticket: every read that started before is now out of date.
            readNow(quiet = true)
            stateFlow.update { it.copy(busy = false) }
        }
    }
}
