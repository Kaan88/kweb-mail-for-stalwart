package ovh.kweb.mail.ui.assistant

import java.util.Locale
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.kweb_core.AiEffort
import uniffi.kweb_core.AiPreview
import uniffi.kweb_core.AiProvider
import uniffi.kweb_core.AiSettings

class ConsentRulesTest {
    private object Words : AssistantWords {
        override fun scopeMessages(count: Int, characters: String) = if (count == 1) "1 message, about $characters characters" else "$count messages, about $characters characters"

        override fun scopeText(characters: String) = "about $characters characters of your text"

        override fun shortened(scope: String) = "$scope, shortened to fit"
    }

    private fun preview(messages: UInt, characters: UInt, cut: Boolean = false) = AiPreview("api.openai.com", "gpt-x", messages, characters, cut)

    private fun settings(consented: Boolean) = AiSettings(true, AiProvider.OPEN_AI_COMPATIBLE, "https://api.openai.com/v1", "gpt-x", AiEffort.DEFAULT, false, consented, true, true, true)

    @Test fun theFirstTaskAsksAndLaterOnesDoNot() {
        assertTrue(ConsentRules.isNeeded(settings(consented = false)))
        assertFalse(ConsentRules.isNeeded(settings(consented = true)))
    }

    @Test fun grantingKeepsServiceAddressAndModel() {
        val before = settings(consented = false)
        val after = ConsentRules.granted(before)
        assertTrue(after.consented)
        assertEquals(before.copy(consented = true), after)
        assertFalse(before.consented)
    }

    @Test fun sizesAreRoundedToTwoDigits() {
        assertEquals(2300, ConsentRules.roundedCharacters(2314))
        assertEquals(31000, ConsentRules.roundedCharacters(30874))
        assertEquals(60000, ConsentRules.roundedCharacters(60000))
        assertEquals(150, ConsentRules.roundedCharacters(150))
        assertEquals(100, ConsentRules.roundedCharacters(101))
        assertEquals(10, ConsentRules.roundedCharacters(12))
        assertEquals(10, ConsentRules.roundedCharacters(3))
        assertEquals(0, ConsentRules.roundedCharacters(0))
    }

    @Test fun oneMessage() = assertEquals("1 message, about 2,300 characters", ConsentRules.scope(preview(1u, 2314u), Words, Locale.US))

    @Test fun aShortenedThread() =
        assertEquals("12 messages, about 31,000 characters, shortened to fit", ConsentRules.scope(preview(12u, 30874u, cut = true), Words, Locale.US))

    @Test fun aRewriteSendsOnlyTheText() = assertEquals("about 400 characters of your text", ConsentRules.scope(preview(0u, 398u), Words, Locale.US))

    @Test fun numbersFollowTheLocale() = assertEquals("12 messages, about 31.000 characters", ConsentRules.scope(preview(12u, 31020u), Words, Locale.GERMANY))

    // ---- after a save ----

    @Test fun theDialogShowsForAServiceThatIsOnAndNotAgreedTo() {
        assertTrue(ConsentRules.isNeededAfterSave(settings(consented = false)))
    }

    @Test fun aServiceAgreedToBeforeNeedsNoNewDialog() {
        // Switching back to it with its values unchanged: the core returns it consented.
        assertFalse(ConsentRules.isNeededAfterSave(settings(consented = true)))
    }

    @Test fun nothingIsAskedWhileTheAssistantIsOff() {
        assertFalse(ConsentRules.isNeededAfterSave(settings(consented = false).copy(enabled = false)))
    }

    @Test fun decliningSavesTheSameValuesSwitchedOff() {
        val returned = settings(consented = false)
        val declined = ConsentRules.declined(returned)
        assertFalse(declined.enabled)
        assertEquals(returned.copy(enabled = false), declined)
    }

    @Test fun agreeingSavesTheSameValuesAgain() {
        val returned = settings(consented = false)
        assertEquals(returned.copy(consented = true), ConsentRules.granted(returned))
    }

    @Test fun theHostIsNamedWithItsPortButWithoutSchemeAndPath() {
        assertEquals("api.anthropic.com", ConsentRules.hostOf("https://api.anthropic.com/v1"))
        assertEquals("192.168.1.20:11434", ConsentRules.hostOf(" http://192.168.1.20:11434/v1 "))
        assertEquals("not a url", ConsentRules.hostOf("not a url"))
    }
}
