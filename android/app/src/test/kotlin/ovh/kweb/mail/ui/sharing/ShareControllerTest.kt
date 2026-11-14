package ovh.kweb.mail.ui.sharing

import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import ovh.kweb.mail.ui.UiMessage
import ovh.kweb.mail.ui.shared.SharedEvent
import uniffi.kweb_core.AddressBook
import uniffi.kweb_core.Calendar
import uniffi.kweb_core.Exception as CoreException
import uniffi.kweb_core.Person
import uniffi.kweb_core.Share
import uniffi.kweb_core.ShareKind
import uniffi.kweb_core.ShareLevel

private fun p(id: String, name: String = id) = Person(id, name, "$id@x.org")

private class FakeSharing : SharingBackend {
    val calls = CopyOnWriteArrayList<String>()
    var shares = mutableListOf<Share>()
    var people: List<Person> = emptyList()
    var peopleFails: Exception? = null
    var sharesFails: Exception? = null
    var setFails: Exception? = null
    var gate: ((String) -> Unit)? = null

    override fun people(text: String): List<Person> {
        gate?.invoke("people:$text")
        calls += "people:$text"
        peopleFails?.let { throw it }
        return people.filter { it.name.contains(text, ignoreCase = true) }
    }

    override fun shares(kind: ShareKind, id: String): List<Share> {
        calls += "shares:$kind:$id"
        sharesFails?.let { throw it }
        return shares.toList()
    }

    override fun setShare(kind: ShareKind, id: String, personId: String, level: ShareLevel?): List<Share> {
        calls += "set:$kind:$id:$personId:$level"
        setFails?.let { throw it }
        gate?.invoke("set")
        shares.removeAll { it.person.id == personId }
        if (level != null) shares += Share(people.firstOrNull { it.id == personId } ?: p(personId), level)
        return shares.toList()
    }

    override fun calendars() = emptyList<Calendar>()
    override fun addressBooks() = emptyList<AddressBook>()
}

