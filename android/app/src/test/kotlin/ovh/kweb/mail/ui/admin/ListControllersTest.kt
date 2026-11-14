package ovh.kweb.mail.ui.admin

import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import ovh.kweb.mail.ui.UiMessage
import uniffi.kweb_core.BlockedAddress
import uniffi.kweb_core.Maintenance
import uniffi.kweb_core.Exception as CoreException

class QueueControllerTest {
    private val rig = Rig()
    private val backend get() = rig.backend
    private lateinit var c: QueueController

    @Before fun setUp() {
        backend.queueMessages = listOf(queued("m1"), queued("m2"), queued("m3"))
        c = QueueController(rig.scope, rig.io, backend)
    }

    @After fun tearDown() = rig.stop()

    private fun loaded() {
        c.refresh()
        waitFor("list") { c.list.state.value.phase == ListPhase.READY }
    }

    private fun idle() = waitFor("idle") { !c.state.value.busy }

    @Test fun listsTheMessagesWithTheTotal() {
        loaded()
        assertEquals(3, c.list.state.value.items.size)
        assertEquals(3, c.list.state.value.total)
        assertFalse(c.list.state.value.more)
    }

    @Test fun anEmptyQueueIsReadyWithNothing() {
        backend.queueMessages = emptyList()
        loaded()
        assertTrue(c.list.state.value.items.isEmpty())
    }

    @Test fun aFailedReadOfTheListCanBeRetried() {
        backend.failWith = CoreException.Invalid("the key may not do that")
        c.refresh()
        waitFor("failed") { c.list.state.value.phase == ListPhase.FAILED }
        assertEquals(UiMessage.Text("the key may not do that"), c.list.state.value.error)
        backend.failWith = null
        c.list.reload(quiet = false)
        waitFor("ready") { c.list.state.value.phase == ListPhase.READY }
    }

    @Test fun retryAsksTheServerAndReadsTheListAgain() {
        loaded()
        c.retry("m2")
        idle()
        assertTrue(backend.calls.contains("retry:m2"))
        assertEquals(UiMessage.Res(ovh.kweb.mail.R.string.admin_retry_done), c.state.value.info)
        waitFor("reread") { backend.calls.count { it.startsWith("queue:") } >= 2 }
    }

    @Test fun cancelAsksFirstAndOnlyThenCancels() {
        loaded()
        c.askCancel(c.list.state.value.items[1])
        assertEquals(QueueDialog.Cancel("m2", "a@example.org"), c.state.value.dialog)
        assertTrue(backend.calls.none { it.startsWith("cancel") })
        c.dismissDialog()
        c.confirmCancel()
        assertTrue(backend.calls.none { it.startsWith("cancel") })
        c.askCancel(c.list.state.value.items[1])
        c.confirmCancel()
        idle()
        assertEquals(1, backend.calls.count { it == "cancel:m2" })
        waitFor("gone") { c.list.state.value.items.none { it.id == "m2" } }
    }

    @Test fun pauseAsksFirstResumeDoesNot() {
        loaded()
        c.askPause()
        assertEquals(QueueDialog.Pause, c.state.value.dialog)
        assertTrue(backend.calls.none { it.startsWith("maintenance") })
        c.confirmPause()
        idle()
        assertTrue(c.state.value.paused)
        assertTrue(backend.calls.contains("maintenance:${Maintenance.PAUSE_QUEUE}"))
        c.resume()
        idle()
        assertFalse(c.state.value.paused)
        assertTrue(backend.calls.contains("maintenance:${Maintenance.RESUME_QUEUE}"))
    }

    @Test fun aRefusalIsShownInTheCoresWordsAndNothingChanges() {
        loaded()
        backend.failWith = CoreException.Invalid("this key may not change the queue")
        c.retry("m1")
        idle()
        assertEquals(UiMessage.Text("this key may not change the queue"), c.state.value.error)
        assertNull(c.state.value.info)
    }

    @Test fun aSecondChangeWhileOneRunsIsRefused() {
        loaded()
        val release = java.util.concurrent.CountDownLatch(1)
        backend.gate = { release.await(2, java.util.concurrent.TimeUnit.SECONDS) }
        c.retry("m1")
        c.retry("m2")
        release.countDown()
        idle()
        assertEquals(1, backend.calls.count { it.startsWith("retry") })
    }

