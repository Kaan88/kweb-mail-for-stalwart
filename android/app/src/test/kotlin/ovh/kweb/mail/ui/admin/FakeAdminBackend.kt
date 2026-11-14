package ovh.kweb.mail.ui.admin

import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.Executors
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.asCoroutineDispatcher
import uniffi.kweb_core.AdminAccess
import uniffi.kweb_core.AdminSummary
import uniffi.kweb_core.BlockedAddress
import uniffi.kweb_core.LogLine
import uniffi.kweb_core.Maintenance
import uniffi.kweb_core.QueuePage
import uniffi.kweb_core.QueuedMessage
import uniffi.kweb_core.QueuedRecipient
import uniffi.kweb_core.ServerAccount
import uniffi.kweb_core.ServerAccountKind
import uniffi.kweb_core.ServerAccountPage
import uniffi.kweb_core.ServerDomain

fun access(
    connected: Boolean = true,
    refused: Boolean = false,
    edition: String = "community",
    all: Boolean = true,
    enterprise: Boolean = false,
) = AdminAccess(
    connected = connected,
    keyRefused = refused,
    edition = edition,
    mayQueue = all,
    mayAccounts = all,
    mayBlocked = all,
    mayLogs = all,
    mayMaintenance = all,
    mayDashboard = enterprise,
    mayHistory = enterprise,
    mayRecovery = enterprise,
    mayAlerts = enterprise,
    mayTenants = enterprise,
)

fun queued(id: String, from: String = "a@example.org", error: String = "") = QueuedMessage(
    id = id,
    from = from,
    recipients = listOf(QueuedRecipient("b@example.net", "waiting", "")),
    size = 1000uL,
    queuedAt = 1_000L,
    nextTryAt = 2_000L,
    tries = 2u,
    error = error,
)

fun account(id: String, name: String = id, suspended: Boolean = false, kind: ServerAccountKind = ServerAccountKind.USER) = ServerAccount(
    id = id,
    name = name,
    email = "$name@example.org",
    description = "",
    kind = kind,
    isAdmin = false,
    isSuspended = suspended,
    quota = null,
    used = null,
    aliases = emptyList(),
    groups = emptyList(),
)

/** A scripted core for the admin controllers: it records every call and fails when told to. */
class FakeAdminBackend : AdminBackend {
    val calls = CopyOnWriteArrayList<String>()
    var accessNow: AdminAccess = access(connected = false, all = false)
    var connectResult: AdminAccess = ovh.kweb.mail.ui.admin.access()
    var connectFails: Exception? = null
    var failWith: Exception? = null
    var gate: (() -> Unit)? = null
    var queueMessages: List<QueuedMessage> = emptyList()
    var accountList: List<ServerAccount> = emptyList()
    var domains: List<ServerDomain> = listOf(ServerDomain("d1", "example.org"))
    var blocked: List<BlockedAddress> = emptyList()
    var logs: List<LogLine> = emptyList()
    var summary = AdminSummary(queueWaiting = 3u, accounts = 42u, blocked = 2u)
    /** The last key given to [connect] and the last password given to the password calls. */
    var lastKey: String? = null
    var lastPassword: String? = null

    private fun check(call: String) {
        calls += call
        gate?.invoke()
        failWith?.let { throw it }
    }

    override fun access(): AdminAccess {
        check("access")
        return accessNow
    }

    override fun connect(apiKey: String): AdminAccess {
        calls += "connect"
        lastKey = apiKey
        connectFails?.let { throw it }
        accessNow = connectResult
        return connectResult
    }

    override fun disconnect() {
        calls += "disconnect"
        accessNow = access(connected = false, all = false)
    }

    /** Held up inside [summary] after it has read its answer, to let a newer call overtake it. */
    var summaryGate: (() -> Unit)? = null

    override fun summary(): AdminSummary {
        calls += "summary"
        val answer = summary
        summaryGate?.invoke()
        failWith?.let { throw it }
        return answer
    }

    override fun queue(offset: UInt, limit: UInt): QueuePage {
        check("queue:$offset:$limit")
        return QueuePage(queueMessages.drop(offset.toInt()).take(limit.toInt()), queueMessages.size.toUInt())
    }

    override fun queuedMessage(id: String): QueuedMessage {
        check("message:$id")
        return queueMessages.firstOrNull { it.id == id } ?: throw uniffi.kweb_core.Exception.Invalid(QUEUE_GONE)
    }

    /** The server delivers the message as soon as it is asked to try again. */
    var deliversOnRetry = false

    override fun queueRetry(id: String) {
        check("retry:$id")
        if (deliversOnRetry) queueMessages = queueMessages.filter { it.id != id }
    }

    override fun queueCancel(id: String) {
        check("cancel:$id")
        queueMessages = queueMessages.filter { it.id != id }
    }

    override fun accounts(text: String, offset: UInt, limit: UInt): ServerAccountPage {
        check("accounts:$text:$offset:$limit")
        val all = accountList.filter { it.name.contains(text) }
        return ServerAccountPage(all.drop(offset.toInt()).take(limit.toInt()), all.size.toUInt())
    }

    override fun account(id: String): ServerAccount {
        check("account:$id")
        return accountList.first { it.id == id }
    }

    override fun domains(): List<ServerDomain> {
        check("domains")
        return domains
    }

    override fun createAccount(name: String, domainId: String, description: String, password: String): ServerAccount {
        check("create:$name:$domainId")
        lastPassword = password
        return account("new", name)
    }

    override fun updateAccount(id: String, description: String, quota: ULong?): ServerAccount {
        check("update:$id:$description:$quota")
        return account(id).copy(description = description, quota = quota)
    }

    override fun resetPassword(id: String, password: String) {
        check("reset:$id")
        lastPassword = password
    }

    override fun suspendAccount(id: String, suspended: Boolean) {
        check("suspend:$id:$suspended")
        accountList = accountList.map { if (it.id == id) it.copy(isSuspended = suspended) else it }
    }

    override fun deleteAccount(id: String) = check("delete:$id")

    override fun blocked(offset: UInt, limit: UInt): List<BlockedAddress> {
        check("blocked:$offset:$limit")
        return blocked.drop(offset.toInt()).take(limit.toInt())
    }

    override fun unblock(id: String) {
        check("unblock:$id")
        blocked = blocked.filter { it.id != id }
    }

    override fun logs(text: String, limit: UInt): List<LogLine> {
        check("logs:$text:$limit")
        return logs
    }

    override fun maintenance(what: Maintenance) = check("maintenance:$what")
}

/** The scope and the dispatcher the controller tests share. */
class Rig {
    val pool = Executors.newCachedThreadPool()
    val scope = CoroutineScope(SupervisorJob() + Dispatchers.Unconfined)
    val io = pool.asCoroutineDispatcher()
    val backend = FakeAdminBackend()

    fun stop() {
        pool.shutdownNow()
    }
}

fun waitFor(what: String, check: () -> Boolean) {
    val end = System.currentTimeMillis() + 5000
    while (!check()) {
        if (System.currentTimeMillis() > end) throw AssertionError("timed out waiting for $what")
        Thread.sleep(5)
    }
}
