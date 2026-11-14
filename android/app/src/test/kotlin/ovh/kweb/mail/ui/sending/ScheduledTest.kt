package ovh.kweb.mail.ui.sending

import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import ovh.kweb.mail.ui.UiMessage
import uniffi.kweb_core.Draft
import uniffi.kweb_core.ScheduledSend
import uniffi.kweb_core.Exception as CoreException

private fun held(id: String, at: Long, subject: String = "S$id", to: List<String> = listOf("a@x.org")) = ScheduledSend(id, subject, to, at)

private fun draftOf(id: String) = Draft(id, "i", emptyList(), emptyList(), emptyList(), "S", "b", emptyList(), null, false, null, null, 0L)

private class FakeScheduled : ScheduledBackend {
    var list: List<ScheduledSend> = emptyList()
    val calls = CopyOnWriteArrayList<String>()
    var failList: Exception? = null
    var failCancel: Exception? = null
    var gate: (() -> Unit)? = null

    override fun scheduledSends(): List<ScheduledSend> {
        val before = list
        gate?.invoke()
        calls += "list"
        failList?.let { throw it }
        return before
    }

    override fun cancelScheduled(id: String): Draft {
        calls += "cancel:$id"
        failCancel?.let { throw it }
        list = list.filter { it.id != id }
        return draftOf("draft-$id")
    }
}

class ScheduledRulesTest {
    @Test fun recipientsShowTwoAndCountTheRest() {
        val r = ScheduledRules.recipients(listOf("a@x", " b@x ", "a@x", "c@x", "d@x", ""))
        assertEquals(listOf("a@x", "b@x"), r.shown)
        assertEquals(2, r.more)
    }

    @Test fun fewRecipientsHaveNoMore() {
        val r = ScheduledRules.recipients(listOf("a@x"))
        assertEquals(listOf("a@x"), r.shown)
        assertEquals(0, r.more)
        assertEquals(ScheduledRules.Recipients(emptyList(), 0), ScheduledRules.recipients(emptyList()))
    }

    @Test fun theListIsSoonestFirst() {
        assertEquals(listOf("b", "a", "c"), ScheduledRules.ordered(listOf(held("a", 20), held("c", 20), held("b", 10))).map { it.id })
    }
}

class ScheduledControllerTest {
    private fun controller(b: FakeScheduled) = ScheduledController(CoroutineScope(SupervisorJob() + Dispatchers.Unconfined), Dispatchers.Unconfined, b)

    @Test fun startReadsOnceAndSortsTheList() {
        val b = FakeScheduled().also { it.list = listOf(held("late", 50), held("soon", 5)) }
        val c = controller(b)
        c.start()
        c.start()
        assertEquals(ScheduledPhase.READY, c.state.value.phase)
        assertEquals(listOf("soon", "late"), c.state.value.items.map { it.id })
        assertEquals(1, b.calls.count { it == "list" })
    }

    @Test fun aFailedFirstReadIsAFailureAndRetryWorks() {
        val b = FakeScheduled().also { it.failList = CoreException.Temporary("offline") }
        val c = controller(b)
        c.start()
        assertEquals(ScheduledPhase.FAILED, c.state.value.phase)
        b.failList = null
        b.list = listOf(held("a", 1))
        c.refresh()
        assertEquals(ScheduledPhase.READY, c.state.value.phase)
    }

    @Test fun aFailedRefreshKeepsTheListAndSaysSo() {
        val b = FakeScheduled().also { it.list = listOf(held("a", 1)) }
        val c = controller(b)
        c.start()
        b.failList = CoreException.Temporary("offline")
        c.refresh(pull = true)
        assertEquals(1, c.state.value.items.size)
        assertNotNull(c.state.value.error)
        assertEquals(false, c.state.value.refreshing)
    }

    @Test fun cancellingAsksFirstAndThenOpensTheDraft() {
        val b = FakeScheduled().also { it.list = listOf(held("a", 1), held("b", 2)) }
        val c = controller(b)
        c.start()
        c.ask(c.state.value.items[0])
        assertEquals("a", c.state.value.asking?.id)
        assertTrue(b.calls.none { it.startsWith("cancel") })
        c.confirmCancel()
        assertEquals(listOf("b"), c.state.value.items.map { it.id })
        assertNull(c.state.value.busyId)
        assertEquals("draft-a", runBlocking { c.openDraft.first() })
    }

    @Test fun decliningTheQuestionCancelsNothing() {
        val b = FakeScheduled().also { it.list = listOf(held("a", 1)) }
        val c = controller(b)
        c.start()
        c.ask(c.state.value.items[0])
        c.dismissAsk()
        c.confirmCancel()
        assertTrue(b.calls.none { it.startsWith("cancel") })
        assertEquals(1, c.state.value.items.size)
    }

    @Test fun tooLateShowsTheCoresWordsAndReadsTheListAgain() {
        val b = FakeScheduled().also { it.list = listOf(held("a", 1)); it.failCancel = CoreException.Invalid("It has already been sent.") }
        val c = controller(b)
        c.start()
        c.ask(c.state.value.items[0])
        b.list = emptyList()
        c.confirmCancel()
        assertEquals(UiMessage.Text("It has already been sent."), c.state.value.error)
        assertNull(c.state.value.busyId)
        assertTrue(c.state.value.items.isEmpty())
        assertEquals(2, b.calls.count { it == "list" })
    }

    @Test fun aCancelThatCouldNotFinishShowsTheCoresWordsAndKeepsTheRowForAnotherTry() {
        val b = FakeScheduled().also { it.list = listOf(held("a", 1)); it.failCancel = CoreException.Temporary("Cancelled; it is in Sent Items for now.") }
        val c = controller(b)
        c.start()
        c.ask(c.state.value.items[0])
        c.confirmCancel()
        assertEquals(UiMessage.Text("Cancelled; it is in Sent Items for now."), c.state.value.error)
        // The server no longer lists it as waiting, yet the row stays.
        b.list = emptyList()
        c.refresh()
        assertEquals(listOf("a"), c.state.value.items.map { it.id })
        b.failCancel = null
        c.ask(c.state.value.items[0])
        c.confirmCancel()
        assertTrue(c.state.value.items.isEmpty())
        assertEquals("draft-a", runBlocking { c.openDraft.first() })
        c.refresh()
        assertTrue(c.state.value.items.isEmpty())
    }

    @Test fun anAnswerOvertakenByANewerReadIsDropped() {
        val pool = Executors.newFixedThreadPool(2)
        try {
            val dispatcher = pool.asCoroutineDispatcher()
            val b = FakeScheduled()
            val c = ScheduledController(CoroutineScope(SupervisorJob() + dispatcher), dispatcher, b)
            val release = CountDownLatch(1)
            val entered = CountDownLatch(1)
            var first = true
            b.gate = {
                if (first) {
                    first = false
                    entered.countDown()
                    release.await(5, TimeUnit.SECONDS)
                }
            }
            b.list = listOf(held("old", 1))
            c.start()
            assertTrue(entered.await(5, TimeUnit.SECONDS))
            // The first read has taken its answer ("old") and waits; a newer read sees the newer list.
            b.list = listOf(held("new", 2))
            c.refresh()
            val deadline = System.currentTimeMillis() + 5000
            while (c.state.value.items.map { it.id } != listOf("new") && System.currentTimeMillis() < deadline) Thread.sleep(10)
            release.countDown()
            Thread.sleep(200)
            assertEquals(listOf("new"), c.state.value.items.map { it.id })
        } finally {
            pool.shutdownNow()
        }
    }
}
