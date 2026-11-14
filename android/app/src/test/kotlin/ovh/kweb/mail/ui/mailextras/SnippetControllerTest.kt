package ovh.kweb.mail.ui.mailextras

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
import uniffi.kweb_core.MarkRange
import uniffi.kweb_core.Marked
import uniffi.kweb_core.Snippet

class SnippetControllerTest {
    private val pool = Executors.newCachedThreadPool()
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Unconfined)
    private val backend = FakeExtras()
    private lateinit var c: SnippetController

    @Before fun setUp() {
        c = SnippetController(scope, pool.asCoroutineDispatcher(), backend)
    }

    @After fun tearDown() {
        pool.shutdownNow()
    }

    private fun marked(text: String, start: Int, end: Int) = Marked(text, listOf(MarkRange(start.toUInt(), end.toUInt())))

    @Test fun theHitsOfTheRowsAreMarked() {
        backend.snippets = listOf(
            Snippet("a", marked("Invoice 7", 0, 7), null),
            Snippet("b", null, marked("pay the invoice soon", 8, 15)),
        )
        c.request("invoice", listOf("a", "b", "c"))
        waitFor("snippets") { c.state.value.byMessage.size == 2 }
        assertEquals("Invoice 7", c.state.value.byMessage["a"]!!.subject!!.text)
        assertNull(c.state.value.byMessage["a"]!!.preview)
        assertEquals(listOf(8..14), c.state.value.byMessage["b"]!!.preview!!.ranges)
        assertNull(c.state.value.byMessage["c"])
        assertEquals("snippets:invoice:a,b,c", backend.calls.single())
    }

    @Test fun aFailedCallLeavesTheRowsAsTheyAre() {
        backend.snippetsFail = java.io.IOException("offline")
        c.request("invoice", listOf("a"))
        waitFor("asked") { backend.calls.size == 1 }
        Thread.sleep(50)
        assertTrue(c.state.value.byMessage.isEmpty())
        assertEquals("invoice", c.state.value.query)
    }

    @Test fun aSnippetWithoutAnyMarkedTextIsLeftOut() {
        backend.snippets = listOf(Snippet("a", null, null), Snippet("b", Marked("", emptyList()), null))
        c.request("x", listOf("a", "b"))
        waitFor("asked") { backend.calls.size == 1 }
        Thread.sleep(50)
        assertTrue(c.state.value.byMessage.isEmpty())
    }

    @Test fun aSnippetForAMessageThatWasNotAskedAboutIsIgnored() {
        backend.snippets = listOf(Snippet("zzz", marked("hello", 0, 1), null))
        c.request("x", listOf("a"))
        waitFor("asked") { backend.calls.size == 1 }
        Thread.sleep(50)
        assertTrue(c.state.value.byMessage.isEmpty())
    }

    @Test fun theAnswerToAnOlderQueryIsDropped() {
        val inCore = CountDownLatch(1)
        val release = CountDownLatch(1)
        backend.snippets = listOf(Snippet("a", marked("old words", 0, 3), null))
        backend.snippetsGate = { q -> if (q == "old") { inCore.countDown(); release.await() } }
        c.request("old", listOf("a"))
        inCore.await()
        backend.snippets = listOf(Snippet("a", marked("new words", 0, 3), null))
        c.request("new", listOf("a"))
        waitFor("new") { c.state.value.byMessage["a"]?.subject?.text == "new words" }
        release.countDown()
        Thread.sleep(100)
        assertEquals("new words", c.state.value.byMessage["a"]!!.subject!!.text)
        assertEquals("new", c.state.value.query)
    }

    @Test fun clearingForgetsTheHitsAndAnAnswerStillOnItsWay() {
        val inCore = CountDownLatch(1)
        val release = CountDownLatch(1)
        backend.snippets = listOf(Snippet("a", marked("words", 0, 3), null))
        backend.snippetsGate = { inCore.countDown(); release.await() }
        c.request("words", listOf("a"))
        inCore.await()
        c.clear()
        release.countDown()
        Thread.sleep(100)
        assertTrue(c.state.value.byMessage.isEmpty())
        assertEquals("", c.state.value.query)
    }

    @Test fun noMessagesAsksNothing() {
        c.request("x", emptyList())
        Thread.sleep(30)
        assertTrue(backend.calls.isEmpty())
    }
}
