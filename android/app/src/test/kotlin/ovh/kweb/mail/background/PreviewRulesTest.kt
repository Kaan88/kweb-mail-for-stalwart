package ovh.kweb.mail.background

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class PreviewRulesTest {
    private fun shown(choice: PreviewChoice, preview: String = "First lines of the mail") =
        PreviewRules.shown(choice, "Merve", "Lunch?", preview, "New mail")

    @Test
    fun theFullChoiceIsWhatTheAppAlwaysShowed() {
        assertEquals(ShownText("Merve", "Lunch?", "First lines of the mail"), shown(PreviewChoice.Full))
        assertEquals(ShownText("Merve", "Lunch?", "Lunch?"), shown(PreviewChoice.Full, preview = " "))
    }

    @Test
    fun subjectOnlyHasNoTextOfTheMail() {
        val texts = shown(PreviewChoice.SubjectOnly)
        assertEquals(ShownText("Merve", "Lunch?", "Lunch?"), texts)
        assertFalse(texts.big.contains("First lines"))
    }

    @Test
    fun onlyThatMailArrivedHasNoSenderAndNoSubject() {
        val texts = shown(PreviewChoice.Nothing)
        assertEquals(ShownText("New mail", "", ""), texts)
        assertFalse("${texts.title}${texts.text}${texts.big}".contains("Merve"))
        assertFalse("${texts.title}${texts.text}${texts.big}".contains("Lunch"))
    }

    @Test
    fun theLockScreenVersionFollowsTheChoice() {
        assertFalse(PreviewRules.hasPublicVersion(PreviewChoice.Full))
        assertTrue(PreviewRules.hasPublicVersion(PreviewChoice.SubjectOnly))
        assertTrue(PreviewRules.hasPublicVersion(PreviewChoice.Nothing))
    }

    @Test
    fun aStoredChoiceIsReadBackAndAnythingElseIsTheFullText() {
        for (c in PreviewChoice.entries) assertEquals(c, PreviewChoice.parse(c.key))
        assertEquals(PreviewChoice.Full, PreviewChoice.parse(null))
        assertEquals(PreviewChoice.Full, PreviewChoice.parse("from a later version"))
    }
}
