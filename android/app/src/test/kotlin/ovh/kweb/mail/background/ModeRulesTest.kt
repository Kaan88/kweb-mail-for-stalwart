package ovh.kweb.mail.background

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import ovh.kweb.mail.accounts.AccountRecord
import ovh.kweb.mail.accounts.AccountStatus
import uniffi.kweb_core.ConnectionStatus
import uniffi.kweb_core.WebPushState
import uniffi.kweb_core.WorkWaiting

class ModeRulesTest {
    private val C = DeliveryMode.Connection
    private val G = DeliveryMode.Google

    // Decision 5: which accounts in which mode -> service runs or not.
    @Test
    fun theServiceRunsOnlyWhenSomeAccountIsInConnectionMode() {
        assertFalse(ModeRules.needsService(emptyList()))
        assertFalse(ModeRules.needsService(listOf(G)))
        assertFalse(ModeRules.needsService(listOf(G, G)))
        assertTrue(ModeRules.needsService(listOf(C)))
        assertTrue(ModeRules.needsService(listOf(G, C)))
        assertTrue(ModeRules.needsService(listOf(C, C)))
    }

    @Test
    fun theServiceStartsOnlyTheConnectionModeAccounts() {
        val accounts = listOf("a" to C, "b" to G, "c" to C)
        assertEquals(listOf("a", "c"), ModeRules.connectionOnly(accounts) { it.second }.map { it.first })
        assertTrue(ModeRules.connectionOnly(listOf("b" to G)) { it.second }.isEmpty())
    }

    private fun record(id: String) = AccountRecord(id, false, "u", "https://mail.example", "j", 0)

    @Test
    fun theEntryAndTheSummaryLeaveOutAGoogleModeAccount() {
        val items = listOf(
            AccountStatus(record("a"), ConnectionStatus.Offline("x")),
            AccountStatus(record("b"), ConnectionStatus.Offline("x"), google = true),
        )
        assertEquals(listOf("a"), ModeRules.serviceItems(items).map { it.record.id })
        assertTrue(ModeRules.serviceItems(items.drop(1)).isEmpty())
    }

    @Test
    fun modeParsingDefaultsToTheConnection() {
        assertEquals(C, DeliveryMode.parse(null))
        assertEquals(C, DeliveryMode.parse("anything"))
        assertEquals(G, DeliveryMode.parse("google"))
        assertEquals(FallbackReason.NotVerified, FallbackReason.parse("not_verified"))
        assertNull(FallbackReason.parse("x"))
    }

    private val day = 24 * 3600 * 1000L
    private fun needs(
        mode: DeliveryMode = G,
        token: String? = "t",
        tokenAt: Long = 100 * day,
        stored: Long = 5,
        current: Long = 5,
        now: Long = 101 * day,
        state: WebPushState = WebPushState.Registered(expires = 1, lastMessageAt = null),
    ) = ModeRules.needsRegistration(mode, token, tokenAt, stored, current, now, state)

    @Test
    fun aFreshRegistrationIsLeftAlone() = assertFalse(needs())

    @Test
    fun registerAgainWhenTheTokenIsMissingTheVersionChangedThirtyDaysPassedOrTheKeyChanged() {
        assertTrue(needs(token = null))
        assertTrue(needs(token = ""))
        assertTrue(needs(current = 6))
        assertTrue(needs(now = 130 * day))
        assertFalse(needs(now = 129 * day + day - 1))
        assertTrue(needs(state = WebPushState.KeyChanged))
    }

    @Test
    fun aConnectionModeAccountNeverRegisters() = assertFalse(needs(mode = C, token = null))

    @Test
    fun theLineFollowsTheState() {
        assertEquals(ModeRules.Line.Off, ModeRules.line(WebPushState.Off))
        assertEquals(ModeRules.Line.Verifying(5), ModeRules.line(WebPushState.Verifying(5)))
        assertEquals(ModeRules.Line.Registered(9, 3), ModeRules.line(WebPushState.Registered(9, 3)))
        assertEquals(ModeRules.Line.Gone, ModeRules.line(WebPushState.Gone))
        assertEquals(ModeRules.Line.KeyChanged, ModeRules.line(WebPushState.KeyChanged))
    }

    @Test
    fun threeTriesThenGiveUp() {
        assertEquals(ModeRules.VerifyNext.TryAgain, ModeRules.afterWaiting(1))
        assertEquals(ModeRules.VerifyNext.TryAgain, ModeRules.afterWaiting(2))
        assertEquals(ModeRules.VerifyNext.GiveUp, ModeRules.afterWaiting(3))
    }

    // Decision 4: the due check.
    @Test
    fun theEarliestOwedWorkWins() {
        assertNull(DueRules.earliest(emptyList()))
        assertNull(DueRules.earliest(listOf(WorkWaiting.Nothing)))
        assertEquals(0L, DueRules.earliest(listOf(WorkWaiting.Due(5_000UL), WorkWaiting.Nothing, WorkWaiting.Due(0UL))))
        assertEquals(7L, DueRules.earliest(listOf(WorkWaiting.Due(9UL), WorkWaiting.Due(7UL))))
    }

