package ovh.kweb.mail.ui.delivery

import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.async
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import ovh.kweb.mail.R
import ovh.kweb.mail.background.DeliveryMode
import ovh.kweb.mail.background.FallbackReason
import ovh.kweb.mail.background.ModeRules
import ovh.kweb.mail.background.SwitchOutcome
import uniffi.kweb_core.WebPushState

class GoogleModeControllerTest {
    private fun controller(on: SwitchOutcome = SwitchOutcome.Done, off: SwitchOutcome = SwitchOutcome.Done) =
        GoogleModeController({ on }, { off })

    @Test
    fun theOutcomesBecomeResults() = runBlocking {
        assertEquals(GoogleModeResult.SwitchedOn, controller().turnOn())
        assertEquals(GoogleModeResult.SwitchedOff, controller().turnOff())
        assertEquals(GoogleModeResult.Failed(FallbackReason.NoServerKey), controller(on = SwitchOutcome.Failed(FallbackReason.NoServerKey, null)).turnOn())
        assertEquals(GoogleModeResult.ConnectionNotUp, controller(off = SwitchOutcome.ConnectionNotUp).turnOff())
    }

    @Test
    fun onlyOneSwitchRunsAtATime() = runBlocking {
        val gate = CompletableDeferred<SwitchOutcome>()
        val c = GoogleModeController({ gate.await() }, { SwitchOutcome.Done })
        val first = async(Dispatchers.Default) { c.turnOn() }
        while (!c.busy.value) Thread.yield()
        assertEquals(GoogleModeResult.Busy, c.turnOff())
        gate.complete(SwitchOutcome.Done)
        assertEquals(GoogleModeResult.SwitchedOn, first.await())
        assertFalse(c.busy.value)
    }

    @Test
    fun busyIsClearedAfterAFailure() = runBlocking {
        val c = GoogleModeController({ error("x") }, { SwitchOutcome.Done })
        runCatching { c.turnOn() }
        assertFalse(c.busy.value)
    }

    @Test
    fun everyResultHasItsTextAndBusyHasNone() {
        assertNull(PushTexts.result(GoogleModeResult.Busy))
        assertEquals(R.string.push_done_on, PushTexts.result(GoogleModeResult.SwitchedOn))
        for (reason in FallbackReason.entries) {
            assertNotNull(PushTexts.result(GoogleModeResult.Failed(reason)))
            PushTexts.reason(reason)
        }
        assertEquals(R.string.push_failed_connection, PushTexts.result(GoogleModeResult.ConnectionNotUp))
    }

    @Test
    fun onlyARegisteredGoogleAccountCountsAsWorking() {
        val reg = ModeRules.line(WebPushState.Registered(1, null))
        assertTrue(PushTexts.working(PushView(DeliveryMode.Google, reg)))
        assertFalse(PushTexts.working(PushView(DeliveryMode.Google, ModeRules.Line.Verifying(1))))
        assertFalse(PushTexts.working(PushView(DeliveryMode.Connection, reg)))
    }
}
