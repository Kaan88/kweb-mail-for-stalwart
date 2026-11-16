package ovh.kweb.mail.devcal

import android.provider.CalendarContract.Calendars
import android.provider.CalendarContract.Events
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import ovh.kweb.mail.accounts.AccountRecord
import uniffi.kweb_core.DeviceCalendar
import uniffi.kweb_core.DeviceEvent
import uniffi.kweb_core.DeviceSyncOutcome
import uniffi.kweb_core.DeviceSyncReport

class DeviceCalendarTest {
    private fun record(id: String, user: String, server: String = "https://mail.example.test") = AccountRecord(id, false, user, server, "j$id", 0L)

    private fun event(
        rowId: Long? = 7L,
        syncId: String? = "a1/e1",
        original: String? = null,
        allDay: Boolean = false,
        rrule: String? = null,
    ) = DeviceEvent(
        rowId = rowId, syncId = syncId, calendarSyncId = "a1/cal1", uid = "u1", title = "Lunch", description = "With Alice", location = "Cafe",
        startMs = 1_795_165_200_000L, endMs = if (rrule == null) 1_795_168_800_000L else null, duration = if (rrule != null) "P3600S" else null,
        allDay = allDay, timeZone = "Europe/Istanbul", rrule = rrule, exdate = null, status = 1, availability = 0, accessLevel = 0,
        originalSyncId = original, originalInstanceMs = if (original != null) 1_795_165_200_000L else null, originalAllDay = false,
        dirty = false, deleted = false, shadow = "{\"id\":\"e1\"}", baseline = "{}",
    )

    @Test
    fun theAndroidAccountIsNamedByTheLoginAndTheServerOnlyWhenTwoShareALogin() {
        val one = record("a", "kaan@kweb.test")
        assertEquals("kaan@kweb.test", DeviceCalendarRules.accountName(one, listOf(one, record("b", "other@x.test"))))
        val two = record("b", "kaan@kweb.test", "https://mail.other.test:8443/path")
        assertEquals("kaan@kweb.test (mail.example.test)", DeviceCalendarRules.accountName(one, listOf(one, two)))
        assertEquals("kaan@kweb.test (mail.other.test)", DeviceCalendarRules.accountName(two, listOf(one, two)))
    }

    @Test
    fun aSyncIsAskedForAtMostOnceAMinute() {
        assertTrue(DeviceCalendarRules.mayRequest(0L, 10L))
        assertFalse(DeviceCalendarRules.mayRequest(1_000L, 30_000L))
        assertTrue(DeviceCalendarRules.mayRequest(1_000L, 61_000L))
        assertTrue("a clock that went back", DeviceCalendarRules.mayRequest(100_000L, 5_000L))
    }

    @Test
    fun everySyncIdOfAnAccountStartsWithItsId() {
        assertEquals("a1/", DeviceCalendarRules.syncPrefix("a1"))
    }

    @Test
    fun theLastRunSurvivesTheSettingsAsText() {
        val report = DeviceSyncReport(
            outcome = DeviceSyncOutcome.DONE, full = false, calendars = 2u, written = 3u, removedHere = 1u, uploaded = 1u, created = 1u,
            destroyed = 0u, conflicts = 2u, reverted = 1u, heldBack = 0u, notSent = 0u,
        )
        val run = DeviceRun.of(report)
        assertEquals(DeviceRun(DeviceRun.Kind.Ok, listOf(3, 2, 2, 1, 0)), run)
        assertEquals(run, DeviceRun.decode(run.encode()))
        assertTrue(run.good)
        val stopped = DeviceRun.of(report.copy(outcome = DeviceSyncOutcome.STOPPED_TOO_MANY_DELETIONS, heldBack = 55u))
        assertEquals("deletions:55", stopped.encode())
        assertFalse(stopped.good)
        assertNull(DeviceRun.decode("nonsense:1"))
        assertNull(DeviceRun.decode(null))
    }

