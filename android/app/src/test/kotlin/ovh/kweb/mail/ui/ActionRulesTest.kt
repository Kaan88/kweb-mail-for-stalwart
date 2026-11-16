package ovh.kweb.mail.ui

import ovh.kweb.mail.ui.ActionRules.DeleteAsk
import ovh.kweb.mail.ui.ActionRules.Swipe
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class ActionRulesTest {
    @Test fun todaysSwipesAreTheDefaults() {
        assertEquals(Swipe.ARCHIVE, ActionRules.DEFAULT_RIGHT)
        assertEquals(Swipe.DELETE, ActionRules.DEFAULT_LEFT)
    }

    @Test fun aStoredKeyGivesItsActionAndAnythingElseTheDefault() {
        for (action in Swipe.entries) assertEquals(action, Swipe.of(action.key, Swipe.NONE))
        assertEquals(Swipe.ARCHIVE, Swipe.of(null, Swipe.ARCHIVE))
        assertEquals(Swipe.DELETE, Swipe.of("nonsense", Swipe.DELETE))
    }

    @Test fun theKeysAreDistinctAndStable() {
        assertEquals(Swipe.entries.size, Swipe.entries.map { it.key }.toSet().size)
        assertEquals(listOf("archive", "delete", "read", "star", "move", "none"), Swipe.entries.map { it.key })
    }

    @Test fun noneTurnsADirectionOffAndSoDoesChoosingRows() {
        assertFalse(ActionRules.enabled(Swipe.NONE, "inbox", selecting = false))
        for (action in Swipe.entries) assertFalse(ActionRules.enabled(action, "inbox", selecting = true))
        for (action in Swipe.entries.filter { it != Swipe.NONE }) assertTrue(ActionRules.enabled(action, "inbox", selecting = false))
    }

    @Test fun archiveDoesNothingInTheArchiveButTheOthersWork() {
        assertFalse(ActionRules.enabled(Swipe.ARCHIVE, "archive", selecting = false))
        assertTrue(ActionRules.enabled(Swipe.DELETE, "archive", selecting = false))
        assertTrue(ActionRules.enabled(Swipe.STAR, "archive", selecting = false))
        assertTrue(ActionRules.enabled(Swipe.DELETE, "trash", selecting = false))
    }

    @Test fun trashAsksForeverOnceAndElsewhereTheSettingDecides() {
        assertEquals(DeleteAsk.FOREVER, ActionRules.deleteAsk(inTrash = true, confirmSetting = false))
        assertEquals(DeleteAsk.FOREVER, ActionRules.deleteAsk(inTrash = true, confirmSetting = true))
        assertEquals(DeleteAsk.CONFIRM, ActionRules.deleteAsk(inTrash = false, confirmSetting = true))
        assertEquals(DeleteAsk.NONE, ActionRules.deleteAsk(inTrash = false, confirmSetting = false))
    }

    @Test fun theUndoTimeIsOneOfTheChoices() {
        assertEquals(listOf(5, 10, 20, 30), ActionRules.UNDO_CHOICES)
        assertEquals(10, ActionRules.DEFAULT_UNDO_SECONDS)
        for (n in ActionRules.UNDO_CHOICES) assertEquals(n, ActionRules.undoSeconds(n))
        assertEquals(10, ActionRules.undoSeconds(0))
        assertEquals(10, ActionRules.undoSeconds(7))
        assertEquals(10, ActionRules.undoSeconds(600))
        assertTrue(ActionRules.DEFAULT_UNDO_SECONDS in ActionRules.UNDO_CHOICES)
    }

    @Test fun replyAllLeadsOnlyWhenTheSettingIsOnAndThereIsMoreThanOneRecipient() {
        assertTrue(ActionRules.replyAllPrimary(setting = true, recipients = 2))
        assertTrue(ActionRules.replyAllPrimary(setting = true, recipients = 5))
        assertFalse(ActionRules.replyAllPrimary(setting = true, recipients = 1))
        assertFalse(ActionRules.replyAllPrimary(setting = true, recipients = 0))
        assertFalse(ActionRules.replyAllPrimary(setting = false, recipients = 4))
    }
}
