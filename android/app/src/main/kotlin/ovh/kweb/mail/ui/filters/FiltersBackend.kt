package ovh.kweb.mail.ui.filters

import uniffi.kweb_core.Core
import uniffi.kweb_core.FilterRule
import uniffi.kweb_core.Filters
import uniffi.kweb_core.Mailbox

/** What the filter screens ask of the core. Every call blocks: run it off the main thread. The tests use a fake. */
interface FiltersBackend {
    fun filters(): Filters
    fun saveFilters(rules: List<FilterRule>, activate: Boolean, overwrite: Boolean): Filters
    fun setFiltersActive(active: Boolean): Filters
    fun filterFromMessage(messageId: String): FilterRule
    fun mailboxes(): List<Mailbox>
}

class CoreFiltersBackend(private val core: () -> Core) : FiltersBackend {
    override fun filters() = core().filters()
    override fun saveFilters(rules: List<FilterRule>, activate: Boolean, overwrite: Boolean) = core().saveFilters(rules, activate, overwrite)
    override fun setFiltersActive(active: Boolean) = core().setFiltersActive(active)
    override fun filterFromMessage(messageId: String) = core().filterFromMessage(messageId)
    override fun mailboxes() = core().mailboxes()
}
