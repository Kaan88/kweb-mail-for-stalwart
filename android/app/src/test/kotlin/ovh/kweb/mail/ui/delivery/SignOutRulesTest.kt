package ovh.kweb.mail.ui.delivery

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.kweb_core.Draft
import uniffi.kweb_core.PendingSend

class SignOutRulesTest {
    private fun draft(id: String, serverId: String? = null) =
        Draft(id, "i", emptyList(), emptyList(), emptyList(), "", "", emptyList(), null, false, serverId, null, 0)

    @Test fun nothingUnsentIsNotAny() = assertFalse(SignOutRules.unsent(emptyList(), emptyList()).any)

    @Test fun draftsOnTheServerAreNotLost() =
        assertEquals(Unsent(0, 1), SignOutRules.unsent(emptyList(), listOf(draft("a"), draft("b", "srv"))))

    @Test fun aHeldMessageCountsOnceAsAMessage() {
        val unsent = SignOutRules.unsent(listOf(PendingSend("a", "s", 1)), listOf(draft("a"), draft("b")))
        assertEquals(Unsent(1, 1), unsent)
        assertTrue(unsent.any)
    }
}
