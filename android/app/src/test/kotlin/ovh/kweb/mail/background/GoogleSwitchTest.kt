package ovh.kweb.mail.background

import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.kweb_core.WebPushRegistration
import uniffi.kweb_core.WebPushState
import uniffi.kweb_core.Exception as CoreException

private class FakeValues : KeyValues {
    val map = mutableMapOf<String, Any>()
    override fun getBoolean(key: String, default: Boolean) = map[key] as Boolean? ?: default
    override fun putBoolean(key: String, value: Boolean) { map[key] = value }
    override fun putString(key: String, value: String) { map[key] = value }
    override fun putLong(key: String, value: Long) { map[key] = value }
    override fun getLong(key: String, default: Long) = map[key] as Long? ?: default
    override fun getString(key: String) = map[key] as String?
    override fun putLongAndString(longKey: String, long: Long, stringKey: String, string: String?) {
        map[longKey] = long
        if (string == null) map.remove(stringKey) else map[stringKey] = string
    }
    override fun remove(vararg keys: String) { keys.forEach { map.remove(it) } }
}

/** A backend that records every call in [log] and answers from fields the test sets. */
private class FakeBackend(private val settings: Settings) : PushBackend {
    val log = mutableListOf<String>()
    var registration: WebPushRegistration? = WebPushRegistration("SENDER", "wp:https://mail.example/#id")
    var prepareError: Exception? = null
    var registerResult: RegistrationResult = RegistrationResult.Token("tok")
    var subscribeStates = ArrayDeque<Any>() // WebPushState or Exception, one per subscribe
    var state: WebPushState = WebPushState.Off
    var verifiedAfterPolls: Int? = null // the state turns Registered after this many polls of state()
    private var polls = 0
    var renewResult: Any = WebPushState.Registered(1, null)
    var serviceStarts = true
    var live = true
    var dropError: Exception? = null
    var clock = 1_000L
    val modesSeenAtSubscribe = mutableListOf<DeliveryMode>()
    var modeChanges = 0

    override fun prepare(): WebPushRegistration? {
        log += "prepare"
        prepareError?.let { throw it }
        return registration
    }

    override suspend fun register(registration: WebPushRegistration): RegistrationResult {
        log += "register"
        return registerResult
    }

    override suspend fun unregister(subtype: String, sender: String) {
        log += "unregister:$subtype:$sender"
    }

    override fun subscribe(token: String): WebPushState {
        log += "subscribe:$token"
        modesSeenAtSubscribe += settings.deliveryMode
        polls = 0
        val next = subscribeStates.removeFirstOrNull() ?: WebPushState.Verifying(1)
        if (next is Exception) throw next
        state = next as WebPushState
        return state
    }

    override fun state(): WebPushState {
        val after = verifiedAfterPolls
        if (after != null && ++polls >= after) state = WebPushState.Registered(9, null)
        return state
    }

    override fun renew(): WebPushState {
        log += "renew"
        val r = renewResult
        if (r is Exception) throw r
        return r as WebPushState
    }

    override fun drop() {
        log += "drop"
        dropError?.let { throw it }
    }

    override fun forget() { log += "forget" }
    override fun startService(): Boolean { log += "service"; return serviceStarts }
    override fun isLive(): Boolean = live
    override fun versionCode(): Long = 7
    override fun nowMillis(): Long = clock
    override suspend fun delay(ms: Long) { clock += ms }
    override fun modeChanged() { modeChanges++ }
}

class GoogleSwitchTest {
    private val settings = Settings(FakeValues())
    private val backend = FakeBackend(settings)
    private val switch = GoogleSwitch(settings, backend)

    private fun on() = runBlocking { switch.switchOn() }

    // ---- switching on

    @Test
    fun switchOnWithAnImmediateRegistrationGoesStraightToGoogle() {
        backend.subscribeStates += WebPushState.Registered(5, null)
        assertEquals(SwitchOutcome.Done, on())
        assertEquals(DeliveryMode.Google, settings.deliveryMode)
        assertFalse(settings.pushSetup)
        assertNull(settings.pushFallback)
        assertEquals("tok", settings.pushToken)
        assertEquals("wp:https://mail.example/#id", settings.pushSubtype)
        assertEquals("SENDER", settings.pushSender)
        assertEquals(7L, settings.pushAppVersion)
        assertEquals(listOf("prepare", "register", "subscribe:tok", "service"), backend.log)
    }

