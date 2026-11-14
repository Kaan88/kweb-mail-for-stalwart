package ovh.kweb.mail.ui.admin

import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import ovh.kweb.mail.ui.UiMessage
import uniffi.kweb_core.Exception as CoreException

class AccessControllerTest {
    private val rig = Rig()
    private val backend get() = rig.backend
    private val changes = mutableListOf<Boolean>()
    private lateinit var c: AccessController

    @Before fun setUp() {
        c = AccessController(rig.scope, rig.io, backend) { changes += it.connected }
    }

    @After fun tearDown() = rig.stop()

    private fun loaded() {
        c.load()
        waitFor("loaded") { c.state.value.phase == AccessPhase.READY }
    }

    @Test fun withoutAKeyTheAreaIsLocked() {
        loaded()
        assertEquals(AccessKind.Locked, c.state.value.kind)
    }

    @Test fun aFailedReadCanBeTriedAgain() {
        backend.failWith = java.io.IOException("x")
        c.load()
        waitFor("failed") { c.state.value.phase == AccessPhase.FAILED }
        backend.failWith = null
        c.load()
        waitFor("ready") { c.state.value.phase == AccessPhase.READY }
    }

    @Test fun anEmptyClipboardIsSaidSoAndNothingIsAsked() {
        loaded()
        c.paste("  ")
        assertEquals(UiMessage.Res(ovh.kweb.mail.R.string.admin_clipboard_no_key), c.state.value.error)
        assertNull(c.state.value.dialog)
    }

    @Test fun aPastedKeyIsShownMaskedAndNotSentUntilConfirmed() {
        loaded()
        c.paste("abcdefgh12345678")
        val dialog = c.state.value.dialog as AccessDialog.ConfirmKey
        assertEquals("abcdefgh" + "•".repeat(8), dialog.shown)
        assertFalse(c.state.value.toString().contains("12345678"))
        assertTrue(backend.calls.none { it == "connect" })
    }

    @Test fun confirmingTakesTheKeyAndTellsTheHost() {
        loaded()
        c.paste("abcdefgh12345678")
        c.confirmKey()
        waitFor("open") { c.state.value.kind == AccessKind.Open }
        assertEquals("abcdefgh12345678", backend.lastKey)
        assertEquals(listOf(true), changes)
        assertNull(c.state.value.dialog)
    }

    @Test fun decliningLetsGoOfTheKey() {
        loaded()
        c.paste("abcdefgh12345678")
        c.declineKey()
        c.confirmKey()
        assertTrue(backend.calls.none { it == "connect" })
        assertNull(c.state.value.dialog)
    }

    @Test fun aKeyTheServerRefusesShowsTheCoresWords() {
        loaded()
        backend.connectFails = CoreException.Invalid("the server did not accept the admin key")
        c.paste("abcdefgh12345678")
        c.confirmKey()
        waitFor("refused") { c.state.value.refused }
        assertEquals(AccessKind.Refused, c.state.value.kind)
        assertEquals(UiMessage.Text("the server did not accept the admin key"), c.state.value.refusal)
        assertTrue(changes.isEmpty())
    }

    @Test fun aNetworkFailureIsAnErrorNotARefusal() {
        loaded()
        backend.connectFails = java.io.IOException("offline")
        c.paste("abcdefgh12345678")
        c.confirmKey()
        waitFor("error") { c.state.value.error != null }
        assertFalse(c.state.value.refused)
        assertEquals(AccessKind.Locked, c.state.value.kind)
    }

    @Test fun twoConfirmsDoNotSendTheKeyTwice() {
        loaded()
        c.paste("abcdefgh12345678")
        c.confirmKey()
        c.confirmKey()
        waitFor("open") { c.state.value.kind == AccessKind.Open }
        assertEquals(1, backend.calls.count { it == "connect" })
    }

    @Test fun forgettingAsksFirstThenLocksAgain() {
        backend.accessNow = access()
        loaded()
        c.askForget()
        assertEquals(AccessDialog.Forget, c.state.value.dialog)
        assertTrue(backend.calls.none { it == "disconnect" })
        c.forget()
        waitFor("locked") { c.state.value.kind == AccessKind.Locked && !c.state.value.busy }
        assertTrue(backend.calls.contains("disconnect"))
        assertEquals(listOf(false), changes)
    }

    @Test fun forgetIsNotOfferedWhenThereIsNoKey() {
        loaded()
        c.askForget()
        assertNull(c.state.value.dialog)
    }

    @Test fun aRefusedKeyThatIsStillStoredCanBeForgotten() {
        backend.accessNow = access(connected = false, refused = true, all = false)
        loaded()
        c.askForget()
        assertEquals(AccessDialog.Forget, c.state.value.dialog)
        backend.accessNow = access(connected = false)
        c.forget()
        waitFor("locked") { c.state.value.kind == AccessKind.Locked && !c.state.value.busy }
        assertTrue(backend.calls.contains("disconnect"))
    }

    @Test fun aRefusedKeyThatWasStoredShowsAsRefused() {
        backend.accessNow = access(connected = false, refused = true, all = false)
        loaded()
        assertEquals(AccessKind.Refused, c.state.value.kind)
    }
}
