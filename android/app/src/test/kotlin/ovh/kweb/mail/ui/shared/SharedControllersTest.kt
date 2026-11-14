package ovh.kweb.mail.ui.shared

import java.time.LocalDate
import java.time.ZoneId
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
import uniffi.kweb_core.Agenda
import uniffi.kweb_core.Calendar
import uniffi.kweb_core.ContactDetail
import uniffi.kweb_core.ContactRow
import uniffi.kweb_core.EventDetail
import uniffi.kweb_core.EventInput
import uniffi.kweb_core.Exception as CoreException
import uniffi.kweb_core.MessageBody
import uniffi.kweb_core.Occurrence
import uniffi.kweb_core.SharedAccount
import uniffi.kweb_core.SharedAction
import uniffi.kweb_core.SharedMailbox
import uniffi.kweb_core.SharedMessage
import uniffi.kweb_core.SharedMessages

private fun body(id: String) = MessageBody(id, "blob-$id", emptyList(), emptyList(), emptyList(), emptyList(), null, null, "text", emptyList(), false)

private fun occurrence(id: String, startsAt: Long, calendar: String = "c1") =
    Occurrence(id, null, calendar, "Event $id", "", startsAt, startsAt + 3600, false, false, false, false, null)

private fun eventDetail(id: String, calendar: String = "c1") = EventDetail(
    id,
    EventInput(calendar, "Event $id", "", "", "2026-11-20T10:00:00", "UTC", 3600uL, false, false, null, emptyList()),
    false, emptyList(), null, false, false,
)

private class FakeBackend : SharedBackend {
    val calls = CopyOnWriteArrayList<String>()
    var accounts: List<SharedAccount> = emptyList()
    var mailboxes: List<SharedMailbox> = emptyList()
    var failMailboxes: Exception? = null
    var folder: List<uniffi.kweb_core.MessageHeader> = emptyList()
    var gate: ((String) -> Unit)? = null
    var actFails: Exception? = null
    var failMessage: Exception? = null
    var entries: List<Occurrence> = emptyList()
    var failAgenda: Exception? = null
    var calendars: List<Calendar> = listOf(Calendar("c1", "Team", null, false, true, false))
    var contacts: List<ContactRow> = emptyList()

    override fun accounts() = accounts.also { calls += "accounts" }

    override fun mailboxes(account: String): List<SharedMailbox> {
        calls += "mailboxes:$account"
        failMailboxes?.let { throw it }
        return mailboxes
    }

    override fun messages(account: String, mailboxId: String, offset: Int, limit: Int): SharedMessages {
        gate?.invoke("messages:$offset")
        if (limit > 100) throw CoreException.Invalid("limit too large")
        calls += "messages:$account:$mailboxId:$offset:$limit"
        return SharedMessages(folder.drop(offset).take(limit), folder.size.toUInt())
    }

    override fun message(account: String, messageId: String): SharedMessage {
        calls += "message:$account:$messageId"
        failMessage?.let { throw it }
        return SharedMessage(header(messageId, unread = true), body(messageId))
    }

    override fun act(account: String, messageId: String, action: SharedAction) {
        calls += "act:$account:$messageId:$action"
        actFails?.let { throw it }
    }

    override fun calendars(account: String): List<Calendar> = calendars.also { calls += "calendars:$account" }

    override fun agenda(account: String, from: Long, to: Long, timeZone: String): Agenda {
        calls += "agenda:$account:${(to - from) / 86400}"
        failAgenda?.let { throw it }
        return Agenda(entries, false)
    }

    override fun event(account: String, eventId: String, recurrenceId: String?): EventDetail {
        calls += "event:$account:$eventId"
        return eventDetail(eventId)
    }

    override fun contacts(account: String, query: String): List<ContactRow> {
        gate?.invoke("contacts:$query")
        calls += "contacts:$account:$query"
        return contacts.filter { it.name.contains(query, ignoreCase = true) }
    }

    override fun contact(account: String, contactId: String): ContactDetail = throw CoreException.Invalid("contact is gone")
}

