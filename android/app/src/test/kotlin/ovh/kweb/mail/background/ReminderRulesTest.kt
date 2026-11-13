package ovh.kweb.mail.background

import java.time.Instant
import java.time.ZoneId
import java.util.Locale
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class ReminderRulesTest {
    private val istanbul = ZoneId.of("Europe/Istanbul")
    private val utc = ZoneId.of("UTC")
    private val now = Instant.parse("2026-11-12T06:00:00Z")

    private fun format(start: String?, tz: String?, allDay: Boolean = false, phone: ZoneId = utc) =
        ReminderRules.formatTime(start, tz, allDay, phone, Locale.GERMANY, now)

    @Test
    fun idsAreStableAndBelowOneThousand() {
        val id = ReminderRules.notificationId("event-1", "alert-1")
        assertEquals(id, ReminderRules.notificationId("event-1", "alert-1"))
        assertTrue(id in 100..899)
        assertNotEquals(id, ReminderRules.notificationId("event-1", "alert-2"))
    }

    @Test
    fun todayShowsTheTimeOnly() {
        assertEquals("10:00", format("2026-11-12T10:00:00", null))
    }

    @Test
    fun aTimeInAnotherZoneIsMovedToThePhonesZone() {
        assertEquals("07:00", format("2026-11-12T10:00:00", "Europe/Istanbul", phone = utc))
        assertEquals("10:00", format("2026-11-12T10:00:00", "Europe/Istanbul", phone = istanbul))
    }

    @Test
    fun anUnknownZoneIsTreatedAsFloating() {
        assertEquals("10:00", format("2026-11-12T10:00:00", "Not/AZone"))
    }

    @Test
    fun anotherDayShowsDateAndTime() {
        val text = format("2026-11-14T10:00:00", null)!!
        assertTrue(text, text.contains("14") && text.contains("10:00"))
    }

    @Test
    fun anAllDayEntryShowsTheDateOnly() {
        val text = format("2026-11-14T00:00:00", null, allDay = true)!!
        assertTrue(text, text.contains("14") && !text.contains(":"))
    }

    @Test
    fun nothingUsableGivesNull() {
        assertNull(format(null, null))
        assertNull(format("", null))
        assertNull(format("garbage", null))
    }
}
