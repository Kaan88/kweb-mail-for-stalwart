package ovh.kweb.mail.ui.filters

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
import uniffi.kweb_core.FilterRule
import uniffi.kweb_core.Filters
import uniffi.kweb_core.Mailbox

enum class FiltersPhase { LOADING, READY, FAILED, NONE }

/** The rule that is open in the editor, with what it was when it was opened (to tell whether anything changed). */
data class EditorState(val original: FilterRule, val rule: FilterRule, val isNew: Boolean) {
    val dirty: Boolean get() = rule != original
}

/** A question or a read-only text on top of the screen. */
sealed interface FiltersDialog {
    /** Switching on would switch these off. */
    data class SwitchOn(val asks: List<SwitchOff>) : FiltersDialog

    data object StartOver : FiltersDialog

    data object DeleteRule : FiltersDialog

    data object DiscardEditor : FiltersDialog

    /** The text of a script, to read: [name] is null for the app's own. */
    data class Script(val name: String?, val text: String) : FiltersDialog
}

data class FiltersState(
    val phase: FiltersPhase = FiltersPhase.LOADING,
    val filters: Filters? = null,
    /** The user's own folders, as the core lists them. */
    val folders: List<Mailbox> = emptyList(),
    /** The core's words when the server has no filters. */
    val unsupported: UiMessage? = null,
    /** A change is being saved. */
    val busy: Boolean = false,
    val error: UiMessage? = null,
    /** Something that is not a failure: what happened to the list. */
    val info: UiMessage? = null,
    val editor: EditorState? = null,
    val dialog: FiltersDialog? = null,
) {
    val rules: List<FilterRule> get() = filters?.rules.orEmpty()
}

/**
 * The state and the actions of the filter screens. The core is asked off the main thread ([io]); an answer that
 * a newer request has overtaken is dropped; a change is refused while another one is being saved.
 */