class SharedControllersTest {
    private val pool = Executors.newCachedThreadPool()
    private val io = pool.asCoroutineDispatcher()
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Unconfined)
    private lateinit var backend: FakeBackend

    @Before fun setUp() {
        backend = FakeBackend()
    }

    @After fun tearDown() {
        scope.cancel()
        pool.shutdownNow()
    }

    private fun await(what: String, cond: () -> Boolean) {
        val end = System.nanoTime() + TimeUnit.SECONDS.toNanos(5)
        while (!cond()) {
            if (System.nanoTime() > end) throw AssertionError("timed out waiting for $what")
            Thread.sleep(5)
        }
    }

    /** Collects the events of a controller as they come. */
    private fun collect(events: kotlinx.coroutines.flow.Flow<SharedEvent>): CopyOnWriteArrayList<SharedEvent> {
        val seen = CopyOnWriteArrayList<SharedEvent>()
        scope.launch { events.collect { seen += it } }
        return seen
    }

    // ---- LoadController

    @Test fun theFirstShowReadsAndAnEmptyAnswerIsAnAnswer() {
        val c = LoadController(scope, io, goneOnInvalid = false) { backend.accounts() }
        assertNull(c.state.value.value)
        c.show()
        await("accounts") { c.state.value.value != null }
        assertEquals(emptyList<SharedAccount>(), c.state.value.value)
        assertTrue(!c.state.value.loading)
    }

    @Test fun aReadThatFailsFirstShowsTheCoresWordsAndRetryWorks() {
        var fail = true
        val c = LoadController(scope, io, goneOnInvalid = false) {
            if (fail) throw CoreException.Temporary("no connection") else listOf("ok")
        }
        c.show()
        await("error") { c.state.value.error != null }
        assertEquals(UiMessage.Text("no connection"), c.state.value.error)
        fail = false
        c.refresh()
        await("value") { c.state.value.value != null }
        assertNull(c.state.value.error)
    }

    @Test fun aQuietReadThatFailsKeepsWhatIsShownAndSaysNothing() {
        var fail = false
        val c = LoadController(scope, io, goneOnInvalid = false) { if (fail) throw CoreException.Temporary("offline") else 1 }
        val events = collect(c.events)
        c.show()
        await("value") { c.state.value.value == 1 }
        fail = true
        c.show()
        Thread.sleep(150)
        assertEquals(1, c.state.value.value)
        assertNull(c.state.value.error)
        assertTrue(events.isEmpty())
        // A pull to refresh that fails says so.
        c.refresh()
        await("say") { events.isNotEmpty() }
        assertEquals(SharedEvent.Say(UiMessage.Text("offline")), events.first())
    }

    @Test fun aRefusalOfAnAccountLoadSaysTheShareIsGone() {
        val c = LoadController<List<SharedMailbox>>(scope, io, goneOnInvalid = true) { throw CoreException.Invalid("no such account") }
        val events = collect(c.events)
        c.show()
        await("gone") { events.contains(SharedEvent.Gone) }
        assertEquals(UiMessage.Text("no such account"), c.state.value.error)
    }

    @Test fun aRefusalOfASingleItemIsNotAGoneShare() {
        val c = LoadController<Int>(scope, io, goneOnInvalid = false) { throw CoreException.Invalid("event is gone") }
        val events = collect(c.events)
        c.show()
        await("error") { c.state.value.error != null }
        Thread.sleep(50)
        assertTrue(events.isEmpty())
    }

    @Test fun anAnswerThatComesLateDoesNotReplaceANewerOne() {
        val started = CountDownLatch(1)
        val release = CountDownLatch(1)
        var n = 0
        val c = LoadController(scope, io, goneOnInvalid = false) {
            val mine = synchronized(this) { ++n }
            if (mine == 1) {
                started.countDown()
                release.await(5, TimeUnit.SECONDS)
            }
            "answer-$mine"
        }
        c.show()
        assertTrue(started.await(5, TimeUnit.SECONDS))
        c.refresh()
        await("newer") { c.state.value.value == "answer-2" }
        release.countDown()
        Thread.sleep(150)
        assertEquals("answer-2", c.state.value.value)
    }

    @Test fun resetForgetsEverythingAndIgnoresWhatIsOnItsWay() {
        val release = CountDownLatch(1)
        val c = LoadController(scope, io, goneOnInvalid = false) { release.await(5, TimeUnit.SECONDS); 1 }
        c.show()
        c.reset()
        release.countDown()
        Thread.sleep(150)
        assertNull(c.state.value.value)
        assertTrue(!c.state.value.loading)
    }

    // ---- FolderController

    private fun folderOf(n: Int) = (1..n).map { header("m$it", unread = it % 2 == 0) }

    private fun folderController() = FolderController(scope, io, backend, "acc", "inbox")

    @Test fun theFirstPageIsFiftyAndTheRightsComeWithIt() {
        backend.mailboxes = listOf(box("inbox", "Inbox", mark = true, remove = true))
        backend.folder = folderOf(120)
        val c = folderController()
        c.show()
        await("page") { c.state.value.loaded }
        val s = c.state.value
        assertEquals(50, s.messages.size)
        assertEquals(120, s.total)
        assertTrue(s.hasMore)
        assertTrue(s.folder!!.mayMark)
        assertTrue("messages:acc:inbox:0:50" in backend.calls)
    }

    @Test fun loadMoreAppendsTheNextPagesUntilTheEnd() {
        backend.mailboxes = listOf(box("inbox"))
        backend.folder = folderOf(120)
        val c = folderController()
        c.show()
        await("page") { c.state.value.loaded }
        c.loadMore()
        await("second page") { c.state.value.messages.size == 100 }
        c.loadMore()
        await("third page") { c.state.value.messages.size == 120 }
        assertTrue(!c.state.value.hasMore)
        assertEquals((1..120).map { "m$it" }, c.state.value.messages.map { it.id })
        assertTrue("messages:acc:inbox:50:50" in backend.calls)
        assertTrue("messages:acc:inbox:100:50" in backend.calls)
        // Nothing more to ask for.
        val before = backend.calls.size
        c.loadMore()
        Thread.sleep(50)
        assertEquals(before, backend.calls.size)
    }

    @Test fun readingAgainKeepsThePagesThatWereShownAlsoAboveWhatOneCallGives() {
        backend.mailboxes = listOf(box("inbox"))
        backend.folder = folderOf(230)
        val c = folderController()
        c.show()
        await("page") { c.state.value.loaded }
        repeat(3) { n ->
            c.loadMore()
            await("page ${n + 2}") { c.state.value.messages.size == 100 + 50 * n || c.state.value.messages.size == 50 * (n + 2) }
        }
        await("150") { c.state.value.messages.size == 200 }
        backend.calls.clear()
        c.show()
        await("again") { backend.calls.contains("messages:acc:inbox:100:100") }
        await("settled") { c.state.value.messages.size == 200 && !c.state.value.loading }
        assertTrue("messages:acc:inbox:0:100" in backend.calls)
        assertEquals((1..200).map { "m$it" }, c.state.value.messages.map { it.id })
        assertEquals(230, c.state.value.total)
    }

    @Test fun readingAgainDropsAMessageThatWentAndKeepsTheRest() {
        backend.mailboxes = listOf(box("inbox"))
        backend.folder = folderOf(150)
        val c = folderController()
        c.show()
        await("page") { c.state.value.loaded }
        c.loadMore()
        await("100") { c.state.value.messages.size == 100 }
        c.loadMore()
        await("150") { c.state.value.messages.size == 150 }
        backend.folder = backend.folder.filter { it.id != "m130" }
        c.show()
        await("shrunk") { c.state.value.total == 149 && !c.state.value.loading }
        assertEquals(149, c.state.value.messages.size)
        assertTrue(c.state.value.messages.none { it.id == "m130" })
    }

    @Test fun aLoadMoreAnsweredAfterTheListWasReadAgainIsDropped() {
        backend.mailboxes = listOf(box("inbox"))
        backend.folder = folderOf(120)
        val c = folderController()
        c.show()
        await("page") { c.state.value.loaded }
        val started = CountDownLatch(1)
        val release = CountDownLatch(1)
        backend.gate = { what -> if (what == "messages:50") { started.countDown(); release.await(5, TimeUnit.SECONDS) } }
        c.loadMore()
        assertTrue(started.await(5, TimeUnit.SECONDS))
        // The folder shrank and the user pulled to refresh meanwhile.
        backend.gate = null
        backend.folder = folderOf(10)
        c.refresh()
        await("shrunk") { c.state.value.total == 10 }
        release.countDown()
        Thread.sleep(150)
        assertEquals(10, c.state.value.messages.size)
        assertTrue(!c.state.value.loadingMore)
    }

    @Test fun aFolderThatIsNotSharedAnyMoreIsAGoneShare() {
        backend.mailboxes = listOf(box("other"))
        val c = folderController()
        val events = collect(c.events)
        c.show()
        await("gone") { events.contains(SharedEvent.Gone) }
        assertTrue(!c.state.value.loaded)
    }

    @Test fun aRefusedFolderReadIsAGoneShare() {
        backend.failMailboxes = CoreException.Invalid("no such account")
        val c = folderController()
        val events = collect(c.events)
        c.show()
        await("gone") { events.contains(SharedEvent.Gone) }
    }

    // ---- SharedMessageController

    private fun messageController(mailbox: String = "inbox") = SharedMessageController(scope, io, backend, "acc", mailbox, "m1")

    private fun shown(c: SharedMessageController) {
        c.show()
        await("message") { c.state.value.message != null }
    }

    @Test fun openingAMessageDoesNotMarkItRead() {
        backend.mailboxes = listOf(box("inbox", mark = true))
        shown(messageController())
        assertTrue(backend.calls.none { it.startsWith("act:") })
    }

    @Test fun aMarkIsSentAndTheMessageIsReadAgain() {
        backend.mailboxes = listOf(box("inbox", mark = true))
        val c = messageController()
        shown(c)
        c.act(SharedAction.MarkRead)
        await("act") { backend.calls.any { it.startsWith("act:acc:m1:") } }
        await("reread") { backend.calls.count { it == "message:acc:m1" } == 2 }
        assertTrue(!c.state.value.busy)
    }

    @Test fun anActionTheFolderDoesNotAllowIsNotSent() {
        backend.mailboxes = listOf(box("inbox"))
        val c = messageController()
        val events = collect(c.events)
        shown(c)
        c.act(SharedAction.Delete)
        c.act(SharedAction.MarkRead)
        c.act(SharedAction.Move("x"))
        Thread.sleep(100)
        assertTrue(backend.calls.none { it.startsWith("act:") })
        assertEquals(3, events.filterIsInstance<SharedEvent.Say>().size)
    }

    @Test fun aMoveEndsTheScreen() {
        backend.mailboxes = listOf(box("inbox", remove = true), box("archive", add = true))
        val c = messageController()
        val events = collect(c.events)
        shown(c)
        assertEquals(listOf("archive"), c.state.value.moveTargets.map { it.mailbox.id })
        c.act(SharedAction.Move("archive"))
        await("done") { events.contains(SharedEvent.Done) }
        assertTrue(backend.calls.any { it.startsWith("act:acc:m1:Move") })
    }

    @Test fun aDeleteEndsTheScreen() {
        backend.mailboxes = listOf(box("inbox", remove = true))
        val c = messageController()
        val events = collect(c.events)
        shown(c)
        c.act(SharedAction.Delete)
        await("done") { events.contains(SharedEvent.Done) }
    }

    @Test fun aRefusedActionSaysWhyAndLooksAgain() {
        backend.mailboxes = listOf(box("inbox", mark = true))
        backend.actFails = CoreException.Invalid("You are not allowed to modify keywords.")
        val c = messageController()
        val events = collect(c.events)
        shown(c)
        c.act(SharedAction.Flag)
        await("say") { events.contains(SharedEvent.Say(UiMessage.Text("You are not allowed to modify keywords."))) }
        await("reread") { backend.calls.count { it == "message:acc:m1" } == 2 }
        assertTrue(!c.state.value.busy)
        assertTrue(events.none { it == SharedEvent.Done })
    }

    @Test fun aMessageStaysReadableWhenTheFoldersCannotBeRead() {
        backend.failMailboxes = CoreException.Temporary("offline")
        val c = messageController()
        shown(c)
        assertNotNull(c.state.value.message)
        assertTrue(!c.state.value.allows(SharedActionKind.MARK))
        assertTrue(!c.state.value.allows(SharedActionKind.DELETE))
    }

    @Test fun aFailedFolderReadKeepsTheRightsAlreadyKnown() {
        backend.mailboxes = listOf(box("inbox", mark = true, remove = true))
        val c = messageController()
        shown(c)
        assertTrue(c.state.value.allows(SharedActionKind.DELETE))
        backend.failMailboxes = CoreException.Temporary("offline")
        c.refresh()
        await("reread") { backend.calls.count { it == "message:acc:m1" } == 2 }
        await("settled") { !c.state.value.loading }
        assertTrue(c.state.value.allows(SharedActionKind.DELETE))
        assertTrue(c.state.value.allows(SharedActionKind.MARK))
        assertTrue(!c.state.value.foldersFailed)
    }

    @Test fun aFolderReadThatFailsFirstIsSaidAndCanBeTriedAgain() {
        backend.mailboxes = listOf(box("inbox", mark = true))
        backend.failMailboxes = CoreException.Temporary("offline")
        val c = messageController()
        shown(c)
        assertTrue(c.state.value.foldersFailed)
        backend.failMailboxes = null
        c.refresh()
        await("rights") { c.state.value.allows(SharedActionKind.MARK) }
        assertTrue(!c.state.value.foldersFailed)
    }

    @Test fun aMarkThatWorkedIsShownEvenWhenTheReReadFails() {
        backend.mailboxes = listOf(box("inbox", mark = true))
        val c = messageController()
        shown(c)
        assertTrue(c.state.value.message!!.header.isUnread)
        backend.failMessage = CoreException.Temporary("offline")
        c.act(SharedAction.MarkRead)
        await("reread") { backend.calls.count { it == "message:acc:m1" } == 2 }
        await("idle") { !c.state.value.busy && !c.state.value.loading }
        assertTrue(!c.state.value.message!!.header.isUnread)
        c.act(SharedAction.Flag)
        await("flag") { backend.calls.count { it.startsWith("act:") } == 2 }
        await("reread2") { backend.calls.count { it == "message:acc:m1" } == 3 }
        await("flagged") { c.state.value.message!!.header.isFlagged }
    }

    @Test fun remoteContentIsAllowedOnlyForThisScreen() {
        backend.mailboxes = listOf(box("inbox"))
        val c = messageController()
        shown(c)
        assertTrue(!c.state.value.remoteAllowed)
        c.allowRemote()
        assertTrue(c.state.value.remoteAllowed)
        c.reset()
        assertTrue(!c.state.value.remoteAllowed)
    }

    // ---- SharedAgendaController

    private val zone = ZoneId.of("UTC")
    private val day = LocalDate.of(2026, 11, 14)

    private fun agendaController() = SharedAgendaController(scope, io, backend, "acc", { zone }, { day })

    private fun at(date: String) = LocalDate.parse(date).atStartOfDay(zone).toEpochSecond() + 9 * 3600

    @Test fun theAgendaAsksForThirtyDaysAndGroupsByDay() {
        backend.entries = listOf(occurrence("a", at("2026-11-20")), occurrence("b", at("2026-11-20")), occurrence("c", at("2026-11-25")))
        val c = agendaController()
        c.show()
        await("loaded") { c.state.value.loaded }
        assertTrue("agenda:acc:30" in backend.calls)
        val rows = c.state.value.rows
        assertEquals(2, rows.count { it is ovh.kweb.mail.ui.calendar.AgendaRules.Row.Header })
        assertEquals(3, rows.count { it is ovh.kweb.mail.ui.calendar.AgendaRules.Row.Entry })
        assertEquals(listOf("c1"), c.state.value.calendarIds)
    }

    @Test fun loadingMoreAsksForSixtyDaysAsAWhole() {
        backend.entries = listOf(occurrence("a", at("2026-11-20")), occurrence("far", at("2026-12-20")))
        val c = agendaController()
        c.show()
        await("loaded") { c.state.value.loaded }
        // The entry 36 days out is outside the first thirty days.
        assertEquals(1, c.state.value.rows.count { it is ovh.kweb.mail.ui.calendar.AgendaRules.Row.Entry })
        c.loadMore()
        await("more") { c.state.value.window.days == 60L && !c.state.value.loadingMore }
        assertTrue("agenda:acc:60" in backend.calls)
        assertEquals(2, c.state.value.rows.count { it is ovh.kweb.mail.ui.calendar.AgendaRules.Row.Entry })
    }

    @Test fun aFailedLoadMoreKeepsTheSpanAndSaysSo() {
        backend.entries = listOf(occurrence("a", at("2026-11-20")))
        val c = agendaController()
        val events = collect(c.events)
        c.show()
        await("loaded") { c.state.value.loaded }
        backend.failAgenda = CoreException.Temporary("no connection")
        c.loadMore()
        await("say") { events.contains(SharedEvent.Say(UiMessage.Text("no connection"))) }
        assertEquals(30L, c.state.value.window.days)
        assertEquals(1, c.state.value.rows.count { it is ovh.kweb.mail.ui.calendar.AgendaRules.Row.Entry })
        assertTrue(!c.state.value.loadingMore)
    }

    @Test fun withoutAConnectionThereIsNoOldAgenda() {
        backend.failAgenda = CoreException.Temporary("no connection")
        val c = agendaController()
        c.show()
        await("error") { c.state.value.error != null }
        assertTrue(!c.state.value.loaded)
    }

    @Test fun theCalendarNamesAreOnlyForTheTones() {
        backend.entries = listOf(occurrence("a", at("2026-11-20")))
        backend.calendars = emptyList()
        val c = agendaController()
        c.show()
        await("loaded") { c.state.value.loaded }
        assertEquals(emptyList<String>(), c.state.value.calendarIds)
    }

    // ---- an event

    @Test fun anEventComesWithTheNameOfItsCalendar() {
        val data = readSharedEvent(backend, "acc", "e1", null)
        assertEquals("Team", data.calendarName)
        assertEquals("e1", data.detail.eventId)
        assertTrue("event:acc:e1" in backend.calls)
    }

    @Test fun anEventIsShownWhenItsCalendarCannotBeNamed() {
        backend.calendars = emptyList()
        assertNull(readSharedEvent(backend, "acc", "e1", null).calendarName)
    }

    // ---- SharedContactsController

    private fun contactsController() = SharedContactsController(scope, io, backend, "acc", debounceMillis = 0)

    @Test fun theContactsAreReadAndNarrowedByTheQuery() {
        backend.contacts = listOf(ContactRow("1", "Anna", "a@x", ""), ContactRow("2", "Bert", "b@x", ""))
        val c = contactsController()
        c.show()
        await("rows") { c.state.value.rows?.size == 2 }
        c.onQueryChange("ber")
        await("narrowed") { c.state.value.answeredQuery == "ber" }
        assertEquals(listOf("2"), c.state.value.rows!!.map { it.id })
        assertTrue("contacts:acc:ber" in backend.calls)
    }

    @Test fun theAnswerToAnOlderQueryDoesNotReplaceTheNewestOne() {
        backend.contacts = listOf(ContactRow("1", "Anna", "a@x", ""), ContactRow("2", "Bert", "b@x", ""))
        val started = CountDownLatch(1)
        val release = CountDownLatch(1)
        backend.gate = { what -> if (what == "contacts:an") { started.countDown(); release.await(5, TimeUnit.SECONDS) } }
        val c = contactsController()
        c.onQueryChange("an")
        assertTrue(started.await(5, TimeUnit.SECONDS))
        backend.gate = null
        c.onQueryChange("be")
        await("newest") { c.state.value.answeredQuery == "be" }
        release.countDown()
        Thread.sleep(150)
        assertEquals("be", c.state.value.answeredQuery)
        assertEquals(listOf("2"), c.state.value.rows!!.map { it.id })
    }

    @Test fun aBlankQueryAsksForAll() {
        backend.contacts = listOf(ContactRow("1", "Anna", "a@x", ""))
        val c = contactsController()
        c.onQueryChange("   ")
        await("answer") { c.state.value.rows != null }
        assertTrue("contacts:acc:" in backend.calls)
    }
}
