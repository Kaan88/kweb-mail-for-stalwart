package ovh.kweb.mail.ui.admin

import uniffi.kweb_core.AdminAccess
import uniffi.kweb_core.AdminSummary
import uniffi.kweb_core.BlockedAddress
import uniffi.kweb_core.Core
import uniffi.kweb_core.LogLine
import uniffi.kweb_core.Maintenance
import uniffi.kweb_core.QueuePage
import uniffi.kweb_core.QueuedMessage
import uniffi.kweb_core.ServerAccount
import uniffi.kweb_core.ServerAccountPage
import uniffi.kweb_core.ServerDomain

/** What the admin screens ask of the core. Every call blocks: run it off the main thread. The tests use a fake. */
interface AdminBackend {
    fun access(): AdminAccess
    fun connect(apiKey: String): AdminAccess
    fun disconnect()
    fun summary(): AdminSummary
    fun queue(offset: UInt, limit: UInt): QueuePage
    fun queuedMessage(id: String): QueuedMessage
    fun queueRetry(id: String)
    fun queueCancel(id: String)
    fun accounts(text: String, offset: UInt, limit: UInt): ServerAccountPage
    fun account(id: String): ServerAccount
    fun domains(): List<ServerDomain>
    fun createAccount(name: String, domainId: String, description: String, password: String): ServerAccount
    fun updateAccount(id: String, description: String, quota: ULong?): ServerAccount
    fun resetPassword(id: String, password: String)
    fun suspendAccount(id: String, suspended: Boolean)
    fun deleteAccount(id: String)
    fun blocked(offset: UInt, limit: UInt): List<BlockedAddress>
    fun unblock(id: String)
    fun logs(text: String, limit: UInt): List<LogLine>
    fun maintenance(what: Maintenance)
}

/** The backend of one account's core. [core] is asked at each call so that a core that was replaced is the one used. */
class CoreAdminBackend(private val core: () -> Core) : AdminBackend {
    override fun access() = core().adminAccess()
    override fun connect(apiKey: String) = core().adminConnect(apiKey)
    override fun disconnect() = core().adminDisconnect()
    override fun summary() = core().adminSummary()
    override fun queue(offset: UInt, limit: UInt) = core().adminQueue(offset, limit)
    override fun queuedMessage(id: String) = core().adminQueuedMessage(id)
    override fun queueRetry(id: String) = core().adminQueueRetry(id)
    override fun queueCancel(id: String) = core().adminQueueCancel(id)
    override fun accounts(text: String, offset: UInt, limit: UInt) = core().adminAccounts(text, offset, limit)
    override fun account(id: String) = core().adminAccount(id)
    override fun domains() = core().adminDomains()
    override fun createAccount(name: String, domainId: String, description: String, password: String) =
        core().adminCreateAccount(name, domainId, description, password)
    override fun updateAccount(id: String, description: String, quota: ULong?) = core().adminUpdateAccount(id, description, quota)
    override fun resetPassword(id: String, password: String) = core().adminResetPassword(id, password)
    override fun suspendAccount(id: String, suspended: Boolean) = core().adminSuspendAccount(id, suspended)
    override fun deleteAccount(id: String) = core().adminDeleteAccount(id)
    override fun blocked(offset: UInt, limit: UInt) = core().adminBlocked(offset, limit)
    override fun unblock(id: String) = core().adminUnblock(id)
    override fun logs(text: String, limit: UInt) = core().adminLogs(text, limit)
    override fun maintenance(what: Maintenance) = core().adminMaintenance(what)
}
