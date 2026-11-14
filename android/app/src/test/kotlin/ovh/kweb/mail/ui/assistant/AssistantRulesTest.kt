package ovh.kweb.mail.ui.assistant

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import ovh.kweb.mail.ui.assistant.AssistantRules.Kind
import uniffi.kweb_core.AiProvider
import uniffi.kweb_core.AiSettings

class AssistantRulesTest {
    private fun settings(
        enabled: Boolean = true,
        provider: AiProvider = AiProvider.OPEN_AI_COMPATIBLE,
        baseUrl: String = "https://api.openai.com/v1",
        model: String = "gpt-x",
        summaries: Boolean = true,
        drafts: Boolean = true,
        rewrites: Boolean = true,
    ) = AiSettings(enabled, provider, baseUrl, model, hasKey = false, consented = false, allowSummaries = summaries, allowDrafts = drafts, allowRewrites = rewrites)

    // ---- which entries show ----

    @Test fun nothingShowsBeforeTheSettingsAreKnown() = assertEquals(emptySet<Kind>(), AssistantRules.visible(null))

    @Test fun nothingShowsWhileTheAssistantIsOff() = assertEquals(emptySet<Kind>(), AssistantRules.visible(settings(enabled = false)))

    @Test fun everythingShowsWhenAllIsOn() = assertEquals(setOf(Kind.SUMMARY, Kind.DRAFT, Kind.REWRITE), AssistantRules.visible(settings()))

    @Test fun eachKindFollowsItsOwnSwitch() {
        assertEquals(setOf(Kind.DRAFT, Kind.REWRITE), AssistantRules.visible(settings(summaries = false)))
        assertEquals(setOf(Kind.SUMMARY, Kind.REWRITE), AssistantRules.visible(settings(drafts = false)))
        assertEquals(setOf(Kind.SUMMARY, Kind.DRAFT), AssistantRules.visible(settings(rewrites = false)))
    }

    @Test fun aSwitchedOffKindIsNotAllowedEvenWhenTheRestIsOn() {
        assertFalse(AssistantRules.isAllowed(settings(rewrites = false), Kind.REWRITE))
        assertTrue(AssistantRules.isAllowed(settings(rewrites = false), Kind.SUMMARY))
    }

    // ---- the default address ----

    @Test fun eachServiceHasItsDefaultAddress() {
        assertEquals("https://api.openai.com/v1", AssistantRules.defaultAddress(AiProvider.OPEN_AI_COMPATIBLE))
        assertEquals("https://api.anthropic.com/v1", AssistantRules.defaultAddress(AiProvider.ANTHROPIC))
    }

    @Test fun anEmptyAddressGetsTheDefaultOfTheChosenService() {
        assertEquals(AssistantRules.ANTHROPIC_ADDRESS, AssistantRules.addressAfterChoosing("", AiProvider.OPEN_AI_COMPATIBLE, AiProvider.ANTHROPIC))
        assertEquals(AssistantRules.OPENAI_ADDRESS, AssistantRules.addressAfterChoosing("  ", AiProvider.ANTHROPIC, AiProvider.OPEN_AI_COMPATIBLE))
    }

    @Test fun theOtherServicesDefaultIsReplaced() {
        assertEquals(AssistantRules.ANTHROPIC_ADDRESS, AssistantRules.addressAfterChoosing(AssistantRules.OPENAI_ADDRESS, AiProvider.OPEN_AI_COMPATIBLE, AiProvider.ANTHROPIC))
        assertEquals(AssistantRules.OPENAI_ADDRESS, AssistantRules.addressAfterChoosing(AssistantRules.ANTHROPIC_ADDRESS + "/", AiProvider.ANTHROPIC, AiProvider.OPEN_AI_COMPATIBLE))
    }

    @Test fun anAddressTheUserWroteIsKept() {
        val own = "http://192.168.1.20:11434/v1"
        assertEquals(own, AssistantRules.addressAfterChoosing(own, AiProvider.OPEN_AI_COMPATIBLE, AiProvider.ANTHROPIC))
    }

