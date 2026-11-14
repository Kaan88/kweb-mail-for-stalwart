package ovh.kweb.mail.ui.folders

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.kweb_core.Mailbox

internal fun folder(
    id: String,
    name: String = id,
    parent: String? = null,
    role: String? = null,
    total: UInt = 0u,
    notify: Boolean = false,
) = Mailbox(id, name, role, parent, 0u, total, 0u, notify)

class FolderRulesTest {
    private val inbox = folder("in", role = "inbox", notify = true)
    private val drafts = folder("dr", role = "drafts")
    private val sent = folder("se", role = "sent")
    private val trash = folder("tr", role = "trash")
    private val junk = folder("ju", role = "junk")
    private val archive = folder("ar", role = "archive")
    private val work = folder("work")
    private val sub = folder("sub", parent = "work")
    private val deep = folder("deep", parent = "sub")
    private val other = folder("other")
    private val all = listOf(inbox, drafts, sent, trash, junk, archive, work, sub, deep, other)

    @Test fun theInboxBellIsFixedAndTheSystemFoldersHaveNone() {
        assertEquals(Bell.FIXED_ON, FolderRules.bell(inbox))
        for (f in listOf(drafts, sent, trash, junk)) assertEquals(Bell.NONE, FolderRules.bell(f))
        assertEquals(Bell.SWITCH, FolderRules.bell(archive))
        assertEquals(Bell.SWITCH, FolderRules.bell(work))
    }

    @Test fun theDrawerBellShowsOnNotifyingFoldersButNeverOnTheInbox() {
        assertFalse(FolderRules.showsDrawerBell(inbox))
        assertTrue(FolderRules.showsDrawerBell(folder("x", notify = true)))
        assertFalse(FolderRules.showsDrawerBell(folder("y", notify = false)))
    }

    @Test fun descendantsAreFoundAtAnyDepth() {
        assertEquals(setOf("sub", "deep"), FolderRules.descendants(all, "work"))
        assertEquals(emptySet<String>(), FolderRules.descendants(all, "other"))
    }

    @Test fun descendantsSurviveAParentCycle() {
        val a = folder("a", parent = "b")
        val b = folder("b", parent = "a")
        assertEquals(setOf("a", "b"), FolderRules.descendants(listOf(a, b), "a"))
    }

    @Test fun aFolderCannotMoveIntoItselfItsOwnFoldersOrTheClosedOnes() {
        val ids = FolderRules.moveTargets(all, "work").map { it.id }
        assertFalse("work" in ids)
        assertFalse("sub" in ids)
        assertFalse("deep" in ids)
        for (closed in listOf("dr", "tr", "ju")) assertFalse(closed in ids)
        assertTrue("other" in ids)
        assertTrue("in" in ids && "ar" in ids && "se" in ids)
    }

    @Test fun theFolderItIsInAlreadyIsNotOffered() {
        val ids = FolderRules.moveTargets(all, "sub").map { it.id }
        assertFalse("work" in ids)
        assertTrue("other" in ids)
    }

    @Test fun theTopLevelIsOfferedOnlyToAFolderInsideAnother() {
        assertTrue(FolderRules.canMoveToTopLevel(all, "sub"))
        assertFalse(FolderRules.canMoveToTopLevel(all, "work"))
        // A parent the cache does not have counts as top level.
        assertFalse(FolderRules.canMoveToTopLevel(listOf(folder("z", parent = "gone")), "z"))
        assertFalse(FolderRules.canMoveToTopLevel(all, "nothing"))
    }

    @Test fun aFolderThatIsNotThereHasNoTargets() {
        assertEquals(emptyList<Mailbox>(), FolderRules.moveTargets(all, "nothing"))
    }

    @Test fun roleFoldersOfferOnlyNewFolderInside() {
        assertEquals(listOf(FolderMenuItem.NEW_INSIDE), FolderRules.menu(archive, withBell = false))
        assertEquals(listOf(FolderMenuItem.NEW_INSIDE), FolderRules.menu(inbox, withBell = false))
        assertEquals(listOf(FolderMenuItem.NEW_INSIDE), FolderRules.menu(sent, withBell = false))
        for (closed in listOf(drafts, trash, junk)) assertEquals(emptyList<FolderMenuItem>(), FolderRules.menu(closed, withBell = true))
    }

    @Test fun userFoldersOfferAllFour() {
        assertEquals(
            listOf(FolderMenuItem.RENAME, FolderMenuItem.MOVE, FolderMenuItem.NEW_INSIDE, FolderMenuItem.DELETE),
            FolderRules.menu(work, withBell = false),
        )
    }

    @Test fun theBellEntryIsOnlyForFoldersWithASwitchAndSaysWhatItWouldDo() {
        assertEquals(FolderMenuItem.NOTIFY_ON, FolderRules.menu(work, withBell = true).first())
        assertEquals(FolderMenuItem.NOTIFY_OFF, FolderRules.menu(folder("n", notify = true), withBell = true).first())
        assertFalse(FolderMenuItem.NOTIFY_ON in FolderRules.menu(inbox, withBell = true))
        assertEquals(listOf(FolderMenuItem.NEW_INSIDE), FolderRules.menu(inbox, withBell = true))
    }

    @Test fun newFolderInsideIsNotOfferedInDraftsTrashOrJunk() {
        for (closed in listOf(drafts, trash, junk)) assertFalse(FolderRules.canHoldFolders(closed))
        assertTrue(FolderRules.canHoldFolders(inbox))
        assertTrue(FolderRules.canHoldFolders(work))
    }

    @Test fun menuEntriesMapToDialogsAndTheBellToNone() {
        assertEquals(null, FolderRules.action(FolderMenuItem.NOTIFY_ON))
        assertEquals(null, FolderRules.action(FolderMenuItem.NOTIFY_OFF))
        assertEquals(FolderAction.DELETE, FolderRules.action(FolderMenuItem.DELETE))
        assertEquals(FolderAction.NEW_INSIDE, FolderRules.action(FolderMenuItem.NEW_INSIDE))
    }

    @Test fun deletePlanSaysWhatWillHappen() {
        val full = folder("full", total = 3u)
        val empty = folder("empty")
        val parent = folder("p", total = 5u)
        val child = folder("c", parent = "p")
        assertEquals(DeletePlan.WithMail(3), FolderRules.deletePlan(listOf(full), "full"))
        assertEquals(DeletePlan.Empty, FolderRules.deletePlan(listOf(empty), "empty"))
        // Folders inside come first, whatever the folder holds.
        assertEquals(DeletePlan.HasFolders, FolderRules.deletePlan(listOf(parent, child), "p"))
        assertEquals(null, FolderRules.deletePlan(listOf(empty), "nothing"))
    }

    @Test fun aBlankNameIsNotSent() {
        assertFalse(FolderRules.nameCanBeSent(""))
        assertFalse(FolderRules.nameCanBeSent("   "))
        assertTrue(FolderRules.nameCanBeSent(" a "))
    }
}
