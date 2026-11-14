package ovh.kweb.mail.ui.account

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
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import ovh.kweb.mail.ui.UiMessage
import uniffi.kweb_core.AccountSecurity
import uniffi.kweb_core.ConnectedApp
import uniffi.kweb_core.Credential
import uniffi.kweb_core.Exception as CoreException
import uniffi.kweb_core.Passkey

class SecurityControllerTest {
    private val pool = Executors.newCachedThreadPool()
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Unconfined)
    private lateinit var backend: FakeAccountBackend
    private lateinit var c: SecurityController

    private val appPassword = Credential("p1", "Printer", 1_700_000_000, null)
    private val key = Credential("k1", "Admin", 1_700_000_000, 1_800_000_000)
    private val phone = Passkey("s1", "Phone", null)
    private val laptop = Passkey("s2", "Laptop", null)
    private val thisApp = ConnectedApp("g1", "kweb mail", listOf("mail", "urn:stalwart:oauth:scope:full"), null, true)
    private val other = ConnectedApp("g2", "Tablet", listOf("calendars"), null, false)

    @Before fun setUp() {
        backend = FakeAccountBackend()
        backend.security = AccountSecurity(false, listOf(appPassword), listOf(key), listOf(phone, laptop), listOf(thisApp, other))
        c = SecurityController(scope, pool.asCoroutineDispatcher(), backend, clock = { 1000L })
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
        waitFor("ready") { c.state.value.phase == SecurityPhase.READY }
    }

    private fun idle() = waitFor("idle") { !c.state.value.busy }

    @Test fun loadsEverything() {
        ready()
        val s = c.state.value.security!!
        assertEquals(1, s.appPasswords.size)
        assertEquals(1, s.apiKeys.size)
        assertEquals(2, s.passkeys.size)
        assertEquals(2, s.connectedApps.size)
    }

    @Test fun notReachableSaysSoInTheCoresWords() {
        backend.failWith = CoreException.Invalid("this sign-in does not reach the security settings")
        c.start()
        waitFor("none") { c.state.value.phase == SecurityPhase.NONE }
        assertEquals(UiMessage.Text("this sign-in does not reach the security settings"), c.state.value.unsupported)
    }

    @Test fun aWrongPasswordStaysInTheFormInTheCoresWords() {
        ready()
        c.askChangePassword()
        backend.failNext = CoreException.Invalid("The current password is wrong.")
        c.changePassword("old", "new")
        idle()
        assertEquals(SecurityDialog.ChangePassword, c.state.value.dialog)
        assertEquals(UiMessage.Text("The current password is wrong."), c.state.value.dialogError)
    }

    @Test fun aChangedPasswordClosesTheFormAndSaysSo() {
        ready()
        c.askChangePassword()
        c.changePassword("old", "new")
        idle()
        waitFor("closed") { c.state.value.dialog == null }
        assertEquals(UiMessage.Res(ovh.kweb.mail.R.string.security_password_changed), c.state.value.info)
    }

    @Test fun twoStepSetupThenEnableUsesTheSecretOfTheSetup() {
        ready()
        c.startTwoStep()
        waitFor("setup") { c.state.value.dialog is SecurityDialog.TwoStep }
        c.enableTwoStep("pw", "123 456")
        idle()
        waitFor("closed") { c.state.value.dialog == null }
        assertEquals("otpauth://totp/x?secret=ABCD", backend.otpauthSeen)
        assertEquals("123456", backend.codeSeen)
        waitFor("on") { c.state.value.security?.twoStep == true }
    }

    @Test fun aWrongCodeKeepsTheSetupOpenSoTheSecretIsNotLost() {
        ready()
        c.startTwoStep()
        waitFor("setup") { c.state.value.dialog is SecurityDialog.TwoStep }
        backend.failNext = CoreException.Invalid("That code is not right.")
        c.enableTwoStep("pw", "000000")
        idle()
        assertTrue(c.state.value.dialog is SecurityDialog.TwoStep)
        assertEquals(UiMessage.Text("That code is not right."), c.state.value.dialogError)
    }

    @Test fun closingTheSetupDropsTheSecret() {
        ready()
        c.startTwoStep()
        waitFor("setup") { c.state.value.dialog is SecurityDialog.TwoStep }
        c.dismissDialog()
        assertNull(c.state.value.dialog)
    }

    @Test fun switchingOffNeedsThePasswordCall() {
        backend.security = backend.security.copy(twoStep = true)
        ready()
        c.askTwoStepOff()
        assertEquals(SecurityDialog.TwoStepOff, c.state.value.dialog)
        c.disableTwoStep("pw", "123456")
        idle()
        waitFor("off") { c.state.value.security?.twoStep == false }
    }

    @Test fun withoutTwoStepNoCodeIsSentEvenIfOneWasTyped() {
        ready()
        c.askChangePassword()
        c.changePassword("old", "new", "123456")
        idle()
        assertEquals(listOf<String?>(null), backend.codesSeen.toList())
    }

    @Test fun withTwoStepOnAPasswordChangeSendsTheCurrentCodeWithoutSpaces() {
        backend.security = backend.security.copy(twoStep = true)
        ready()
        c.askChangePassword()
        c.changePassword("old", "new", "123 456")
        idle()
        assertEquals(listOf<String?>("123456"), backend.codesSeen.toList())
    }

    @Test fun withTwoStepOnAChangeWithoutACompleteCodeIsNotSent() {
        backend.security = backend.security.copy(twoStep = true)
        ready()
        c.askChangePassword()
        c.changePassword("old", "new", "12")
        c.changePassword("old", "new")
        c.askTwoStepOff()
        c.disableTwoStep("pw", "abcdef")
        idle()
        assertTrue(backend.calls.none { it == "password" || it == "disableTwoStep" })
        assertNotNull(c.state.value.dialog)
    }

    @Test fun withTwoStepOnSwitchingOffSendsTheCurrentCode() {
        backend.security = backend.security.copy(twoStep = true)
        ready()
        c.askTwoStepOff()
        c.disableTwoStep("pw", "654 321")
        idle()
        waitFor("off") { c.state.value.security?.twoStep == false }
        assertEquals(listOf<String?>("654321"), backend.codesSeen.toList())
    }

    @Test fun replacingTheSecretNeedsTheOldCodeAndKeepsTheNewCodeApart() {
        backend.security = backend.security.copy(twoStep = true)
        ready()
        c.startTwoStep()
        waitFor("setup") { c.state.value.dialog is SecurityDialog.TwoStep }
        c.enableTwoStep("pw", "111 111")
        idle()
        assertTrue(c.state.value.dialog is SecurityDialog.TwoStep)
        assertTrue(backend.calls.none { it == "enableTwoStep" })
        c.enableTwoStep("pw", "111 111", "222 222")
        idle()
        waitFor("closed") { c.state.value.dialog == null }
        assertEquals("111111", backend.codeSeen)
        assertEquals(listOf<String?>("222222"), backend.codesSeen.toList())
    }

    @Test fun settingUpTwoStepTheFirstTimeSendsNoOldCode() {
        ready()
        c.startTwoStep()
        waitFor("setup") { c.state.value.dialog is SecurityDialog.TwoStep }
        c.enableTwoStep("pw", "111 111")
        idle()
        waitFor("closed") { c.state.value.dialog == null }
        assertEquals(listOf<String?>(null), backend.codesSeen.toList())
    }

    @Test fun anAppPasswordIsShownOnceAndGoesWithTheDialog() {
        ready()
        c.askNewCredential(CredentialKind.APP_PASSWORD)
        c.createCredential(CredentialKind.APP_PASSWORD, " Printer ", Expiry.DAYS_30)
        waitFor("secret") { c.state.value.dialog is SecurityDialog.Secret }
        assertEquals("app-secret", (c.state.value.dialog as SecurityDialog.Secret).secret.secret)
        assertEquals(1000 + 30 * 86_400L, backend.lastExpires)
        assertTrue(backend.calls.contains("createAppPassword:Printer"))
        c.dismissDialog()
        assertNull(c.state.value.dialog)
    }

    @Test fun anApiKeyWithoutAnEndDateSendsNone() {
        ready()
        c.askNewCredential(CredentialKind.API_KEY)
        c.createCredential(CredentialKind.API_KEY, "", Expiry.NEVER)
        waitFor("secret") { c.state.value.dialog is SecurityDialog.Secret }
        assertNull(backend.lastExpires)
        assertTrue(backend.calls.contains("createApiKey:"))
    }

    @Test fun theLimitOfKeysIsShownInTheForm() {
        ready()
        c.askNewCredential(CredentialKind.API_KEY)
        backend.failNext = CoreException.Invalid("The server allows 5 API keys.")
        c.createCredential(CredentialKind.API_KEY, "x", Expiry.NEVER)
        idle()
        assertEquals(UiMessage.Text("The server allows 5 API keys."), c.state.value.dialogError)
        assertTrue(c.state.value.dialog is SecurityDialog.NewCredential)
    }

    @Test fun deletingACredentialAsksFirst() {
        ready()
        c.askDeleteCredential(CredentialKind.API_KEY, key)
        assertFalse(backend.calls.any { it.startsWith("delete") })
        c.deleteCredential()
        idle()
        assertTrue(backend.calls.contains("deleteApiKey:k1"))
        assertNull(c.state.value.dialog)
    }

    @Test fun deletingAnAppPasswordCallsTheRightOne() {
        ready()
        c.askDeleteCredential(CredentialKind.APP_PASSWORD, appPassword)
        c.deleteCredential()
        idle()
        assertTrue(backend.calls.contains("deleteAppPassword:p1"))
    }

    @Test fun renamingAPasskeySendsTheTrimmedName() {
        ready()
        c.askRenamePasskey(phone)
        c.renamePasskey(" Work phone ")
        idle()
        assertTrue(backend.calls.contains("rename:s1:Work phone"))
    }

    @Test fun anEmptyNameIsNotSent() {
        ready()
        c.askRenamePasskey(phone)
        c.renamePasskey("  ")
        assertFalse(backend.calls.any { it.startsWith("rename") })
    }

    @Test fun theLastPasskeyIsWarnedAbout() {
        backend.security = backend.security.copy(passkeys = listOf(phone))
        ready()
        c.askDeletePasskey(phone)
        assertEquals(true, (c.state.value.dialog as SecurityDialog.DeletePasskey).last)
    }

    @Test fun oneOfTwoPasskeysIsNotTheLast() {
        ready()
        c.askDeletePasskey(phone)
        assertEquals(false, (c.state.value.dialog as SecurityDialog.DeletePasskey).last)
        c.deletePasskey()
        idle()
        assertTrue(backend.calls.contains("deletePasskey:s1"))
    }

    @Test fun thisAppCannotBeRevoked() {
        ready()
        c.askRevoke(thisApp)
        assertNull(c.state.value.dialog)
        assertFalse(backend.calls.any { it.startsWith("revoke") })
    }

    @Test fun anotherAppIsRevokedAfterAsking() {
        ready()
        c.askRevoke(other)
        assertTrue(c.state.value.dialog is SecurityDialog.Revoke)
        c.revoke()
        idle()
        assertTrue(backend.calls.contains("revoke:g2"))
    }

    @Test fun aFailureWithoutADialogGoesOntoTheScreen() {
        ready()
        backend.failNext = java.io.IOException("x")
        c.startTwoStep()
        idle()
        waitFor("error") { c.state.value.error != null }
        assertNull(c.state.value.dialog)
    }

    @Test fun theDialogCannotBeClosedWhileAChangeIsOnItsWay() {
        ready()
        c.askChangePassword()
        val release = CountDownLatch(1)
        backend.gate = { release.await(5, TimeUnit.SECONDS) }
        c.changePassword("a", "b")
        waitFor("busy") { c.state.value.busy }
        c.dismissDialog()
        assertEquals(SecurityDialog.ChangePassword, c.state.value.dialog)
        release.countDown()
        idle()
    }

    @Test fun onlyOneChangeAtATime() {
        ready()
        c.askChangePassword()
        val release = CountDownLatch(1)
        backend.gate = { release.await(5, TimeUnit.SECONDS) }
        c.changePassword("a", "b")
        waitFor("busy") { c.state.value.busy }
        c.changePassword("a", "b")
        release.countDown()
        idle()
        assertEquals(1, backend.calls.count { it == "password" })
    }

    @Test fun aLateReadDoesNotReplaceANewerOne() {
        val first = CountDownLatch(1)
        val entered = CountDownLatch(1)
        var slow = true
        backend.gate = { if (slow) { slow = false; entered.countDown(); first.await(5, TimeUnit.SECONDS) } }
        c.start()
        entered.await(5, TimeUnit.SECONDS)
        backend.security = backend.security.copy(twoStep = true)
        c.load()
        waitFor("newer") { c.state.value.security?.twoStep == true }
        first.countDown()
        Thread.sleep(100)
        assertTrue(c.state.value.security!!.twoStep)
    }

    @Test fun showsSecretOnlyForTheDialogsThatHoldOne() {
        assertTrue(SecurityDialog.ChangePassword.showsSecret)
        assertTrue(SecurityDialog.TwoStepOff.showsSecret)
        assertTrue(SecurityDialog.Secret(CredentialKind.API_KEY, uniffi.kweb_core.NewSecret("1", "s")).showsSecret)
        assertFalse(SecurityDialog.Revoke(other).showsSecret)
        assertFalse(SecurityDialog.DeletePasskey(phone, false).showsSecret)
    }

    @Test fun dismissAllDropsAShownSecret() {
        ready()
        c.askNewCredential(CredentialKind.APP_PASSWORD)
        c.createCredential(CredentialKind.APP_PASSWORD, "x", Expiry.NEVER)
        waitFor("secret") { c.state.value.dialog is SecurityDialog.Secret }
        c.dismissAll()
        assertNull(c.state.value.dialog)
    }

    @Test fun dismissAllWhileACreateIsOnItsWayShowsNoSecretAfterwards() {
        ready()
        val release = CountDownLatch(1)
        backend.secretGate = { release.await(5, TimeUnit.SECONDS) }
        c.askNewCredential(CredentialKind.APP_PASSWORD)
        c.createCredential(CredentialKind.APP_PASSWORD, "x", Expiry.NEVER)
        assertTrue(c.state.value.busy)
        c.dismissAll()
        release.countDown()
        waitFor("call made") { backend.calls.contains("createAppPassword:x") }
        Thread.sleep(200)
        assertNull(c.state.value.dialog)
        assertFalse(c.state.value.busy)
    }
}
