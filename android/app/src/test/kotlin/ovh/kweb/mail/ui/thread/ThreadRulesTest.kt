package ovh.kweb.mail.ui.thread

import java.util.Locale
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.kweb_core.Address
import uniffi.kweb_core.Mailbox
import uniffi.kweb_core.MessageHeader

class ThreadRulesTest {
    private fun msg(id: String, unread: Boolean = false, boxes: List<String> = listOf("inbox")) = MessageHeader(
        id, "t", boxes, "", "", emptyList(), "", "", 0, unread, false, false, 0u,
    )

    private fun addr(name: String, email: String) = Address(name, email)

    @Test fun theNamedMessageAloneStartsOpen() =
        assertEquals(setOf("b"), ThreadRules.initiallyExpanded(listOf(msg("a", true), msg("b"), msg("c")), "b"))

    @Test fun withoutANameUnreadMessagesAndTheLastOpen() =
        assertEquals(setOf("a", "c", "d"), ThreadRules.initiallyExpanded(listOf(msg("a", true), msg("b"), msg("c", true), msg("d")), null))

    @Test fun aNamedMessageThatIsNotThereFallsBackToTheRule() =
        assertEquals(setOf("b"), ThreadRules.initiallyExpanded(listOf(msg("a"), msg("b")), "gone"))

    @Test fun anEmptyThreadHasNothingOpen() = assertEquals(emptySet<String>(), ThreadRules.initiallyExpanded(emptyList(), null))

    @Test fun aThreadIsGoneOnlyAfterTheServerAnswered() {
        assertFalse(ThreadRules.isGone(loaded = false, refreshed = true, count = 0))
        assertFalse(ThreadRules.isGone(loaded = true, refreshed = false, count = 0))
        assertFalse(ThreadRules.isGone(loaded = true, refreshed = true, count = 1))
        assertTrue(ThreadRules.isGone(loaded = true, refreshed = true, count = 0))
    }

    private fun box(id: String, role: String?) = Mailbox(id, id, role, null, 0u, 0u, 0u)

    private val boxes = listOf(
        box("inbox", "inbox"), box("sent", "sent"), box("drafts", "drafts"), box("trash", "trash"), box("work", null),
    )

    @Test fun actionsTargetTheMessagesInTheFolderTheThreadWasOpenedFrom() {
        val messages = listOf(msg("a", boxes = listOf("inbox")), msg("b", boxes = listOf("sent")), msg("c", boxes = listOf("inbox", "x")))
        assertEquals(listOf("a", "c"), ThreadRules.targets(messages, "inbox", boxes))
    }

    @Test fun fromASearchTrashDraftsAndSentAreLeftOut() {
        val messages = listOf(
            msg("a", boxes = listOf("inbox")), msg("b", boxes = listOf("sent")),
            msg("c", boxes = listOf("trash")), msg("d", boxes = listOf("drafts")), msg("e", boxes = listOf("work")),
        )
        assertEquals(listOf("a", "e"), ThreadRules.targets(messages, null, boxes))
    }

    @Test fun fromASearchAThreadOnlyInSpecialFoldersIsTargetedWhole() {
        val messages = listOf(msg("b", boxes = listOf("sent")), msg("c", boxes = listOf("trash")))
        assertEquals(listOf("b", "c"), ThreadRules.targets(messages, null, boxes))
    }

    @Test fun noMessagesInTheFolderMeansNoTargets() =
        assertEquals(emptyList<String>(), ThreadRules.targets(listOf(msg("a", boxes = listOf("inbox"))), "work", boxes))

    @Test fun deleteIsForeverWhenATargetIsInTrash() {
        val messages = listOf(msg("a", boxes = listOf("inbox")), msg("c", boxes = listOf("trash")))
        assertTrue(ThreadRules.deletesForever(messages, listOf("a", "c"), boxes))
        assertFalse(ThreadRules.deletesForever(messages, listOf("a"), boxes))
        assertFalse(ThreadRules.deletesForever(messages, listOf("c"), emptyList()))
    }

    @Test fun meComesFirstThenFirstNames() = assertEquals(
        "me, Ayşe",
        ThreadRules.recipientNames(listOf(addr("Ayşe Demir", "a@x.org"), addr("Tester", "ME@x.org")), emptyList(), "me@x.org", "me"),
    )

    @Test fun ccIsIncludedAndDuplicatesAreNot() = assertEquals(
        "Ali, b@x.org",
        ThreadRules.recipientNames(listOf(addr("Ali Veli", "a@x.org")), listOf(addr("", "b@x.org"), addr("Ali", "A@x.org")), null, "me"),
    )

    @Test fun manyRecipientsAreCounted() = assertEquals(
        "A, B, C +2",
        ThreadRules.recipientNames(listOf("A", "B", "C", "D", "E").map { addr(it, "$it@x.org") }, emptyList(), null, "me"),
    )

    @Test fun noRecipientsIsNull() = assertNull(ThreadRules.recipientNames(emptyList(), emptyList(), null, "me"))

    @Test fun fullAddressHasTheNameWhenThereIsOne() {
        assertEquals("Ali <a@x.org>", ThreadRules.fullAddress(addr(" Ali ", "a@x.org")))
        assertEquals("a@x.org", ThreadRules.fullAddress(addr("", "a@x.org")))
    }

    @Test fun smallSizesAreBytes() = assertEquals("812 B", SizeFormat.format(812, Locale.UK))

    @Test fun largerSizesHaveOneDecimal() {
        assertEquals("1.5 KB", SizeFormat.format(1_500, Locale.UK))
        assertEquals("23.4 MB", SizeFormat.format(23_400_000, Locale.UK))
    }

    @Test fun aSizeThatRoundsUpMovesToTheNextUnit() = assertEquals("1.0 MB", SizeFormat.format(999_999, Locale.UK))

    @Test fun theLocaleSetsTheDecimalSeparator() = assertEquals("1,5 KB", SizeFormat.format(1_500, Locale.GERMANY))

    @Test fun repliesActOnTheNewestMessage() =
        assertEquals("c", ThreadRules.replyTarget(listOf(msg("a"), msg("b"), msg("c")), boxes)?.id)

    @Test fun aDraftSavedInTheThreadIsNotAnswered() {
        val messages = listOf(msg("a"), msg("b"), msg("draft", boxes = listOf("drafts")))
        assertEquals("b", ThreadRules.replyTarget(messages, listOf(box("inbox", "inbox"), box("drafts", "drafts")))?.id)
    }

    @Test fun whenEveryMessageIsADraftTheNewestOfThemIsUsed() {
        val messages = listOf(msg("d1", boxes = listOf("drafts")), msg("d2", boxes = listOf("drafts")))
        assertEquals("d2", ThreadRules.replyTarget(messages, listOf(box("drafts", "drafts")))?.id)
    }

    @Test fun anEmptyThreadHasNothingToAnswer() = assertNull(ThreadRules.replyTarget(emptyList(), boxes))
}
