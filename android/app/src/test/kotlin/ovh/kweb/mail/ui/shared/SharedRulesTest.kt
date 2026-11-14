package ovh.kweb.mail.ui.shared

import java.time.LocalDate
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import ovh.kweb.mail.ui.BackStack
import ovh.kweb.mail.ui.Route
import ovh.kweb.mail.ui.UiMessage
import ovh.kweb.mail.ui.calendar.AgendaWindow
import ovh.kweb.mail.ui.thread.FileNames
import uniffi.kweb_core.Exception as CoreException
import uniffi.kweb_core.MessageHeader
import uniffi.kweb_core.SharedAccount
import uniffi.kweb_core.SharedAction
import uniffi.kweb_core.SharedMailbox

internal fun box(
    id: String,
    name: String = id,
    parent: String? = null,
    mark: Boolean = false,
    add: Boolean = false,
    remove: Boolean = false,
    unread: Int = 0,
) = SharedMailbox(id, name, parent, 10u, unread.toUInt(), mark, add, remove)

internal fun header(id: String, unread: Boolean = false, flagged: Boolean = false) =
    MessageHeader(id, "t$id", listOf("b"), "", "", emptyList(), "s", "p", 0, unread, flagged, false, 0u)

class SharedRulesTest {
    // ---- the folder tree

    @Test fun aChildFollowsItsSharedParent() {
        val tree = SharedRules.tree(listOf(box("c", "Child", parent = "p"), box("p", "Parent"), box("z", "Zeta")))
        assertEquals(listOf("p" to 0, "c" to 1, "z" to 0), tree.map { it.mailbox.id to it.depth })
    }

    @Test fun aChildWhoseParentIsNotSharedIsTopLevel() {
        val tree = SharedRules.tree(listOf(box("c", "Child", parent = "hidden")))
        assertEquals(listOf("c" to 0), tree.map { it.mailbox.id to it.depth })
    }

    @Test fun siblingsAreByNameWithoutRegardToCase() {
        val tree = SharedRules.tree(listOf(box("1", "beta"), box("2", "Alpha"), box("3", "Gamma")))
        assertEquals(listOf("Alpha", "beta", "Gamma"), tree.map { it.mailbox.name })
    }

    @Test fun deepNestingAndACycleLoseNothing() {
        val deep = SharedRules.tree(listOf(box("a"), box("b", parent = "a"), box("c", parent = "b")))
        assertEquals(listOf(0, 1, 2), deep.map { it.depth })
        val cycle = SharedRules.tree(listOf(box("x", parent = "y"), box("y", parent = "x"), box("self", parent = "self")))
        assertEquals(setOf("x", "y", "self"), cycle.map { it.mailbox.id }.toSet())
        assertEquals(3, cycle.size)
    }

    @Test fun noFoldersNoTree() = assertEquals(emptyList<SharedFolderEntry>(), SharedRules.tree(emptyList()))

    // ---- rights

    private val here = box("here", mark = true, add = true, remove = true)
    private val target = box("target", add = true)
    private val readOnlyTarget = box("ro")

    @Test fun moveTargetsTakeMessagesAndNotTheFolderItself() {
        val targets = SharedRules.moveTargets(listOf(here, target, readOnlyTarget), here)
        assertEquals(listOf("target"), targets.map { it.mailbox.id })
    }

    @Test fun moveTargetsAreATreeOfTheirOwn() {
        val parent = box("p", "P", add = false)
        val child = box("c", "C", parent = "p", add = true)
        val targets = SharedRules.moveTargets(listOf(parent, child), null)
        assertEquals(listOf("c" to 0), targets.map { it.mailbox.id to it.depth })
    }

    @Test fun everythingIsAllowedWithAllRights() {
        val folders = listOf(here, target)
        assertTrue(SharedRules.allows(SharedActionKind.MARK, here, folders))
        assertTrue(SharedRules.allows(SharedActionKind.DELETE, here, folders))
        assertTrue(SharedRules.allows(SharedActionKind.MOVE, here, folders))
    }

    @Test fun aReadOnlyFolderAllowsNothing() {
        val ro = box("ro")
        for (kind in SharedActionKind.entries) assertFalse(kind.name, SharedRules.allows(kind, ro, listOf(ro, target)))
    }

    @Test fun movingNeedsTheRightToRemoveHereAndAPlaceToAdd() {
        val addOnly = box("x", add = true)
        assertFalse(SharedRules.allows(SharedActionKind.MOVE, addOnly, listOf(addOnly, target)))
        // May remove, but no other folder takes messages.
        val removeOnly = box("y", remove = true)
        assertFalse(SharedRules.allows(SharedActionKind.MOVE, removeOnly, listOf(removeOnly, readOnlyTarget)))
        assertTrue(SharedRules.allows(SharedActionKind.DELETE, removeOnly, listOf(removeOnly)))
    }