    @Test
    fun theModeStaysConnectionUntilVerifiedSoNothingIsEverInterrupted() {
        backend.verifiedAfterPolls = 3
        assertEquals(SwitchOutcome.Done, on())
        // While subscribing (and waiting) the account was still in connection mode.
        assertEquals(listOf(DeliveryMode.Connection), backend.modesSeenAtSubscribe)
        assertEquals(DeliveryMode.Google, settings.deliveryMode)
        // The service is told only after the mode changed.
        assertEquals("service", backend.log.last())
        assertEquals(1, backend.log.count { it.startsWith("subscribe") })
    }

    @Test
    fun noKeyOnTheServerStopsWithNothingChanged() {
        backend.registration = null
        val r = on()
        assertEquals(SwitchOutcome.Failed(FallbackReason.NoServerKey, null), r)
        assertEquals(DeliveryMode.Connection, settings.deliveryMode)
        assertEquals(FallbackReason.NoServerKey, settings.pushFallback?.reason)
        assertFalse(settings.pushSetup)
        assertFalse(backend.log.contains("register"))
    }

    @Test
    fun noPlayServicesStopsAndKeepsTheErrorName() {
        backend.registerResult = RegistrationResult.NoPlayServices("SecurityException")
        assertEquals(SwitchOutcome.Failed(FallbackReason.NoPlayServices, "SecurityException"), on())
        assertEquals("SecurityException", settings.pushFallback?.detail)
        assertEquals(DeliveryMode.Connection, settings.deliveryMode)
        assertFalse(backend.log.any { it.startsWith("subscribe") })
    }

    @Test
    fun aRefusedRegistrationIsReportedAsRefused() {
        backend.registerResult = RegistrationResult.Refused("INVALID_PARAMETERS")
        assertEquals(SwitchOutcome.Failed(FallbackReason.RegistrationRefused, "INVALID_PARAMETERS"), on())
    }

    @Test
    fun theServerRefusingTheSubscriptionIsRefusedAndCleanedUp() {
        backend.subscribeStates += CoreException.Invalid("the server said no")
        val r = on() as SwitchOutcome.Failed
        assertEquals(FallbackReason.RegistrationRefused, r.reason)
        assertEquals("the server said no", r.detail)
        assertCleanedUp()
    }

    @Test
    fun anUnreachableServerChangesNothingAndSaysSo() {
        backend.subscribeStates += CoreException.Temporary("offline")
        assertEquals(SwitchOutcome.Failed(FallbackReason.Unreachable, null), on())
        assertEquals(DeliveryMode.Connection, settings.deliveryMode)
        assertCleanedUp()
        backend.log.clear()
    }

    @Test
    fun unreachableAlreadyAtPrepareIsTheSame() {
        backend.prepareError = CoreException.Temporary("offline")
        assertEquals(SwitchOutcome.Failed(FallbackReason.Unreachable, null), on())
        assertEquals(DeliveryMode.Connection, settings.deliveryMode)
    }

    @Test
    fun noVerificationInNinetySecondsTriesAgainAndGivesUpAfterThree() {
        val r = on()
        assertEquals(SwitchOutcome.Failed(FallbackReason.NotVerified, null), r)
        assertEquals(3, backend.log.count { it.startsWith("subscribe") })
        // Three waits of 90 s each: the fake clock moved by that much (the one second polls).
        assertTrue(backend.clock - 1_000L >= 3 * ModeRules.VERIFY_WAIT_MS)
        assertEquals(DeliveryMode.Connection, settings.deliveryMode)
        assertEquals(FallbackReason.NotVerified, settings.pushFallback?.reason)
        assertCleanedUp()
    }

    @Test
    fun theSecondTryCanSucceed() {
        backend.subscribeStates += WebPushState.Verifying(1)
        backend.subscribeStates += WebPushState.Registered(5, null)
        assertEquals(SwitchOutcome.Done, on())
        assertEquals(2, backend.log.count { it.startsWith("subscribe") })
        assertEquals(DeliveryMode.Google, settings.deliveryMode)
    }

