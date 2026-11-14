package ovh.kweb.mail.ui.account

import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
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
import ovh.kweb.mail.ui.UiMessage
import uniffi.kweb_core.AccountFeatures
import uniffi.kweb_core.AccountSecurity
import uniffi.kweb_core.Exception as CoreException
import uniffi.kweb_core.MaskedAddress
import uniffi.kweb_core.MaskedAddresses
import uniffi.kweb_core.NewSecret
import uniffi.kweb_core.TwoStepSetup

fun masked(id: String, enabled: Boolean = true, note: String = "") = MaskedAddress(id, "$id@example.org", note, "", enabled, 1_700_000_000, null)

/** A backend that remembers what it was asked. Every call is recorded by name; a secret is recorded only as "x". */
class FakeAccountBackend : AccountBackend {
    val calls = CopyOnWriteArrayList<String>()
    @Volatile var addresses: List<MaskedAddress> = listOf(masked("a"), masked("b"))
    @Volatile var limit: UInt? = 5u
    @Volatile var failWith: Exception? = null
    @Volatile var failNext: Exception? = null
    @Volatile var gate: (() -> Unit)? = null
    /** Held only by a change of a masked address, not by a read. */
    @Volatile var updateGate: (() -> Unit)? = null
    @Volatile var secretGate: (() -> Unit)? = null
    @Volatile var security = AccountSecurity(false, emptyList(), emptyList(), emptyList(), emptyList())
    var lastExpires: Long? = null
    var otpauthSeen: String? = null
    var codeSeen: String? = null
    /** The current two-step code each change was given (null: none), in order; a code is not recorded as a call. */
    val codesSeen = CopyOnWriteArrayList<String?>()

    private fun fail() {
        failNext?.let { failNext = null; throw it }
        failWith?.let { throw it }
    }

    override fun accountFeatures() = AccountFeatures("enterprise", true, true, false)

    override fun maskedAddresses(): MaskedAddresses {
        val before = MaskedAddresses(addresses, limit)
        gate?.invoke()
        calls += "masked"
        failWith?.let { throw it }
        return before
    }

    override fun createMaskedAddress(description: String, forDomain: String, prefix: String): MaskedAddress {
        calls += "create:$description:$forDomain:$prefix"
        fail()
        val made = masked("new${addresses.size}", note = description)
        addresses = listOf(made) + addresses
        return made
    }

    override fun updateMaskedAddress(id: String, enabled: Boolean, description: String?) {
        calls += "update:$id:$enabled:$description"
        gate?.invoke()
        updateGate?.invoke()
        fail()
        addresses = addresses.map { if (it.id == id) it.copy(enabled = enabled, description = description ?: it.description) else it }
    }

    override fun deleteMaskedAddress(id: String) {
        calls += "delete:$id"
        fail()
        addresses = addresses.filter { it.id != id }
    }

    override fun accountSecurity(): AccountSecurity {
        val before = security
        gate?.invoke()
        calls += "security"
        failWith?.let { throw it }
        return before
    }

    override fun changePassword(current: String, new: String, code: String?) { calls += "password"; codesSeen += code; gate?.invoke(); fail() }

    override fun newTwoStep(): TwoStepSetup { calls += "newTwoStep"; fail(); return TwoStepSetup("otpauth://totp/x?secret=ABCD", "ABCD EFGH") }

    override fun enableTwoStep(currentPassword: String, otpauthUrl: String, code: String, oldCode: String?) {
        codesSeen += oldCode
        calls += "enableTwoStep"
        otpauthSeen = otpauthUrl
        codeSeen = code
        fail()
        security = security.copy(twoStep = true)
    }

    override fun disableTwoStep(currentPassword: String, code: String?) { calls += "disableTwoStep"; codesSeen += code; fail(); security = security.copy(twoStep = false) }

    override fun createAppPassword(description: String, expiresAt: Long?): NewSecret { calls += "createAppPassword:$description"; lastExpires = expiresAt; secretGate?.invoke(); fail(); return NewSecret("p1", "app-secret") }

    override fun deleteAppPassword(id: String) { calls += "deleteAppPassword:$id"; fail() }

    override fun createApiKey(description: String, expiresAt: Long?): NewSecret { calls += "createApiKey:$description"; lastExpires = expiresAt; fail(); return NewSecret("k1", "key-secret") }

    override fun deleteApiKey(id: String) { calls += "deleteApiKey:$id"; fail() }

    override fun renamePasskey(id: String, name: String) { calls += "rename:$id:$name"; fail() }

