package ovh.kweb.mail.ui.admin

import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.UiMessage
import uniffi.kweb_core.AuthResult
import uniffi.kweb_core.Exception as CoreException
import uniffi.kweb_core.Maintenance
import uniffi.kweb_core.SpamTag

class ActionsControllerTest {
    private val rig = Rig()
    private lateinit var c: ActionsController

    @Before fun setUp() {
        rig.backend.accessNow = access()
        c = ActionsController(rig.scope, rig.io, rig.backend)
        c.start()
        waitFor("access") { c.state.value.phase == ActionsPhase.READY }
    }

    @After fun tearDown() = rig.stop()

    @Test fun theScreenKnowsWhatTheKeyMayStart() {
        assertEquals(Maintenance.entries.toList(), c.state.value.allowed)
        assertTrue(c.state.value.mayDmarc && c.state.value.maySpam)
    }

    @Test fun anActionAsksFirst() {
        c.ask(Maintenance.INVALIDATE_CACHES)
        assertEquals(Maintenance.INVALIDATE_CACHES, c.state.value.asking)
        assertTrue(rig.backend.calls.none { it.startsWith("action") })
        c.dismissDialog()
        c.confirm()
        assertTrue(rig.backend.calls.none { it.startsWith("action") })
    }

    @Test fun confirmingRunsItOnceAndSaysSo() {
        c.ask(Maintenance.RELOAD_CERTIFICATES)
        c.confirm()
        c.confirm()
        waitFor("done") { c.state.value.info != null && c.state.value.running == null }
        assertEquals(1, rig.backend.calls.count { it == "action:${Maintenance.RELOAD_CERTIFICATES}" })
        assertEquals(UiMessage.Res(R.string.admin_reload_certs_done), c.state.value.info)
        assertTrue(c.state.value.warnings.isEmpty())
    }

    @Test fun theWarningsOfAReloadAreShown() {
        rig.backend.actionWarnings = listOf("Licence key is not valid")
        c.ask(Maintenance.RELOAD_SETTINGS)
        c.confirm()
        waitFor("done") { c.state.value.info != null }
        assertEquals(listOf("Licence key is not valid"), c.state.value.warnings)
        c.dismissInfo()
        assertTrue(c.state.value.warnings.isEmpty())
        // The next question clears what the last run said.
        rig.backend.actionWarnings = emptyList()
        c.ask(Maintenance.UPDATE_APPS)
        assertTrue(c.state.value.warnings.isEmpty() && c.state.value.info == null)
    }

    @Test fun anActionTheKeyMayNotStartIsNotAskedAbout() {
        rig.backend.accessNow = access().copy(allowedActions = listOf(Maintenance.PAUSE_QUEUE))
        c.start()
        waitFor("fewer") { c.state.value.allowed.size == 1 }
        c.ask(Maintenance.RELOAD_SETTINGS)
        assertNull(c.state.value.asking)
        c.confirm()
        assertTrue(rig.backend.calls.none { it.startsWith("action") })
        c.ask(Maintenance.PAUSE_QUEUE)
        assertEquals(Maintenance.PAUSE_QUEUE, c.state.value.asking)
    }

    @Test fun aRefusalIsShownInTheCoresWords() {
        rig.backend.failWith = CoreException.Invalid("this key may not run that action")
        c.ask(Maintenance.RELOAD_SETTINGS)
        c.confirm()
        waitFor("error") { c.state.value.error != null }
        assertEquals(UiMessage.Text("this key may not run that action"), c.state.value.error)
        assertNull(c.state.value.running)
    }

    @Test fun theGroupsFollowThePermissions() {
        val all = ServerActionRules.visible(Maintenance.entries.toList())
        assertEquals(listOf(5, 2, 2), all.map { it.actions.size })
        assertEquals(Maintenance.entries.toSet(), all.flatMap { it.actions }.toSet())
        val few = ServerActionRules.visible(listOf(Maintenance.INVALIDATE_CACHES, Maintenance.RESUME_QUEUE))
        assertEquals(listOf(listOf(Maintenance.INVALIDATE_CACHES), listOf(Maintenance.RESUME_QUEUE)), few.map { it.actions })
        assertTrue(ServerActionRules.visible(emptyList()).isEmpty())
    }
}

class DmarcCheckControllerTest {
    private val rig = Rig()
    private lateinit var c: DmarcCheckController

    @Before fun setUp() {
        c = DmarcCheckController(rig.scope, rig.io, rig.backend)
    }

    @After fun tearDown() = rig.stop()

    @Test fun itDoesNotRunWithoutTheRequiredFields() {
        c.edit { it.copy(remoteIp = "192.0.2.1", ehloDomain = "mail.example.org") }
        c.run()
        assertTrue(rig.backend.calls.none { it == "dmarc" })
    }

