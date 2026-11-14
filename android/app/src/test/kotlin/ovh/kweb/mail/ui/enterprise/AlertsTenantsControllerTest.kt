package ovh.kweb.mail.ui.enterprise

import java.util.concurrent.Executors
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.asCoroutineDispatcher
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.UiMessage
import uniffi.kweb_core.Tenant
import uniffi.kweb_core.Exception as CoreException

class AlertsTenantsControllerTest {
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

    private fun alerts() = AlertsController(scope, pool.asCoroutineDispatcher(), backend)

    @Test fun alertsAreListed() {
        val c = alerts()
        c.start()
        waitUntil("ready") { c.state.value.phase == Phase.READY }
        assertEquals(listOf("a1", "a2"), c.state.value.alerts.map { it.id })
        c.start()
        assertEquals(1, backend.calls.count { it == "alerts" })
    }

    @Test fun aSwitchShowsAtOnceAndIsSentToTheServer() {
        val c = alerts()
        c.start()
        waitUntil("ready") { c.state.value.phase == Phase.READY }
        c.setEnabled("a1", true)
        assertTrue(c.state.value.alerts.first { it.id == "a1" }.isEnabled)
        waitUntil("done") { c.state.value.pending.isEmpty() }
        assertEquals("alert:a1:true", backend.calls.last())
        assertTrue(c.state.value.alerts.first { it.id == "a1" }.isEnabled)
    }

    @Test fun aRefusedSwitchGoesBackAndSaysWhy() {
        val c = alerts()
        c.start()
        waitUntil("ready") { c.state.value.phase == Phase.READY }
        backend.failWith = CoreException.Invalid("the alert is not yours to change")
        c.setEnabled("a2", false)
        waitUntil("error") { c.state.value.error != null && c.state.value.pending.isEmpty() }
        assertEquals(UiMessage.Text("the alert is not yours to change"), c.state.value.error)
        assertTrue(c.state.value.alerts.first { it.id == "a2" }.isEnabled)
    }

    @Test fun aSwitchToWhatItAlreadyIsDoesNothing() {
        val c = alerts()
        c.start()
        waitUntil("ready") { c.state.value.phase == Phase.READY }
        c.setEnabled("a2", true)
        c.setEnabled("nobody", true)
        Thread.sleep(30)
        assertTrue(backend.calls.none { it.startsWith("alert:") })
    }

    @Test fun aFailedReadCanBeRetried() {
        backend.failWith = java.io.IOException("x")
        val c = alerts()
        c.start()
        waitUntil("failed") { c.state.value.phase == Phase.FAILED }
        assertEquals(UiMessage.Res(R.string.ent_alerts_failed), c.state.value.error)
        backend.failWith = null
        c.load()
        waitUntil("ready") { c.state.value.phase == Phase.READY }
        assertNull(c.state.value.error)
    }

    // ---- the generic read, as the tenants use it ----

    private fun tenants() = LoadController<List<Tenant>>(scope, pool.asCoroutineDispatcher(), R.string.ent_tenants_failed) { backend.tenants() }

    @Test fun tenantsAreReadOnce() {
        val c = tenants()
        c.start()
        waitUntil("ready") { c.state.value.phase == Phase.READY }
        c.start()
        Thread.sleep(30)
        assertEquals(listOf("tenants"), backend.calls.toList())
        assertEquals("Acme", c.state.value.value!!.single().name)
    }

    @Test fun aFailedTenantReadCanBeRetriedAndKeepsWhatWasShownOnALaterFailure() {
        backend.failWith = CoreException.Invalid("this needs an Enterprise licence on the server")
        val c = tenants()
        c.start()
        waitUntil("failed") { c.state.value.phase == Phase.FAILED }
        assertEquals(UiMessage.Text("this needs an Enterprise licence on the server"), c.state.value.error)
        backend.failWith = null
        c.reload()
        waitUntil("ready") { c.state.value.phase == Phase.READY }
        assertNotNull(c.state.value.value)
        backend.failWith = java.io.IOException("x")
        c.reload()
        waitUntil("error over data") { c.state.value.error != null }
        assertNotNull(c.state.value.value)
        assertEquals(Phase.READY, c.state.value.phase)
        c.dismissError()
        assertFalse(c.state.value.error != null)
    }
}