    @Test
    fun anEventIsWrittenWithItsTimingAndTheSyncsOwnColumns() {
        val values = CalendarColumns.eventValues(event(), 3L)
        assertEquals(3L, values[Events.CALENDAR_ID])
        assertEquals("a1/e1", values[Events._SYNC_ID])
        assertEquals("u1", values[Events.UID_2445])
        assertEquals(1_795_168_800_000L, values[Events.DTEND])
        assertNull(values[Events.DURATION])
        assertEquals(0, values[Events.ALL_DAY])
        assertEquals(0, values[Events.DIRTY])
        assertEquals(0, values[Events.DELETED])
        assertNull(values[Events.ORIGINAL_ALL_DAY])
    }

    @Test
    fun aSeriesHasADurationAndNoEnd() {
        val values = CalendarColumns.eventValues(event(rrule = "FREQ=WEEKLY"), 3L)
        assertNull(values[Events.DTEND])
        assertEquals("P3600S", values[Events.DURATION])
        assertEquals("FREQ=WEEKLY", values[Events.RRULE])
    }

    @Test
    fun anExceptionRowNamesItsSeries() {
        val values = CalendarColumns.eventValues(event(syncId = "a1/e1#2026-12-04T12:00:00", original = "a1/e1"), 3L)
        assertEquals("a1/e1", values[Events.ORIGINAL_SYNC_ID])
        assertEquals(1_795_165_200_000L, values[Events.ORIGINAL_INSTANCE_TIME])
        assertEquals(0, values[Events.ORIGINAL_ALL_DAY])
    }

    @Test
    fun aRowReadBackIsTheEventTheCoreSees() {
        val row = mapOf<String, Any?>(
            Events._ID to 7L, Events._SYNC_ID to "a1/e1", Events.CALENDAR_ID to 3L, Events.UID_2445 to "u1", Events.TITLE to "Lunch",
            Events.DESCRIPTION to null, Events.EVENT_LOCATION to "Cafe", Events.DTSTART to 1_795_165_200_000L, Events.DTEND to 1_795_168_800_000L,
            Events.DURATION to null, Events.ALL_DAY to 0L, Events.EVENT_TIMEZONE to "Europe/Istanbul", Events.RRULE to null, Events.EXDATE to null,
            Events.STATUS to 1L, Events.AVAILABILITY to 0L, Events.ACCESS_LEVEL to 0L, Events.ORIGINAL_SYNC_ID to null,
            Events.ORIGINAL_INSTANCE_TIME to null, Events.ORIGINAL_ALL_DAY to null, Events.DIRTY to 1L, Events.DELETED to 0L,
            Events.SYNC_DATA1 to "{}", Events.SYNC_DATA4 to null,
        )
        val read = CalendarColumns.event(row, mapOf(3L to "a1/cal1"))!!
        assertEquals("a1/cal1", read.calendarSyncId)
        assertEquals("", read.description)
        assertTrue(read.dirty)
        assertFalse(read.deleted)
        assertEquals(7L, read.rowId)
        // A row of a calendar that is not the account's is not ours.
        assertNull(CalendarColumns.event(row, mapOf(4L to "a1/cal2")))
    }

    @Test
    fun aCalendarTakesNoRemindersAndStartsVisibleOnlyWhenMade() {
        val calendar = DeviceCalendar("a1/cal1", "Personal", -1, 700, "kaan@kweb.test", "Europe/Istanbul")
        val made = CalendarColumns.calendarValues(calendar, true)
        assertEquals(0, made[Calendars.MAX_REMINDERS])
        assertEquals(1, made[Calendars.SYNC_EVENTS])
        assertEquals(1, made[Calendars.VISIBLE])
        assertEquals("kaan@kweb.test", made[Calendars.OWNER_ACCOUNT])
        assertFalse("the user's own choice of visibility stands", CalendarColumns.calendarValues(calendar, false).containsKey(Calendars.VISIBLE))
        assertFalse("the provider's feed column is left alone", made.containsKey(Calendars.CAL_SYNC1))
        val back = CalendarColumns.calendar(
            mapOf(
                Calendars._SYNC_ID to "a1/cal1", Calendars.CALENDAR_DISPLAY_NAME to "Personal", Calendars.CALENDAR_COLOR to -1L,
                Calendars.CALENDAR_ACCESS_LEVEL to 700L, Calendars.OWNER_ACCOUNT to "kaan@kweb.test", Calendars.CALENDAR_TIME_ZONE to "Europe/Istanbul",
            ),
        )
        assertEquals(calendar, back)
    }
}
