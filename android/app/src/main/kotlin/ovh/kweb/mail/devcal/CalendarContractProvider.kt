package ovh.kweb.mail.devcal

import android.accounts.Account
import android.content.ContentProviderClient
import android.content.ContentUris
import android.content.ContentValues
import android.database.Cursor
import android.net.Uri
import android.os.RemoteException
import android.provider.CalendarContract
import android.provider.CalendarContract.Calendars
import android.provider.CalendarContract.Events
import android.provider.SyncStateContract
import uniffi.kweb_core.CalendarProvider
import uniffi.kweb_core.DeviceCalendar
import uniffi.kweb_core.DeviceEvent
import uniffi.kweb_core.ProviderException

/**
 * The core's port to the phone's calendar provider, over `CalendarContract` as the sync adapter of [account]. Every write is
 * made as a sync adapter (`CALLER_IS_SYNCADAPTER`), so it neither marks the row dirty nor asks for another sync; the dirty
 * flag is written as the core says. Only rows of [account] are ever read or written.
 *
 * Titles, descriptions and places pass through here and are never put in an error or a log: a failure is named by its kind.
 */
class CalendarContractProvider(private val client: ContentProviderClient, private val account: Account) : CalendarProvider {
    private fun Uri.forSync(): Uri = buildUpon()
        .appendQueryParameter(CalendarContract.CALLER_IS_SYNCADAPTER, "true")
        .appendQueryParameter(Calendars.ACCOUNT_NAME, account.name)
        .appendQueryParameter(Calendars.ACCOUNT_TYPE, account.type)
        .build()

    private val calendarsUri get() = Calendars.CONTENT_URI.forSync()
    private val eventsUri get() = Events.CONTENT_URI.forSync()
    private val stateUri get() = CalendarContract.SyncState.CONTENT_URI.forSync()

    private fun <T> guard(block: () -> T): T = try {
        block()
    } catch (e: ProviderException) {
        throw e
    } catch (e: SecurityException) {
        throw ProviderException.PermissionDenied()
    } catch (e: RemoteException) {
        throw ProviderException.Failed("the calendar provider is not reachable")
    } catch (e: RuntimeException) {
        throw ProviderException.Failed("the calendar provider refused (${e.javaClass.simpleName})")
    }

    private fun rows(uri: Uri, projection: Array<String>, selection: String?, args: Array<String>?): List<Map<String, Any?>> {
        val cursor = client.query(uri, projection, selection, args, null) ?: throw ProviderException.Failed("the calendar provider gave no answer")
        return cursor.use { c ->
            val out = ArrayList<Map<String, Any?>>(c.count)
            while (c.moveToNext()) out.add(c.asMap())
            out
        }
    }

    private fun Cursor.asMap(): Map<String, Any?> {
        val map = HashMap<String, Any?>(columnCount * 2)
        for (i in 0 until columnCount) {
            map[getColumnName(i)] = when (getType(i)) {
                Cursor.FIELD_TYPE_INTEGER -> getLong(i)
                Cursor.FIELD_TYPE_FLOAT -> getDouble(i)
                Cursor.FIELD_TYPE_STRING -> getString(i)
                else -> null
            }
        }
        return map
    }

    private fun Map<String, Any?>.toValues(): ContentValues {
        val values = ContentValues()
        for ((key, value) in this) {
            when (value) {
                null -> values.putNull(key)
                is Int -> values.put(key, value)
                is Long -> values.put(key, value)
                is String -> values.put(key, value)
                else -> values.put(key, value.toString())
            }
        }
        return values
    }

    /** The provider's id of each calendar of the account, by the calendar's sync id. */
    private fun calendarIds(): Map<String, Long> =
        rows(calendarsUri, arrayOf(Calendars._ID, Calendars._SYNC_ID), null, null)
            .mapNotNull { r -> (r[Calendars._SYNC_ID] as? String)?.let { it to (r[Calendars._ID] as Long) } }
            .toMap()

    override fun readState(): String? = guard {
        SyncStateContract.Helpers.get(client, stateUri, account)?.toString(Charsets.UTF_8)
    }

    override fun writeState(state: String) = guard {
        SyncStateContract.Helpers.set(client, stateUri, account, state.toByteArray(Charsets.UTF_8))
    }

