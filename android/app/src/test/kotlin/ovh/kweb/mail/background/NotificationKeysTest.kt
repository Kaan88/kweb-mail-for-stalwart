package ovh.kweb.mail.background

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import ovh.kweb.mail.accounts.record

class NotificationKeysTest {
    private val legacy = record("a1", legacy = true, jmap = "d333")
    private val other = record("b2", jmap = "d333")
    private val third = record("c3", jmap = "d333")

    @Test
    fun theLegacyAccountKeepsNoTagTheOldGroupAndTheOldUris() {
        assertNull(NotificationKeys.tag(legacy))
        assertEquals("ovh.kweb.mail.mail.d333", NotificationKeys.group(legacy))
        assertEquals("kwebmail://notification/message/m1", NotificationKeys.uriText(legacy, "message", "m1"))
        assertEquals(listOf("reply", "m1"), NotificationKeys.uriSegments(legacy, "reply", "m1"))
    }

    @Test
    fun otherAccountsHaveTheirOwnTagGroupAndUris() {
        assertEquals("b2", NotificationKeys.tag(other))
        assertEquals("ovh.kweb.mail.mail.local-b2", NotificationKeys.group(other))
        assertEquals("kwebmail://notification/a/b2/message/m1", NotificationKeys.uriText(other, "message", "m1"))
    }

    @Test
    fun theSameMessageIdInTwoAccountsDiffersInTagAndUri() {
        // The core's ids repeat between accounts; (tag, id) and the URI must not.
        assertNotEquals(NotificationKeys.tag(legacy), NotificationKeys.tag(other))
        assertNotEquals(NotificationKeys.tag(other), NotificationKeys.tag(third))
        assertNotEquals(NotificationKeys.uriText(legacy, "message", "m1"), NotificationKeys.uriText(other, "message", "m1"))
        assertNotEquals(NotificationKeys.uriText(other, "message", "m1"), NotificationKeys.uriText(third, "message", "m1"))
        assertNotEquals(NotificationKeys.group(other), NotificationKeys.group(third))
    }

    @Test
    fun theLegacyAccountOwnsItsSummaryItsRemindersAndItsMailButNotTheAppsOwnEntries() {
        assertTrue(NotificationKeys.owns(legacy, null, 1))
        assertTrue(NotificationKeys.owns(legacy, null, 100))
        assertTrue(NotificationKeys.owns(legacy, null, 899))
        assertTrue(NotificationKeys.owns(legacy, null, 1000))
        assertTrue(NotificationKeys.owns(legacy, null, 123456))
        for (id in listOf(0, 2, 3, 4, 99, 900, 999)) assertFalse("id $id", NotificationKeys.owns(legacy, null, id))
    }

    @Test
    fun theLegacyAccountNeverOwnsATaggedNotification() {
        assertFalse(NotificationKeys.owns(legacy, "b2", 1000))
        assertFalse(NotificationKeys.owns(legacy, "anything", 1))
    }

    @Test
    fun anotherAccountOwnsExactlyWhatCarriesItsTag() {
        assertTrue(NotificationKeys.owns(other, "b2", 1))
        assertTrue(NotificationKeys.owns(other, "b2", 1000))
        assertFalse(NotificationKeys.owns(other, "c3", 1000))
        assertFalse(NotificationKeys.owns(other, null, 1000))
        for (id in listOf(2, 3, 4)) assertFalse("untagged id $id is the app's", NotificationKeys.owns(other, null, id))
    }
}
