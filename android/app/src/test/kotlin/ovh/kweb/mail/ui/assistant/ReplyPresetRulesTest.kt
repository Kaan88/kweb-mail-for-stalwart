package ovh.kweb.mail.ui.assistant

import java.util.Locale
import org.junit.Assert.assertEquals
import org.junit.Test
import ovh.kweb.mail.ui.assistant.ReplyPresetRules.Choice
import uniffi.kweb_core.ReplyPreset

class ReplyPresetRulesTest {
    @Test fun aTapReplacesTheOtherOfItsGroupAndASecondTapSwitchesItOff() {
        var chosen = ReplyPresetRules.toggle(emptySet(), Choice.ACCEPT)
        chosen = ReplyPresetRules.toggle(chosen, Choice.SHORT)
        chosen = ReplyPresetRules.toggle(chosen, Choice.FORMAL)
        assertEquals(setOf(Choice.ACCEPT, Choice.SHORT, Choice.FORMAL), chosen)
        chosen = ReplyPresetRules.toggle(chosen, Choice.DECLINE)
        chosen = ReplyPresetRules.toggle(chosen, Choice.FRIENDLY)
        assertEquals(setOf(Choice.DECLINE, Choice.SHORT, Choice.FRIENDLY), chosen)
        assertEquals(setOf(Choice.DECLINE, Choice.FRIENDLY), ReplyPresetRules.toggle(chosen, Choice.SHORT))
    }

    @Test fun everyGroupHoldsAtMostOneAfterAnyTaps() {
        var chosen = emptySet<Choice>()
        for (choice in Choice.entries + Choice.entries.reversed()) {
            chosen = ReplyPresetRules.toggle(chosen, choice)
            assertEquals(chosen.size, chosen.map { it.group }.toSet().size)
        }
    }

    @Test fun thePresetsGoInAFixedOrderAndTheLanguageIsNamedInEnglish() {
        val turkish = Locale.forLanguageTag("tr")
        val presets = ReplyPresetRules.presets(setOf(Choice.MY_LANGUAGE, Choice.FORMAL, Choice.THANKS), turkish)
        assertEquals(listOf(ReplyPreset.Thanks, ReplyPreset.Formal, ReplyPreset.InLanguage("Turkish")), presets)
        assertEquals(setOf(Choice.MY_LANGUAGE, Choice.FORMAL, Choice.THANKS), ReplyPresetRules.choices(presets))
        assertEquals("Türkçe", ReplyPresetRules.ownLanguageName(turkish))
    }

    @Test fun aLocaleWithoutALanguageGivesNoLanguagePreset() {
        assertEquals(emptyList<ReplyPreset>(), ReplyPresetRules.presets(setOf(Choice.MY_LANGUAGE), Locale.ROOT))
    }

    @Test fun everyChoiceComesBackFromItsPreset() {
        val all = Choice.entries.toSet()
        // One per group is all that can be sent together; each round trips on its own.
        for (choice in all) {
            assertEquals(setOf(choice), ReplyPresetRules.choices(ReplyPresetRules.presets(setOf(choice), Locale.ENGLISH)))
        }
    }
}
