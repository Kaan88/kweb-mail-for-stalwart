package ovh.kweb.mail.ui.enterprise

import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.asCoroutineDispatcher
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.UiMessage
import uniffi.kweb_core.ChartPoint
import uniffi.kweb_core.DashboardHistory
import uniffi.kweb_core.LiveFigures
import uniffi.kweb_core.Exception as CoreException

class DashboardControllerTest {
    private val pool = Executors.newCachedThreadPool()
    private val job = SupervisorJob()
    private val scope = CoroutineScope(job + Dispatchers.Unconfined)
    private lateinit var backend: FakeEnterpriseBackend
    private val runs = AtomicInteger()

    @Before fun setUp() {
        backend = FakeEnterpriseBackend()
    }

    @After fun tearDown() {
        job.cancel()
        pool.shutdownNow()
    }

    private fun controller(retryMs: Long = 60_000) =
        DashboardController(scope, pool.asCoroutineDispatcher(), backend, { "run-${runs.incrementAndGet()}" }, retryMs)

    private val figures = LiveFigures(3u, 18u, 42u, 412_000_000u)

    @Test fun readsTheHistoryOfTheDefaultRangeOnStart() {
        val c = controller()
        c.start()
        waitUntil("history") { c.state.value.historyPhase == Phase.READY }
        assertEquals(listOf("history:24"), backend.calls.toList())
        assertEquals(backend.history, c.state.value.history)
    }

    @Test fun changingTheRangeReadsAgainAndKeepsTheOldChartMeanwhile() {
        val c = controller()
        c.start()
        waitUntil("first") { c.state.value.historyPhase == Phase.READY }
        val gate = CountDownLatch(1)
        backend.historyGate = { gate.await(5, TimeUnit.SECONDS) }
        c.setRange(ChartRange.WEEK)
        assertEquals(ChartRange.WEEK, c.state.value.range)
        assertNotNull(c.state.value.history)
        assertEquals(Phase.READY, c.state.value.historyPhase)
        gate.countDown()
        waitUntil("second call") { backend.calls.contains("history:168") }
    }

    @Test fun aLateAnswerDoesNotReplaceANewerOne() {
        val c = controller()
        val slow = DashboardHistory(listOf(ChartPoint(1, 1.0)), emptyList(), emptyList())
        val fast = DashboardHistory(listOf(ChartPoint(2, 2.0)), emptyList(), emptyList())
        val gate = CountDownLatch(1)
        backend.history = slow
        backend.historyGate = { if (it == 24u) gate.await(5, TimeUnit.SECONDS) }
        c.start()
        waitUntil("first call") { backend.calls.contains("history:24") }
        backend.history = fast
        c.setRange(ChartRange.MONTH)
        waitUntil("newer answer") { c.state.value.history == fast }
        gate.countDown()
        Thread.sleep(100)
        assertEquals(fast, c.state.value.history)
    }

    @Test fun aFailedFirstReadShowsTheCoresWordsAndCanBeRetried() {
        backend.failWith = CoreException.Invalid("this needs an Enterprise licence on the server")
        val c = controller()
        c.start()
        waitUntil("failed") { c.state.value.historyPhase == Phase.FAILED }
        assertEquals(UiMessage.Text("this needs an Enterprise licence on the server"), c.state.value.error)
        backend.failWith = null
        c.loadHistory()
        waitUntil("ready") { c.state.value.historyPhase == Phase.READY }
        assertNull(c.state.value.error)
    }

    @Test fun otherFailuresUseTheAppsOwnWords() {
        backend.failWith = java.io.IOException("socket")
        val c = controller()
        c.start()
        waitUntil("failed") { c.state.value.historyPhase == Phase.FAILED }
        assertEquals(UiMessage.Res(R.string.ent_dash_history_failed), c.state.value.error)
    }

