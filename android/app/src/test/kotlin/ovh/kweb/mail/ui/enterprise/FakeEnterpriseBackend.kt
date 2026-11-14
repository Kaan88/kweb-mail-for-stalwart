package ovh.kweb.mail.ui.enterprise

import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import uniffi.kweb_core.ChartPoint
import uniffi.kweb_core.DashboardHistory
import uniffi.kweb_core.DeletedItem
import uniffi.kweb_core.DeletedKind
import uniffi.kweb_core.DeletedPage
import uniffi.kweb_core.Delivery
import uniffi.kweb_core.DeliveryDetail
import uniffi.kweb_core.DeliveryPage
import uniffi.kweb_core.DeliveryStatus
import uniffi.kweb_core.DeliveryStep
import uniffi.kweb_core.LiveListener
import uniffi.kweb_core.RecoveryState
import uniffi.kweb_core.ServerAlert
import uniffi.kweb_core.Tenant

internal fun sampleDelivery(id: String, status: DeliveryStatus = DeliveryStatus.DELIVERED, to: List<String> = listOf("bob@example.org")) =
    Delivery(id, "ada@example.org", to, 1_700_000_000L, 1000u, status)

internal fun deleted(id: String, kind: DeletedKind = DeletedKind.MAIL, title: String = "t-$id", at: Long? = 1_700_000_000L) =
    DeletedItem(id, kind, title, at)

/** The core as far as the Enterprise screens use it, with calls recorded and answers the test can change or hold back. */
internal class FakeEnterpriseBackend : EnterpriseBackend {
    val calls = CopyOnWriteArrayList<String>()
    var failWith: Exception? = null

    var history = DashboardHistory(listOf(ChartPoint(0, 1.0)), listOf(ChartPoint(0, 1.0)), listOf(ChartPoint(0, 1.0)))
    var historyGate: ((UInt) -> Unit)? = null

    /** All the deliveries there are; the fake pages through them. */
    var allDeliveries: List<Delivery> = (1..5).map { sampleDelivery("d$it") }
    var deliveriesGate: ((String) -> Unit)? = null
    var detail: DeliveryDetail = DeliveryDetail(sampleDelivery("d1"), listOf(DeliveryStep(1, "Started", "", "raw one")))

    var recovery = RecoveryState(true, 30u)
    var deletedItems: Map<String?, List<DeletedItem>> = mapOf(null to listOf(deleted("m1"), deleted("m2", DeletedKind.EVENT)))
    var alerts = listOf(ServerAlert("a1", "Queue is long", false, "when 100 wait"), ServerAlert("a2", "Disk", true, ""))
    var tenants = listOf(Tenant("t1", "Acme", 1000u, 250u))

    // The live stream: blocks until stopped, or until the test makes it end.
    val liveRuns = CopyOnWriteArrayList<String>()
    val liveStopped = CopyOnWriteArrayList<String>()
    val listeners = CopyOnWriteArrayList<LiveListener>()
    private val stops = java.util.concurrent.ConcurrentHashMap<String, CountDownLatch>()
    @Volatile var liveFailsAfterFirst = false
    /** The core refuses the stream (a dead key, no permission): what `Invalid` means there. */
    @Volatile var liveRefusal: Exception? = null

    private fun fail() {
        failWith?.let { throw it }
    }

    override fun live(runId: String, listener: LiveListener) {
        calls += "live:$runId"
        liveRuns += runId
        listeners += listener
        val latch = stops.getOrPut(runId) { CountDownLatch(1) }
        liveRefusal?.let { throw it }
        if (liveFailsAfterFirst) throw java.io.IOException("gone")
        latch.await(10, TimeUnit.SECONDS)
    }

    override fun liveStop(runId: String) {
        liveStopped += runId
        stops.getOrPut(runId) { CountDownLatch(1) }.countDown()
    }

    override fun history(hours: UInt): DashboardHistory {
        calls += "history:$hours"
        historyGate?.invoke(hours)
        fail()
        return history
    }

    override fun deliveries(text: String, since: Long?, offset: UInt, limit: UInt): DeliveryPage {
        calls += "deliveries:$text:${since != null}:$offset:$limit"
        deliveriesGate?.invoke(text)
        fail()
        val rows = allDeliveries.filter { text.isBlank() || it.id.contains(text) }
        return DeliveryPage(rows.drop(offset.toInt()).take(limit.toInt()), rows.size.toUInt())
    }

    override fun delivery(id: String): DeliveryDetail {
        calls += "delivery:$id"
        fail()
        return detail
    }

    override fun recoveryState(): RecoveryState {
        calls += "recovery"
        fail()
        return recovery
    }

    /** Held back for a page read (given its offset). */
    @Volatile var deletedGate: ((UInt) -> Unit)? = null

    override fun deletedItems(accountId: String?, offset: UInt, limit: UInt): DeletedPage {
        calls += "deleted:$accountId:$offset:$limit"
        deletedGate?.invoke(offset)
        fail()
        val rows = deletedItems[accountId] ?: emptyList()
        return DeletedPage(rows.drop(offset.toInt()).take(limit.toInt()), rows.size.toUInt())
    }

    override fun restore(accountId: String?, ids: List<String>) {
        calls += "restore:$accountId:${ids.joinToString(",")}"
        fail()
    }

    override fun alerts(): List<ServerAlert> {
        calls += "alerts"
        fail()
        return alerts
    }

    override fun setAlertEnabled(id: String, enabled: Boolean) {
        calls += "alert:$id:$enabled"
        fail()
    }

    /** Whether the key may read accounts, and the accounts the chooser can find. */
    @Volatile var mayRead = true
    var accountMatches = listOf(AccountMatch("acc9", "bob@example.org", "Bob"), AccountMatch("acc7", "carol@example.org"))

    override fun mayReadAccounts() = mayRead

    override fun findAccounts(text: String): List<AccountMatch> {
        calls += "find:$text"
        fail()
        return accountMatches.filter { text.isBlank() || it.label.contains(text) }
    }

    override fun tenants(): List<Tenant> {
        calls += "tenants"
        fail()
        return tenants
    }
}

internal fun waitUntil(what: String, check: () -> Boolean) {
    val end = System.currentTimeMillis() + 5000
    while (!check()) {
        if (System.currentTimeMillis() > end) throw AssertionError("timed out waiting for $what")
        Thread.sleep(5)
    }
}
