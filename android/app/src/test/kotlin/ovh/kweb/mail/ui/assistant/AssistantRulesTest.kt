package ovh.kweb.mail.ui.assistant

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import ovh.kweb.mail.ui.assistant.AssistantRules.Kind
import uniffi.kweb_core.AiActionMode
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
        ideas: Boolean = false,
        extract: Boolean = false,
        explain: Boolean = false,
        chat: Boolean = false,
        consented: Boolean = false,
        chatConsented: Boolean = false,
        actions: AiActionMode = AiActionMode.ASK_FIRST,
    ) = AiSettings(enabled, provider, baseUrl, model, effort = AiEffort.DEFAULT, hasKey = false, consented = consented, allowSummaries = summaries, allowDrafts = drafts, allowRewrites = rewrites, allowIdeas = ideas, allowExtract = extract, allowExplain = explain, allowChat = chat, chatConsented = chatConsented, actions = actions)

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

    @Test fun theNewKindsAreOffUntilSwitchedOn() {
        // What a user of stage 1 has after the update: the three old kinds, none of the new ones.
        assertEquals(setOf(Kind.SUMMARY, Kind.DRAFT, Kind.REWRITE), AssistantRules.visible(settings()))
        assertFalse(AssistantRules.isAllowed(settings(), Kind.IDEAS))
        assertFalse(AssistantRules.isAllowed(settings(), Kind.EXTRACT))
        assertFalse(AssistantRules.isAllowed(settings(), Kind.EXPLAIN))
    }

    @Test fun eachNewKindFollowsItsOwnSwitch() {
        val none = settings(summaries = false, drafts = false, rewrites = false)
        assertEquals(setOf(Kind.IDEAS), AssistantRules.visible(none.copy(allowIdeas = true)))
        assertEquals(setOf(Kind.EXTRACT), AssistantRules.visible(none.copy(allowExtract = true)))
        assertEquals(setOf(Kind.EXPLAIN), AssistantRules.visible(none.copy(allowExplain = true)))
        assertEquals(Kind.entries.toSet(), AssistantRules.visible(settings(ideas = true, extract = true, explain = true)))
    }

    @Test fun theNewKindsDoNotShowWhileTheAssistantIsOff() =
        assertEquals(emptySet<Kind>(), AssistantRules.visible(settings(enabled = false, ideas = true, extract = true, explain = true)))

    @Test fun theChatSwitchOffersNoKindOfTheSheet() =
        assertEquals(emptySet<Kind>(), AssistantRules.visible(settings(summaries = false, drafts = false, rewrites = false, chat = true)))

    // ---- the chat's gate ----

    @Test fun theChatIsOffWhileTheSettingsAreUnknownOrTheAssistantIsOff() {
        assertEquals(AssistantRules.ChatGate.OFF, AssistantRules.chatReady(null))
        assertEquals(AssistantRules.ChatGate.OFF, AssistantRules.chatReady(settings(enabled = false, chat = true, consented = true, chatConsented = true)))
    }

    @Test fun theChatNeedsItsOwnSwitch() =
        assertEquals(AssistantRules.ChatGate.CHAT_OFF, AssistantRules.chatReady(settings(chat = false, consented = true, chatConsented = true)))

    @Test fun theChatNeedsBothAgreements() {
        assertEquals(AssistantRules.ChatGate.NOT_AGREED, AssistantRules.chatReady(settings(chat = true)))
        assertEquals(AssistantRules.ChatGate.NOT_AGREED, AssistantRules.chatReady(settings(chat = true, consented = true)))
        // The second yes without the first is no yes (the core never stores it so; the screen does not trust it either).
        assertEquals(AssistantRules.ChatGate.NOT_AGREED, AssistantRules.chatReady(settings(chat = true, chatConsented = true)))
    }

    @Test fun theChatIsReadyWithTheSwitchAndBothAgreements() =
        assertEquals(AssistantRules.ChatGate.READY, AssistantRules.chatReady(settings(chat = true, consented = true, chatConsented = true)))

    @Test fun theActionModeDoesNotGateTheChat() =
        assertEquals(AssistantRules.ChatGate.READY, AssistantRules.chatReady(settings(chat = true, consented = true, chatConsented = true, actions = AiActionMode.OFF)))

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

    @Test fun savingNeverGrantsTheChatsConsentEither() {
        // Switching the chat on in the form and saving: neither yes comes out of the form.
        val stored = settings()
        val sent = AssistantRules.formOf(stored).copy(allowChat = true).toSettings(stored)
        assertTrue(sent.allowChat)
        assertFalse(sent.consented)
        assertFalse(sent.chatConsented)
        // Agreed to the service only: the chat's yes is still not given by a save.
        val agreed = settings(consented = true)
        val again = AssistantRules.formOf(agreed).copy(allowChat = true).toSettings(agreed)
        assertTrue(again.consented)
        assertFalse(again.chatConsented)
    }

    @Test fun theChatsConsentIsKeptOnlyForTheSameServiceAndModel() {
        val stored = settings(chat = true, consented = true, chatConsented = true)
        val form = AssistantRules.formOf(stored)
        assertTrue(form.copy(allowIdeas = true, actions = AiActionMode.OFF).toSettings(stored).chatConsented)
        assertFalse(form.copy(model = "other").toSettings(stored).chatConsented)
        assertFalse(form.copy(baseUrl = "https://example.org/v1").toSettings(stored).chatConsented)
        assertFalse(form.withProvider(AiProvider.ANTHROPIC).toSettings(stored).chatConsented)
    }

    @Test fun theChatsConsentNeverOutlivesTheFirst() {
        // A state the core does not store; the form does not make a yes out of it.
        val odd = settings(chat = true, consented = false, chatConsented = true)
        assertFalse(AssistantRules.formOf(odd).toSettings(odd).chatConsented)
    }

    @Test fun theFormRoundTripsTheNewSwitches() {
        val stored = settings(ideas = true, extract = false, explain = true, chat = true, actions = AiActionMode.OFF)
        val form = AssistantRules.formOf(stored)
        assertTrue(form.allowIdeas)
        assertFalse(form.allowExtract)
        assertTrue(form.allowExplain)
        assertTrue(form.allowChat)
        assertEquals(AiActionMode.OFF, form.actions)
        assertEquals(stored, form.toSettings(stored))
    }

    @Test fun theFormCarriesTheNewSwitchesToTheSettings() {
        val stored = settings()
        val sent = AssistantRules.formOf(stored).copy(allowIdeas = true, allowExtract = true, allowExplain = true, allowChat = true, actions = AiActionMode.OFF).toSettings(stored)
        assertTrue(sent.allowIdeas)
        assertTrue(sent.allowExtract)
        assertTrue(sent.allowExplain)
        assertTrue(sent.allowChat)
        assertEquals(AiActionMode.OFF, sent.actions)
    }

    @Test fun aNewSwitchOrTheActionModeIsAChange() {
        val form = AssistantRules.formOf(settings())
        assertTrue(AssistantRules.isChanged(form.copy(allowIdeas = true), form, keyTyped = false))
        assertTrue(AssistantRules.isChanged(form.copy(allowExtract = true), form, keyTyped = false))
        assertTrue(AssistantRules.isChanged(form.copy(allowExplain = true), form, keyTyped = false))
        assertTrue(AssistantRules.isChanged(form.copy(allowChat = true), form, keyTyped = false))
        assertTrue(AssistantRules.isChanged(form.copy(actions = AiActionMode.OFF), form, keyTyped = false))
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

    @Test fun switchingKeepsTheNewSwitchesAndTheActionModeAsShown() {
        val form = AssistantRules.formOf(settings()).copy(allowIdeas = true, allowExtract = true, allowExplain = true, allowChat = true, actions = AiActionMode.OFF)
        val after = form.switchedTo(settings(provider = AiProvider.ANTHROPIC))
        assertTrue(after.allowIdeas)
        assertTrue(after.allowExtract)
        assertTrue(after.allowExplain)
        assertTrue(after.allowChat)
        assertEquals(AiActionMode.OFF, after.actions)
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