class FiltersController(
    private val scope: CoroutineScope,
    private val io: CoroutineDispatcher,
    private val backend: FiltersBackend,
) {
    private val stateFlow = MutableStateFlow(FiltersState())
    private var started = false

    /** Counts requests; an answer is used only when no newer request was started. */
    private var ticket = 0

    val state: StateFlow<FiltersState> get() = stateFlow

    /** Reads the filters once; with [fromMessageId] the editor then opens for a new rule made from that message. */
    fun start(fromMessageId: String?) {
        if (started) return
        started = true
        scope.launch {
            readNow()
            if (fromMessageId != null && stateFlow.value.phase == FiltersPhase.READY) {
                if (stateFlow.value.filters?.isEdited == true) noRuleWhileEdited() else openFromMessage(fromMessageId)
            }
        }
    }

    fun reset() {
        ticket++
        started = false
        stateFlow.value = FiltersState()
    }

    /** Reads again (Retry, and the screen coming back). A form that is open stays open. */
    fun load() {
        scope.launch { readNow() }
    }

    /** Reads the filters again without showing the loading state when they are on screen already. */
    fun refresh() {
        if (stateFlow.value.phase != FiltersPhase.READY || stateFlow.value.busy) return
        scope.launch { readNow(quiet = true) }
    }

    private suspend fun readNow(quiet: Boolean = false, info: UiMessage? = null) {
        val mine = ++ticket
        if (!quiet) stateFlow.update { it.copy(phase = if (it.filters == null) FiltersPhase.LOADING else it.phase) }
        try {
            val (filters, folders) = withContext(io) { backend.filters() to backend.mailboxes() }
            if (mine != ticket) return
            stateFlow.update {
                it.copy(phase = FiltersPhase.READY, filters = filters, folders = folders, unsupported = null, error = null, info = info ?: it.info.takeIf { quiet })
            }
        } catch (e: CancellationException) {
            throw e
        } catch (e: CoreException.Invalid) {
            if (mine != ticket) return
            // Filters that are on screen stay (a script gone between two requests is not "no filters").
            if (stateFlow.value.filters != null) {
                stateFlow.update { it.copy(error = UiMessage.of(e, R.string.filters_load_failed), info = info ?: it.info.takeIf { quiet }) }
                return
            }
            stateFlow.update { it.copy(phase = FiltersPhase.NONE, filters = null, unsupported = UiMessage.of(e, R.string.filters_none)) }
        } catch (e: Exception) {
            if (mine != ticket) return
            // Rules that are on screen stay; a failed read of a quiet refresh says so.
            stateFlow.update {
                if (it.filters != null) it.copy(error = UiMessage.Res(R.string.filters_load_failed)) else it.copy(phase = FiltersPhase.FAILED)
            }
        }
    }

    fun dismissError() = stateFlow.update { it.copy(error = null) }

    fun dismissInfo() = stateFlow.update { it.copy(info = null) }

    fun dismissDialog() = stateFlow.update { it.copy(dialog = null) }

    // ---- the list ----

    /** The switch "Use these filters". Switching on asks first when it switches something else off. */
    fun setActive(on: Boolean) {
        val filters = stateFlow.value.filters ?: return
        if (stateFlow.value.busy) return
        if (!on) return run { backend.setFiltersActive(false) }
        val asks = FilterRules.askBeforeSwitchingOn(filters)
        if (asks.isEmpty()) switchOn() else stateFlow.update { it.copy(dialog = FiltersDialog.SwitchOn(asks)) }
    }

    fun confirmSwitchOn() {
        stateFlow.update { it.copy(dialog = null) }
        switchOn()
    }

    /** With no script of the app's yet the core refuses to switch one on: the rules are saved, active, instead. */
    private fun switchOn() {
        val filters = stateFlow.value.filters ?: return
        if (filters.text.isEmpty()) run { backend.saveFilters(filters.rules, activate = true, overwrite = false) } else run { backend.setFiltersActive(true) }
    }

    private fun noRuleWhileEdited() = stateFlow.update { it.copy(info = UiMessage.Res(R.string.filters_edited_no_add)) }

    fun moveRule(index: Int, delta: Int) {
        val rules = stateFlow.value.rules
        val moved = FilterRules.move(rules, index, delta)
        if (moved == rules) return
        run { backend.saveFilters(moved, activate = false, overwrite = false) }
    }

    fun setRuleEnabled(index: Int, enabled: Boolean) {
        val rules = stateFlow.value.rules
        if (index !in rules.indices) return
        run { backend.saveFilters(FilterRules.withEnabled(rules, index, enabled), activate = false, overwrite = false) }
    }

    /** "Update": saves the rules again, which brings the script up to date. */
    fun update() {
        val rules = stateFlow.value.rules
        run { backend.saveFilters(rules, activate = false, overwrite = false) }
    }

    fun askStartOver() = stateFlow.update { it.copy(dialog = FiltersDialog.StartOver) }

    fun confirmStartOver() {
        stateFlow.update { it.copy(dialog = null) }
        run { backend.saveFilters(emptyList(), activate = false, overwrite = true) }
    }

    fun showOwnScript() {
        val text = stateFlow.value.filters?.text ?: return
        stateFlow.update { it.copy(dialog = FiltersDialog.Script(null, text)) }
    }

    fun showOtherScript() {
        val filters = stateFlow.value.filters ?: return
        val text = filters.otherText ?: return
        stateFlow.update { it.copy(dialog = FiltersDialog.Script(filters.otherActive.orEmpty(), text)) }
    }

    // ---- the editor ----

    fun addRule() {
        if (stateFlow.value.phase != FiltersPhase.READY) return
        if (stateFlow.value.filters?.isEdited == true) return noRuleWhileEdited()
        val rule = FilterRules.newRule()
        stateFlow.update { it.copy(editor = EditorState(rule, rule, isNew = true), error = null, info = null) }
    }

    fun openRule(index: Int) {
        val rule = stateFlow.value.rules.getOrNull(index) ?: return
        stateFlow.update { it.copy(editor = EditorState(rule, rule, isNew = false), error = null, info = null) }
    }

    private suspend fun openFromMessage(messageId: String) {
        try {
            val rule = withContext(io) { backend.filterFromMessage(messageId) }
            // Something else may have been opened meanwhile; it is not replaced.
            stateFlow.update { if (it.editor != null) it else it.copy(editor = EditorState(rule, rule, isNew = true)) }
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            stateFlow.update { it.copy(error = UiMessage.of(e, R.string.filters_from_message_failed)) }
        }
    }

    fun edit(change: (FilterRule) -> FilterRule) = stateFlow.update { s ->
        // What is typed while a save runs would be thrown away when its answer arrives.
        if (s.busy) return@update s
        s.copy(editor = s.editor?.let { it.copy(rule = change(it.rule)) }, error = null)
    }

    /** The first fault of the rule being edited, for the button to say. */
    fun problem(): RuleProblem? {
        val s = stateFlow.value
        val editor = s.editor ?: return null
        val count = FilterRules.withRule(s.rules, editor.rule).size
        return FilterRules.check(editor.rule, FilterRules.folderTree(s.folders).mapTo(HashSet()) { it.id }, count)
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
        if (s.busy || s.filters == null || problem() != null) return
        val rules = FilterRules.withRule(s.rules, editor.rule)
        run(onDone = { closeEditor() }) { backend.saveFilters(rules, activate = false, overwrite = false) }
    }

    fun askDeleteRule() {
        val editor = stateFlow.value.editor ?: return
        if (editor.isNew) return
        stateFlow.update { it.copy(dialog = FiltersDialog.DeleteRule) }
    }

    fun confirmDeleteRule() {
        val s = stateFlow.value
        val editor = s.editor ?: return
        stateFlow.update { it.copy(dialog = null) }
        if (s.filters == null) return
        val rules = FilterRules.withoutRule(s.rules, editor.original.id)
        run(onDone = { closeEditor() }) { backend.saveFilters(rules, activate = false, overwrite = false) }
    }

    /** Runs one change on the server. The answer replaces the list; a failure says why; "changed elsewhere" reads again. */
    private fun run(onDone: () -> Unit = {}, call: () -> Filters) {
        if (stateFlow.value.busy) return
        val mine = ++ticket
        stateFlow.update { it.copy(busy = true, error = null, info = null) }
        scope.launch {
            try {
                val filters = withContext(io) { call() }
                if (mine != ticket) {
                    stateFlow.update { it.copy(busy = false) }
                    return@launch
                }
                stateFlow.update { it.copy(busy = false, filters = filters, phase = FiltersPhase.READY, error = null) }
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
        if (editor != null && editor.isNew && s.rules.any { it.copy(id = "", folderMissing = false) == editor.rule.copy(id = "", folderMissing = false) }) {
            s.copy(editor = null, dialog = null)
        } else {
            s
        }
    }

    companion object {
        /** The core's refusal when the script changed on the server since it was read. */
        fun changedElsewhere(message: String) = message.contains("changed elsewhere", ignoreCase = true)
    }
}