    private fun assertCleanedUp() {
        val log = backend.log
        val drop = log.indexOf("drop")
        val unreg = log.indexOfFirst { it.startsWith("unregister") }
        val forget = log.indexOf("forget")
        // The server first, then Play services, then the keys.
        assertTrue(drop in 0 until unreg)
        assertTrue(unreg < forget)
        assertNull(settings.pushToken)
        assertNull(settings.pushSubtype)
        assertFalse(settings.pushSetup)
    }

    // ---- switching off

    private fun google() {
        settings.deliveryMode = DeliveryMode.Google
        settings.recordPushRegistration("wp:https://mail.example/#id", "SENDER", "tok", 1, 7)
    }

    @Test
    fun switchOffStartsTheConnectionFirstThenTearsGoogleDown() {
        google()
        val r = runBlocking { switch.switchOff() }
        assertEquals(SwitchOutcome.Done, r)
        assertEquals(DeliveryMode.Connection, settings.deliveryMode)
        assertEquals(listOf("service", "drop", "unregister:wp:https://mail.example/#id:SENDER", "forget"), backend.log)
        assertNull(settings.pushToken)
    }

    @Test
    fun switchOffWaitsForTheConnectionToComeUp() {
        google()
        var asked = 0
        val b = object : PushBackend by backend {
            override fun isLive(): Boolean = ++asked > 3
        }
        assertEquals(SwitchOutcome.Done, runBlocking { GoogleSwitch(settings, b).switchOff() })
        assertTrue(asked > 3)
        assertEquals("forget", backend.log.last())
    }

    @Test
    fun switchOffWhenTheServiceCannotStartKeepsGoogleAndTearsNothingDown() {
        google()
        backend.serviceStarts = false
        assertEquals(SwitchOutcome.ConnectionNotUp, runBlocking { switch.switchOff() })
        assertEquals(DeliveryMode.Google, settings.deliveryMode)
        assertFalse(backend.log.contains("drop"))
        assertNotNull(settings.pushToken)
    }

    @Test
    fun switchOffWhenNoConnectionComesUpKeepsGoogle() {
        google()
        backend.live = false
        assertEquals(SwitchOutcome.ConnectionNotUp, runBlocking { switch.switchOff() })
        assertEquals(DeliveryMode.Google, settings.deliveryMode)
        assertFalse(backend.log.contains("drop"))
        assertTrue(backend.clock - 1_000L >= GoogleSwitch.LIVE_WAIT_MS)
    }

    @Test
    fun aFailingDropDoesNotStopTheRestOfSwitchingOff() {
        google()
        backend.dropError = CoreException.Temporary("offline")
        assertEquals(SwitchOutcome.Done, runBlocking { switch.switchOff() })
        assertTrue(backend.log.any { it.startsWith("unregister") })
        assertEquals("forget", backend.log.last())
    }

    // ---- falling back, renewing, registering again

    @Test
    fun fallingBackGoesToTheConnectionBeforeTearingDownAndKeepsTheReason() {
        google()
        runBlocking { switch.fallBack(FallbackReason.SubscriptionGone) }
        assertEquals(DeliveryMode.Connection, settings.deliveryMode)
        assertEquals("service", backend.log.first())
        assertEquals(FallbackReason.SubscriptionGone, settings.pushFallback?.reason)
        assertEquals(1_000L, settings.pushFallback?.atMillis)
        assertNull(settings.pushToken)
    }

    @Test
    fun fallingBackInTheBackgroundWithoutAServiceLeavesTheOldRoadUntilTheConnectionIsUp() {
        google()
        backend.serviceStarts = false
        runBlocking { switch.fallBack(FallbackReason.NotVerified) }
        assertEquals(DeliveryMode.Connection, settings.deliveryMode)
        assertEquals(FallbackReason.NotVerified, settings.pushFallback?.reason)
        assertFalse("no gap: nothing is torn down while no connection runs", backend.log.contains("drop"))
        assertNotNull(settings.pushSubtype)
        // The next start or check, with the service accepted and the connection up, ends it.
        backend.serviceStarts = true
        runBlocking { switch.finishCleanUp() }
        assertTrue(backend.log.containsAll(listOf("drop", "forget")))
        assertNull(settings.pushSubtype)
    }

