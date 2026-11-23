package ovh.kweb.mail.ui.attention

import java.time.LocalDateTime
import java.time.ZoneId
import java.util.Locale
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import ovh.kweb.mail.R
import uniffi.kweb_core.AttentionFamily
import uniffi.kweb_core.AttentionRule
import uniffi.kweb_core.HomeTile
import uniffi.kweb_core.PhoneAction
import uniffi.kweb_core.ServiceAction
import uniffi.kweb_core.TimeSlot

class AttentionWordsTest {
    private val zone = ZoneId.of("UTC")
    // Tuesday 2026-11-17 09:40 UTC.
    private val now = LocalDateTime.of(2026, 11, 17, 9, 40).atZone(zone).toEpochSecond()
    private val clock = WordClock(zone, Locale.US, now)
    private val t = FakeTexts

    private fun f(
        rule: AttentionRule,
        family: AttentionFamily = AttentionFamily.CALENDAR,
        tile: HomeTile = HomeTile.NEXT_EVENT,
        names: List<String> = emptyList(),
        times: List<Long> = emptyList(),
        count: Int = 0,
        slots: List<TimeSlot> = emptyList(),
    ) = finding(rule, family, tile, names = names, times = times, count = count, slots = slots)

    @Test fun anOverlapNamesTheLaterTimeAndBothEvents() {
        val finding = f(AttentionRule.OVERLAP, names = listOf("Dentist", "Standup"), times = listOf(now + 100, now + 3600))
        assertEquals("s${R.string.att_overlap}:" + AttentionWords.time(now + 3600, clock) + "|Dentist|Standup", AttentionWords.title(finding, clock, t))
        assertNull(AttentionWords.meta(finding, clock, t))
    }

    @Test fun overdueTasksAreAPluralWithTheirNamesBeneath() {
        val finding = f(AttentionRule.OVERDUE, AttentionFamily.TASKS, HomeTile.DUE_TODAY, names = listOf("A", "B"), count = 3)
        assertEquals("p${R.plurals.att_overdue}:3:", AttentionWords.title(finding, clock, t))
        assertEquals("s${R.string.att_more_names}:A, B", AttentionWords.meta(finding, clock, t))
        val two = f(AttentionRule.OVERDUE, AttentionFamily.TASKS, HomeTile.DUE_TODAY, names = listOf("A", "B"), count = 2)
        assertEquals("A, B", AttentionWords.meta(two, clock, t))
    }

    @Test fun noPlaceSaysHowSoonAndHowManyMailsMentionIt() {
        val soon = f(AttentionRule.NO_PLACE, names = listOf("Review"), times = listOf(now + 80 * 60), count = 2)
        assertEquals("p${R.plurals.att_no_place_mails}:2:s${R.string.home_unit_hours_minutes}:1|20", AttentionWords.meta(soon, clock, t))
        val none = f(AttentionRule.NO_PLACE, names = listOf("Review"), times = listOf(now + 42 * 60), count = 0)
        assertEquals("s${R.string.att_no_place_in}:s${R.string.home_unit_minutes}:42", AttentionWords.meta(none, clock, t))
    }

    @Test fun anInvitationSaysWhatItOverlapsOrThatItIsFree() {
        val clash = f(AttentionRule.OPEN_INVITATION, names = listOf("Lunch", "Standup"), times = listOf(now))
        assertEquals("s${R.string.att_invitation_overlaps}:Standup", AttentionWords.meta(clash, clock, t))
        val free = f(AttentionRule.OPEN_INVITATION, names = listOf("Lunch", ""), times = listOf(now))
        assertEquals("s${R.string.att_invitation_free}:", AttentionWords.meta(free, clock, t))
    }

