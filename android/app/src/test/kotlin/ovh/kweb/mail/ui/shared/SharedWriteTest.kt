package ovh.kweb.mail.ui.shared

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
import org.junit.Assert.assertTrue
import org.junit.Test
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.UiMessage
import uniffi.kweb_core.AddressBook
import uniffi.kweb_core.DraftKind
import uniffi.kweb_core.Exception as CoreException

private class FakeWrite : SharedWriteBackend {
    val calls = CopyOnWriteArrayList<String>()
    var draftFails: Exception? = null
    var gate: CountDownLatch? = null

    override fun newDraftFromShared(account: String, kind: DraftKind): String {
        gate?.await(5, TimeUnit.SECONDS)
        calls += "draft:$account:$kind"
        draftFails?.let { throw it }
        return "d${calls.size}"
    }

    override fun discardDraft(draftId: String) {
        calls += "discard:$draftId"
    }

    override fun deleteEvent(account: String, eventId: String, recurrenceId: String?) {
        calls += "deleteEvent:$account:$eventId:$recurrenceId"
    }

    override fun addressBooks(account: String) = emptyList<AddressBook>()

    override fun deleteContact(account: String, contactId: String) {
        calls += "deleteContact:$account:$contactId"
    }
}

class SharedWriteTest {
    private val pool = Executors.newCachedThreadPool()
    private val io = pool.asCoroutineDispatcher()
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Unconfined)
    private val backend = FakeWrite()

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

    // ---- SharedDraftStarter

    @Test fun aReplyStartsADraftInTheSharedAccountAndHandsItsIdOver() {
        val starter = SharedDraftStarter(scope, io, backend, "acc")
        val ids = CopyOnWriteArrayList<String>()
        scope.launch { starter.draftStarted.collect { ids += it } }
        starter.start(DraftKind.Reply("m1"))
        await("draft") { ids.isNotEmpty() }
        assertEquals(listOf("d1"), ids.toList())
        assertEquals("draft:acc:${DraftKind.Reply("m1")}", backend.calls.first())
    }

    @Test fun aDoubleTapMakesOneDraft() {
        val gate = CountDownLatch(1)
        backend.gate = gate
        val starter = SharedDraftStarter(scope, io, backend, "acc")
        val ids = CopyOnWriteArrayList<String>()
        scope.launch { starter.draftStarted.collect { ids += it } }
        starter.start(DraftKind.Forward("m1"))
        starter.start(DraftKind.Forward("m1"))
        gate.countDown()
        await("draft") { ids.isNotEmpty() }
        Thread.sleep(100)
        assertEquals(1, backend.calls.count { it.startsWith("draft:") })
    }

    @Test fun aDraftThatCannotBeStartedSaysWhyAndANewTryWorks() {
        backend.draftFails = CoreException.Invalid("That message is gone.")
        val starter = SharedDraftStarter(scope, io, backend, "acc")
        val problems = CopyOnWriteArrayList<UiMessage>()
        val ids = CopyOnWriteArrayList<String>()
        scope.launch { starter.problems.collect { problems += it } }
        scope.launch { starter.draftStarted.collect { ids += it } }
        starter.start(DraftKind.ReplyAll("m1"))
        await("problem") { problems.isNotEmpty() }
        assertEquals(UiMessage.Text("That message is gone."), problems.first())
        backend.draftFails = null
        await("free again") {
            starter.start(DraftKind.ReplyAll("m1"))
            ids.isNotEmpty()
        }
    }

    @Test fun anUnexpectedFailureSaysTheStartFailedLine() {
        backend.draftFails = IllegalStateException("boom")
        val starter = SharedDraftStarter(scope, io, backend, "acc")
        val problems = CopyOnWriteArrayList<UiMessage>()
        scope.launch { starter.problems.collect { problems += it } }
        starter.start(DraftKind.Reply("m1"))
        await("problem") { problems.isNotEmpty() }
        assertEquals(UiMessage.Res(R.string.compose_start_failed), problems.first())
    }

    @Test fun aDraftNobodyOpensBecauseTheScreenWasLeftIsThrownAway() {
        val gate = CountDownLatch(1)
        backend.gate = gate
        val own = CoroutineScope(SupervisorJob() + Dispatchers.Unconfined)
        val starter = SharedDraftStarter(own, io, backend, "acc")
        starter.start(DraftKind.Reply("m1"))
        own.cancel()
        gate.countDown()
        await("discard") { backend.calls.any { it.startsWith("discard:") } }
        assertTrue(backend.calls.any { it == "discard:d1" })
    }

    // ---- SharedDeleter

    @Test fun aDeleteRunsTheActionOnceAndSaysDone() {
        val seen = CopyOnWriteArrayList<String>()
        val release = CountDownLatch(1)
        val deleter = SharedDeleter<String>(scope, io, R.string.shared_failed) {
            release.await(5, TimeUnit.SECONDS)
            seen += it
        }
        val events = CopyOnWriteArrayList<SharedEvent>()
        scope.launch { deleter.events.collect { events += it } }
        deleter.delete("all")
        deleter.delete("again")
        assertTrue(deleter.busy.value)
        release.countDown()
        await("done") { events.isNotEmpty() }
        assertEquals(listOf("all"), seen.toList())
        assertEquals(SharedEvent.Done, events.first())
        assertTrue(!deleter.busy.value)
    }

    @Test fun aRefusedDeleteSaysTheCoresWordsAndCanBeTriedAgain() {
        var fail: Exception? = CoreException.Invalid("You may not delete here.")
        val deleter = SharedDeleter<Unit>(scope, io, R.string.shared_failed) { fail?.let { throw it } }
        val events = CopyOnWriteArrayList<SharedEvent>()
        scope.launch { deleter.events.collect { events += it } }
        deleter.delete(Unit)
        await("say") { events.isNotEmpty() }
        assertEquals(SharedEvent.Say(UiMessage.Text("You may not delete here.")), events.first())
        assertTrue(!deleter.busy.value)
        fail = null
        deleter.delete(Unit)
        await("done") { events.contains(SharedEvent.Done) }
    }

    @Test fun aDeleteThatFailsWithoutWordsSaysTheGivenFallback() {
        val deleter = SharedDeleter<Unit>(scope, io, R.string.contact_delete_failed) { throw IllegalStateException("x") }
        val events = CopyOnWriteArrayList<SharedEvent>()
        scope.launch { deleter.events.collect { events += it } }
        deleter.delete(Unit)
        await("say") { events.isNotEmpty() }
        assertEquals(SharedEvent.Say(UiMessage.Res(R.string.contact_delete_failed)), events.first())
    }

    @Test fun resetIgnoresADeleteStillOnItsWay() {
        val release = CountDownLatch(1)
        val deleter = SharedDeleter<Unit>(scope, io, R.string.shared_failed) { release.await(5, TimeUnit.SECONDS) }
        val events = CopyOnWriteArrayList<SharedEvent>()
        scope.launch { deleter.events.collect { events += it } }
        deleter.delete(Unit)
        deleter.reset()
        release.countDown()
        Thread.sleep(150)
        assertTrue(events.isEmpty())
        assertTrue(!deleter.busy.value)
    }

    @Test fun theMessageFallbackReplacesOnlyTheGenericLine() {
        assertEquals(UiMessage.Res(R.string.compose_start_failed), SharedRules.message(IllegalStateException(), R.string.compose_start_failed))
        assertEquals(UiMessage.Text("words"), SharedRules.message(CoreException.Invalid("words"), R.string.compose_start_failed))
        assertEquals(UiMessage.Res(R.string.shared_sign_in_needed), SharedRules.message(CoreException.SignInNeeded("sign in"), R.string.compose_start_failed))
    }
}