    @Test
    fun aSwitchOffCutShortIsFinishedWhenTheConnectionIsUp() {
        // The process died after the mode went back to the connection and before the clean-up.
        settings.deliveryMode = DeliveryMode.Connection
        settings.recordPushRegistration("wp:https://mail.example/#id", "SENDER", "tok", 1, 7)
        runBlocking { switch.finishCleanUp() }
        assertEquals(listOf("service", "drop", "unregister:wp:https://mail.example/#id:SENDER", "forget"), backend.log)
        assertNull(settings.pushSubtype)
        backend.log.clear()
        runBlocking { switch.finishCleanUp() }
        assertEquals("nothing left to end", emptyList<String>(), backend.log)
    }

    @Test
    fun theCleanUpDoesNotTouchASetupThatIsUnderWay() {
        settings.deliveryMode = DeliveryMode.Connection
        settings.recordPushRegistration("wp:https://mail.example/#id", "SENDER", "tok", 1, 7)
        settings.pushSetup = true
        runBlocking { switch.finishCleanUp() }
        assertEquals(emptyList<String>(), backend.log)
    }

    @Test
    fun renewStepRegisteredDoesNothing() {
        google()
        backend.state = WebPushState.Registered(5, null)
        backend.renewResult = WebPushState.Registered(5, null)
        runBlocking { switch.renewStep() }
        assertEquals(listOf("renew"), backend.log)
        assertEquals(DeliveryMode.Google, settings.deliveryMode)
    }

    @Test
    fun renewStepGoneFallsBack() {
        google()
        backend.state = WebPushState.Registered(5, null)
        backend.renewResult = WebPushState.Gone
        runBlocking { switch.renewStep() }
        assertEquals(DeliveryMode.Connection, settings.deliveryMode)
        assertEquals(FallbackReason.SubscriptionGone, settings.pushFallback?.reason)
    }

    @Test
    fun renewStepKeyChangedRegistersAgain() {
        google()
        backend.state = WebPushState.Registered(5, null)
        backend.renewResult = WebPushState.KeyChanged
        backend.subscribeStates += WebPushState.Registered(9, null)
        runBlocking { switch.renewStep() }
        assertEquals(DeliveryMode.Google, settings.deliveryMode)
        assertTrue(backend.log.containsAll(listOf("prepare", "register", "subscribe:tok")))
        assertFalse(settings.pushSetup)
    }

    @Test
    fun renewStepErrorIsLeftToTheNextRun() {
        google()
        backend.state = WebPushState.Registered(5, null)
        backend.renewResult = CoreException.Temporary("offline")
        runBlocking { switch.renewStep() }
        assertEquals(DeliveryMode.Google, settings.deliveryMode)
        assertNull(settings.pushFallback)
    }

    @Test
    fun registeringAgainThatFailsForRealFallsBack() {
        google()
        backend.registerResult = RegistrationResult.NoPlayServices(null)
        val r = runBlocking { switch.reregister() }
        assertEquals(SwitchOutcome.Failed(FallbackReason.NoPlayServices, null), r)
        assertEquals(DeliveryMode.Connection, settings.deliveryMode)
        assertEquals(FallbackReason.NoPlayServices, settings.pushFallback?.reason)
    }

    @Test
    fun aFallbackOutOfRegisteringAgainIsAWholeOneTheServiceStartsAndTheRestIsCleanedUp() {
        google()
        backend.subscribeStates += WebPushState.Verifying(1)
        backend.subscribeStates += WebPushState.Verifying(1)
        backend.subscribeStates += WebPushState.Verifying(1)
        val r = runBlocking { switch.reregister() }
        assertEquals(SwitchOutcome.Failed(FallbackReason.NotVerified, null), r)
        assertEquals(DeliveryMode.Connection, settings.deliveryMode)
        assertEquals(FallbackReason.NotVerified, settings.pushFallback?.reason)
        assertFalse("left true, the next app start would switch on again unasked", settings.pushSetup)
        // The service is asked for, and with the connection up the subscription, the registration and the keys go.
        assertEquals(
            listOf("service", "drop", "unregister:wp:https://mail.example/#id:SENDER", "forget"),
            backend.log.dropWhile { it != "service" },
        )
        assertNull(settings.pushToken)
    }