    @Test fun aBusyDayWithNoGapsSaysSo() {
        val day = f(AttentionRule.BUSY_DAY, count = 6, times = listOf(now))
        assertEquals("p${R.plurals.att_busy}:6:s${R.string.att_today}:", AttentionWords.title(day, clock, t))
        assertEquals("s${R.string.att_busy_no_free}:", AttentionWords.meta(day, clock, t))
        val withGaps = f(AttentionRule.BUSY_DAY, count = 5, times = listOf(now + 86_400), slots = listOf(TimeSlot(now, now + 3600)))
        assertEquals("p${R.plurals.att_busy}:5:s${R.string.att_tomorrow}:", AttentionWords.title(withGaps, clock, t))
        assertNull(AttentionWords.meta(withGaps, clock, t))
    }

    @Test fun aWaitingMailCountsFullDaysAndAtLeastOne() {
        val finding = f(AttentionRule.WAITING_FOR_YOU, AttentionFamily.MAIL, HomeTile.UNREAD, names = listOf("Ayse", "Menu"), times = listOf(now - 3 * 86_400 - 5))
        assertEquals("p${R.plurals.att_waiting_meta}:3:Menu", AttentionWords.meta(finding, clock, t))
        val fresh = f(AttentionRule.WAITING_FOR_YOU, AttentionFamily.MAIL, HomeTile.UNREAD, names = listOf("Ayse", "Menu"), times = listOf(now - 60))
        assertEquals("p${R.plurals.att_waiting_meta}:1:Menu", AttentionWords.meta(fresh, clock, t))
    }

    @Test fun aSentMailWithNoReplySaysWhoAndHowManyDaysAndQuotesTheSubject() {
        val finding = f(AttentionRule.NO_REPLY, AttentionFamily.MAIL, HomeTile.UNREAD, names = listOf("Ayse", "Menu"), times = listOf(now - 5 * 86_400), count = 5)
        assertEquals("p${R.plurals.att_no_reply}:5:Ayse", AttentionWords.title(finding, clock, t))
        assertEquals("s${R.string.att_no_reply_meta}:Menu|" + AttentionWords.time(now - 5 * 86_400, clock), AttentionWords.meta(finding, clock, t))
    }

    @Test fun timesAreTheClockTodayAndTheWeekdayLater() {
        val today = AttentionWords.time(now + 3600, clock)
        val tomorrow = AttentionWords.time(now + 86_400, clock)
        val far = AttentionWords.time(now + 20 * 86_400, clock)
        assertTrue(today.startsWith("10:40"))
        assertTrue(tomorrow.startsWith("Wed "))
        assertTrue(far.startsWith("7 Dec "))
    }

    @Test fun everyRuleHasATitle() {
        for (rule in AttentionRule.entries) {
            val finding = f(rule, names = listOf("a", "b", "c", "d"), times = listOf(now, now), count = 2)
            assertTrue(AttentionWords.title(finding, clock, t).isNotEmpty())
        }
    }

    @Test fun everyServiceActionHasALabel() {
        for (a in ServiceAction.entries) AttentionWords.serviceLabel(a)
    }

    @Test fun theMoveButtonNamesTheLaterEvent() {
        val finding = f(AttentionRule.OVERLAP, names = listOf("Dentist", "Standup"))
        assertEquals("s${R.string.att_move}:Standup", AttentionWords.phoneLabel(PhoneAction.MoveEvent("e", null), finding, t))
    }

    @Test fun spansRoundUpToTheMinute() {
        assertEquals("s${R.string.home_unit_minutes}:1", AttentionWords.span(1, t))
        assertEquals("s${R.string.home_unit_hours}:2", AttentionWords.span(7200, t))
        assertEquals("s${R.string.home_unit_minutes}:0", AttentionWords.span(-5, t))
    }

    @Test fun aSlotChipHasTheWeekdayOnlyOnALaterDay() {
        val sameDay = AttentionWords.slotChip(now + 3600, clock, now)
        val nextDay = AttentionWords.slotChip(now + 86_400, clock, now)
        assertTrue(!sameDay.contains("Wed"))
        assertTrue(nextDay.startsWith("Wed "))
    }
}
