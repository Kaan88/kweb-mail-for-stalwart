package ovh.kweb.mail.ui.transfer

import java.util.Locale
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.kweb_core.Exception as CoreException

class TransfersTest {
    @Test fun aSecondTapOnTheSameFileStartsNothing() {
        val t = ActiveTransfer()
        val first = t.begin("blob", 100)
        assertNotNull(first)
        assertNull("one at a time, so no second transfer from zero", t.begin("blob", 100))
        assertNull(t.begin("other", null))
        t.finish(first!!)
        assertNull(t.state.value)
        assertNotNull(t.begin("blob", 100))
    }

    @Test fun progressIsKeptForTheRunThatReportsIt() {
        val t = ActiveTransfer()
        val run = t.begin("b", 1000)!!
        assertEquals(ActiveDownload("b", DownloadProgress(0, 1000)), t.state.value)
        t.report(run, 250, null)
        assertEquals(DownloadProgress(250, 1000), t.state.value!!.progress)
        t.report(run, 500, 900)
        assertEquals(DownloadProgress(500, 900), t.state.value!!.progress)
    }

    @Test fun anOldRunCannotEndOrChangeTheNewOne() {
        val t = ActiveTransfer()
        val old = t.begin("b", null)!!
        t.finish(old)
        val next = t.begin("b", null)!!
        t.report(old, 99, 99)
        t.finish(old)
        assertEquals(DownloadProgress(0, null), t.state.value!!.progress)
        t.finish(next)
        assertNull(t.state.value)
    }

    @Test fun cancellingStopsTheCoreCallAndTheRunEndsItself() {
        val t = ActiveTransfer()
        val run = t.begin("b", null)!!
        var stopped = 0
        run.stop.attach { stopped++ }
        t.cancel()
        assertEquals(1, stopped)
        assertTrue(run.stop.stopped)
        // The row stays until the core call returns and finishes it.
        assertNotNull(t.state.value)
        t.finish(run)
        assertNull(t.state.value)
    }

    @Test fun aStopThatCameFirstReachesTheCallThatStartsLater() {
        val stop = DownloadStop()
        stop.stop()
        var stopped = 0
        stop.attach { stopped++ }
        assertEquals(1, stopped)
    }

    @Test fun aStopNeverReachesAHookThatWasTakenAway() {
        val stop = DownloadStop()
        var stopped = 0
        stop.attach { stopped++ }
        stop.attach(null)
        stop.stop()
        assertEquals(0, stopped)
    }

    @Test fun cancellingWithNothingRunningDoesNothing() {
        ActiveTransfer().cancel()
    }

    // ---- rules

    @Test fun theBarIsDeterminateOnlyWhereTheSizeIsKnown() {
        assertEquals(0.5f, DownloadRules.fraction(DownloadProgress(50, 100))!!, 0.0001f)
        assertNull(DownloadRules.fraction(DownloadProgress(50, null)))
        assertNull(DownloadRules.fraction(DownloadProgress(50, 0)))
        assertEquals(1f, DownloadRules.fraction(DownloadProgress(120, 100))!!, 0.0001f)
        assertEquals(0f, DownloadRules.fraction(DownloadProgress(0, 100))!!, 0.0001f)
    }

    @Test fun theLineSaysHowMuchOfHowMuchWhenItKnows() {
        val of = { a: String, b: String -> "$a of $b" }
        assertEquals("12.3 MB of 45.0 MB", DownloadRules.line(DownloadProgress(12_300_000, 45_000_000), Locale.US, of))
        assertEquals("12.3 MB", DownloadRules.line(DownloadProgress(12_300_000, null), Locale.US, of))
    }

    @Test fun onlyAPositiveSizeIsExpected() {
        assertEquals(5L, DownloadRules.expected(5))
        assertNull(DownloadRules.expected(0))
        assertNull(DownloadRules.expected(null))
    }

    @Test fun offlineIsSaidOnlyWhenTheServerCouldNotBeReached() {
        assertEquals(DownloadFailure.Offline, DownloadRules.describe(CoreException.Temporary("download: could not reach the server (connection refused or no route)")))
        assertEquals(DownloadFailure.Offline, DownloadRules.describe(CoreException.Temporary("Transport error: error sending request for url (http://x/.well-known/jmap)")))
    }

    @Test fun otherFailuresSayWhatHappened() {
        assertEquals(DownloadFailure.Words("the server answered 503"), DownloadRules.describe(CoreException.Temporary("download: the server answered 503")))
        assertEquals(DownloadFailure.Words("the server stopped sending"), DownloadRules.describe(CoreException.Temporary("download: the server stopped sending")))
        assertEquals(DownloadFailure.Words("the server does not have this file"), DownloadRules.describe(CoreException.Invalid("the server does not have this file")))
        assertEquals(DownloadFailure.Words("saving the file: No space left on device"), DownloadRules.describe(CoreException.Store("saving the file: No space left on device")))
        assertEquals(DownloadFailure.SignIn, DownloadRules.describe(CoreException.SignInNeeded("x")))
        assertEquals(DownloadFailure.Unknown, DownloadRules.describe(RuntimeException("boom")))
        assertEquals(DownloadFailure.Unknown, DownloadRules.describe(CoreException.Temporary("")))
        assertFalse(DownloadRules.isOffline("download: the server answered 503"))
    }
}
