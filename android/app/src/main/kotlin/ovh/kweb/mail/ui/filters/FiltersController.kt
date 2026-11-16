package ovh.kweb.mail.ui.filters

import java.time.ZoneId
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
import uniffi.kweb_core.Exception as CoreException
import uniffi.kweb_core.FiltersState
import uniffi.kweb_core.Label
import uniffi.kweb_core.Mailbox

enum class FiltersPhase { LOADING, READY, FAILED, NONE }

/** The rule that is open in the editor, with what it was when it was opened (to tell whether anything changed). */
data class EditorState(val original: RuleDraft, val rule: RuleDraft, val isNew: Boolean) {
    val dirty: Boolean get() = rule != original
}

/** The script's text being edited by hand. */
data class RawState(val original: String, val text: String) {
    val dirty: Boolean get() = text != original
}

/** What "Undo" puts back after a rule was deleted: the whole list as it was. */
data class UndoDelete(val ruleName: String, val rulesJson: String)

/** A question on top of the screen. */
sealed interface FiltersDialog {
    /** Switching on switches the away message off (the server cannot run both here). */
    data object SwitchOn : FiltersDialog

    data object DiscardEditor : FiltersDialog

    data object DiscardRaw : FiltersDialog

    data object ResetToVisual : FiltersDialog

    data class Migrate(val count: Int) : FiltersDialog

    data object RemoveOld : FiltersDialog
}

data class FiltersUi(
    val phase: FiltersPhase = FiltersPhase.LOADING,
    /** What the core said last. */
    val core: FiltersState? = null,
    val rules: List<RuleDraft> = emptyList(),
    /** The user's own folders, as the core lists them. */
    val folders: List<Mailbox> = emptyList(),
    val labels: List<Label> = emptyList(),
    val unsupported: UiMessage? = null,
    /** A change is being saved. */
    val busy: Boolean = false,
    val error: UiMessage? = null,
    val info: UiMessage? = null,
    val undo: UndoDelete? = null,
    val editor: EditorState? = null,
    val raw: RawState? = null,
    val dialog: FiltersDialog? = null,
) {
    /** The script cannot be shown as rules: only text is edited. */
    val textOnly: Boolean get() = core?.let { it.opaque || it.foreign } == true
}

/**
 * The state and the actions of the filter screens. The core is asked off the main thread ([io]); an answer that
 * a newer request has overtaken is dropped; a change is refused while another one is being saved.
 */
