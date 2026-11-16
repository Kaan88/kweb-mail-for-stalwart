package ovh.kweb.mail.ui.catchup

import org.junit.Assert.assertEquals
import org.junit.Test
import uniffi.kweb_core.AiCatchUpItem
import uniffi.kweb_core.AiMessageRef

class CatchUpRulesTest {
    private fun item(id: String, needsAnswer: Boolean) =
        AiCatchUpItem(AiMessageRef(id, "t-$id", "Subject $id", "Ada <ada@example.org>", "Inbox"), "line $id", needsAnswer)

    @Test fun onlyTheItemsThatAskForAnAnswerAreCounted() {
        assertEquals(0, CatchUpRules.needingAnswer(emptyList()))
        assertEquals(2, CatchUpRules.needingAnswer(listOf(item("a", true), item("b", false), item("c", true))))
    }

    @Test fun theFinderHasItsOwnIdThatIsNotAFindingTheCoreMakes() {
        // The core's findings have ids like "rule-hash"; this one is never kept or dismissed in the core.
        assertEquals("catchup-finder", CatchUpRules.FINDER_ID)
    }
}
