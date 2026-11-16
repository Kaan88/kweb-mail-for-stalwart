package ovh.kweb.mail.devcal

import android.provider.CalendarContract.Calendars
import android.provider.CalendarContract.Events
import uniffi.kweb_core.DeviceCalendar
import uniffi.kweb_core.DeviceEvent

/**
 * The columns of the phone's calendar tables and the core's records, in both directions, with plain maps so that a JVM
 * test can check them. The sync's own columns of an event: `SYNC_DATA1` holds the server's copy (the shadow),
 * `SYNC_DATA4` what the row held after the sync's last write (the baseline), `UID_2445` the event's uid. `CAL_SYNC1` of a
 * calendar is left alone: the provider hands it to every sync it asks for.
 */
object CalendarColumns {
    val EVENT_PROJECTION = arrayOf(
        Events._ID, Events._SYNC_ID, Events.CALENDAR_ID, Events.UID_2445, Events.TITLE, Events.DESCRIPTION,
        Events.EVENT_LOCATION, Events.DTSTART, Events.DTEND, Events.DURATION, Events.ALL_DAY, Events.EVENT_TIMEZONE,
        Events.RRULE, Events.EXDATE, Events.STATUS, Events.AVAILABILITY, Events.ACCESS_LEVEL, Events.ORIGINAL_SYNC_ID,
        Events.ORIGINAL_INSTANCE_TIME, Events.ORIGINAL_ALL_DAY, Events.DIRTY, Events.DELETED, Events.SYNC_DATA1,
        Events.SYNC_DATA4,
    )

    val CALENDAR_PROJECTION = arrayOf(
        Calendars._ID, Calendars._SYNC_ID, Calendars.CALENDAR_DISPLAY_NAME, Calendars.CALENDAR_COLOR,
        Calendars.CALENDAR_ACCESS_LEVEL, Calendars.OWNER_ACCOUNT, Calendars.CALENDAR_TIME_ZONE,
    )

    /** The values to write for [event] (what the sync owns; not the row id). [calendarId] is the provider's id of its calendar. */
    fun eventValues(event: DeviceEvent, calendarId: Long): Map<String, Any?> = mapOf(
        Events.CALENDAR_ID to calendarId,
        Events._SYNC_ID to event.syncId,
        Events.UID_2445 to event.uid,
        Events.TITLE to event.title,
        Events.DESCRIPTION to event.description,
        Events.EVENT_LOCATION to event.location,
        Events.DTSTART to event.startMs,
        Events.DTEND to event.endMs,
        Events.DURATION to event.duration,
        Events.ALL_DAY to if (event.allDay) 1 else 0,
        Events.EVENT_TIMEZONE to event.timeZone,
        Events.RRULE to event.rrule,
        Events.EXDATE to event.exdate,
        Events.STATUS to event.status,
        Events.AVAILABILITY to event.availability,
        Events.ACCESS_LEVEL to event.accessLevel,
        Events.ORIGINAL_SYNC_ID to event.originalSyncId,
        Events.ORIGINAL_INSTANCE_TIME to event.originalInstanceMs,
        Events.ORIGINAL_ALL_DAY to if (event.originalSyncId != null) (if (event.originalAllDay) 1 else 0) else null,
        Events.DIRTY to if (event.dirty) 1 else 0,
        Events.DELETED to if (event.deleted) 1 else 0,
    )

    /** The event of a row read with [EVENT_PROJECTION]; [calendarSyncIds] maps the provider's calendar ids to their sync ids. */
    fun event(row: Map<String, Any?>, calendarSyncIds: Map<Long, String>): DeviceEvent? {
        val calendar = calendarSyncIds[row.long(Events.CALENDAR_ID) ?: return null] ?: return null
        return DeviceEvent(
            rowId = row.long(Events._ID),
            syncId = row.text(Events._SYNC_ID),
            calendarSyncId = calendar,
            uid = row.text(Events.UID_2445),
            title = row.text(Events.TITLE).orEmpty(),
            description = row.text(Events.DESCRIPTION).orEmpty(),
            location = row.text(Events.EVENT_LOCATION).orEmpty(),
            startMs = row.long(Events.DTSTART) ?: 0L,
            endMs = row.long(Events.DTEND),
            duration = row.text(Events.DURATION),
            allDay = row.long(Events.ALL_DAY) == 1L,
            timeZone = row.text(Events.EVENT_TIMEZONE).orEmpty(),
            rrule = row.text(Events.RRULE),
            exdate = row.text(Events.EXDATE),
            status = (row.long(Events.STATUS) ?: 1L).toInt(),
            availability = (row.long(Events.AVAILABILITY) ?: 0L).toInt(),
            accessLevel = (row.long(Events.ACCESS_LEVEL) ?: 0L).toInt(),
            originalSyncId = row.text(Events.ORIGINAL_SYNC_ID),
            originalInstanceMs = row.long(Events.ORIGINAL_INSTANCE_TIME),
            originalAllDay = row.long(Events.ORIGINAL_ALL_DAY) == 1L,
            dirty = row.long(Events.DIRTY) == 1L,
            deleted = row.long(Events.DELETED) == 1L,
            shadow = row.text(Events.SYNC_DATA1),
            baseline = row.text(Events.SYNC_DATA4),
        )
    }

    /**
     * The values of a new or changed calendar. The account is added by the caller. [insert] adds what a calendar needs once:
     * it starts visible (afterwards the user's own choice stands). Reminders are not written by the sync: a calendar takes
     * none, so the apps do not offer any here and nothing reminds twice.
     */
    fun calendarValues(calendar: DeviceCalendar, insert: Boolean): Map<String, Any?> {
        val values = linkedMapOf<String, Any?>(
            Calendars._SYNC_ID to calendar.syncId,
            Calendars.NAME to calendar.name,
            Calendars.CALENDAR_DISPLAY_NAME to calendar.name,
            Calendars.CALENDAR_COLOR to calendar.color,
            Calendars.CALENDAR_ACCESS_LEVEL to calendar.access,
            Calendars.OWNER_ACCOUNT to calendar.owner,
            Calendars.CALENDAR_TIME_ZONE to calendar.timeZone,
            Calendars.SYNC_EVENTS to 1,
            Calendars.MAX_REMINDERS to 0,
            Calendars.ALLOWED_REMINDERS to "1",
            Calendars.ALLOWED_AVAILABILITY to "0,1",
            Calendars.ALLOWED_ATTENDEE_TYPES to "0,1,2,3",
            Calendars.CAN_ORGANIZER_RESPOND to 0,
        )
        if (insert) values[Calendars.VISIBLE] = 1
        return values
    }

    fun calendar(row: Map<String, Any?>): DeviceCalendar? = DeviceCalendar(
        syncId = row.text(Calendars._SYNC_ID) ?: return null,
        name = row.text(Calendars.CALENDAR_DISPLAY_NAME).orEmpty(),
        color = (row.long(Calendars.CALENDAR_COLOR) ?: 0L).toInt(),
        access = (row.long(Calendars.CALENDAR_ACCESS_LEVEL) ?: 0L).toInt(),
        owner = row.text(Calendars.OWNER_ACCOUNT).orEmpty(),
        timeZone = row.text(Calendars.CALENDAR_TIME_ZONE).orEmpty(),
    )

    private fun Map<String, Any?>.long(key: String): Long? = (this[key] as? Number)?.toLong()

    private fun Map<String, Any?>.text(key: String): String? = (this[key] as? String)?.takeIf { it.isNotEmpty() }
}
