package ovh.kweb.mail.ui.folders

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.UiMessage
import uniffi.kweb_core.MessageHeader
import uniffi.kweb_core.Exception as CoreException

private fun message(id: String, vararg boxes: String) = MessageHeader(
    id, "t", boxes.toList(), "", "a@b.c", emptyList(), "s", "p", 0L, false, false, false, 0uL,
)

class SpamRulesTest {
    private val folders = listOf(folder("in", role = "inbox"), folder("ju", role = "junk"), folder("dr", role = "drafts"), folder("se", role = "sent"))

    @Test fun offeredNeedsAJunkFolderAndNotDraftsOrSent() {
        assertTrue(SpamRules.offered("inbox", true))
        assertTrue(SpamRules.offered(null, true))
        assertTrue(SpamRules.offered("junk", true))
        assertFalse(SpamRules.offered("inbox", false))
        assertFalse(SpamRules.offered("drafts", true))
        assertFalse(SpamRules.offered("sent", true))
    }

    @Test fun inJunkTheEntryReadsNotSpam() {
        assertTrue(SpamRules.isNotSpamEntry("junk"))
        assertFalse(SpamRules.isNotSpamEntry("inbox"))
        assertFalse(SpamRules.isNotSpamEntry(null))
    }

    @Test fun aThreadOpenedFromAFolderIsNotSpamOnlyInJunk() {
        val m = listOf(message("1", "in"))
        assertTrue(SpamRules.threadIsNotSpam(m, listOf("1"), "ju", folders))
        assertFalse(SpamRules.threadIsNotSpam(m, listOf("1"), "in", folders))
    }

    @Test fun aThreadFromASearchIsNotSpamWhenEveryTargetIsInJunk() {
        val inJunk = listOf(message("1", "ju"), message("2", "ju"))
        assertTrue(SpamRules.threadIsNotSpam(inJunk, listOf("1", "2"), null, folders))
        val mixed = listOf(message("1", "ju"), message("2", "in"))
        assertFalse(SpamRules.threadIsNotSpam(mixed, listOf("1", "2"), null, folders))
        assertFalse(SpamRules.threadIsNotSpam(emptyList(), emptyList(), null, folders))
    }

    @Test fun withoutAJunkFolderNothingIsNotSpam() {
        assertFalse(SpamRules.threadIsNotSpam(listOf(message("1", "in")), listOf("1"), null, folders.filter { it.role != "junk" }))
    }

    @Test fun theThreadMenuIsNotOfferedFromDraftsOrSentOrWithoutJunk() {
        assertTrue(SpamRules.threadOffered(folders[0], true))
        assertTrue(SpamRules.threadOffered(null, true))
        assertFalse(SpamRules.threadOffered(folders[2], true))
        assertFalse(SpamRules.threadOffered(folders[3], true))
        assertFalse(SpamRules.threadOffered(folders[0], false))
    }

    @Test fun aRefusalIsShownInTheCoresWordsAndAnythingElseGetsTheAppsOwn() {
        assertEquals(UiMessage.Text("There is no Junk folder."), SpamRules.failure(CoreException.Invalid("There is no Junk folder.")))
        assertEquals(UiMessage.Res(R.string.spam_failed), SpamRules.failure(java.io.IOException("x")))
    }

    @Test fun undoOfAReportGoesBackToTheFolderItCameFrom() {
        assertEquals("tr", SpamRules.undoGoesTo(true, "tr", "trash"))
        assertEquals("proj", SpamRules.undoGoesTo(true, "proj", null))
    }

    @Test fun undoOfAReportFromTheInboxOrOfNotSpamUsesTheCore() {
        assertNull(SpamRules.undoGoesTo(true, "in", "inbox"))
        assertNull(SpamRules.undoGoesTo(false, "junk", "junk"))
        assertNull(SpamRules.undoGoesTo(true, null, null))
    }
}