    @Test
    fun aFallbackOutOfRegisteringAgainWithoutAConnectionKeepsTheRegistrationForTheNextCleanUp() {
        google()
        backend.registerResult = RegistrationResult.NoPlayServices(null)
        backend.serviceStarts = false
        runBlocking { switch.reregister() }
        assertEquals(DeliveryMode.Connection, settings.deliveryMode)
        assertFalse(settings.pushSetup)
        assertTrue("service" in backend.log)
        assertFalse("drop" in backend.log)
        // Later, with the connection up, the clean-up is not held back by a setup flag.
        backend.serviceStarts = true
        runBlocking { switch.finishCleanUp() }
        assertTrue("drop" in backend.log)
        assertNull(settings.pushSubtype)
    }

    @Test
    fun playServicesThatIsNotThereEndsOnTheConnectionAlthoughTheServerStillListsTheSubscription() {
        google()
        backend.state = WebPushState.Registered(5, null)
        backend.registerResult = RegistrationResult.NoPlayServices("no service", absent = true)
        runBlocking { switch.reregister() }
        assertEquals(DeliveryMode.Connection, settings.deliveryMode)
        assertEquals(FallbackReason.NoPlayServices, settings.pushFallback?.reason)
        assertEquals("no service", settings.pushFallback?.detail)
        assertFalse(settings.pushSetup)
        assertTrue("service" in backend.log)
    }

    @Test
    fun playServicesSilentOrRefusingIsLetPassTwiceAndThenTheRoadIsGivenUp() {
        for (result in listOf(RegistrationResult.NoPlayServices("no answer"), RegistrationResult.Refused("SERVICE_NOT_AVAILABLE"))) {
            val settings = Settings(FakeValues())
            val backend = FakeBackend(settings)
            val switch = GoogleSwitch(settings, backend)
            settings.deliveryMode = DeliveryMode.Google
            settings.recordPushRegistration("wp:https://mail.example/#id", "SENDER", "tok", 1, 7)
            backend.state = WebPushState.Registered(5, null)
            backend.registerResult = result
            repeat(2) {
                runBlocking { switch.reregister() }
                assertEquals(DeliveryMode.Google, settings.deliveryMode)
                assertNull(settings.pushFallback)
            }
            runBlocking { switch.reregister() }
            assertEquals(DeliveryMode.Connection, settings.deliveryMode)
            assertNotNull("the reason is kept for the Delivery check", settings.pushFallback)
            assertTrue("service" in backend.log)
        }
    }

    @Test
    fun aRegistrationThatWorksAgainForgetsTheEarlierFailures() {
        google()
        backend.state = WebPushState.Registered(5, null)
        backend.registerResult = RegistrationResult.NoPlayServices("no answer")
        repeat(2) { runBlocking { switch.reregister() } }
        backend.registerResult = RegistrationResult.Token("tok2")
        backend.subscribeStates += WebPushState.Registered(5, null)
        assertEquals(SwitchOutcome.Done, runBlocking { switch.reregister() })
        backend.registerResult = RegistrationResult.NoPlayServices("no answer")
        repeat(2) { runBlocking { switch.reregister() } }
        assertEquals(DeliveryMode.Google, settings.deliveryMode)
    }

    @Test
    fun anUnreachableServerNeverCountsAgainstPlayServices() {
        google()
        backend.state = WebPushState.Registered(5, null)
        backend.prepareError = CoreException.Temporary("offline")
        repeat(5) { runBlocking { switch.reregister() } }
        assertEquals(DeliveryMode.Google, settings.deliveryMode)
        assertNull(settings.pushFallback)
    }

    @Test
    fun registeringAgainWithAnUnreachableServerStaysInGoogleModeAndTriesLater() {
        google()
        backend.prepareError = CoreException.Temporary("offline")
        runBlocking { switch.reregister() }
        assertEquals(DeliveryMode.Google, settings.deliveryMode)
        assertTrue("the next start or check finishes it", settings.pushSetup)
        assertNotNull(settings.pushToken)
    }