    @Test fun morePagesAreAddedAtTheEnd() {
        backend.queueMessages = (1..120).map { queued("m$it") }
        loaded()
        assertEquals(50, c.list.state.value.items.size)
        assertTrue(c.list.state.value.more)
        c.list.loadMore()
        waitFor("page 2") { c.list.state.value.items.size == 100 }
        c.list.loadMore()
        waitFor("page 3") { c.list.state.value.items.size == 120 }
        assertFalse(c.list.state.value.more)
        assertEquals("m51", c.list.state.value.items[50].id)
    }

    @Test fun aLateListAnswerDoesNotReplaceANewerOne() {
        val release = java.util.concurrent.CountDownLatch(1)
        var first = true
        backend.gate = {
            if (first) {
                first = false
                release.await(2, java.util.concurrent.TimeUnit.SECONDS)
            }
        }
        backend.queueMessages = listOf(queued("old"))
        c.refresh()
        Thread.sleep(50)
        backend.queueMessages = listOf(queued("new"))
        c.refresh()
        waitFor("new") { c.list.state.value.items.map { it.id } == listOf("new") }
        release.countDown()
        Thread.sleep(100)
        assertEquals(listOf("new"), c.list.state.value.items.map { it.id })
    }
}

class QueuedMessageControllerTest {
    private val rig = Rig()
    private lateinit var c: QueuedMessageController

    @Before fun setUp() {
        rig.backend.queueMessages = listOf(queued("m1", error = "timeout"))
        c = QueuedMessageController(rig.scope, rig.io, rig.backend, "m1")
    }

    @After fun tearDown() = rig.stop()

    @Test fun readsOneMessageAndRetriesIt() {
        c.refresh()
        waitFor("message") { c.state.value.message != null }
        assertEquals("timeout", c.state.value.message!!.error)
        c.retry()
        waitFor("info") { c.state.value.info != null }
        assertTrue(rig.backend.calls.contains("retry:m1"))
    }

    @Test fun cancellingClosesTheScreenAfterAsking() {
        c.refresh()
        waitFor("message") { c.state.value.message != null }
        c.askCancel()
        assertTrue(c.state.value.askCancel)
        assertTrue(rig.backend.calls.none { it.startsWith("cancel") })
        c.confirmCancel()
        waitFor("gone") { c.state.value.gone }
    }

    @Test fun aMessageThatLeftTheQueueClosesTheScreen() {
        rig.backend.queueMessages = emptyList()
        c.refresh()
        waitFor("gone") { c.state.value.gone }
    }

    @Test fun aRetryThatDeliversTheMessageClosesTheScreenInsteadOfShowingStaleRows() {
        c.refresh()
        waitFor("message") { c.state.value.message != null }
        rig.backend.deliversOnRetry = true
        c.retry()
        waitFor("gone") { c.state.value.gone }
        assertNull(c.state.value.error)
    }
}

class BlockedControllerTest {
    private val rig = Rig()
    private lateinit var c: BlockedController

    private fun block(id: String) = BlockedAddress(id, "203.0.113.$id", "", null, null)

    @Before fun setUp() {
        rig.backend.blocked = (1..3).map { block(it.toString()) }
        c = BlockedController(rig.scope, rig.io, rig.backend)
    }

    @After fun tearDown() = rig.stop()

    @Test fun unblockAsksFirstAndNamesTheAddress() {
        c.refresh()
        waitFor("list") { c.list.state.value.phase == ListPhase.READY }
        c.askUnblock(c.list.state.value.items[0])
        assertEquals("203.0.113.1", c.state.value.asking!!.address)
        assertTrue(rig.backend.calls.none { it.startsWith("unblock") })
        c.confirmUnblock()
        waitFor("removed") { c.list.state.value.items.size == 2 }
        assertTrue(rig.backend.calls.contains("unblock:1"))
        assertNull(c.state.value.asking)
    }

    @Test fun aFullPageMeansThereMayBeMore() {
        rig.backend.blocked = (1..50).map { block(it.toString()) }
        c.refresh()
        waitFor("list") { c.list.state.value.phase == ListPhase.READY }
        assertTrue(c.list.state.value.more)
        assertNull(c.list.state.value.total)
    }

    @Test fun aFewAddressesMeanNoMore() {
        c.refresh()
        waitFor("list") { c.list.state.value.phase == ListPhase.READY }
        assertFalse(c.list.state.value.more)
    }

    @Test fun aFailedUnblockSaysWhy() {
        c.refresh()
        waitFor("list") { c.list.state.value.phase == ListPhase.READY }
        rig.backend.failWith = CoreException.Invalid("no such address")
        c.askUnblock(c.list.state.value.items[0])
        c.confirmUnblock()
        waitFor("error") { c.state.value.error != null }
        assertEquals(3, c.list.state.value.items.size)
    }
}
