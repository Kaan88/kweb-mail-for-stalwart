package ovh.kweb.mail.ui.admin

import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import ovh.kweb.mail.ui.UiMessage
import uniffi.kweb_core.Exception as CoreException
import uniffi.kweb_core.ServerAccountKind

class AccountsControllerTest {
    private val rig = Rig()
    private lateinit var c: AccountsController

    @Before fun setUp() {
        rig.backend.accountList = listOf("ada", "bob", "carol").map { account(it) }
        c = AccountsController(rig.scope, rig.io, rig.backend, debounceMs = 0)
    }

    @After fun tearDown() = rig.stop()

    @Test fun listsAccountsAndRemembersHowManyThereAre() {
        c.refresh()
        waitFor("list") { c.list.state.value.phase == ListPhase.READY }
        assertEquals(3, c.list.state.value.items.size)
        waitFor("total") { c.state.value.knownTotal == 3 }
    }

    @Test fun typingSearchesOnTheServerAndKeepsTheKnownTotal() {
        c.refresh()
        waitFor("list") { c.list.state.value.phase == ListPhase.READY }
        waitFor("total") { c.state.value.knownTotal == 3 }
        c.setText("bo")
        waitFor("search") { c.list.state.value.items.map { it.name } == listOf("bob") }
        assertTrue(rig.backend.calls.any { it.startsWith("accounts:bo:") })
        assertEquals(3, c.state.value.knownTotal)
    }

    @Test fun morePagesAreAskedWithTheTextOfTheFirstPage() {
        rig.backend.accountList = (1..120).map { account("u$it") }
        val slow = AccountsController(rig.scope, rig.io, rig.backend, debounceMs = 60_000)
        slow.setText("u")
        slow.list.reload(quiet = false)
        waitFor("page 1") { slow.list.state.value.items.size == 50 }
        slow.setText("uu")
        slow.list.loadMore()
        waitFor("page 2") { slow.list.state.value.items.size == 100 }
        assertTrue(rig.backend.calls.contains("accounts:u:50:50"))
        assertTrue(rig.backend.calls.none { it.startsWith("accounts:uu:") })
    }

    @Test fun aSearchThatFailedDoesNotChangeWhatMoreRowsAreReadFor() {
        rig.backend.accountList = (1..120).map { account("u$it") }
        val slow = AccountsController(rig.scope, rig.io, rig.backend, debounceMs = 60_000)
        slow.setText("u")
        slow.list.reload(quiet = false)
        waitFor("page 1") { slow.list.state.value.items.size == 50 }
        // "uu" is typed and its read fails: the list of "u" stays on screen, and more rows are still those of "u".
        slow.setText("uu")
        rig.backend.failWith = java.io.IOException("no network")
        slow.list.reload(quiet = true)
        waitFor("error") { slow.list.state.value.error != null }
        rig.backend.failWith = null
        slow.list.loadMore()
        waitFor("page 2") { slow.list.state.value.items.size == 100 }
        assertTrue(rig.backend.calls.contains("accounts:u:50:50"))
        assertTrue(rig.backend.calls.none { it == "accounts:uu:50:50" })
        assertEquals(slow.list.state.value.items.map { it.id }.distinct(), slow.list.state.value.items.map { it.id })
    }
}

class AccountControllerTest {
    private val rig = Rig()
    private val backend get() = rig.backend
    private lateinit var c: AccountController

    @Before fun setUp() {
        backend.accountList = listOf(account("a1", "bob"), account("g1", "sales", kind = ServerAccountKind.GROUP))
        c = AccountController(rig.scope, rig.io, backend, "a1")
    }

    @After fun tearDown() = rig.stop()

    private fun loaded() {
        c.refresh()
        waitFor("account") { c.state.value.account != null }
    }

    private fun idle() = waitFor("idle") { !c.state.value.busy }

    @Test fun readsTheAccount() {
        loaded()
        assertEquals("bob", c.state.value.account!!.name)
    }

