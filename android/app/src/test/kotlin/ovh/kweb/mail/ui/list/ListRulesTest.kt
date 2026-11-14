package ovh.kweb.mail.ui.list

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.kweb_core.Draft
import uniffi.kweb_core.PendingSend

class ListRulesTest {
    private fun draft(id: String, error: String? = null) =
        Draft(id, "i", emptyList(), emptyList(), emptyList(), "", "", emptyList(), null, false, null, error, 0)

    @Test fun aTapInDraftsOpensComposeAndAnywhereElseTheThread() {
        assertTrue(ListRules.opensCompose("drafts"))
        for (role in listOf("inbox", "sent", "archive", "trash", "junk", null)) assertFalse(ListRules.opensCompose(role))
    }

    @Test fun theNewestRefusedDraftIsTheOneTheBarOpens() =
        assertEquals("b", ListRules.failedDraft(listOf(draft("a"), draft("b", "refused"), draft("c", "refused")))?.id)

    @Test fun noRefusedDraftNoBar() = assertNull(ListRules.failedDraft(listOf(draft("a"), draft("b"))))

    @Test fun waitingCountsEveryPendingMessage() {
        assertEquals(0, ListRules.waiting(emptyList()))
        assertEquals(2, ListRules.waiting(listOf(PendingSend("a", "s", 1), PendingSend("b", "s", 2))))
    }

    @Test fun theHeldMessageIsTheLastOneStillInTheFuture() {
        val pending = listOf(PendingSend("a", "s", 100), PendingSend("b", "s", 108), PendingSend("c", "s", 90))
        assertEquals("b", ListRules.holding(pending, 99_000)?.draftId)
        assertEquals("b", ListRules.holding(pending, 105_000)?.draftId)
        assertNull(ListRules.holding(pending, 108_000))
        assertNull(ListRules.holding(emptyList(), 0))
    }
}
