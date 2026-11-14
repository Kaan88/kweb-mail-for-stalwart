package ovh.kweb.mail.ui.accounts

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import ovh.kweb.mail.accounts.AccountRecord

class AccountRowsTest {
    private fun record(id: String, user: String, server: String = "https://mail.kweb.ovh") =
        AccountRecord(id, false, user, server, "j-$id", 0)

    @Test
    fun rowsFollowTheRegistryAndMarkTheActiveOne() {
        val rows = AccountRows.build(
            listOf(record("a", "kaan"), record("b", "bob", "https://mail.example.org:8443")),
            activeId = "b",
            unread = mapOf("a" to 3, "b" to 120),
            needsSignIn = setOf("a"),
        )
        assertEquals(listOf("a", "b"), rows.map { it.id })
        assertEquals(listOf(false, true), rows.map { it.active })
        assertEquals(listOf(true, false), rows.map { it.needsSignIn })
        assertEquals("mail.example.org", rows[1].host)
        assertEquals(listOf(3, 120), rows.map { it.unread })
    }

    @Test
    fun anUnknownCountStaysUnknown() {
        val rows = AccountRows.build(listOf(record("a", "kaan")), "a", emptyMap(), emptySet())
        assertNull(rows[0].unread)
        assertNull(AccountRows.badge(rows[0].unread))
    }

    @Test
    fun twoEqualLoginsAreToldApart() {
        val rows = AccountRows.build(
            listOf(record("a", "kaan"), record("b", "kaan", "https://mail.example.org")),
            "a", emptyMap(), emptySet(),
        )
        assertEquals("kaan · mail.kweb.ovh", rows[0].label)
        assertEquals("kaan · mail.example.org", rows[1].label)
    }

    @Test
    fun theInitialIsTheFirstLetterOrDigitInCapitals() {
        assertEquals("K", AccountRows.initial("kaan@kweb.ovh"))
        assertEquals("7", AccountRows.initial("_7up"))
        assertEquals("?", AccountRows.initial(""))
        assertEquals("?", AccountRows.initial("--"))
    }

    @Test
    fun theBadgeIsQuietForNoneAndCapsLargeNumbers() {
        assertNull(AccountRows.badge(0))
        assertEquals("1", AccountRows.badge(1))
        assertEquals("99", AccountRows.badge(99))
        assertEquals("99+", AccountRows.badge(100))
    }

    @Test
    fun accountNamesShowOnlyWithSeveralAccounts() {
        assertFalse(AccountRows.showsAccountNames(0))
        assertFalse(AccountRows.showsAccountNames(1))
        assertTrue(AccountRows.showsAccountNames(2))
    }
}
