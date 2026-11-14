package ovh.kweb.mail.ui.calendar

import uniffi.kweb_core.Calendar
import uniffi.kweb_core.Core
import uniffi.kweb_core.EventDetail
import uniffi.kweb_core.EventInput

/**
 * What the event form asks of the core: the user's own calendars, or the calendars a user shared (one account).
 * One form, two backends. Every call blocks: run it off the main thread.
 */
interface EventFormBackend {
    fun calendars(): List<Calendar>
    fun event(eventId: String): EventDetail
    fun save(eventId: String?, input: EventInput): String
}

class CoreEventFormBackend(private val core: () -> Core) : EventFormBackend {
    override fun calendars() = core().calendars()
    override fun event(eventId: String) = core().event(eventId, null)
    override fun save(eventId: String?, input: EventInput) = core().saveEvent(eventId, input)
}

/** The calendars [account] shares. */
class SharedEventFormBackend(private val core: () -> Core, private val account: String) : EventFormBackend {
    override fun calendars() = core().sharedCalendars(account)
    override fun event(eventId: String) = core().sharedEvent(account, eventId, null)
    override fun save(eventId: String?, input: EventInput) = core().sharedSaveEvent(account, eventId, input)
}

/** The backend of the form for [account] (null: the user's own). */
fun eventFormBackend(core: () -> Core, account: String?): EventFormBackend =
    if (account == null) CoreEventFormBackend(core) else SharedEventFormBackend(core, account)
