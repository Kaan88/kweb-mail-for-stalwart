package ovh.kweb.mail.ui.mailextras

import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executors
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.asCoroutineDispatcher
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.UiMessage
import uniffi.kweb_core.Unsubscribe
import uniffi.kweb_core.Exception as CoreException

class UnsubscribeControllerTest {
    private val pool = Executors.newCachedThreadPool()
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Unconfined)
    private val backend = FakeExtras()
    private val errors = CopyOnWriteArrayList<UiMessage>()
    private lateinit var c: UnsubscribeController

    @Before fun setUp() {
        c = UnsubscribeController(scope, pool.asCoroutineDispatcher(), backend) { errors += it }
    }

    @After fun tearDown() {
        pool.shutdownNow()
    }

    private val oneClick = Unsubscribe(true, false, null)

    @Test fun aMessageWithAWayGetsAChip() {
        backend.infos["m1"] = oneClick
        c.check("m1")
        waitFor("chip") { c.state.value["m1"] is UnsubEntry.Available }
        assertEquals(UnsubscribeRules.Way.REQUEST, (c.state.value["m1"] as UnsubEntry.Available).way)
    }

    @Test fun aMessageWithoutOneGetsNone() {
        backend.infos["m1"] = null
        backend.infos["m2"] = Unsubscribe(false, false, null)
        c.check("m1")
        c.check("m2")
        waitFor("asked") { backend.calls.size == 2 }
        Thread.sleep(50)
        assertTrue(c.state.value.isEmpty())
    }

    @Test fun aFailedQuestionShowsNothingAndMayBeAskedAgain() {
        backend.infoFails = java.io.IOException("offline")
        c.check("m1")
        waitFor("asked") { backend.calls.size == 1 }
        Thread.sleep(50)
        assertTrue(c.state.value.isEmpty())
        assertTrue(errors.isEmpty())
        backend.infoFails = null
        backend.infos["m1"] = oneClick
        c.check("m1")
        waitFor("chip") { c.state.value["m1"] is UnsubEntry.Available }
    }

    @Test fun eachMessageIsAskedOnce() {
        backend.infos["m1"] = oneClick
        c.check("m1")
        c.check("m1")
        waitFor("chip") { c.state.value["m1"] is UnsubEntry.Available }
        c.check("m1")
        assertEquals(1, backend.calls.count { it == "info:m1" })
    }

    @Test fun confirmingSendsTheRequestAndTheChipSaysRequested() {
        backend.infos["m1"] = oneClick
        c.check("m1")
        waitFor("chip") { c.state.value["m1"] is UnsubEntry.Available }
        c.confirm("m1")
        waitFor("requested") { c.state.value["m1"] == UnsubEntry.Requested }
        assertEquals(1, backend.calls.count { it == "unsubscribe:m1" })
    }

    @Test fun whileItWorksTheChipIsBusyAndAnotherTapDoesNothing() {
        backend.infos["m1"] = oneClick
        val gate = CountDownLatch(1)
        backend.unsubscribeGate = { gate.await() }
        c.check("m1")
        waitFor("chip") { c.state.value["m1"] is UnsubEntry.Available }
        c.confirm("m1")
        assertEquals(UnsubEntry.Working, c.state.value["m1"])
        c.confirm("m1")
        gate.countDown()
        waitFor("requested") { c.state.value["m1"] == UnsubEntry.Requested }
        assertEquals(1, backend.calls.count { it == "unsubscribe:m1" })
    }

    @Test fun aRefusalComesBackAsTheCoresWordsAndTheChipReturns() {
        backend.infos["m1"] = oneClick
        c.check("m1")
        waitFor("chip") { c.state.value["m1"] is UnsubEntry.Available }
        backend.unsubscribeFails = CoreException.Invalid("the list does not take this")
        c.confirm("m1")
        waitFor("error") { errors.isNotEmpty() }
        assertEquals(UiMessage.Text("the list does not take this"), errors.single())
        waitFor("chip back") { c.state.value["m1"] is UnsubEntry.Available }
    }

    @Test fun anyOtherFailureSaysItInTheAppsWords() {
        backend.infos["m1"] = oneClick
        c.check("m1")
        waitFor("chip") { c.state.value["m1"] is UnsubEntry.Available }
        backend.unsubscribeFails = CoreException.Temporary("timeout")
        c.confirm("m1")
        waitFor("error") { errors.isNotEmpty() }
        assertEquals(UiMessage.Res(R.string.unsub_failed), errors.single())
    }

    @Test fun theBrowserWayIsLeftToTheScreen() {
        backend.infos["m1"] = Unsubscribe(false, false, "https://x.example/u")
        c.check("m1")
        waitFor("chip") { c.state.value["m1"] is UnsubEntry.Available }
        assertEquals("https://x.example/u", (c.state.value["m1"] as UnsubEntry.Available).webUrl)
        c.confirm("m1")
        assertTrue(backend.calls.none { it.startsWith("unsubscribe:") })
        assertTrue(c.state.value["m1"] is UnsubEntry.Available)
    }

    @Test fun anAnswerThatArrivesAfterAResetIsDropped() {
        backend.infos["m1"] = oneClick
        val gate = CountDownLatch(1)
        val answered = CountDownLatch(1)
        val slow = object : MailExtrasBackend by backend {
            override fun unsubscribeInfo(messageId: String): Unsubscribe? {
                gate.await()
                return backend.unsubscribeInfo(messageId).also { answered.countDown() }
            }
        }
        val c2 = UnsubscribeController(scope, pool.asCoroutineDispatcher(), slow) { }
        c2.check("m1")
        c2.reset()
        gate.countDown()
        answered.await()
        Thread.sleep(50)
        assertNull(c2.state.value["m1"])
    }
}
