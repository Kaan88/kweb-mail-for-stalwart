package ovh.kweb.mail.ui.enterprise

import java.util.concurrent.Executors
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.asCoroutineDispatcher
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.UiMessage
import uniffi.kweb_core.RecoveryState
import uniffi.kweb_core.Exception as CoreException

class RecoveryControllerTest {
    private val pool = Executors.newCachedThreadPool()
    private val job = SupervisorJob()
    private val scope = CoroutineScope(job + Dispatchers.Unconfined)
    private lateinit var backend: FakeEnterpriseBackend
    private lateinit var c: RecoveryController

    @Before fun setUp() {
        backend = FakeEnterpriseBackend()
        c = RecoveryController(scope, pool.asCoroutineDispatcher(), backend, pageSize = 50u)
    }

    @After fun tearDown() {
        job.cancel()
        pool.shutdownNow()
    }

    private fun ready() {
        c.start()
        waitUntil("ready") { c.state.value.phase == Phase.READY && !c.state.value.listing }
    }

    @Test fun readsTheStateThenTheOwnAccountsItems() {
        ready()
        assertEquals(listOf("recovery", "deleted:null:0:50"), backend.calls.toList())
        assertEquals(listOf("m1", "m2"), c.state.value.items.map { it.id })
    }

    @Test fun whenRecoveryIsOffNoListIsAskedFor() {
        backend.recovery = RecoveryState(false, null)
        ready()
        assertEquals(listOf("recovery"), backend.calls.toList())
        assertFalse(c.state.value.recovery!!.isOn)
        c.askAccount()
        assertNull("no chooser when there is nothing to choose for", c.state.value.dialog)
    }

    @Test fun anotherAccountIsChosenByIdAndItsItemsAreListed() {
        backend.deletedItems = backend.deletedItems + ("acc9" to listOf(deleted("z1")))
        ready()
        c.askAccount()
        c.setAccountChoice(AccountChoice(true, " acc9 "))
        c.applyAccount()
        waitUntil("other list") { c.state.value.items.map { it.id } == listOf("z1") }
        assertNull(c.state.value.dialog)
        assertEquals("acc9", c.state.value.account.id)
        assertEquals("deleted:acc9:0:50", backend.calls.last())
    }

    @Test fun anotherAccountWithoutAnIdStaysInTheDialogWithAMessage() {
        ready()
        c.askAccount()
        c.setAccountChoice(AccountChoice(true, "  "))
        c.applyAccount()
        val d = c.state.value.dialog as RecoveryDialog.Account
        assertEquals(UiMessage.Res(R.string.ent_account_id_needed), d.error)
        assertEquals(2, backend.calls.size)
    }

    @Test fun anotherAccountIsPickedFromTheServersAccountsAndShownByItsAddress() {
        backend.deletedItems = backend.deletedItems + ("acc9" to listOf(deleted("z1")))
        c = RecoveryController(scope, pool.asCoroutineDispatcher(), backend, pageSize = 50u, searchDebounceMs = 0)
        ready()
        assertTrue(c.state.value.canPick)
        c.askAccount()
        c.setAccountChoice(AccountChoice(other = true))
        c.searchAccounts("bob")
        waitUntil("matches") { (c.state.value.dialog as RecoveryDialog.Account).matches.map { it.id } == listOf("acc9") }
        c.pickAccount((c.state.value.dialog as RecoveryDialog.Account).matches.first())
        c.applyAccount()
        waitUntil("other list") { c.state.value.items.map { it.id } == listOf("z1") }
        assertEquals("bob@example.org", c.state.value.account.display)
        assertEquals("deleted:acc9:0:50", backend.calls.last())
    }

    @Test fun aKeyThatMayNotReadAccountsOffersOnlyTheOwnAccount() {
        backend.mayRead = false
        ready()
        assertFalse(c.state.value.canPick)
        c.askAccount()
        assertNull(c.state.value.dialog)
    }

