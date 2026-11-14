package ovh.kweb.mail.ui.enterprise

import java.time.ZoneId
import java.util.Locale
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.kweb_core.DeletedKind

class RecoveryRulesTest {
    @Test fun itemsAreGroupedByKindInAFixedOrderAndKeepTheirOrder() {
        val items = listOf(
            deleted("f1", DeletedKind.FILE), deleted("m1"), deleted("c1", DeletedKind.CONTACT), deleted("m2"), deleted("e1", DeletedKind.EVENT),
        )
        val groups = RecoveryRules.byKind(items)
        assertEquals(listOf(DeletedKind.MAIL, DeletedKind.EVENT, DeletedKind.CONTACT, DeletedKind.FILE), groups.map { it.first })
        assertEquals(listOf("m1", "m2"), groups.first().second.map { it.id })
        assertEquals(emptyList<Any>(), RecoveryRules.byKind(emptyList()))
    }

    @Test fun daysLeftCountUpToWholeDays() {
        val day = 86_400L
        val deleted = 1_700_000_000L
        assertEquals(30, RecoveryRules.daysLeft(deleted, 30u, deleted * 1000))
        assertEquals(29, RecoveryRules.daysLeft(deleted, 30u, (deleted + day) * 1000))
        assertEquals(1, RecoveryRules.daysLeft(deleted, 30u, (deleted + 29 * day + 10) * 1000))
        assertEquals(0, RecoveryRules.daysLeft(deleted, 30u, (deleted + 31 * day) * 1000))
        assertNull(RecoveryRules.daysLeft(null, 30u, 0))
        assertNull(RecoveryRules.daysLeft(deleted, null, 0))
    }

    @Test fun theDayDeletedIsShortDate() {
        assertEquals("14 Nov", RecoveryRules.deletedDay(1_700_000_000L, ZoneId.of("UTC"), Locale.ROOT))
    }

    @Test fun anotherAccountNeedsAnId() {
        assertNull(AccountChoice().accountId)
        assertTrue(AccountChoice().isValid)
        assertEquals("abc", AccountChoice(true, " abc ").accountId)
        assertFalse(AccountChoice(true, "  ").isValid)
    }
}