    @Test fun anUnknownAccountIsAFailedRead() {
        c = AccountController(rig.scope, rig.io, backend, "nope")
        c.refresh()
        waitFor("failed") { c.state.value.phase == AccountPhase.FAILED }
    }

    @Test fun editingChangesTheDescriptionAndTheLimit() {
        loaded()
        c.openEdit()
        c.typeDescription("  Bob B ")
        c.typeLimit("2")
        c.saveEdit()
        idle()
        assertTrue(backend.calls.contains("update:a1:Bob B:${2uL * 1024uL * 1024uL * 1024uL}"))
        assertNull(c.state.value.dialog)
        assertEquals(2uL * 1024uL * 1024uL * 1024uL, c.state.value.account!!.quota)
    }

    @Test fun aLimitThatWasNotEditedIsSentBackExactly() {
        // 5,000,000,000 bytes is "4.66 GB" in the field: saving only the description must not turn it into 5,003,636,899.
        backend.accountList = listOf(account("a1", "bob").copy(quota = 5_000_000_000uL))
        loaded()
        c.openEdit()
        c.typeDescription("Only the words")
        c.saveEdit()
        idle()
        assertTrue(backend.calls.contains("update:a1:Only the words:5000000000"))
        // A limit so small that it shows as "0 MB" does not block saving either.
        backend.accountList = listOf(account("a1", "bob").copy(quota = 3000uL))
        c = AccountController(rig.scope, rig.io, backend, "a1")
        loaded()
        c.openEdit()
        assertFalse((c.state.value.dialog as AccountDialog.Edit).limitInvalid)
        c.typeDescription("x")
        c.saveEdit()
        idle()
        assertTrue(backend.calls.contains("update:a1:x:3000"))
    }

    @Test fun anEmptyLimitMeansNoLimitAndAnInvalidOneSavesNothing() {
        loaded()
        c.openEdit()
        c.typeLimit("lots")
        c.saveEdit()
        assertTrue(backend.calls.none { it.startsWith("update") })
        c.typeLimit("")
        c.saveEdit()
        idle()
        assertTrue(backend.calls.contains("update:a1::null"))
    }

    @Test fun aPasswordMustBeTypedTwiceBeforeItIsSent() {
        loaded()
        c.openReset()
        c.typePassword("longenough1")
        c.typeRepeat("longenough2")
        c.confirmReset()
        assertTrue(backend.calls.none { it.startsWith("reset") })
        c.typeRepeat("longenough1")
        c.confirmReset()
        idle()
        assertEquals("longenough1", backend.lastPassword)
        assertTrue(backend.calls.contains("reset:a1"))
    }

    @Test fun thePasswordIsGoneFromTheStateWhenTheDialogCloses() {
        loaded()
        c.openReset()
        c.typePassword("longenough1")
        c.typeRepeat("longenough1")
        c.confirmReset()
        idle()
        assertNull(c.state.value.dialog)
        assertFalse(c.state.value.toString().contains("longenough1"))
    }

    @Test fun cancellingTheResetDropsWhatWasTyped() {
        loaded()
        c.openReset()
        c.typePassword("longenough1")
        c.dismissDialog()
        assertNull(c.state.value.dialog)
        assertFalse(c.state.value.toString().contains("longenough1"))
    }

    @Test fun aFailedResetKeepsTheDialogAndSaysWhy() {
        loaded()
        backend.failWith = CoreException.Invalid("the password is too weak")
        c.openReset()
        c.typePassword("longenough1")
        c.typeRepeat("longenough1")
        c.confirmReset()
        idle()
        assertNotNull(c.state.value.dialog)
        assertEquals(UiMessage.Text("the password is too weak"), c.state.value.error)
    }

    @Test fun suspendingAsksFirstLiftingDoesNot() {
        loaded()
        c.toggleSuspend()
        assertEquals(AccountDialog.Suspend, c.state.value.dialog)
        assertTrue(backend.calls.none { it.startsWith("suspend") })
        c.confirmSuspend()
        idle()
        assertTrue(backend.calls.contains("suspend:a1:true"))
        waitFor("suspended") { c.state.value.account?.isSuspended == true }
        c.toggleSuspend()
        idle()
        assertNull(c.state.value.dialog)
        assertTrue(backend.calls.contains("suspend:a1:false"))
    }

