package ovh.kweb.mail.ui.sender

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.kweb_core.MethodResult
import uniffi.kweb_core.SenderCheck
import uniffi.kweb_core.SenderReason
import uniffi.kweb_core.SenderStatus

class SenderRulesTest {
    private fun check(status: SenderStatus, reason: SenderReason = SenderReason.NO_RESULTS) =
        SenderCheck(status, reason, "example.org", MethodResult.NOT_RUN, MethodResult.NOT_RUN, MethodResult.NOT_RUN, "")

    @Test fun aMessageTheServerDidNotCheckShowsNoMark() {
        assertFalse(SenderRules.shows(check(SenderStatus.NOT_CHECKED, SenderReason.NOT_FROM_THIS_SERVER)))
        for (s in listOf(SenderStatus.VERIFIED, SenderStatus.UNVERIFIED, SenderStatus.FAILED)) assertTrue("$s", SenderRules.shows(check(s)))
    }

    @Test fun onlyAFailedCheckWarnsAndOnlyAVerifiedOneIsGood() {
        assertTrue(SenderRules.warns(check(SenderStatus.FAILED, SenderReason.DMARC_FAIL)))
        for (s in listOf(SenderStatus.VERIFIED, SenderStatus.UNVERIFIED, SenderStatus.NOT_CHECKED)) assertFalse("$s", SenderRules.warns(check(s)))
        assertEquals(SenderRules.Tone.GOOD, SenderRules.tone(check(SenderStatus.VERIFIED)))
        assertEquals(SenderRules.Tone.BAD, SenderRules.tone(check(SenderStatus.FAILED)))
        assertEquals(SenderRules.Tone.NEUTRAL, SenderRules.tone(check(SenderStatus.UNVERIFIED)))
    }

    @Test fun everyReasonHasItsOwnSentenceAndEveryStatusItsWord() {
        val sentences = SenderReason.values().map { SenderRules.why(check(SenderStatus.UNVERIFIED, it)) }
        assertEquals(sentences.size, sentences.toSet().size)
        val words = listOf(SenderStatus.VERIFIED, SenderStatus.UNVERIFIED, SenderStatus.FAILED).map { SenderRules.chipText(check(it)) }
        assertEquals(3, words.toSet().size)
        assertEquals(MethodResult.values().size, MethodResult.values().map { SenderRules.method(it) }.toSet().size)
    }
}
