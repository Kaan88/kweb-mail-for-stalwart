package ovh.kweb.mail.ui.admin

import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test

class HomeControllerTest {
    private val rig = Rig()
    private lateinit var c: HomeController

    @Before fun setUp() {
        c = HomeController(rig.scope, rig.io, rig.backend)
    }

    @After fun tearDown() = rig.stop()

    @Test fun readsTheAccessAndTheFigures() {
        rig.backend.accessNow = access()
        c.refresh()
        waitFor("figures") { c.state.value.summary != null && c.access.state.value.access != null }
        assertEquals(3u, c.state.value.summary!!.queueWaiting)
        assertEquals(42u, c.state.value.summary!!.accounts)
        assertEquals(AccessKind.Open, c.access.state.value.kind)
    }

    @Test fun aFailedFigureReadSaysSo() {
        rig.backend.failWith = java.io.IOException("x")
        c.refresh()
        waitFor("error") { c.state.value.error != null }
        assertTrue(c.state.value.loaded)
    }

    @Test fun takingAKeyReadsTheFiguresAtOnce() {
        rig.backend.accessNow = access(connected = false, all = false)
        c.refresh()
        waitFor("loaded") { c.state.value.loaded }
        rig.backend.failWith = null
        c.access.paste("abcdefgh12345678")
        c.access.confirmKey()
        waitFor("figures") { c.state.value.summary != null }
        assertNotNull(c.state.value.summary)
    }

    @Test fun aLateAnswerDoesNotReplaceANewerOne() {
        rig.backend.accessNow = access()
        val first = java.util.concurrent.CountDownLatch(1)
        var slow = true
        rig.backend.summaryGate = {
            if (slow) {
                slow = false
                first.await(2, java.util.concurrent.TimeUnit.SECONDS)
            }
        }
        rig.backend.summary = uniffi.kweb_core.AdminSummary(1u, 1u, 1u)
        c.refresh()
        Thread.sleep(50)
        rig.backend.summary = uniffi.kweb_core.AdminSummary(9u, 9u, 9u)
        c.refresh()
        waitFor("newer") { c.state.value.summary?.queueWaiting == 9u }
        first.countDown()
        Thread.sleep(100)
        assertEquals(9u, c.state.value.summary!!.queueWaiting)
    }
}
