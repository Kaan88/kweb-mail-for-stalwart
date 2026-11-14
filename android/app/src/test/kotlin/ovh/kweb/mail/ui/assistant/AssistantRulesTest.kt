package ovh.kweb.mail.ui.assistant

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import ovh.kweb.mail.ui.assistant.AssistantRules.Kind
import uniffi.kweb_core.AiEffort
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
    ) = AiSettings(enabled, provider, baseUrl, model, effort = AiEffort.DEFAULT, hasKey = false, consented = false, allowSummaries = summaries, allowDrafts = drafts, allowRewrites = rewrites)

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
        assertEquals("", AssistantRules.keyToSend("  ", remove = true)) // the model list is asked without the key being removed
        assertNull(AssistantRules.keyToSend("  ", remove = false))
    }

    // ---- switching the service ----

    @Test fun switchingTakesTheOtherServicesOwnValues() {
        val form = AssistantRules.formOf(settings(model = "gpt-x")).copy(model = "typed", removeKey = true)
        val other = settings(provider = AiProvider.ANTHROPIC, baseUrl = "https://api.anthropic.com/v1", model = "claude-x").copy(effort = AiEffort.HIGH)
        val after = form.switchedTo(other)
        assertEquals(AiProvider.ANTHROPIC, after.provider)
        assertEquals("claude-x", after.model)
        assertEquals(AiEffort.HIGH, after.effort)
        assertEquals("https://api.anthropic.com/v1", after.baseUrl)
        assertFalse(after.removeKey)
    }

    @Test fun switchingKeepsTheSwitchesThatBelongToNoService() {
        val form = AssistantRules.formOf(settings(enabled = false, summaries = false)).copy(enabled = true, allowDrafts = false)
        val other = settings(provider = AiProvider.ANTHROPIC, enabled = false, summaries = true, drafts = true, rewrites = true)
        val after = form.switchedTo(other)
        assertTrue(after.enabled)
        assertFalse(after.allowSummaries)
        assertFalse(after.allowDrafts)
    }

    @Test fun aServiceNeverSetUpGetsItsDefaultAddress() {
        val other = settings(provider = AiProvider.ANTHROPIC, baseUrl = "", model = "")
        assertEquals(AssistantRules.ANTHROPIC_ADDRESS, AssistantRules.formOf(settings()).switchedTo(other).baseUrl)
    }

    @Test fun theFormCarriesTheEffortToTheSettings() {
        val stored = settings(provider = AiProvider.ANTHROPIC)
        val form = AssistantRules.formOf(stored).copy(effort = AiEffort.MAX)
        assertEquals(AiEffort.MAX, form.toSettings(stored).effort)
        assertTrue(AssistantRules.isChanged(form, AssistantRules.formOf(stored), keyTyped = false))
    }
}
