package ovh.kweb.mail.background

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Test
import ovh.kweb.mail.accounts.record
import uniffi.kweb_core.Notice
import uniffi.kweb_core.NoticeKind
import uniffi.kweb_core.NoticeThing

class NoticeRulesTest {
    private fun notice(id: String, at: Long = 0L, thing: NoticeThing = NoticeThing.EVENT) =
        Notice(id, NoticeKind.EVENT_ADDED, thing, "Ann", "Lunch", null, null, false, at)

    @Test
    fun everyNoticeOfEveryAccountHasItsOwnTagUnderOneId() {
        val a = record("a1")
        val b = record("b2")
        assertNotEquals(NoticeRules.tag(a, notice("ce:1")), NoticeRules.tag(a, notice("ce:2")))
        assertNotEquals(NoticeRules.tag(a, notice("ce:1")), NoticeRules.tag(b, notice("ce:1")))
        assertEquals(NoticeRules.tag(a, notice("ce:1")), NoticeRules.tag(a, notice("ce:1")))
    }

    @Test
    fun onlyTheNewestFewArePostedAndNewestFirst() {
        val many = (1..25).map { notice("ce:$it", at = it.toLong()) }
        val shown = NoticeRules.newest(many)
        assertEquals(NoticeRules.MOST, shown.size)
        assertEquals("ce:25", shown.first().id)
        assertEquals("ce:16", shown.last().id)
    }

    @Test
    fun aTapOnAnEventOpensTheAgendaAndOnAShareSharedWithMe() {
        assertEquals(Notifier.OPEN_CALENDAR, NoticeRules.opens(notice("ce:1")))
        assertEquals(Notifier.OPEN_SHARED, NoticeRules.opens(notice("sh:1", thing = NoticeThing.CALENDAR)))
        assertEquals(Notifier.OPEN_SHARED, NoticeRules.opens(notice("sh:2", thing = NoticeThing.OTHER)))
    }
}