    override fun deletePasskey(id: String) { calls += "deletePasskey:$id"; fail() }

    override fun revokeConnectedApp(id: String) { calls += "revoke:$id"; fail() }
}

class MaskedControllerTest {
    private val pool = Executors.newCachedThreadPool()
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Unconfined)
    private lateinit var backend: FakeAccountBackend
    private lateinit var c: MaskedController

    @Before fun setUp() {
        backend = FakeAccountBackend()
        c = MaskedController(scope, pool.asCoroutineDispatcher(), backend)
    }

    @After fun tearDown() {
        pool.shutdownNow()
    }

    private fun waitFor(what: String = "state", check: () -> Boolean) {
        val end = System.currentTimeMillis() + 5000
        while (!check()) {
            if (System.currentTimeMillis() > end) throw AssertionError("timed out waiting for $what: ${c.state.value}")
            Thread.sleep(5)
        }
    }

    private fun ready() {
        c.start()
        waitFor("ready") { c.state.value.phase == MaskedPhase.READY }
    }

    private fun idle() = waitFor("idle") { !c.state.value.busy && c.state.value.pending.isEmpty() }

    @Test fun loadsTheListAndTheLimit() {
        ready()
        assertEquals(listOf("a", "b"), c.state.value.addresses.map { it.id })
        assertEquals(5, c.state.value.limit)
        assertFalse(c.state.value.atLimit)
    }

    @Test fun notOfferedSaysSoInTheCoresWords() {
        backend.failWith = CoreException.Invalid("masked addresses need the Enterprise edition")
        c.start()
        waitFor("none") { c.state.value.phase == MaskedPhase.NONE }
        assertEquals(UiMessage.Text("masked addresses need the Enterprise edition"), c.state.value.unsupported)
    }

    @Test fun aFailedLoadCanBeRetried() {
        backend.failWith = java.io.IOException("x")
        c.start()
        waitFor("failed") { c.state.value.phase == MaskedPhase.FAILED }
        backend.failWith = null
        c.load()
        waitFor("ready") { c.state.value.phase == MaskedPhase.READY }
    }

    @Test fun theSwitchChangesAtOnceAndCallsTheCore() {
        ready()
        c.setEnabled("a", false)
        assertFalse(c.state.value.addresses.first { it.id == "a" }.enabled)
        idle()
        assertTrue(backend.calls.contains("update:a:false:null"))
        assertFalse(backend.addresses.first { it.id == "a" }.enabled)
    }

    @Test fun aRefusedSwitchGoesBackAndSaysWhy() {
        ready()
        backend.failNext = CoreException.Invalid("not allowed")
        c.setEnabled("a", false)
        idle()
        assertTrue(c.state.value.addresses.first { it.id == "a" }.enabled)
        assertEquals(UiMessage.Text("not allowed"), c.state.value.error)
    }

    @Test fun aSecondTapOnASwitchThatIsOnItsWayIsDropped() {
        ready()
        val release = CountDownLatch(1)
        backend.gate = { release.await(5, TimeUnit.SECONDS) }
        c.setEnabled("a", false)
        c.setEnabled("a", true)
        release.countDown()
        idle()
        assertEquals(1, backend.calls.count { it.startsWith("update:a") })
    }

    @Test fun aReadDuringASwitchKeepsTheSwitchTheUserChose() {
        ready()
        val release = CountDownLatch(1)
        backend.updateGate = { release.await(5, TimeUnit.SECONDS) }
        c.setEnabled("a", false)
        // The server has not applied the switch yet, so this read still says "on".
        c.load()
        waitFor("read") { backend.calls.count { it == "masked" } >= 2 }
        Thread.sleep(100)
        assertFalse(c.state.value.addresses.first { it.id == "a" }.enabled)
        release.countDown()
        idle()
        assertFalse(c.state.value.addresses.first { it.id == "a" }.enabled)
    }

    @Test fun savingANoteSendsTheSwitchAsItIsNowNotAsItWasWhenTheDialogOpened() {
        ready()
        c.askNote(c.state.value.addresses.first { it.id == "a" })
        // Switched off elsewhere while the dialog is open; the resume read brings that in.
        backend.addresses = listOf(masked("a", enabled = false), masked("b"))
        c.load()
        waitFor("read") { !c.state.value.addresses.first { it.id == "a" }.enabled }
        c.saveNote("Shop")
        idle()
        assertTrue(backend.calls.contains("update:a:false:Shop"))
        assertFalse(backend.addresses.first { it.id == "a" }.enabled)
    }

    @Test fun savingANoteWaitsWhileThatAddressSwitchIsOnItsWay() {
        ready()
        val release = CountDownLatch(1)
        backend.updateGate = { release.await(5, TimeUnit.SECONDS) }
        c.setEnabled("a", false)
        c.askNote(c.state.value.addresses.first { it.id == "a" })
        c.saveNote("Shop")
        release.countDown()
        idle()
        assertEquals(listOf("update:a:false:null"), backend.calls.filter { it.startsWith("update") })
    }

    @Test fun deleteAsksThenDeletes() {
        ready()
        c.askDelete(c.state.value.addresses[0])
        assertTrue(c.state.value.dialog is MaskedDialog.Delete)
        assertFalse(backend.calls.any { it.startsWith("delete") })
        c.confirmDelete()
        idle()
        waitFor("gone") { c.state.value.addresses.map { it.id } == listOf("b") }
        assertNull(c.state.value.dialog)
    }

    @Test fun aRefusedDeleteClosesTheQuestionAndSaysWhy() {
        ready()
        c.askDelete(c.state.value.addresses[0])
        backend.failNext = java.io.IOException("x")
        c.confirmDelete()
        idle()
        assertNull(c.state.value.dialog)
        assertEquals(UiMessage.Res(ovh.kweb.mail.R.string.masked_delete_failed), c.state.value.error)
        assertEquals(2, c.state.value.addresses.size)
    }

    @Test fun changingTheNoteSendsTheTrimmedText() {
        ready()
        c.askNote(c.state.value.addresses[0])
        c.saveNote("  Bookshop ")
        idle()
        waitFor("note") { c.state.value.addresses[0].description == "Bookshop" }
        assertTrue(backend.calls.contains("update:a:true:Bookshop"))
    }

    @Test fun anUnchangedNoteSendsNothing() {
        ready()
        c.askNote(c.state.value.addresses[0])
        c.saveNote("")
        assertNull(c.state.value.dialog)
        assertFalse(backend.calls.any { it.startsWith("update") })
    }

    @Test fun makingOneShowsItAndPutsItFirst() {
        ready()
        c.openSheet()
        c.create(" Bookshop ", "https://example.com/x", "shop")
        waitFor("created") { c.state.value.sheet is MaskedSheet.Created }
        assertTrue(backend.calls.contains("create:Bookshop:example.com:shop"))
        assertEquals(3, c.state.value.addresses.size)
        assertEquals((c.state.value.sheet as MaskedSheet.Created).address.id, c.state.value.addresses[0].id)
        c.closeSheet()
        assertNull(c.state.value.sheet)
    }

    @Test fun aBadPrefixIsNotSent() {
        ready()
        c.openSheet()
        c.create("x", "", "no spaces")
        assertFalse(backend.calls.any { it.startsWith("create") })
    }

    @Test fun atTheLimitTheSheetDoesNotOpen() {
        backend.limit = 2u
        ready()
        assertTrue(c.state.value.atLimit)
        c.openSheet()
        assertNull(c.state.value.sheet)
    }

    @Test fun theLimitReachedElsewhereStaysInTheSheetAndReadsAgain() {
        ready()
        c.openSheet()
        backend.failNext = CoreException.Invalid("This server allows 5 masked addresses.")
        c.create("x", "", "")
        waitFor("error") { c.state.value.sheetError != null }
        assertEquals(UiMessage.Text("This server allows 5 masked addresses."), c.state.value.sheetError)
        assertTrue(c.state.value.sheet is MaskedSheet.Make)
        waitFor("read again") { backend.calls.count { it == "masked" } >= 2 }
    }

    @Test fun aLateReadDoesNotReplaceANewerOne() {
        val first = CountDownLatch(1)
        val entered = CountDownLatch(1)
        var slow = true
        backend.gate = { if (slow) { slow = false; entered.countDown(); first.await(5, TimeUnit.SECONDS) } }
        c.start()
        entered.await(5, TimeUnit.SECONDS)
        backend.addresses = listOf(masked("only"))
        c.load()
        waitFor("newer") { c.state.value.addresses.map { it.id } == listOf("only") }
        first.countDown()
        Thread.sleep(100)
        assertEquals(listOf("only"), c.state.value.addresses.map { it.id })
    }

    @Test fun resetDropsEverything() {
        ready()
        c.reset()
        assertEquals(MaskedState(), c.state.value)
    }
}