    @Test fun theSpfDomainsFollowTheOtherFieldsWhenLeftBlank() {
        c.edit { it.copy(remoteIp = " 192.0.2.1 ", ehloDomain = "mail.example.org", mailFrom = "a@example.org", rcptTo = "b@example.net") }
        c.run()
        waitFor("result") { c.state.value.result != null }
        val sent = rig.backend.lastDmarc!!
        assertEquals("192.0.2.1", sent.remoteIp)
        assertEquals("mail.example.org", sent.spfEhloDomain)
        assertEquals("example.org", sent.spfMailFromDomain)
        assertEquals("b@example.net", sent.rcptTo)
        assertEquals("", sent.message)
        assertFalse(c.state.value.running)
    }

    @Test fun typedSpfDomainsAreKept() {
        c.edit { it.copy(remoteIp = "192.0.2.1", ehloDomain = "e.org", mailFrom = "a@example.org", spfEhloDomain = "x.org", spfMailFromDomain = "y.org") }
        assertEquals("x.org" to "y.org", c.state.value.form.toCheck().let { it.spfEhloDomain to it.spfMailFromDomain })
    }

    @Test fun aRefusalIsShownAndTheFormKeepsWhatWasTyped() {
        c.edit { it.copy(remoteIp = "nope", ehloDomain = "e.org", mailFrom = "a@example.org") }
        rig.backend.failWith = CoreException.Invalid("the remote IP address is not valid")
        c.run()
        waitFor("error") { c.state.value.error != null }
        assertEquals(UiMessage.Text("the remote IP address is not valid"), c.state.value.error)
        assertEquals("nope", c.state.value.form.remoteIp)
        assertNull(c.state.value.result)
    }

    @Test fun theResultIsKept() {
        rig.backend.dmarcResult = rig.backend.dmarcResult.copy(dmarc = AuthResult("fail", false, "Policy not aligned"), dmarcPass = false, dmarcPolicy = "reject")
        c.edit { it.copy(remoteIp = "192.0.2.1", ehloDomain = "e.org", mailFrom = "a@example.org") }
        c.run()
        waitFor("result") { c.state.value.result != null }
        assertEquals("Policy not aligned", c.state.value.result!!.dmarc.details)
        assertEquals(R.string.admin_dmarc_policy_reject, ServerActionRules.policyWords(c.state.value.result!!.dmarcPolicy))
    }
}

class SpamCheckControllerTest {
    private val rig = Rig()
    private lateinit var c: SpamCheckController

    @Before fun setUp() {
        c = SpamCheckController(rig.scope, rig.io, rig.backend)
    }

    @After fun tearDown() = rig.stop()

    private fun fill() = c.edit {
        it.copy(message = "Subject: x\n\nhi", remoteIp = "192.0.2.1", ehloDomain = "e.org", mailFrom = "a@example.org", rcptTo = "b@example.net")
    }

    @Test fun itNeedsTheMessageAndEveryEnvelopeField() {
        c.edit { it.copy(remoteIp = "192.0.2.1", ehloDomain = "e.org", mailFrom = "a@example.org", rcptTo = "b@example.net") }
        c.run()
        assertTrue(rig.backend.calls.none { it == "spam" })
        fill()
        assertTrue(c.state.value.form.canRun)
    }

    @Test fun theRequestCarriesTheSwitchAndTheLogin() {
        fill()
        c.edit { it.copy(isTls = true, authenticatedAs = " tester ") }
        c.run()
        waitFor("result") { c.state.value.result != null }
        val sent = rig.backend.lastSpam!!
        assertTrue(sent.isTls)
        assertEquals("tester", sent.authenticatedAs)
        assertEquals("Subject: x\n\nhi", sent.message)
        assertEquals(7.5, c.state.value.result!!.score, 0.0)
    }

    @Test fun aRefusalIsShown() {
        fill()
        rig.backend.failWith = CoreException.Invalid("this key may not run that action")
        c.run()
        waitFor("error") { c.state.value.error != null }
        assertEquals(UiMessage.Text("this key may not run that action"), c.state.value.error)
        assertFalse(c.state.value.running)
    }

    @Test fun onlyTagsThatMatteredAreListed() {
        val tags = listOf(
            SpamTag("X_HDR_FROM", 0.0, "reject"),
            SpamTag("SPF_FAIL", 3.5, "score"),
            SpamTag("DKIM_NA", 0.0, "score"),
            SpamTag("TINY", 0.01, "score"),
        )
        assertEquals(listOf("X_HDR_FROM", "SPF_FAIL"), ServerActionRules.mattering(tags).map { it.name })
        assertEquals("+3.5", ServerActionRules.tagScoreText(3.5))
        assertEquals("-1.0", ServerActionRules.tagScoreText(-1.0))
        assertEquals("14.4", ServerActionRules.scoreText(14.40000057220459))
        assertTrue(ServerActionRules.verdictGood("ham"))
        assertFalse(ServerActionRules.verdictGood("spam"))
    }
}
