package ovh.kweb.mail.background

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import ovh.kweb.mail.accounts.record

class IntentAccountTest {
    private val legacy = record("a1", legacy = true)
    private val other = record("b2")

    @Test
    fun aRegisteredIdGivesThatAccount() {
        assertEquals(other, IntentAccount.resolve("b2", listOf(legacy, other)))
        assertEquals(legacy, IntentAccount.resolve("a1", listOf(legacy, other)))
    }

    @Test
    fun anUnknownIdGivesNothing() {
        assertNull(IntentAccount.resolve("gone", listOf(legacy, other)))
    }

    @Test
    fun noExtraGivesTheLegacyAccountBecauseTheOldVersionPostedThose() {
        assertEquals(legacy, IntentAccount.resolve(null, listOf(other, legacy)))
    }

    @Test
    fun noExtraAndNoLegacyAccountGivesNothing() {
        assertNull(IntentAccount.resolve(null, listOf(other)))
        assertNull(IntentAccount.resolve(null, emptyList()))
    }
}
