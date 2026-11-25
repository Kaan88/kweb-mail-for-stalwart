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

class EffortRulesTest {
    private val list = AiModelList(
        models = listOf(
            AiModel("claude-a", "Claude A", listOf(AiEffort.LOW, AiEffort.HIGH)),
            AiModel("claude-b", "Claude B", emptyList()),
        ),
        fromService = true,
    )

    @Test fun theRowShowsOnlyWhenThereIsMoreThanDefault() {
        assertTrue(EffortRules.isShown(EffortRules.offered("claude-a", list)))
        assertFalse(EffortRules.isShown(EffortRules.offered("claude-b", list)))
        assertFalse(EffortRules.isShown(listOf(AiEffort.DEFAULT)))
    }

    @Test fun aModelThatNamesItsLevelsOffersDefaultAndThose() =
        assertEquals(listOf(AiEffort.DEFAULT, AiEffort.LOW, AiEffort.HIGH), EffortRules.offered("claude-a", list))

    @Test fun aModelWithoutLevelsOffersOnlyDefault() = assertEquals(listOf(AiEffort.DEFAULT), EffortRules.offered("claude-b", list))

    @Test fun aModelTheListDoesNotHaveOffersOnlyDefault() {
        assertEquals(listOf(AiEffort.DEFAULT), EffortRules.offered("typed-by-hand", list, AiEffort.HIGH))
        assertEquals(listOf(AiEffort.DEFAULT), EffortRules.offered("", list))
    }

    @Test fun withoutAListTheCurrentLevelStays() {
        assertEquals(listOf(AiEffort.DEFAULT, AiEffort.HIGH), EffortRules.offered("claude-a", null, AiEffort.HIGH))
        assertEquals(listOf(AiEffort.DEFAULT), EffortRules.offered("claude-a", null))
        assertEquals(AiEffort.MAX, EffortRules.normalised(AiEffort.MAX, "anything", null))
    }

    @Test fun spacesAroundTheModelDoNotMatter() =
        assertEquals(listOf(AiEffort.DEFAULT, AiEffort.LOW, AiEffort.HIGH), EffortRules.offered(" claude-a ", list))

    @Test fun levelsComeLowestFirstAndDefaultIsNeverListedTwice() {
        val odd = AiModelList(listOf(AiModel("m", "m", listOf(AiEffort.MAX, AiEffort.DEFAULT, AiEffort.OFF, AiEffort.MAX, AiEffort.MINIMAL))), true)
        assertEquals(listOf(AiEffort.DEFAULT, AiEffort.OFF, AiEffort.MINIMAL, AiEffort.MAX), EffortRules.offered("m", odd))
    }

    @Test fun aStoredLevelThatIsNotOfferedFallsBackToDefault() {
        assertEquals(AiEffort.DEFAULT, EffortRules.normalised(AiEffort.MAX, "claude-a", list))
        assertEquals(AiEffort.HIGH, EffortRules.normalised(AiEffort.HIGH, "claude-a", list))
        assertEquals(AiEffort.DEFAULT, EffortRules.normalised(AiEffort.MAX, "claude-b", list))
    }

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
