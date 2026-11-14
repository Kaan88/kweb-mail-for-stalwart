package ovh.kweb.mail.ui.search

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class SearchRulesTest {
    @Test fun twoCharactersAreEnough() = assertTrue(SearchRules.isSearchable("ab"))

    @Test fun oneCharacterIsNot() = assertFalse(SearchRules.isSearchable("a"))

    @Test fun blanksDoNotCount() {
        assertFalse(SearchRules.isSearchable("  a "))
        assertFalse(SearchRules.isSearchable("   "))
        assertTrue(SearchRules.isSearchable(" ab "))
    }

    @Test fun aSurrogatePairIsOneCharacter() {
        assertFalse(SearchRules.isSearchable("𝐀"))
        assertTrue(SearchRules.isSearchable("𝐀b"))
    }
}