    @Test
    fun aRegistrationThatPlayServicesDoesNotAnswerDoesNotTearDownARoadThatStillWorks() {
        google()
        backend.state = WebPushState.Registered(5, null)
        backend.registerResult = RegistrationResult.NoPlayServices("timeout")
        val r = runBlocking { switch.reregister() }
        assertEquals(SwitchOutcome.Failed(FallbackReason.NoPlayServices, "timeout"), r)
        assertEquals(DeliveryMode.Google, settings.deliveryMode)
        assertEquals(listOf("prepare", "register"), backend.log)
        assertNotNull(settings.pushToken)
        assertNull(settings.pushFallback)
        assertTrue(settings.pushSetup)
    }

    @Test
    fun aVerificationThatDidNotComeAndAnUnreachableServerIsRetriedByTheNextCheck() {
        google()
        backend.subscribeStates += WebPushState.Verifying(1)
        backend.subscribeStates += CoreException.Temporary("offline")
        runBlocking { switch.reregister() }
        assertEquals(DeliveryMode.Google, settings.deliveryMode)
        assertTrue(settings.pushSetup)
        // The 15-minute check: it registers again, and now it works.
        backend.log.clear()
        backend.subscribeStates += WebPushState.Registered(9, null)
        runBlocking { switch.renewStep() }
        assertTrue(backend.log.containsAll(listOf("prepare", "register", "subscribe:tok")))
        assertFalse(settings.pushSetup)
        assertEquals(DeliveryMode.Google, settings.deliveryMode)
    }

    @Test
    fun aGoogleModeAccountWithNothingSubscribedRegistersAgainAtTheNextCheck() {
        google()
        backend.state = WebPushState.Off
        backend.subscribeStates += WebPushState.Registered(9, null)
        runBlocking { switch.renewStep() }
        assertTrue(backend.log.contains("subscribe:tok"))
        assertFalse(backend.log.contains("renew"))
    }

    @Test
    fun aVerificationThatIsOverdueIsRetriedAndOneThatIsNotIsLeftAlone() {
        google()
        backend.state = WebPushState.Verifying(backend.clock / 1000 - 10)
        backend.renewResult = WebPushState.Verifying(backend.clock / 1000 - 10)
        runBlocking { switch.renewStep() }
        assertFalse("still within its wait", backend.log.contains("subscribe:tok"))
        backend.clock += ModeRules.VERIFY_WAIT_MS
        backend.subscribeStates += WebPushState.Registered(9, null)
        runBlocking { switch.renewStep() }
        assertTrue(backend.log.contains("subscribe:tok"))
    }

    @Test
    fun aSecondSwitchOnForAnAccountThatAlreadyWorksChangesNothing() {
        google()
        backend.state = WebPushState.Registered(5, null)
        assertEquals(SwitchOutcome.Done, on())
        assertEquals(DeliveryMode.Google, settings.deliveryMode)
        assertEquals(emptyList<String>(), backend.log)
    }

    @Test
    fun aSecondSwitchOnThatFailsInGoogleModeDoesNotTearDownTheAccount() {
        google()
        backend.state = WebPushState.Registered(5, null)
        backend.prepareError = CoreException.Temporary("offline")
        settings.pushSetup = true // as a first switch that was cut short left it
        on()
        assertEquals(DeliveryMode.Google, settings.deliveryMode)
        assertFalse(backend.log.contains("drop"))
        assertNotNull(settings.pushToken)
    }

    // ---- removal

    @Test
    fun removalDestroysOnTheServerThenUnregistersAndLeavesTheKeysToSignOut() {
        google()
        runBlocking { switch.tearDownForRemoval() }
        assertEquals(listOf("drop", "unregister:wp:https://mail.example/#id:SENDER"), backend.log)
    }

    @Test
    fun removalWithAFailingServerStillUnregisters() {
        google()
        backend.dropError = CoreException.Temporary("offline")
        runBlocking { switch.tearDownForRemoval() }
        assertTrue(backend.log.any { it.startsWith("unregister") })
    }
}
