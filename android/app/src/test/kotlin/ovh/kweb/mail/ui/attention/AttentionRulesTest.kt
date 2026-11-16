package ovh.kweb.mail.ui.attention

import java.time.LocalDate
import java.time.ZoneId
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.kweb_core.AttentionFamily
import uniffi.kweb_core.AttentionRule
import uniffi.kweb_core.HomeTile
import uniffi.kweb_core.PhoneAction
import uniffi.kweb_core.ServiceAction

class AttentionRulesTest {
    private val overlap = finding(
        AttentionRule.OVERLAP, AttentionFamily.CALENDAR, HomeTile.NEXT_EVENT,
        phone = listOf(PhoneAction.MoveEvent("e2", null), PhoneAction.FindSlot("e2", null)),
    )
    private val overdue = finding(
        AttentionRule.OVERDUE, AttentionFamily.TASKS, HomeTile.DUE_TODAY, count = 3,
        phone = listOf(PhoneAction.MoveToday(listOf("t1"))), service = ServiceAction.PLAN_MY_DAY,
    )
    private val finish = finding(
        AttentionRule.MAIL_MAY_FINISH, AttentionFamily.TASKS, HomeTile.DUE_TODAY, id = "mf", threadId = "th1", messageIds = listOf("m1"),
        phone = listOf(PhoneAction.OpenMail("m1", "th1"), PhoneAction.MarkDone("t9")), service = ServiceAction.CHECK_THE_MAIL,
    )
    private val date = finding(
        AttentionRule.DATE_IN_MAIL, AttentionFamily.MAIL, HomeTile.UNREAD, id = "dm", threadId = "th2", messageIds = listOf("m2"),
        phone = listOf(PhoneAction.OpenMail("m2", "th2")),
    )
    private val all = listOf(overlap, overdue, finish, date)

    @Test fun placesPickTheirFamilies() {
        assertEquals(all, AttentionRules.forPlace(all, Place.Home))
        assertEquals(listOf(overlap), AttentionRules.forPlace(all, Place.Calendar))
        assertEquals(listOf(overdue, finish), AttentionRules.forPlace(all, Place.Tasks))
        assertEquals(listOf(date), AttentionRules.forPlace(all, Place.MailRows))
    }

    @Test fun aThreadPlaceIsTheMailFindingsOfThatThread() {
        assertEquals(listOf(date), AttentionRules.forPlace(all, Place.Thread("th2")))
        assertTrue(AttentionRules.forPlace(all, Place.Thread("other")).isEmpty())
        // A task finding about a mail is not a mail finding of its thread.
        assertTrue(AttentionRules.forPlace(all, Place.Thread("th1")).isEmpty())
    }

    @Test fun theSheetGroupsFollowTheTilesAndSkipEmptyOnes() {
        assertEquals(listOf(Group.MAIL, Group.CALENDAR, Group.TASKS), AttentionRules.groups(all).map { it.first })
        assertEquals(listOf(Group.TASKS), AttentionRules.groups(listOf(overdue)).map { it.first })
        assertTrue(AttentionRules.groups(emptyList()).isEmpty())
    }

    @Test fun aTileCountsItsOwnFindings() {
        assertEquals(1, AttentionRules.count(all, HomeTile.UNREAD))
        assertEquals(1, AttentionRules.count(all, HomeTile.NEXT_EVENT))
        assertEquals(2, AttentionRules.count(all, HomeTile.DUE_TODAY))
        assertEquals(Group.CALENDAR, AttentionRules.groupOfTile(HomeTile.NEXT_EVENT))
    }

    @Test fun theUnreadTileMarksTheMailsOfItsFindingsOnly() =
        assertEquals(setOf("m2"), AttentionRules.markedMessages(all))

    @Test fun withoutAServiceTheFirstTwoPhoneActionsAreShown() {
        val b = AttentionRules.buttons(overlap)
        assertEquals(overlap.phone, b.phone)
        assertNull(b.service)
    }

    @Test fun withAServiceOnePhoneActionIsShownBesideIt() {
        val b = AttentionRules.buttons(finish)
        assertEquals(listOf<PhoneAction>(PhoneAction.MarkDone("t9")), b.phone)
        assertEquals(ServiceAction.CHECK_THE_MAIL, b.service)
    }

    @Test fun aCardOfAMailOpensTheMailOnATap() {
        assertEquals(PhoneAction.OpenMail("m2", "th2"), AttentionRules.cardAction(date))
        assertNull(AttentionRules.cardAction(overlap))
    }

    @Test fun movingATaskKeepsItsTimeOfDay() {
        val today = LocalDate.of(2026, 11, 17)
        assertEquals("2026-11-17T10:00:00", AttentionRules.movedToToday("2026-11-14T10:00:00", today))
        assertEquals("2026-11-17T00:00:00", AttentionRules.movedToToday("2026-11-14T00:00:00", today))
        assertEquals("2026-11-17T00:00:00", AttentionRules.movedToToday("", today))
    }

    @Test fun theEditorGetsALocalStartWithSeconds() {
        val zone = ZoneId.of("UTC")
        assertEquals("2026-11-19T15:00:00", AttentionRules.localStart(java.time.LocalDateTime.of(2026, 11, 19, 15, 0).atZone(zone).toEpochSecond(), zone))
    }

    @Test fun theServerIsAskedAgainAfterThreeMinutes() {
        assertTrue(AttentionRules.dueForRefresh(null, 1000))
        assertTrue(!AttentionRules.dueForRefresh(1000, 1100))
        assertTrue(AttentionRules.dueForRefresh(1000, 1181))
        // A clock that went back counts as due.
        assertTrue(AttentionRules.dueForRefresh(5000, 1000))
    }
}