class FiltersController(
    private val scope: CoroutineScope,
    private val io: CoroutineDispatcher,
    private val backend: FiltersBackend,
    private val zone: ZoneId = ZoneId.systemDefault(),
) {
    private val stateFlow = MutableStateFlow(FiltersUi())
    private var started = false

    /** Counts requests; an answer is used only when no newer request was started. */
    private var ticket = 0

    val state: StateFlow<FiltersUi> get() = stateFlow

    /** Reads the filters once; with [fromMessageId] the editor then opens for a new rule made from that message. */
    fun start(fromMessageId: String?) {
        if (started) return
        started = true
        scope.launch {
            readNow()
            if (fromMessageId != null && stateFlow.value.phase == FiltersPhase.READY) {
                val s = stateFlow.value
                if (s.textOnly || s.core?.migrationPending == true) {
                    stateFlow.update { it.copy(info = UiMessage.Res(R.string.filters_no_add_now)) }
                } else {
                    openFromMessage(fromMessageId)
                }
            }
        }
    }

    fun reset() {
        ticket++
        started = false
        stateFlow.value = FiltersUi()
    }

    /** Reads again (Retry, and the screen coming back). A form that is open stays open. */
    fun load() {
        scope.launch { readNow() }
    }

    /** Reads the filters again without showing the loading state when they are on screen already. */
    fun refresh() {
        val s = stateFlow.value
        if (s.phase != FiltersPhase.READY || s.busy || s.editor != null || s.raw != null) return
        scope.launch { readNow(quiet = true) }
    }

    private fun apply(s: FiltersUi, core: FiltersState): FiltersUi =
        s.copy(phase = FiltersPhase.READY, core = core, rules = parse(core), unsupported = null)

    private fun parse(core: FiltersState): List<RuleDraft> = try {
        RuleModel.parseList(core.rulesJson, zone)
    } catch (_: Json.ParseError) {
        emptyList()
    }

    private suspend fun readNow(quiet: Boolean = false, info: UiMessage? = null) {
        val mine = ++ticket
        if (!quiet) stateFlow.update { it.copy(phase = if (it.core == null) FiltersPhase.LOADING else it.phase) }
        try {
            val (core, folders, labels) = withContext(io) {
                val core = backend.state()
                if (!core.supported) Triple(core, emptyList(), emptyList())
                else Triple(core, backend.mailboxes(), runCatching { backend.labels() }.getOrDefault(emptyList()))
            }
            if (mine != ticket) return
            if (!core.supported) {
                stateFlow.update { it.copy(phase = FiltersPhase.NONE, core = null, rules = emptyList(), unsupported = UiMessage.Res(R.string.filters_none)) }
                return
            }
            stateFlow.update {
                apply(it, core).copy(folders = folders, labels = labels, error = null, info = info ?: it.info.takeIf { quiet })
            }
        } catch (e: CancellationException) {
            throw e
        } catch (e: CoreException.Invalid) {
            if (mine != ticket) return
            if (stateFlow.value.core != null) {
                stateFlow.update { it.copy(error = UiMessage.of(e, R.string.filters_load_failed), info = info ?: it.info.takeIf { quiet }) }
                return
            }
            stateFlow.update { it.copy(phase = FiltersPhase.NONE, core = null, unsupported = UiMessage.of(e, R.string.filters_none)) }
        } catch (e: Exception) {
            if (mine != ticket) return
            stateFlow.update {
                if (it.core != null) it.copy(error = UiMessage.Res(R.string.filters_load_failed)) else it.copy(phase = FiltersPhase.FAILED)
            }
        }
    }

    fun dismissError() = stateFlow.update { it.copy(error = null) }

    fun dismissInfo() = stateFlow.update { it.copy(info = null, undo = null) }

    fun dismissDialog() = stateFlow.update { it.copy(dialog = null) }

    // ---- the list ----

    /** "Use these filters". Switching on asks first when it switches the away message off. */
    fun setEnabled(on: Boolean) {
        val core = stateFlow.value.core ?: return
        if (stateFlow.value.busy) return
        if (!on) {
            if (core.scriptName.isEmpty()) return
            return run { backend.setEnabled(false) }
        }
        if (core.vacationConflict) stateFlow.update { it.copy(dialog = FiltersDialog.SwitchOn) } else switchOn()
    }

    fun confirmSwitchOn() {
        stateFlow.update { it.copy(dialog = null) }
        switchOn()
    }

    /** With no script yet the core refuses to switch one on: the rules (none) are saved instead, which makes it and runs it. */
    private fun switchOn() {
        val core = stateFlow.value.core ?: return
        if (core.scriptName.isEmpty()) run { backend.save(core.rulesJson) } else run { backend.setEnabled(true) }
    }

    fun setRuleEnabled(id: String, enabled: Boolean) {
        if (stateFlow.value.textOnly) return
        run { backend.toggle(id, enabled) }
    }

    fun moveRule(id: String, up: Boolean) {
        if (stateFlow.value.textOnly) return
        run { backend.move(id, up) }
    }

    /** Deletes at once; the notice offers Undo, which saves the list as it was. */
    fun deleteRule(id: String, afterwards: () -> Unit = {}) {
        val s = stateFlow.value
        val core = s.core ?: return
        val rule = s.rules.firstOrNull { it.id == id } ?: return
        run(onDone = { afterwards() }, undo = UndoDelete(rule.name, core.rulesJson)) { backend.delete(id) }
    }

    fun undoDelete() {
        val undo = stateFlow.value.undo ?: return
        stateFlow.update { it.copy(undo = null) }
        run { backend.save(undo.rulesJson) }
    }

    /** "Update": saves the rules again, which brings the script up to the way this editor writes it. */
    fun update() {
        val core = stateFlow.value.core ?: return
        run { backend.save(core.rulesJson) }
    }

    fun askMigrate() {
        val s = stateFlow.value
        if (s.core?.migrationPending == true) stateFlow.update { it.copy(dialog = FiltersDialog.Migrate(s.rules.size)) }
    }

    /** Saves the converted rules as the new script, then has the old one removed. */
    fun confirmMigrate() {
        val core = stateFlow.value.core ?: return
        stateFlow.update { it.copy(dialog = null) }
        run { backend.save(core.rulesJson); backend.migrateOld() }
    }

    fun askRemoveOld() = stateFlow.update { it.copy(dialog = FiltersDialog.RemoveOld) }

    fun confirmRemoveOld() {
        stateFlow.update { it.copy(dialog = null) }
        run { backend.migrateOld() }
    }

    fun askReset() = stateFlow.update { it.copy(dialog = FiltersDialog.ResetToVisual) }

    fun confirmReset() {
        stateFlow.update { it.copy(dialog = null) }
        run(onDone = { stateFlow.update { it.copy(raw = null) } }) { backend.resetToVisual() }
    }

    // ---- the raw editor ----

    fun openRaw() {
        val core = stateFlow.value.core ?: return
        if (stateFlow.value.busy) return
        val text = core.rawText
        stateFlow.update { it.copy(raw = RawState(text, text), error = null, info = null, undo = null) }
    }

    fun editRaw(text: String) = stateFlow.update { s -> if (s.busy) s else s.copy(raw = s.raw?.copy(text = text), error = null) }

    fun leaveRaw() {
        val raw = stateFlow.value.raw ?: return
        if (stateFlow.value.busy) return
        if (raw.dirty) stateFlow.update { it.copy(dialog = FiltersDialog.DiscardRaw) } else stateFlow.update { it.copy(raw = null) }
    }

    fun discardRaw() = stateFlow.update { it.copy(raw = null, dialog = null, error = null) }

    fun saveRaw() {
        val raw = stateFlow.value.raw ?: return
        if (stateFlow.value.busy) return
        run(onDone = { stateFlow.update { it.copy(raw = null) } }) { backend.rawSave(raw.text) }
    }

    // ---- the rule editor ----

    fun addRule() {
        val s = stateFlow.value
        if (s.phase != FiltersPhase.READY || s.textOnly || s.core?.migrationPending == true) return
        val rule = RuleModel.newRule()
        stateFlow.update { it.copy(editor = EditorState(rule, rule, isNew = true), error = null, info = null, undo = null) }
    }

    fun openRule(id: String) {
        val s = stateFlow.value
        if (s.textOnly || s.core?.migrationPending == true || s.busy) return
        val rule = s.rules.firstOrNull { it.id == id } ?: return
        val resolved = resolveFolders(rule, RuleModel.folderTree(s.folders))
        stateFlow.update { it.copy(editor = EditorState(resolved, resolved, isNew = false), error = null, info = null, undo = null) }
    }

    /** A rule another client made may name a folder by path only: the id is found from the path. */
    private fun resolveFolders(rule: RuleDraft, tree: List<FolderRow>): RuleDraft =
        rule.copy(
            actions = rule.actions.map { a ->
                if (!RuleModel.usesFolder(a)) return@map a
                val byId = a.mailboxId?.let { id -> tree.firstOrNull { it.id == id } }
                val row = byId ?: tree.firstOrNull { it.path == a.value }
                if (row != null) a.copy(mailboxId = row.id, value = row.path) else a
            },
        )

    private suspend fun openFromMessage(messageId: String) {
        try {
            val seed = withContext(io) { backend.seed(messageId) }
            val rule = RuleModel.fromSeed(seed)
            stateFlow.update { if (it.editor != null) it else it.copy(editor = EditorState(rule, rule, isNew = true)) }
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            stateFlow.update { it.copy(error = UiMessage.of(e, R.string.filters_from_message_failed)) }
        }
    }

    fun edit(change: (RuleDraft) -> RuleDraft) = stateFlow.update { s ->
        // What is typed while a save runs would be thrown away when its answer arrives.
        if (s.busy) return@update s
        s.copy(editor = s.editor?.let { it.copy(rule = change(it.rule)) }, error = null)
    }

    /** The first fault of the rule being edited, for the button to say. */
    fun problem(): RuleProblem? {
        val s = stateFlow.value
        val editor = s.editor ?: return null
        val core = s.core ?: return null
        val count = RuleModel.withRule(s.rules, editor.rule).size
        return RuleModel.check(editor.rule, core.capabilities, RuleModel.folderTree(s.folders).mapTo(HashSet()) { it.id }, count, zone)
    }

    /** Leaving the editor: asks when something was changed. */
    fun leaveEditor() {
        val editor = stateFlow.value.editor ?: return
        if (stateFlow.value.busy) return
        if (editor.dirty) stateFlow.update { it.copy(dialog = FiltersDialog.DiscardEditor) } else closeEditor()
    }

    fun discardEditor() = stateFlow.update { it.copy(editor = null, dialog = null, error = null) }

    private fun closeEditor() = stateFlow.update { it.copy(editor = null, dialog = null) }

    fun saveEditor() {
        val s = stateFlow.value
        val editor = s.editor ?: return
        if (s.busy || s.core == null || problem() != null) return
        val json = RuleModel.listToJson(RuleModel.withRule(s.rules, editor.rule), zone)
        run(onDone = { closeEditor() }) { backend.save(json) }
    }

    /** Deletes the rule being edited (with Undo) and closes the editor. */
    fun deleteEditedRule() {
        val editor = stateFlow.value.editor ?: return
        if (editor.isNew) return
        deleteRule(editor.original.id) { closeEditor() }
    }

    /** Runs one change on the server. The answer replaces the list; a failure says why; "changed elsewhere" reads again. */
    private fun run(onDone: () -> Unit = {}, undo: UndoDelete? = null, call: () -> FiltersState) {
        if (stateFlow.value.busy) return
        val mine = ++ticket
        stateFlow.update { it.copy(busy = true, error = null, info = null, undo = null) }
        scope.launch {
            try {
                val core = withContext(io) { call() }
                if (mine != ticket) {
                    stateFlow.update { it.copy(busy = false) }
                    return@launch
                }
                stateFlow.update { apply(it, core).copy(busy = false, error = null, undo = undo) }
                onDone()
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                if (e is CoreException.Invalid && changedElsewhere(e.msg)) {
                    stateFlow.update { it.copy(busy = false) }
                    readNow(quiet = true, info = UiMessage.Res(R.string.filters_changed_elsewhere))
                    closeEditorIfSaved()
                } else if (mine != ticket) {
                    stateFlow.update { it.copy(busy = false) }
                } else {
                    stateFlow.update { it.copy(busy = false, error = UiMessage.of(e, R.string.filters_save_failed)) }
                }
            }
        }
    }

    /** A new rule whose save was written although its answer was lost is on the fresh list already: the editor closes. */
    private fun closeEditorIfSaved() = stateFlow.update { s ->
        val editor = s.editor
        if (editor != null && editor.isNew && s.rules.any { it.id == editor.rule.id }) s.copy(editor = null, dialog = null) else s
    }

    companion object {
        /** The core's refusal when the script changed on the server since it was read. */
        fun changedElsewhere(message: String) = message.contains("changed elsewhere", ignoreCase = true)
    }
}
