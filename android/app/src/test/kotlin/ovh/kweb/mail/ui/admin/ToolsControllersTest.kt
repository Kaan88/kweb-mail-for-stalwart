package ovh.kweb.mail.ui.admin

import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import ovh.kweb.mail.ui.UiMessage
import uniffi.kweb_core.Exception as CoreException
import uniffi.kweb_core.LogLine
import uniffi.kweb_core.Maintenance

class LogsControllerTest {
    private val rig = Rig()
    private lateinit var c: LogsController

    @Before fun setUp() {
        rig.backend.logs = listOf(LogLine(3, "error", "delivery failed"), LogLine(2, "warning", "auth.failed"), LogLine(1, "info", "auth.success"))
        c = LogsController(rig.scope, rig.io, rig.backend)
    }

    @After fun tearDown() = rig.stop()

    @Test fun theFirstLookReadsTheNewestLines() {
        c.start()
        waitFor("lines") { c.state.value.phase == LogPhase.READY }
        assertEquals(3, c.state.value.lines.size)
        assertTrue(rig.backend.calls.contains("logs::200"))
    }

    @Test fun startingAgainKeepsTheLinesAndDoesNotAsk() {
        c.start()
        waitFor("lines") { c.state.value.phase == LogPhase.READY }
        c.start()
        assertEquals(1, rig.backend.calls.count { it.startsWith("logs") })
    }

    @Test fun searchingSendsTheTrimmedTextWithTheLimit() {
        c.typeText("  203.0.113.45 ")
        c.search()
        waitFor("lines") { c.state.value.phase == LogPhase.READY }
        assertTrue(rig.backend.calls.contains("logs:203.0.113.45:200"))
        assertEquals("203.0.113.45", c.state.value.searched)
    }

    @Test fun theLevelPillsOnlyNarrowTheLoadedLines() {
        c.start()
        waitFor("lines") { c.state.value.phase == LogPhase.READY }
        c.setLevel(LevelFilter.Errors)
        assertEquals(listOf("delivery failed"), c.state.value.shown.map { it.text })
        c.setLevel(LevelFilter.Warnings)
        assertEquals(listOf("auth.failed"), c.state.value.shown.map { it.text })
        c.setLevel(LevelFilter.All)
        assertEquals(3, c.state.value.shown.size)
        assertEquals(1, rig.backend.calls.count { it.startsWith("logs") })
    }

    @Test fun aFailedSearchKeepsTheLinesOnScreen() {
        c.start()
        waitFor("lines") { c.state.value.phase == LogPhase.READY }
        rig.backend.failWith = CoreException.Invalid("this key may not read the log")
        c.search()
        waitFor("error") { c.state.value.error != null }
        assertEquals(3, c.state.value.lines.size)
        assertEquals(LogPhase.READY, c.state.value.phase)
        assertEquals(UiMessage.Text("this key may not read the log"), c.state.value.error)
    }

    @Test fun aFailedFirstSearchIsAFailedPhase() {
        rig.backend.failWith = java.io.IOException("x")
        c.start()
        waitFor("failed") { c.state.value.phase == LogPhase.FAILED }
    }
}

class MaintenanceControllerTest {
    private val rig = Rig()
    private lateinit var c: MaintenanceController

    @Before fun setUp() {
        c = MaintenanceController(rig.scope, rig.io, rig.backend)
    }

    @After fun tearDown() = rig.stop()

    @Test fun anActionAsksFirst() {
        c.ask(Maintenance.RELOAD_SETTINGS)
        assertEquals(Maintenance.RELOAD_SETTINGS, c.state.value.asking)
        assertTrue(rig.backend.calls.none { it.startsWith("maintenance") })
        c.dismissDialog()
        c.confirm()
        assertTrue(rig.backend.calls.none { it.startsWith("maintenance") })
    }

    @Test fun confirmingRunsItOnceAndSaysSo() {
        c.ask(Maintenance.RELOAD_CERTIFICATES)
        c.confirm()
        c.confirm()
        waitFor("done") { c.state.value.info != null && c.state.value.running == null }
        assertEquals(1, rig.backend.calls.count { it == "maintenance:${Maintenance.RELOAD_CERTIFICATES}" })
        assertEquals(UiMessage.Res(ovh.kweb.mail.R.string.admin_reload_certs_done), c.state.value.info)
    }

    @Test fun aRefusalIsShownInTheCoresWords() {
        rig.backend.failWith = CoreException.Invalid("this key may not reload settings")
        c.ask(Maintenance.RELOAD_SETTINGS)
        c.confirm()
        waitFor("error") { c.state.value.error != null }
        assertEquals(UiMessage.Text("this key may not reload settings"), c.state.value.error)
        assertNull(c.state.value.running)
    }
}