class ShareControllerTest {
    private val pool = Executors.newCachedThreadPool()
    private val io = pool.asCoroutineDispatcher()
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Unconfined)
    private lateinit var backend: FakeSharing

    @Before fun setUp() {
        backend = FakeSharing()
    }

    @After fun tearDown() {
        scope.cancel()
        pool.shutdownNow()
    }

    private fun controller(debounce: Long = 10) = ShareController(scope, io, backend, ShareKind.CALENDAR, "cal1", debounce)

    private fun await(what: String, cond: () -> Boolean) {
        val end = System.nanoTime() + TimeUnit.SECONDS.toNanos(5)
        while (!cond()) {
            if (System.nanoTime() > end) throw AssertionError("timed out waiting for $what")
            Thread.sleep(5)
        }
    }

    private fun collect(c: ShareController): CopyOnWriteArrayList<SharedEvent> {
        val seen = CopyOnWriteArrayList<SharedEvent>()
        scope.launch { c.events.collect { seen += it } }
        return seen
    }

    @Test fun theSharesAreReadAndShownInOrder() {
        backend.shares = mutableListOf(Share(p("2", "bob"), ShareLevel.WRITE), Share(p("1", "alice"), ShareLevel.READ))
        val c = controller()
        c.show()
        await("shares") { c.state.value.shares != null }
        assertEquals(listOf("1", "2"), c.state.value.shares!!.map { it.person.id })
        assertTrue("shares:CALENDAR:cal1" in backend.calls)
    }

    @Test fun aReadThatFailsFirstSaysWhyAndRetryWorks() {
        backend.sharesFails = CoreException.Temporary("no connection")
        val c = controller()
        c.show()
        await("error") { c.state.value.error != null }
        assertEquals(UiMessage.Text("no connection"), c.state.value.error)
        backend.sharesFails = null
        c.refresh()
        await("shares") { c.state.value.shares != null }
        assertNull(c.state.value.error)
    }

    @Test fun aReadThatComesLateDoesNotReplaceANewerOne() {
        val started = CountDownLatch(1)
        val release = CountDownLatch(1)
        var n = 0
        val c = controller()
        backend.gate = null
        val slow = object : SharingBackend by backend {
            override fun shares(kind: ShareKind, id: String): List<Share> {
                val mine = synchronized(this) { ++n }
                if (mine == 1) {
                    started.countDown()
                    release.await(5, TimeUnit.SECONDS)
                    return listOf(Share(p("old"), ShareLevel.READ))
                }
                return listOf(Share(p("new"), ShareLevel.READ))
            }
        }
        val c2 = ShareController(scope, io, slow, ShareKind.MAILBOX, "m1", 10)
        c2.show()
        assertTrue(started.await(5, TimeUnit.SECONDS))
        c2.refresh()
        await("newer") { c2.state.value.shares?.firstOrNull()?.person?.id == "new" }
        release.countDown()
        Thread.sleep(150)
        assertEquals("new", c2.state.value.shares!!.first().person.id)
        assertNotNull(c)
    }

    // ---- the lookup

    @Test fun oneCharacterAsksNothingAndTwoAsksOnceAfterTheDebounce() {
        backend.people = listOf(p("1", "alice"), p("2", "albert"))
        val c = controller()
        c.show()
        await("shares") { c.state.value.shares != null }
        c.onQueryChange("a")
        Thread.sleep(80)
        assertTrue(backend.calls.none { it.startsWith("people:") })
        c.onQueryChange("al")
        c.onQueryChange("alb")
        await("answer") { c.state.value.answeredQuery == "alb" }
        assertEquals(listOf("2"), c.state.value.people.map { it.id })
        assertEquals(listOf("people:alb"), backend.calls.filter { it.startsWith("people:") })
    }

    @Test fun anAnswerOfAnOlderQueryNeverReplacesANewerOne() {
        val started = CountDownLatch(1)
        val release = CountDownLatch(1)
        backend.people = listOf(p("1", "alice"), p("2", "albert"))
        backend.gate = { what ->
            if (what == "people:ali") {
                started.countDown()
                release.await(5, TimeUnit.SECONDS)
            }
        }
        val c = controller(debounce = 0)
        c.onQueryChange("ali")
        assertTrue(started.await(5, TimeUnit.SECONDS))
        c.onQueryChange("alb")
        await("newer") { c.state.value.answeredQuery == "alb" }
        release.countDown()
        Thread.sleep(150)
        assertEquals("alb", c.state.value.answeredQuery)
        assertEquals(listOf("2"), c.state.value.people.map { it.id })
    }

    @Test fun aServerThatRefusesLookupsHasItsWordsShownAndSharesCanStillBeChanged() {
        backend.shares = mutableListOf(Share(p("1", "alice"), ShareLevel.READ))
        backend.peopleFails = CoreException.Invalid("The server does not let apps look users up.")
        val c = controller()
        c.show()
        await("shares") { c.state.value.shares != null }
        c.onQueryChange("al")
        await("refusal") { c.state.value.lookupRefused != null }
        assertEquals(UiMessage.Text("The server does not let apps look users up."), c.state.value.lookupRefused)
        // No further lookups are asked for.
        c.onQueryChange("alic")
        Thread.sleep(80)
        assertEquals(1, backend.calls.count { it.startsWith("people:") })
        // The existing share can be changed.
        c.changeLevel(p("1", "alice"), ShareLevel.WRITE)
        await("changed") { c.state.value.shares?.first()?.level == ShareLevel.WRITE }
    }

    @Test fun aLookupThatFailsOtherwiseSaysSoAndTypingAgainAsksAgain() {
        backend.peopleFails = CoreException.Temporary("offline")
        val c = controller()
        c.onQueryChange("al")
        await("error") { c.state.value.searchError != null }
        assertNull(c.state.value.lookupRefused)
        backend.peopleFails = null
        backend.people = listOf(p("1", "alice"))
        c.onQueryChange("ali")
        await("answer") { c.state.value.people.isNotEmpty() }
        assertNull(c.state.value.searchError)
    }

    // ---- changing shares

    @Test fun addingSharesAtTheChosenLevelAndEmptiesTheSearch() {
        backend.people = listOf(p("1", "alice"))
        val c = controller()
        c.show()
        c.setNewLevel(ShareLevel.WRITE)
        c.onQueryChange("al")
        await("answer") { c.state.value.people.isNotEmpty() }
        c.add(c.state.value.people.first())
        await("added") { c.state.value.shares?.any { it.person.id == "1" } == true }
        assertEquals(ShareLevel.WRITE, c.state.value.shares!!.first().level)
        assertEquals("", c.state.value.query)
        assertTrue(c.state.value.people.isEmpty())
        assertTrue("set:CALENDAR:cal1:1:WRITE" in backend.calls)
    }

    @Test fun aLevelIsChangedForThatPersonOnlyAndNotAskedForWhenItIsTheSame() {
        backend.shares = mutableListOf(Share(p("1"), ShareLevel.READ), Share(p("2"), ShareLevel.READ))
        val c = controller()
        c.show()
        await("shares") { c.state.value.shares != null }
        c.changeLevel(p("1"), ShareLevel.READ)
        c.changeLevel(p("2"), ShareLevel.WRITE)
        await("changed") { c.state.value.shares?.first { it.person.id == "2" }?.level == ShareLevel.WRITE }
        assertEquals(1, backend.calls.count { it.startsWith("set:") })
        assertEquals(ShareLevel.READ, c.state.value.shares!!.first { it.person.id == "1" }.level)
    }

    @Test fun takingAShareBackAsksFirstAndNothingHappensOnCancel() {
        backend.shares = mutableListOf(Share(p("1"), ShareLevel.READ))
        val c = controller()
        c.show()
        await("shares") { c.state.value.shares != null }
        c.askRemove(p("1"))
        assertEquals("1", c.state.value.confirmRemove?.id)
        c.cancelRemove()
        assertNull(c.state.value.confirmRemove)
        assertTrue(backend.calls.none { it.startsWith("set:") })
        c.askRemove(p("1"))
        c.confirmRemove()
        await("removed") { c.state.value.shares?.isEmpty() == true }
        assertTrue("set:CALENDAR:cal1:1:null" in backend.calls)
        assertNull(c.state.value.confirmRemove)
    }

    @Test fun aRefusedChangeSaysTheCoresWordsAndKeepsTheList() {
        backend.shares = mutableListOf(Share(p("1"), ShareLevel.READ))
        val c = controller()
        val events = collect(c)
        c.show()
        await("shares") { c.state.value.shares != null }
        backend.setFails = CoreException.Invalid("You cannot share this with yourself.")
        c.changeLevel(p("1"), ShareLevel.WRITE)
        await("say") { events.isNotEmpty() }
        assertEquals(SharedEvent.Say(UiMessage.Text("You cannot share this with yourself.")), events.first())
        assertEquals(ShareLevel.READ, c.state.value.shares!!.first().level)
        assertNull(c.state.value.busyPersonId)
    }

    @Test fun textTypedWhileAnAddIsOnItsWayIsKept() {
        val release = CountDownLatch(1)
        backend.people = listOf(p("1", "alice"))
        val c = controller()
        c.show()
        c.onQueryChange("al")
        await("answer") { c.state.value.people.isNotEmpty() }
        backend.gate = { if (it == "set") release.await(5, TimeUnit.SECONDS) }
        c.add(c.state.value.people.first())
        await("busy") { c.state.value.busyPersonId != null }
        c.onQueryChange("bo")
        release.countDown()
        await("added") { c.state.value.busyPersonId == null && c.state.value.shares?.any { it.person.id == "1" } == true }
        assertEquals("bo", c.state.value.query)
    }

    @Test fun onlyOneChangeRunsAtATime() {
        val release = CountDownLatch(1)
        backend.shares = mutableListOf(Share(p("1"), ShareLevel.READ), Share(p("2"), ShareLevel.READ))
        backend.gate = { if (it == "set") release.await(5, TimeUnit.SECONDS) }
        val c = controller()
        c.show()
        await("shares") { c.state.value.shares != null }
        c.changeLevel(p("1"), ShareLevel.WRITE)
        c.changeLevel(p("2"), ShareLevel.WRITE)
        release.countDown()
        await("done") { c.state.value.busyPersonId == null && c.state.value.shares?.first()?.level == ShareLevel.WRITE }
        assertEquals(1, backend.calls.count { it.startsWith("set:") })
    }

    @Test fun resetForgetsEverythingAndIgnoresWhatIsOnItsWay() {
        val release = CountDownLatch(1)
        backend.shares = mutableListOf(Share(p("1"), ShareLevel.READ))
        backend.gate = { if (it == "set") release.await(5, TimeUnit.SECONDS) }
        val c = controller()
        c.show()
        await("shares") { c.state.value.shares != null }
        c.changeLevel(p("1"), ShareLevel.WRITE)
        c.reset()
        release.countDown()
        Thread.sleep(150)
        assertNull(c.state.value.shares)
        assertNull(c.state.value.busyPersonId)
    }
}