    override fun listCalendars(): List<DeviceCalendar> = guard {
        rows(calendarsUri, CalendarColumns.CALENDAR_PROJECTION, null, null).mapNotNull(CalendarColumns::calendar)
    }

    override fun upsertCalendar(calendar: DeviceCalendar) = guard {
        val existing = calendarIds()[calendar.syncId]
        if (existing != null) {
            client.update(ContentUris.withAppendedId(Calendars.CONTENT_URI, existing).forSync(), CalendarColumns.calendarValues(calendar, false).toValues(), null, null)
        } else {
            val values = CalendarColumns.calendarValues(calendar, true).toValues()
            values.put(Calendars.ACCOUNT_NAME, account.name)
            values.put(Calendars.ACCOUNT_TYPE, account.type)
            client.insert(calendarsUri, values) ?: throw ProviderException.Failed("the calendar was not made")
        }
        Unit
    }

    override fun deleteCalendar(syncId: String) = guard {
        calendarIds()[syncId]?.let { client.delete(ContentUris.withAppendedId(Calendars.CONTENT_URI, it).forSync(), null, null) }
        Unit
    }

    override fun listEvents(): List<DeviceEvent> = guard {
        val calendars = calendarIds()
        if (calendars.isEmpty()) return@guard emptyList()
        val bySync = calendars.entries.associate { it.value to it.key }
        val ids = calendars.values.joinToString(",")
        // As a sync adapter the rows marked deleted are in the answer too.
        rows(eventsUri, CalendarColumns.EVENT_PROJECTION, "${Events.CALENDAR_ID} IN ($ids)", null)
            .mapNotNull { CalendarColumns.event(it, bySync) }
    }

    /**
     * How many events of the account carry an edit made on the phone that the sync has not sent yet: changed, deleted
     * or made there. Counts only; nothing of an event is read.
     */
    fun waitingEdits(): Int = guard {
        val ids = calendarIds().values.joinToString(",")
        if (ids.isEmpty()) return@guard 0
        rows(eventsUri, arrayOf(Events._ID), "${Events.CALENDAR_ID} IN ($ids) AND (${Events.DIRTY} = 1 OR ${Events.DELETED} = 1)", null).size
    }

    override fun getEvent(rowId: Long): DeviceEvent? = guard {
        val bySync = calendarIds().entries.associate { it.value to it.key }
        rows(eventsUri, CalendarColumns.EVENT_PROJECTION, "${Events._ID}=?", arrayOf(rowId.toString()))
            .firstNotNullOfOrNull { CalendarColumns.event(it, bySync) }
    }

    override fun upsertEvent(event: DeviceEvent): DeviceEvent = guard {
        val calendars = calendarIds()
        val calendarId = calendars[event.calendarSyncId] ?: throw ProviderException.Failed("the event's calendar is not on the phone")
        val values = CalendarColumns.eventValues(event, calendarId).toValues()
        val rowId = event.rowId ?: event.syncId?.let { findBySyncId(it) }
        val id = if (rowId != null) {
            client.update(ContentUris.withAppendedId(Events.CONTENT_URI, rowId).forSync(), values, null, null)
            rowId
        } else {
            val made = client.insert(eventsUri, values) ?: throw ProviderException.Failed("the event was not made")
            ContentUris.parseId(made)
        }
        getEvent(id) ?: throw ProviderException.Failed("the event is not there after it was written")
    }

    private fun findBySyncId(syncId: String): Long? =
        rows(eventsUri, arrayOf(Events._ID), "${Events._SYNC_ID}=?", arrayOf(syncId)).firstNotNullOfOrNull { it[Events._ID] as? Long }

    override fun setSyncData(rowId: Long, syncId: String?, shadow: String?, baseline: String?, dirty: Boolean) = guard {
        val values = ContentValues()
        if (syncId != null) values.put(Events._SYNC_ID, syncId)
        values.put(Events.SYNC_DATA1, shadow)
        values.put(Events.SYNC_DATA4, baseline)
        values.put(Events.DIRTY, if (dirty) 1 else 0)
        client.update(ContentUris.withAppendedId(Events.CONTENT_URI, rowId).forSync(), values, null, null)
        Unit
    }

    override fun purgeEvent(rowId: Long) = guard {
        client.delete(ContentUris.withAppendedId(Events.CONTENT_URI, rowId).forSync(), null, null)
        Unit
    }
}