    @Test fun markingIsIndependentOfRemoving() {
        val markOnly = box("m", mark = true)
        assertTrue(SharedRules.allows(SharedActionKind.MARK, markOnly, listOf(markOnly)))
        assertFalse(SharedRules.allows(SharedActionKind.DELETE, markOnly, listOf(markOnly)))
    }

    @Test fun anUnknownFolderAllowsNothing() {
        for (kind in SharedActionKind.entries) assertFalse(SharedRules.allows(kind, null, listOf(here, target)))
    }

    @Test fun actionsAreKindsOfChange() {
        assertEquals(SharedActionKind.MARK, SharedRules.kindOf(SharedAction.MarkRead))
        assertEquals(SharedActionKind.MARK, SharedRules.kindOf(SharedAction.MarkUnread))
        assertEquals(SharedActionKind.MARK, SharedRules.kindOf(SharedAction.Flag))
        assertEquals(SharedActionKind.MARK, SharedRules.kindOf(SharedAction.Unflag))
        assertEquals(SharedActionKind.MOVE, SharedRules.kindOf(SharedAction.Move("x")))
        assertEquals(SharedActionKind.DELETE, SharedRules.kindOf(SharedAction.Delete))
    }

    @Test fun theMenuTogglesWhatIsThere() {
        assertEquals(SharedAction.MarkRead, SharedRules.toggleRead(header("a", unread = true)))
        assertEquals(SharedAction.MarkUnread, SharedRules.toggleRead(header("a", unread = false)))
        assertEquals(SharedAction.Unflag, SharedRules.toggleFlag(header("a", flagged = true)))
        assertEquals(SharedAction.Flag, SharedRules.toggleFlag(header("a", flagged = false)))
    }

    // ---- errors and accounts

    @Test fun aRefusalByTheCoreMeansTheShareIsGone() {
        assertTrue(SharedRules.isGone(CoreException.Invalid("no such account")))
        assertFalse(SharedRules.isGone(CoreException.Temporary("offline")))
        assertFalse(SharedRules.isGone(IllegalStateException()))
    }

    @Test fun theCoresWordsAreShownAsTheyAre() {
        assertEquals(UiMessage.Text("You may not do that"), SharedRules.message(CoreException.Invalid("You may not do that")))
        assertEquals(UiMessage.Text("try later"), SharedRules.message(CoreException.Temporary("try later")))
        assertTrue(SharedRules.message(CoreException.Invalid("")) is UiMessage.Res)
        assertTrue(SharedRules.message(IllegalStateException("secret")) is UiMessage.Res)
    }

    @Test fun anAccountListsOnlyTheKindsItShares() {
        val account = SharedAccount("a", "boss@example.org", hasMail = true, hasCalendars = false, hasContacts = true)
        assertEquals(listOf(SharedRules.Kind.MAIL, SharedRules.Kind.CONTACTS), SharedRules.kinds(account))
    }

    @Test fun anAccountThatSharesNothingIsNotListed() {
        val empty = SharedAccount("e", "x", hasMail = false, hasCalendars = false, hasContacts = false)
        val some = SharedAccount("s", "y", hasMail = false, hasCalendars = true, hasContacts = false)
        assertEquals(listOf(some), SharedRules.listed(listOf(empty, some)))
    }

    // ---- paging

    @Test fun thereIsMoreUntilEverythingIsLoaded() {
        assertTrue(SharedRules.hasMore(50, 120))
        assertFalse(SharedRules.hasMore(120, 120))
        assertFalse(SharedRules.hasMore(0, 0))
    }

    @Test fun readingAgainKeepsEverythingThatWasShown() {
        assertEquals(50, SharedRules.reloadCount(0))
        assertEquals(50, SharedRules.reloadCount(50))
        assertEquals(80, SharedRules.reloadCount(80))
        assertEquals(350, SharedRules.reloadCount(350))
    }

    @Test fun theCoreGetsOnlyAnIanaZoneName() {
        assertEquals("Europe/Berlin", SharedRules.coreZone(java.time.ZoneId.of("Europe/Berlin")))
        assertEquals("UTC", SharedRules.coreZone(java.time.ZoneId.of("UTC")))
        assertEquals("Etc/GMT-3", SharedRules.coreZone(java.time.ZoneId.of("GMT+03:00")))
        assertEquals("Etc/GMT+5", SharedRules.coreZone(java.time.ZoneId.of("-05:00")))
        assertEquals("UTC", SharedRules.coreZone(java.time.ZoneId.of("GMT+05:30")))
        assertEquals("UTC", SharedRules.coreZone(java.time.ZoneId.of("GMT")))
    }