    @Test fun theStreamStartsFiguresArriveAndLiveShows() {
        val c = controller()
        c.startLive()
        assertEquals(LiveStatus.CONNECTING, c.state.value.live)
        waitUntil("run") { backend.listeners.isNotEmpty() }
        backend.listeners[0].figures(figures)
        assertEquals(LiveStatus.LIVE, c.state.value.live)
        assertEquals(figures, c.state.value.figures)
    }

    @Test fun stoppingClosesTheRunAndShowsPaused() {
        val c = controller()
        c.startLive()
        waitUntil("run") { backend.listeners.isNotEmpty() }
        c.stopLive()
        waitUntil("stopped") { backend.liveStopped.toList() == listOf("run-1") }
        assertEquals(LiveStatus.PAUSED, c.state.value.live)
        // Figures that still arrive from the stopped run are ignored.
        backend.listeners[0].figures(figures)
        assertNull(c.state.value.figures)
    }

    @Test fun aSecondStartWhileRunningDoesNotOpenASecondStream() {
        val c = controller()
        c.startLive()
        waitUntil("run") { backend.listeners.isNotEmpty() }
        c.startLive()
        Thread.sleep(100)
        assertEquals(1, backend.liveRuns.size)
    }

    @Test fun startingAgainAfterAStopOpensANewRunWithItsOwnName() {
        val c = controller()
        c.startLive()
        waitUntil("run 1") { backend.listeners.size == 1 }
        c.stopLive()
        c.startLive()
        waitUntil("run 2") { backend.liveRuns.size == 2 }
        assertEquals(listOf("run-1", "run-2"), backend.liveRuns.toList())
        // The first run's figures do not count any more, the second one's do.
        backend.listeners[0].figures(figures)
        assertNull(c.state.value.figures)
        backend.listeners[1].figures(figures)
        assertEquals(figures, c.state.value.figures)
    }

    @Test fun aStreamThatEndsByItselfShowsPausedAndIsOpenedAgain() {
        backend.liveFailsAfterFirst = true
        val c = controller(retryMs = 20)
        c.startLive()
        waitUntil("paused") { c.state.value.live == LiveStatus.PAUSED || backend.liveRuns.size >= 2 }
        waitUntil("second attempt") { backend.liveRuns.size >= 2 }
        backend.liveFailsAfterFirst = false
        waitUntil("third attempt held open") { backend.liveRuns.size >= 3 && backend.listeners.size >= 3 }
        c.stopLive()
        val count = backend.liveRuns.size
        Thread.sleep(100)
        assertEquals("no more attempts after stop", count, backend.liveRuns.size)
    }

    @Test fun aRefusedKeyIsNotTriedAgainEveryFewSecondsAndTheReasonIsShown() {
        backend.liveRefusal = uniffi.kweb_core.Exception.Invalid("the server did not accept the admin key")
        val c = controller(retryMs = 10)
        c.startLive()
        waitUntil("paused with the reason") { c.state.value.error != null }
        Thread.sleep(150)
        assertEquals("one try only", 1, backend.liveRuns.size)
        assertEquals(LiveStatus.PAUSED, c.state.value.live)
        assertEquals(UiMessage.Text("the server did not accept the admin key"), c.state.value.error)
        // Starting the screen again is a new try.
        c.startLive()
        waitUntil("second try") { backend.liveRuns.size == 2 }
    }

    @Test fun shutdownStopsTheRunEvenWhenTheScopeIsGone() {
        val c = controller()
        c.startLive()
        waitUntil("run") { backend.listeners.isNotEmpty() }
        job.cancel()
        c.shutdown()
        assertTrue(backend.liveStopped.contains("run-1"))
    }

    @Test fun resetEmptiesTheScreenAndStopsTheStream() {
        val c = controller()
        c.start()
        c.startLive()
        waitUntil("run") { backend.listeners.isNotEmpty() }
        waitUntil("history") { c.state.value.history != null }
        c.reset()
        assertNull(c.state.value.history)
        assertNull(c.state.value.figures)
        waitUntil("stopped") { backend.liveStopped.isNotEmpty() }
    }
}
