package ovh.kweb.mail.ui.compose

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class TranslateRulesTest {
    @Test
    fun the_list_has_english_names_and_own_names() {
        val turkish = TranslateRules.all.first { it.name == "Turkish" }
        assertEquals("Türkçe", turkish.native)
        assertNull(TranslateRules.all.first { it.name == "English" }.native)
        assertEquals(TranslateRules.all.size, TranslateRules.all.map { it.name }.distinct().size)
    }

    @Test
    fun search_matches_either_name() {
        assertTrue(TranslateRules.filter("türk").any { it.name == "Turkish" })
        assertTrue(TranslateRules.filter("GERM").any { it.name == "German" })
        assertEquals(TranslateRules.all, TranslateRules.filter("  "))
    }

    @Test
    fun a_typed_language_is_offered_only_when_not_in_the_list() {
        assertEquals("Klingon", TranslateRules.typed(" Klingon "))
        assertNull(TranslateRules.typed("german"))
        assertNull(TranslateRules.typed(""))
    }

    @Test
    fun recent_keeps_the_newest_first_without_repeats() {
        assertEquals(listOf("German", "Turkish", "French"), TranslateRules.remember(listOf("Turkish", "german", "French"), "German"))
        assertEquals(TranslateRules.RECENT, TranslateRules.remember(listOf("a", "b", "c", "d"), "e").size)
    }
}