    @Test fun deleteWantsTheNameTypedAndTheScreenCloses() {
        loaded()
        c.askDelete()
        c.typeDelete("bo")
        c.confirmDelete()
        assertTrue(backend.calls.none { it.startsWith("delete") })
        c.typeDelete("bob")
        c.confirmDelete()
        waitFor("gone") { c.state.value.gone }
        assertTrue(backend.calls.contains("delete:a1"))
    }

    @Test fun aRefusedDeleteSaysWhyAndStays() {
        loaded()
        backend.failWith = CoreException.Invalid("take the members out first")
        c.askDelete()
        c.typeDelete("bob")
        c.confirmDelete()
        idle()
        assertFalse(c.state.value.gone)
        assertEquals(UiMessage.Text("take the members out first"), c.state.value.error)
    }

    @Test fun leavingTheScreenClosesAnOpenDialog() {
        loaded()
        c.openReset()
        c.typePassword("longenough1")
        c.clear()
        assertNull(c.state.value.dialog)
    }
}

class NewAccountControllerTest {
    private val rig = Rig()
    private lateinit var c: NewAccountController

    @Before fun setUp() {
        c = NewAccountController(rig.scope, rig.io, rig.backend)
    }

    @After fun tearDown() = rig.stop()

    private fun loaded() {
        c.load()
        waitFor("domains") { c.state.value.phase == NewAccountPhase.READY }
    }

    private fun fill() {
        c.typeName("ada")
        c.typePassword("longenough1")
        c.typeRepeat("longenough1")
    }

    @Test fun theFirstDomainIsChosenAtOnce() {
        loaded()
        assertEquals("d1", c.state.value.domainId)
    }

    @Test fun noDomainMeansNoCreate() {
        rig.backend.domains = emptyList()
        loaded()
        fill()
        assertFalse(c.state.value.ready)
        c.create()
        assertTrue(rig.backend.calls.none { it.startsWith("create") })
    }

    @Test fun aBadNameOrPasswordIsNotSent() {
        loaded()
        c.typeName("a b")
        c.typePassword("longenough1")
        c.typeRepeat("longenough1")
        c.create()
        c.typeName("ada")
        c.typeRepeat("different12")
        c.create()
        assertTrue(rig.backend.calls.none { it.startsWith("create") })
    }

    @Test fun creatingSendsTheNameDomainAndPasswordOnceThenForgetsThePassword() {
        loaded()
        fill()
        c.create()
        c.create()
        waitFor("created") { c.state.value.created != null }
        assertEquals(1, rig.backend.calls.count { it.startsWith("create") })
        assertEquals("longenough1", rig.backend.lastPassword)
        assertEquals("", c.state.value.password)
        assertEquals("", c.state.value.repeat)
    }

    @Test fun aRefusalIsShownInTheCoresWordsAndTheFormKeepsItsInput() {
        loaded()
        fill()
        rig.backend.failWith = CoreException.Invalid("that name is taken")
        c.create()
        waitFor("error") { c.state.value.error != null }
        assertEquals(UiMessage.Text("that name is taken"), c.state.value.error)
        assertEquals("ada", c.state.value.name)
        assertNull(c.state.value.created)
    }

    @Test fun leavingTheScreenForgetsThePassword() {
        loaded()
        fill()
        c.clear()
        assertEquals("", c.state.value.password)
        assertFalse(c.state.value.toString().contains("longenough1"))
    }

    @Test fun domainsThatFailToLoadCanBeReadAgain() {
        rig.backend.failWith = java.io.IOException("x")
        c.load()
        waitFor("failed") { c.state.value.phase == NewAccountPhase.FAILED }
        rig.backend.failWith = null
        c.load()
        waitFor("ready") { c.state.value.phase == NewAccountPhase.READY }
    }
}
