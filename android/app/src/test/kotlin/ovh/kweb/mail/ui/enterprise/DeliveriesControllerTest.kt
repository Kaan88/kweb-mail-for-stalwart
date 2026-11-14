package ovh.kweb.mail.ui.enterprise

import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.asCoroutineDispatcher
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.UiMessage
import uniffi.kweb_core.DeliveryDetail
import uniffi.kweb_core.DeliveryStatus
import uniffi.kweb_core.DeliveryStep
import uniffi.kweb_core.Exception as CoreException

class DeliveriesControllerTest {
    private val pool = Executors.newCachedThreadPool()
    private val job = SupervisorJob()
    private val scope = CoroutineScope(job + Dispatchers.Unconfined)
    private lateinit var backend: FakeEnterpriseBackend

    @Before fun setUp() {
        backend = FakeEnterpriseBackend()
    }

    @After fun tearDown() {
        job.cancel()
        pool.shutdownNow()
    }

    private fun controller(pageSize: UInt = 2u) = DeliveriesController(scope, pool.asCoroutineDispatcher(), backend, { 1_700_000_000_000L }, pageSize)

    @Test fun readsTheFirstPageWithTheRangeAndAnEmptySearch() {
        val c = controller()
        c.start()
        waitUntil("ready") { c.state.value.phase == Phase.READY }
        assertEquals(listOf("deliveries::true:0:2"), backend.calls.toList())
        assertEquals(listOf("d1", "d2"), c.state.value.deliveries.map { it.id })
        assertEquals(5u, c.state.value.total)
        assertTrue(c.state.value.hasMore)
    }

    @Test fun typingDoesNotSearchUntilAsked() {
        val c = controller()
        c.start()
        waitUntil("ready") { c.state.value.phase == Phase.READY }
        c.setText("  d3 ")
        assertEquals("  d3 ", c.state.value.text)
        assertEquals(1, backend.calls.size)
        c.search()
        waitUntil("found") { c.state.value.deliveries.map { it.id } == listOf("d3") }
        assertEquals("deliveries:d3:true:0:2", backend.calls.last())
    }

    @Test fun anotherRangeSearchesAgain() {
        val c = controller()
        c.start()
        waitUntil("ready") { c.state.value.phase == Phase.READY }
        c.setRange(HistoryRange.MONTH)
        waitUntil("second") { backend.calls.size == 2 && c.state.value.phase == Phase.READY }
        assertEquals(HistoryRange.MONTH, c.state.value.range)
    }

    @Test fun loadMoreAddsTheNextPageUntilTheEnd() {
        val c = controller()
        c.start()
        waitUntil("ready") { c.state.value.phase == Phase.READY }
        c.loadMore()
        waitUntil("page 2") { c.state.value.deliveries.size == 4 && !c.state.value.loadingMore }
        c.loadMore()
        waitUntil("page 3") { c.state.value.deliveries.size == 5 && !c.state.value.loadingMore }
        assertFalse(c.state.value.hasMore)
        val before = backend.calls.size
        c.loadMore()
        Thread.sleep(50)
        assertEquals(before, backend.calls.size)
        assertEquals(listOf("d1", "d2", "d3", "d4", "d5"), c.state.value.deliveries.map { it.id })
    }

    @Test fun morePagesKeepAskingWithTheTextOfTheListNotOfTheField() {
        backend.allDeliveries = (1..6).map { sampleDelivery("x$it") } + sampleDelivery("y1")
        val c = controller()
        c.setText("x")
        c.start()
        waitUntil("ready") { c.state.value.phase == Phase.READY }
        c.setText("something else typed afterwards")
        c.loadMore()
        waitUntil("page 2") { c.state.value.deliveries.size == 4 }
        assertTrue(backend.calls.last().startsWith("deliveries:x:true:2:"))
    }

    @Test fun aLateAnswerOfAnOlderSearchIsDropped() {
        val c = controller()
        val gate = CountDownLatch(1)
        backend.deliveriesGate = { if (it == "slow") gate.await(5, TimeUnit.SECONDS) }
        c.start()
        waitUntil("ready") { c.state.value.phase == Phase.READY }
        backend.allDeliveries = listOf(sampleDelivery("slow1"), sampleDelivery("fast1"))
        c.setText("slow")
        c.search()
        waitUntil("slow asked") { backend.calls.last().startsWith("deliveries:slow") }
        c.setText("fast")
        c.search()
        waitUntil("fast shown") { c.state.value.deliveries.map { it.id } == listOf("fast1") }
        gate.countDown()
        Thread.sleep(100)
        assertEquals(listOf("fast1"), c.state.value.deliveries.map { it.id })
    }

    @Test fun aRefusalShowsTheCoresMessageAndCanBeRetried() {
        backend.failWith = CoreException.Invalid("this needs an Enterprise licence on the server")
        val c = controller()
        c.start()
        waitUntil("failed") { c.state.value.phase == Phase.FAILED }
        assertEquals(UiMessage.Text("this needs an Enterprise licence on the server"), c.state.value.error)
        backend.failWith = null
        c.search()
        waitUntil("ready") { c.state.value.phase == Phase.READY }
        assertNull(c.state.value.error)
    }

    @Test fun aFailedNextPageKeepsTheList() {
        val c = controller()
        c.start()
        waitUntil("ready") { c.state.value.phase == Phase.READY }
        backend.failWith = java.io.IOException("x")
        c.loadMore()
        waitUntil("error") { c.state.value.error != null }
        assertEquals(UiMessage.Res(R.string.ent_deliveries_failed), c.state.value.error)
        assertEquals(Phase.READY, c.state.value.phase)
        assertEquals(2, c.state.value.deliveries.size)
    }

    // ---- one delivery ----

    private fun detailController() = DeliveryDetailController(scope, pool.asCoroutineDispatcher(), backend)

    @Test fun oneDeliveryIsReadOnceForAnId() {
        val c = detailController()
        c.start("d1")
        waitUntil("ready") { c.state.value.phase == Phase.READY }
        c.start("d1")
        Thread.sleep(30)
        assertEquals(listOf("delivery:d1"), backend.calls.toList())
        assertEquals(1, c.state.value.detail!!.steps.size)
    }

    @Test fun theServersOwnLinesAreAToggle() {
        val c = detailController()
        c.start("d1")
        waitUntil("ready") { c.state.value.phase == Phase.READY }
        assertFalse(c.state.value.showRaw)
        c.toggleRaw()
        assertTrue(c.state.value.showRaw)
        c.toggleRaw()
        assertFalse(c.state.value.showRaw)
    }

    @Test fun aFailedDeliveryReadCanBeRetried() {
        backend.failWith = java.io.IOException("x")
        val c = detailController()
        c.start("d1")
        waitUntil("failed") { c.state.value.phase == Phase.FAILED }
        assertEquals(UiMessage.Res(R.string.ent_delivery_failed), c.state.value.error)
        backend.failWith = null
        backend.detail = DeliveryDetail(sampleDelivery("d1", DeliveryStatus.FAILED), listOf(DeliveryStep(1, "x", "", "")))
        c.load()
        waitUntil("ready") { c.state.value.detail != null }
        assertEquals(DeliveryStatus.FAILED, c.state.value.detail!!.delivery.status)
    }

    @Test fun aLateAnswerForAnIdThatWasLeftIsDropped() {
        val c = detailController()
        c.start("d1")
        waitUntil("ready") { c.state.value.phase == Phase.READY }
        backend.detail = DeliveryDetail(sampleDelivery("d2"), emptyList())
        c.start("d2")
        waitUntil("second") { c.state.value.detail?.delivery?.id == "d2" }
    }
}