    @Test fun choosingTheSameServiceChangesNothing() =
        assertEquals("", AssistantRules.addressAfterChoosing("", AiProvider.ANTHROPIC, AiProvider.ANTHROPIC))

    @Test fun theFormFollowsTheChoiceOfService() {
        val form = AssistantRules.formOf(settings()).withProvider(AiProvider.ANTHROPIC)
        assertEquals(AiProvider.ANTHROPIC, form.provider)
        assertEquals(AssistantRules.ANTHROPIC_ADDRESS, form.baseUrl)
    }

    @Test fun anEmptyStoredAddressIsPrefilled() =
        assertEquals(AssistantRules.OPENAI_ADDRESS, AssistantRules.formOf(settings(baseUrl = "")).baseUrl)

    // ---- the state line ----

    @Test fun theLineIsEmptyWhileUnknown() = assertNull(AssistantRules.stateLine(null))

    @Test fun theLineSaysOffWhenOff() = assertEquals(AssistantRules.StateLine.Off, AssistantRules.stateLine(settings(enabled = false, model = "gpt-x")))

    @Test fun theLineNamesTheModelWhenOn() =
        assertEquals(AssistantRules.StateLine.Model("gpt-x"), AssistantRules.stateLine(settings(model = " gpt-x ")))

    @Test fun theLineSaysOnWhenThereIsNoModelYet() = assertEquals(AssistantRules.StateLine.On, AssistantRules.stateLine(settings(model = " ")))

    // ---- the form ----

    @Test fun savingNeverGrantsConsent() {
        val unconsented = settings()
        assertFalse(AssistantRules.formOf(unconsented).toSettings(unconsented).consented)
    }

    @Test fun consentSurvivesChangesThatKeepTheServiceAndModel() {
        val stored = settings().also { it.consented = true }
        val form = AssistantRules.formOf(stored)
        assertTrue(form.copy(model = " gpt-x ", baseUrl = stored.baseUrl + "/ ").toSettings(stored).consented)
        assertTrue(form.copy(allowRewrites = false, enabled = false).toSettings(stored).consented)
    }

    @Test fun consentIsDroppedWhenTheModelTheAddressOrTheServiceChanges() {
        val stored = settings().also { it.consented = true }
        val form = AssistantRules.formOf(stored)
        assertFalse(form.copy(model = "other").toSettings(stored).consented)
        assertFalse(form.copy(baseUrl = "https://example.org/v1").toSettings(stored).consented)
        assertFalse(form.withProvider(AiProvider.ANTHROPIC).toSettings(stored).consented)
    }

    @Test fun anUntouchedFormIsNotChanged() {
        val form = AssistantRules.formOf(settings())
        assertFalse(AssistantRules.isChanged(form, form, keyTyped = false))
    }

    @Test fun anEditedOrKeyedFormIsChanged() {
        val form = AssistantRules.formOf(settings())
        assertTrue(AssistantRules.isChanged(form.copy(model = "x"), form, keyTyped = false))
        assertTrue(AssistantRules.isChanged(form, form, keyTyped = true))
        assertTrue(AssistantRules.isChanged(form.copy(removeKey = true), form, keyTyped = false))
    }

    @Test fun spacesAroundTheAddressAreNotAChange() {
        val form = AssistantRules.formOf(settings())
        assertFalse(AssistantRules.isChanged(form.copy(baseUrl = form.baseUrl + " "), form, keyTyped = false))
    }

    @Test fun theKeyToSend() {
        assertEquals("sk-1", AssistantRules.keyToSend(" sk-1 ", remove = false))
        assertEquals("sk-1", AssistantRules.keyToSend("sk-1", remove = true))
        assertEquals("", AssistantRules.keyToSend("", remove = true))
        assertNull(AssistantRules.keyToSend("  ", remove = false))
    }
}
