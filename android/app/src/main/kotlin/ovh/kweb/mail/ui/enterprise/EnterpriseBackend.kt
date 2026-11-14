package ovh.kweb.mail.ui.enterprise

import uniffi.kweb_core.Core
import uniffi.kweb_core.DashboardHistory
import uniffi.kweb_core.DeletedPage
import uniffi.kweb_core.DeliveryDetail
import uniffi.kweb_core.DeliveryPage
import uniffi.kweb_core.LiveListener
import uniffi.kweb_core.RecoveryState
import uniffi.kweb_core.ServerAlert
import uniffi.kweb_core.Tenant

/**
 * What the Enterprise screens ask of the core (each with the account's admin key, which the core holds). Every call
 * blocks, [live] for as long as the run lasts: run them off the main thread. The tests use a fake.
 */
interface EnterpriseBackend {
    /** Blocks until [liveStop] is called with [runId] (returns) or the connection ends (throws). */
    fun live(runId: String, listener: LiveListener)

    /** Does not block. */
    fun liveStop(runId: String)

    fun history(hours: UInt): DashboardHistory
    fun deliveries(text: String, since: Long?, offset: UInt, limit: UInt): DeliveryPage
    fun delivery(id: String): DeliveryDetail
    fun recoveryState(): RecoveryState
    fun deletedItems(accountId: String?, offset: UInt, limit: UInt): DeletedPage
    fun restore(accountId: String?, ids: List<String>)
    fun alerts(): List<ServerAlert>
    fun setAlertEnabled(id: String, enabled: Boolean)
    fun tenants(): List<Tenant>

    /** Whether the key may read the server's accounts: only then another account can be picked. */
    fun mayReadAccounts(): Boolean

    /** The accounts whose name, address or description has [text] (at most a short list), for the account chooser. */
    fun findAccounts(text: String): List<AccountMatch>
}

class CoreEnterpriseBackend(private val core: () -> Core) : EnterpriseBackend {
    override fun live(runId: String, listener: LiveListener) = core().adminLive(runId, listener)
    override fun liveStop(runId: String) = core().adminLiveStop(runId)
    override fun history(hours: UInt) = core().adminHistory(hours)
    override fun deliveries(text: String, since: Long?, offset: UInt, limit: UInt) = core().adminDeliveries(text, since, offset, limit)
    override fun delivery(id: String) = core().adminDelivery(id)
    override fun recoveryState() = core().adminRecoveryState()
    override fun deletedItems(accountId: String?, offset: UInt, limit: UInt) = core().adminDeletedItems(accountId, offset, limit)
    override fun restore(accountId: String?, ids: List<String>) = core().adminRestore(accountId, ids)
    override fun alerts() = core().adminAlerts()
    override fun setAlertEnabled(id: String, enabled: Boolean) = core().adminSetAlertEnabled(id, enabled)
    override fun tenants() = core().adminTenants()
    override fun mayReadAccounts() = core().adminAccess().mayAccounts
    override fun findAccounts(text: String) =
        core().adminAccounts(text, 0u, 20u).accounts.map { AccountMatch(it.id, it.email.ifBlank { it.name }, it.description) }
}
