package ovh.kweb.mail.ui.assistant

import java.util.UUID
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.kweb_core.AiEffort
import uniffi.kweb_core.AiModel
import uniffi.kweb_core.AiModelList
import uniffi.kweb_core.AiProvider

class EffortRulesTest {
    private val list = AiModelList(
        models = listOf(
            AiModel("claude-a", "Claude A", listOf(AiEffort.LOW, AiEffort.HIGH)),
            AiModel("claude-b", "Claude B", emptyList()),
        ),
        fromService = true,
    )

    @Test fun onlyAnthropicShowsTheRow() {
        assertTrue(EffortRules.isShown(AiProvider.ANTHROPIC))
        assertFalse(EffortRules.isShown(AiProvider.OPEN_AI_COMPATIBLE))
    }

    @Test fun aModelThatNamesItsLevelsOffersDefaultAndThose() =
        assertEquals(listOf(AiEffort.DEFAULT, AiEffort.LOW, AiEffort.HIGH), EffortRules.offered(AiProvider.ANTHROPIC, "claude-a", list))

    @Test fun aModelWithoutLevelsOffersAll() = assertEquals(EffortRules.ALL, EffortRules.offered(AiProvider.ANTHROPIC, "claude-b", list))

    @Test fun anUnknownModelOrNoListOffersAll() {
        assertEquals(EffortRules.ALL, EffortRules.offered(AiProvider.ANTHROPIC, "typed-by-hand", list))
        assertEquals(EffortRules.ALL, EffortRules.offered(AiProvider.ANTHROPIC, "claude-a", null))
        assertEquals(EffortRules.ALL, EffortRules.offered(AiProvider.ANTHROPIC, "", list))
    }

    @Test fun spacesAroundTheModelDoNotMatter() =
        assertEquals(listOf(AiEffort.DEFAULT, AiEffort.LOW, AiEffort.HIGH), EffortRules.offered(AiProvider.ANTHROPIC, " claude-a ", list))

    @Test fun defaultIsNeverListedTwice() {
        val odd = AiModelList(listOf(AiModel("m", "m", listOf(AiEffort.DEFAULT, AiEffort.MAX, AiEffort.MAX))), true)
        assertEquals(listOf(AiEffort.DEFAULT, AiEffort.MAX), EffortRules.offered(AiProvider.ANTHROPIC, "m", odd))
    }

    @Test fun aStoredLevelThatIsNotOfferedFallsBackToDefault() {
        assertEquals(AiEffort.DEFAULT, EffortRules.normalised(AiEffort.MAX, AiProvider.ANTHROPIC, "claude-a", list))
        assertEquals(AiEffort.HIGH, EffortRules.normalised(AiEffort.HIGH, AiProvider.ANTHROPIC, "claude-a", list))
        assertEquals(AiEffort.MAX, EffortRules.normalised(AiEffort.MAX, AiProvider.ANTHROPIC, "claude-b", list))
    }

    @Test fun otherServicesHaveNoEffort() = assertEquals(AiEffort.DEFAULT, EffortRules.normalised(AiEffort.HIGH, AiProvider.OPEN_AI_COMPATIBLE, "gpt", null))

    @Test fun findIgnoresAnEmptyName() = assertNull(EffortRules.find(list, " "))

    @Test fun theIdShowsUnderTheNameOnlyWhenDifferent() {
        assertEquals("claude-a", ModelRules.subtitle(AiModel("claude-a", "Claude A", emptyList())))
        assertNull(ModelRules.subtitle(AiModel("gpt-x", "gpt-x", emptyList())))
        assertEquals("gpt-x", ModelRules.title(AiModel("gpt-x", "", emptyList())))
    }

    @Test fun aBuiltInListIsRecognised() {
        assertTrue(ModelRules.isBuiltIn(list.copy(fromService = false)))
        assertFalse(ModelRules.isBuiltIn(list))
    }

    @Test fun everyRunGetsItsOwnUuid() {
        val a = RunRules.newRunId()
        val b = RunRules.newRunId()
        assertNotEquals(a, b)
        assertEquals(a, UUID.fromString(a).toString())
    }

    @Test fun aTaskIsCancelledWhileItRunsOrIsBeingPrepared() {
        assertTrue(RunRules.cancelsWhenClosed(AssistPhase.Running))
        assertTrue(RunRules.cancelsWhenClosed(AssistPhase.Checking))
        assertFalse(RunRules.cancelsWhenClosed(AssistPhase.Done))
        assertFalse(RunRules.cancelsWhenClosed(AssistPhase.Stopped))
        assertFalse(RunRules.cancelsWhenClosed(AssistPhase.Input))
        assertFalse(RunRules.cancelsWhenClosed(null))
    }
}