    @Test fun changingTheAccountWhileAPageIsComingDoesNotLeaveTheSpinnerOn() {
        backend.deletedItems = mapOf(null to (1..120).map { deleted("m$it") }, "acc9" to (1..120).map { deleted("z$it") })
        ready()
        val gate = java.util.concurrent.CountDownLatch(1)
        backend.deletedGate = { offset -> if (offset > 0u) gate.await(5, java.util.concurrent.TimeUnit.SECONDS) }
        c.loadMore()
        waitUntil("page asked") { backend.calls.any { it == "deleted:null:50:50" } }
        c.askAccount()
        c.setAccountChoice(AccountChoice(true, "acc9", "bob@example.org"))
        c.applyAccount()
        waitUntil("new first page") { c.state.value.items.firstOrNull()?.id == "z1" && !c.state.value.listing }
        gate.countDown()
        Thread.sleep(100)
        assertFalse("the late page is gone and the spinner with it", c.state.value.loadingMore)
        assertEquals(50, c.state.value.items.size)
        c.loadMore()
        waitUntil("second page of the new account") { c.state.value.items.size == 100 }
    }

    @Test fun restoreAsksFirstAndOnlyThenCallsTheCore() {
        ready()
        c.askRestore("m1")
        assertTrue(c.state.value.dialog is RecoveryDialog.Restore)
        assertTrue(backend.calls.none { it.startsWith("restore") })
        c.confirmRestore()
        waitUntil("restored") { !c.state.value.busy && c.state.value.notice != null }
        assertEquals("restore:null:m1", backend.calls.last())
        assertEquals(listOf("m2"), c.state.value.items.map { it.id })
        assertEquals(1u, c.state.value.total)
        assertEquals(UiMessage.Res(R.string.ent_restored), c.state.value.notice)
    }

    @Test fun cancellingTheQuestionRestoresNothing() {
        ready()
        c.askRestore("m1")
        c.dismissDialog()
        c.confirmRestore()
        assertTrue(backend.calls.none { it.startsWith("restore") })
        assertEquals(2, c.state.value.items.size)
    }

    @Test fun restoreFromAnotherAccountNamesThatAccount() {
        backend.deletedItems = backend.deletedItems + ("acc9" to listOf(deleted("z1")))
        ready()
        c.askAccount()
        c.setAccountChoice(AccountChoice(true, "acc9"))
        c.applyAccount()
        waitUntil("other list") { c.state.value.items.map { it.id } == listOf("z1") }
        c.askRestore("z1")
        c.confirmRestore()
        waitUntil("restored") { !c.state.value.busy && c.state.value.notice != null }
        assertEquals("restore:acc9:z1", backend.calls.last())
    }

    @Test fun aRefusedRestoreKeepsTheItemAndShowsTheCoresWords() {
        ready()
        backend.failWith = CoreException.Invalid("the server refused to restore it")
        c.askRestore("m2")
        c.confirmRestore()
        waitUntil("error") { !c.state.value.busy && c.state.value.error != null }
        assertEquals(UiMessage.Text("the server refused to restore it"), c.state.value.error)
        assertEquals(2, c.state.value.items.size)
    }

    @Test fun aFailedStateReadCanBeRetried() {
        backend.failWith = java.io.IOException("x")
        c.start()
        waitUntil("failed") { c.state.value.phase == Phase.FAILED }
        assertEquals(UiMessage.Res(R.string.ent_recovery_failed), c.state.value.error)
        backend.failWith = null
        c.load()
        waitUntil("ready") { c.state.value.phase == Phase.READY && !c.state.value.listing }
        assertEquals(2, c.state.value.items.size)
    }

    @Test fun aRestoreIsRefusedWhileAnotherIsOnItsWay() {
        ready()
        c.askRestore("m1")
        c.confirmRestore()
        c.askRestore("m2")
        // Either the first one is done by now (the item is gone and "m2" may be asked), or the second question was refused.
        waitUntil("idle") { !c.state.value.busy }
        assertEquals(1, backend.calls.count { it.startsWith("restore") })
    }

    @Test fun morePagesAreAddedWithoutRepeats() {
        backend.deletedItems = mapOf(null to (1..5).map { deleted("i$it") })
        c = RecoveryController(scope, pool.asCoroutineDispatcher(), backend, pageSize = 2u)
        ready()
        assertTrue(c.state.value.hasMore)
        c.loadMore()
        waitUntil("page 2") { c.state.value.items.size == 4 && !c.state.value.loadingMore }
        c.loadMore()
        waitUntil("page 3") { c.state.value.items.size == 5 && !c.state.value.loadingMore }
        assertEquals((1..5).map { "i$it" }, c.state.value.items.map { it.id })
        assertFalse(c.state.value.hasMore)
    }
}
