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
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.UiMessage
import uniffi.kweb_core.Label
import uniffi.kweb_core.Exception as CoreException

class LabelStoreTest {
    private val pool = Executors.newCachedThreadPool()
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Unconfined)
    private val backend = FakeExtras()
    private val errors = CopyOnWriteArrayList<UiMessage>()
    private lateinit var store: LabelStore

    @Before fun setUp() {
        backend.labelList = listOf(Label("work", "Work"), Label("home", "Home"))
        store = LabelStore(scope, pool.asCoroutineDispatcher(), backend) { errors += it }
    }

    @After fun tearDown() {
        pool.shutdownNow()
    }

    private val by get() = store.state.value.byMessage

    @Test fun readsTheUsersLabels() {
        store.refreshLabels()
        waitFor("labels") { store.state.value.labels.size == 2 }
    }

    @Test fun aFailedReadOfTheListLeavesWhatIsThere() {
        store.refreshLabels()
        waitFor("labels") { store.state.value.labels.size == 2 }
        backend.labelsFail = java.io.IOException("x")
        store.refreshLabels()
        waitFor("asked twice") { backend.calls.count { it == "labels" } == 2 }
        Thread.sleep(30)
        assertEquals(2, store.state.value.labels.size)
        assertTrue(errors.isEmpty())
    }

    @Test fun readsTheLabelsOfManyMessagesInOneCall() {
        backend.keywords["a"] = listOf("work")
        backend.keywords["b"] = listOf("home", "work")
        store.load(listOf("a", "b", "c", "a"))
        waitFor("labels") { by.size == 3 }
        assertEquals(listOf("work"), by["a"])
        assertEquals(listOf("home", "work"), by["b"])
        assertEquals(emptyList<String>(), by["c"])
        assertEquals(1, backend.calls.count { it.startsWith("messageLabels") })
        assertEquals("messageLabels:a,b,c", backend.calls.single { it.startsWith("messageLabels") })
    }

    @Test fun nothingIsAskedForNoMessages() {
        store.load(emptyList())
        Thread.sleep(30)
        assertTrue(backend.calls.isEmpty())
    }

    @Test fun aFailedReadLeavesTheChipsAsTheyWereAndSaysNothing() {
        backend.keywords["a"] = listOf("work")
        store.load(listOf("a"))
        waitFor("labels") { by["a"] == listOf("work") }
        backend.messageLabelsFails = java.io.IOException("x")
        store.load(listOf("a"))
        waitFor("asked twice") { backend.calls.size == 2 }
        Thread.sleep(30)
        assertEquals(listOf("work"), by["a"])
        assertTrue(errors.isEmpty())
    }

    @Test fun aChangeShowsAtOnceAndGoesToTheCore() {
        store.set(listOf("a", "b"), "work", true)
        assertEquals(listOf("work"), by["a"])
        assertEquals(listOf("work"), by["b"])
        waitFor("core") { backend.calls.any { it == "set:a,b:work:true" } }
        store.set(listOf("a"), "work", false)
        assertEquals(emptyList<String>(), by["a"])
        waitFor("core off") { backend.calls.any { it == "set:a:work:false" } }
        waitFor("settled") { backend.keywords["a"].orEmpty().isEmpty() && by["b"] == listOf("work") }
    }

    @Test fun aRefusedChangeIsTakenBackAndTheCoresWordsShown() {
        backend.keywords["a"] = emptyList()
        backend.setLabelFails = CoreException.Invalid("too many labels")
        store.set(listOf("a"), "work", true)
        waitFor("error") { errors.isNotEmpty() }
        assertEquals(UiMessage.Text("too many labels"), errors.single())
        waitFor("taken back") { by["a"] == emptyList<String>() }
    }

    @Test fun aRefusalIsAlsoInTheStateForTheSheetAndGoesWhenTheUserActsAgain() {
        backend.setLabelFails = CoreException.Invalid("nope")
        store.set(listOf("a"), "work", true)
        waitFor("error in state") { store.state.value.error == UiMessage.Text("nope") }
        store.clearError()
        assertEquals(null, store.state.value.error)
        backend.setLabelFails = CoreException.Invalid("again")
        store.set(listOf("a"), "work", true)
        waitFor("second") { store.state.value.error == UiMessage.Text("again") }
        backend.setLabelFails = null
        store.set(listOf("a"), "home", true)
        assertEquals(null, store.state.value.error)
    }

    @Test fun aRefusedNewLabelIsInTheStateToo() {
        backend.createFails = CoreException.Invalid("exists")
        var made: Boolean? = null
        store.create("Work", emptyList()) { made = it }
        waitFor("refused") { made == false }
        assertEquals(UiMessage.Text("exists"), store.state.value.error)
    }

    @Test fun theLabelListIsNotReadUntilTheCoreHasAnswered() {
        assertEquals(false, store.state.value.labelsRead)
        backend.labelsFail = java.io.IOException("x")
        store.refreshLabels()
        waitFor("asked") { backend.calls.contains("labels") }
        Thread.sleep(30)
        assertEquals(false, store.state.value.labelsRead)
        backend.labelsFail = null
        store.refreshLabels()
        waitFor("read") { store.state.value.labelsRead }
        // An empty answer is "none", which is different from "not read".
        backend.labelList = emptyList()
        store.reset()
        assertEquals(false, store.state.value.labelsRead)
        store.refreshLabels()
        waitFor("read empty") { store.state.value.labelsRead }
        assertTrue(store.state.value.labels.isEmpty())
    }

    @Test fun aSecondQuickChangeIsNeverOverwrittenByTheReadOfTheFirst() {
        backend.keywords["a"] = emptyList()
        val inSecond = CountDownLatch(1)
        val release = CountDownLatch(1)
        backend.setLabelGate = { label -> if (label == "home") { inSecond.countDown(); release.await() } }
        store.set(listOf("a"), "work", true)
        store.set(listOf("a"), "home", true)
        inSecond.await()
        // The first change is done and has read the core (Work only); the second is still on its way.
        waitFor("first read asked") { backend.calls.any { it.startsWith("messageLabels") } }
        Thread.sleep(100)
        assertEquals(listOf("work", "home"), by["a"])
        release.countDown()
        waitFor("settled") { backend.calls.count { it.startsWith("messageLabels") } == 2 && by["a"]?.toSet() == setOf("work", "home") }
    }

    @Test fun anotherFailureSaysItInTheAppsWords() {
        backend.setLabelFails = java.io.IOException("x")
        store.set(listOf("a"), "work", true)
        waitFor("error") { errors.isNotEmpty() }
        assertEquals(UiMessage.Res(R.string.labels_change_failed), errors.single())
    }

    @Test fun aSlowOldAnswerNeverUndoesANewerChange() {
        // The read that was started first is held back until the change has been made on screen.
        backend.keywords["a"] = emptyList()
        val inCore = CountDownLatch(1)
        val release = CountDownLatch(1)
        backend.messageLabelsGate = { inCore.countDown(); release.await() }
        store.load(listOf("a"))
        inCore.await()
        backend.messageLabelsGate = null
        store.set(listOf("a"), "work", true)
        assertEquals(listOf("work"), by["a"])
        release.countDown()
        waitFor("core") { backend.calls.any { it == "set:a:work:true" } }
        Thread.sleep(100)
        assertEquals(listOf("work"), by["a"])
    }

    @Test fun ANewerAnswerWinsOverAnOlderOneForTheSameMessage() {
        backend.keywords["a"] = listOf("home")
        val inCore = CountDownLatch(1)
        val release = CountDownLatch(1)
        backend.messageLabelsGate = { inCore.countDown(); release.await() }
        store.load(listOf("a"))
        inCore.await()
        backend.messageLabelsGate = null
        backend.keywords["a"] = listOf("work")
        store.load(listOf("a"))
        waitFor("newer") { by["a"] == listOf("work") }
        release.countDown()
        Thread.sleep(100)
        assertEquals(listOf("work"), by["a"])
    }

    @Test fun anAnswerForOtherMessagesDoesNotTouchTheOnesItWasNotAskedAbout() {
        backend.keywords["a"] = listOf("work")
        store.load(listOf("a"))
        waitFor("a") { by["a"] == listOf("work") }
        backend.keywords["b"] = listOf("home")
        store.load(listOf("b"))
        waitFor("b") { by["b"] == listOf("home") }
        assertEquals(listOf("work"), by["a"])
    }

    @Test fun anAnswerThatArrivesAfterAResetIsDropped() {
        backend.keywords["a"] = listOf("work")
        val inCore = CountDownLatch(1)
        val release = CountDownLatch(1)
        backend.messageLabelsGate = { inCore.countDown(); release.await() }
        store.load(listOf("a"))
        inCore.await()
        store.reset()
        release.countDown()
        Thread.sleep(100)
        assertTrue(by.isEmpty())
    }

    @Test fun aNewLabelIsMadeAndPutOnTheMessages() {
        var made: Boolean? = null
        store.create("Tax", listOf("a")) { made = it }
        waitFor("made") { made == true }
        assertTrue(store.state.value.labels.any { it.name == "Tax" })
        waitFor("applied") { backend.calls.contains("set:a:tax:true") }
        assertEquals(listOf("tax"), by["a"])
    }

    @Test fun aNameThatExistsIsRefusedInTheCoresWords() {
        backend.createFails = CoreException.Invalid("a label with this name exists")
        var made: Boolean? = null
        store.create("Work", emptyList()) { made = it }
        waitFor("refused") { made == false }
        assertEquals(UiMessage.Text("a label with this name exists"), errors.single())
        assertTrue(backend.calls.none { it.startsWith("set:") })
    }

    @Test fun deletingALabelTakesItOutOfTheList() {
        store.refreshLabels()
        waitFor("labels") { store.state.value.labels.size == 2 }
        var done: Boolean? = null
        store.delete("work") { done = it }
        waitFor("deleted") { done == true }
        assertEquals(listOf("home"), store.state.value.labels.map { it.id })
    }

    @Test fun aFailedDeleteKeepsTheLabelAndSaysSo() {
        store.refreshLabels()
        waitFor("labels") { store.state.value.labels.size == 2 }
        backend.deleteFails = java.io.IOException("x")
        var done: Boolean? = null
        store.delete("work") { done = it }
        waitFor("failed") { done == false }
        assertEquals(2, store.state.value.labels.size)
        assertEquals(UiMessage.Res(R.string.labels_delete_failed), errors.single())
    }
}