    @Test
    fun upToAMinuteAheadItIsExpeditedAndWaitsInTheWorker() {
        assertEquals(DuePlan.Expedited(0), DueRules.plan(0))
        assertEquals(DuePlan.Expedited(10_000), DueRules.plan(10_000))
        assertEquals(DuePlan.Expedited(60_000), DueRules.plan(60_000))
        assertEquals(DuePlan.Delayed(60_001), DueRules.plan(60_001))
        assertEquals(DuePlan.Delayed(3_600_000), DueRules.plan(3_600_000))
    }

    @Test
    fun workStillOwedAfterAPassIsNeverNowSoItCannotLoop() {
        assertEquals(DuePlan.Delayed(60_000), DueRules.plan(0, fromWorker = true))
        assertEquals(DuePlan.Delayed(120_000), DueRules.plan(120_000, fromWorker = true))
    }

    @Test
    fun theRetriesOfOwedWorkBackOffAndEnd() {
        assertEquals(DuePlan.Delayed(60_000), DueRules.plan(0, fromWorker = true, attempt = 0))
        assertEquals(DuePlan.Delayed(120_000), DueRules.plan(0, fromWorker = true, attempt = 1))
        assertEquals(DuePlan.Delayed(240_000), DueRules.plan(0, fromWorker = true, attempt = 2))
        assertEquals(DuePlan.Delayed(480_000), DueRules.plan(0, fromWorker = true, attempt = 3))
        assertNull("then the 15-minute check carries on", DueRules.plan(0, fromWorker = true, attempt = 4))
        // Asked by the app, not by a worker, it is never cut off.
        assertEquals(DuePlan.Expedited(0), DueRules.plan(0, fromWorker = false, attempt = 9))
    }

    @Test
    fun workThatCameInWhileAPassRanGetsOnePassAtOnceAndOnlyOne() {
        // The check after an action went well, and a second action was queued while it ran.
        assertEquals(DuePlan.Expedited(0), DueRules.plan(0, fromWorker = true, attempt = 0, fresh = true))
        // The pass that follows is one for owed work (not fresh): what it still leaves backs off as before.
        assertEquals(DuePlan.Delayed(60_000), DueRules.plan(0, fromWorker = true, attempt = 0, fresh = false))
        // A real time ahead is kept either way.
        assertEquals(DuePlan.Expedited(5_000), DueRules.plan(5_000, fromWorker = true, fresh = true))
    }

    @Test
    fun registeringAgainFallsBackOnADeadRoadAndOnTheThirdSilenceOfPlayServices() {
        val f = ModeRules::fallsBack
        assertFalse(f(FallbackReason.Unreachable, false, false, 9))
        assertTrue(f(FallbackReason.NotVerified, false, true, 0))
        assertTrue(f(FallbackReason.NoServerKey, false, true, 0))
        assertTrue("Play services is not there", f(FallbackReason.NoPlayServices, true, true, 1))
        assertTrue("nothing subscribed that works", f(FallbackReason.RegistrationRefused, false, false, 1))
        assertFalse(f(FallbackReason.NoPlayServices, false, true, 1))
        assertFalse(f(FallbackReason.RegistrationRefused, false, true, 2))
        assertTrue(f(FallbackReason.RegistrationRefused, false, true, 3))
    }

    @Test
    fun aSendTimeAFewSecondsAwayIsNotAFailureAndKeepsItsRealTimeAlsoFromAWorker() {
        assertEquals(DuePlan.Expedited(5_000), DueRules.plan(5_000, fromWorker = true))
        assertEquals(DuePlan.Expedited(60_000), DueRules.plan(60_000, fromWorker = true))
    }

    @Test
    fun aGoogleModeAccountWithNothingSubscribedOrAnOverdueVerificationNeedsRegistering() {
        assertTrue(ModeRules.isStuck(WebPushState.Off, 0))
        assertFalse(ModeRules.isStuck(WebPushState.Verifying(100), 100_000 + 1000))
        assertTrue(ModeRules.isStuck(WebPushState.Verifying(100), 100_000 + ModeRules.VERIFY_WAIT_MS))
        assertFalse(ModeRules.isStuck(WebPushState.Registered(5, null), 0))
        assertFalse(ModeRules.isStuck(WebPushState.Gone, 0))
    }

    @Test
    fun aWorkerNeverEnqueuesItsOwnName() {
        assertEquals("kweb.check.due", DueRules.workName(0))
        assertEquals("kweb.check.due.b", DueRules.workName(1))
        assertEquals(1, DueRules.otherSlot(0))
        assertEquals(0, DueRules.otherSlot(1))
        for (slot in 0 until DueRules.SLOTS) assertTrue(DueRules.workName(slot) != DueRules.workName(DueRules.otherSlot(slot)))
    }

    @Test
    fun theWorkerWaitsAtMostTheWindow() {
        assertEquals(0, DueRules.waitLeft(1000, 2000))
        assertEquals(500, DueRules.waitLeft(1500, 1000))
        assertEquals(60_000, DueRules.waitLeft(10_000_000, 1000))
    }
}
