package ovh.kweb.mail.ui.filters

import uniffi.kweb_core.Core
import uniffi.kweb_core.FilterRule
import uniffi.kweb_core.FiltersState
import uniffi.kweb_core.Label
import uniffi.kweb_core.Mailbox
import uniffi.kweb_core.RuleMove

/** What the filter screens ask of the core. Every call blocks: run it off the main thread. The tests use a fake. */
interface FiltersBackend {
    fun state(): FiltersState
    fun save(rulesJson: String): FiltersState
    fun setEnabled(on: Boolean): FiltersState
    fun toggle(id: String, enabled: Boolean): FiltersState
    fun move(id: String, up: Boolean): FiltersState
    fun delete(id: String): FiltersState
    fun rawSave(text: String): FiltersState
    fun resetToVisual(): FiltersState
    fun migrateOld(): FiltersState
    fun mailboxes(): List<Mailbox>
    fun labels(): List<Label>

    /** The core's start for a rule made from a message (its sender, or its mailing list). Nothing is stored. */
    fun seed(messageId: String): FilterRule
}

class CoreFiltersBackend(private val core: () -> Core) : FiltersBackend {
    override fun state() = core().filtersState()
    override fun save(rulesJson: String) = core().filtersSave(rulesJson)
    override fun setEnabled(on: Boolean) = core().filtersSetEnabled(on)
    override fun toggle(id: String, enabled: Boolean) = core().filtersRuleToggle(id, enabled)
    override fun move(id: String, up: Boolean) = core().filtersRuleMove(id, if (up) RuleMove.UP else RuleMove.DOWN)
    override fun delete(id: String) = core().filtersRuleDelete(id)
    override fun rawSave(text: String) = core().filtersRawSave(text)
    override fun resetToVisual() = core().filtersResetToVisual()
    override fun migrateOld() = core().filtersMigrateOld()
    override fun mailboxes() = core().mailboxes()
    override fun labels() = core().labels()
    override fun seed(messageId: String) = core().filterFromMessage(messageId)
}