    @Test fun aPageIsAppendedWithoutDuplicates() {
        val merged = SharedRules.append(listOf(header("a"), header("b")), listOf(header("b"), header("c")))
        assertEquals(listOf("a", "b", "c"), merged.map { it.id })
    }

    // ---- keys

    @Test fun theSameIdInTwoAccountsIsTwoKeys() {
        assertNotEquals(SharedRules.key("a", "1"), SharedRules.key("b", "1"))
        assertNotEquals(SharedRules.key("a", "bc"), SharedRules.key("ab", "c"))
    }

    @Test fun routesOfTwoAccountsAreNotTheSameRoute() {
        assertNotEquals(Route.SharedMail("a", "box", "m1"), Route.SharedMail("b", "box", "m1"))
        assertNotEquals(Route.SharedFolder("a", "1"), Route.SharedFolder("b", "1"))
        assertNotEquals(Route.SharedEventView("a", "e", null), Route.SharedEventView("b", "e", null))
        assertNotEquals(Route.SharedContactView("a", "c"), Route.SharedContactView("b", "c"))
        // So one account's message is never taken for another's by the stack, a double tap or a store.
        val stack = BackStack.push(BackStack.push(BackStack.initial, Route.SharedMail("a", "box", "m")), Route.SharedMail("b", "box", "m"))
        assertEquals(3, stack.size)
    }

    @Test fun aBlobOfASharedAccountHasItsOwnCacheName() {
        assertEquals("blob1", FileNames.blobKey(null, "blob1"))
        assertNotEquals(FileNames.blobKey("a", "blob1"), FileNames.blobKey("b", "blob1"))
        assertNotEquals(FileNames.blobKey("a", "blob1"), FileNames.blobKey(null, "blob1"))
        assertNotEquals(FileNames.blobKey("a", "bc"), FileNames.blobKey("ab", "c"))
        assertEquals(FileNames.blobKey("a", "x"), SharedRules.blobKey("a", "x"))
        assertNotEquals(FileNames.sha256Hex(FileNames.blobKey("a", "x")), FileNames.sha256Hex(FileNames.blobKey(null, "x")))
    }

    // ---- the agenda's span

    @Test fun theFirstSpanIsThirtyDaysFromToday() {
        val span = SharedRules.firstSpan(LocalDate.of(2026, 11, 14))
        assertEquals(LocalDate.of(2026, 11, 14), span.from)
        assertEquals(LocalDate.of(2026, 12, 14), span.to)
        assertEquals(30L, span.days)
    }

    @Test fun loadingMoreAddsThirtyDaysAtTheEnd() {
        val first = SharedRules.firstSpan(LocalDate.of(2026, 11, 14))
        val next = SharedRules.extend(first)
        assertEquals(first.from, next.from)
        assertEquals(60L, next.days)
        assertTrue(SharedRules.canExtend(next))
    }

    @Test fun theSpanNeverPassesWhatTheCoreTakes() {
        var span = SharedRules.firstSpan(LocalDate.of(2026, 1, 1))
        repeat(20) { span = SharedRules.extend(span) }
        assertEquals(AgendaWindow.MAX_DAYS, span.days)
        assertFalse(SharedRules.canExtend(span))
        assertEquals(span, SharedRules.extend(span))
    }

    // ---- going back when the share is gone

    @Test fun goneGoesBackToTheSharedScreen() {
        val stack = listOf(
            Route.MailList, Route.Shared, Route.SharedFolders("a", "boss"), Route.SharedFolder("a", "1"), Route.SharedMail("a", "1", "m"),
        )
        assertEquals(listOf(Route.MailList, Route.Shared), BackStack.backToShared(stack))
    }

    @Test fun goneWithoutASharedScreenOnTheStackGoesToTheRoot() =
        assertEquals(BackStack.initial, BackStack.backToShared(listOf(Route.MailList, Route.SharedFolder("a", "1"))))

    @Test fun theSharedScreensAreNotRootsOfTheBottomBar() {
        listOf(Route.Shared, Route.SharedFolders("a", "o"), Route.SharedCalendar("a", "o"), Route.SharedContacts("a", "o")).forEach {
            assertFalse(BackStack.isRoot(it))
        }
    }
}
