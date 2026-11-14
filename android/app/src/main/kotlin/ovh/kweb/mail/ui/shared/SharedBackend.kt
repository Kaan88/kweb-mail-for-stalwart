package ovh.kweb.mail.ui.shared

import uniffi.kweb_core.Agenda
import uniffi.kweb_core.Calendar
import uniffi.kweb_core.ContactDetail
import uniffi.kweb_core.ContactRow
import uniffi.kweb_core.Core
import uniffi.kweb_core.EventDetail
import uniffi.kweb_core.SharedAccount
import uniffi.kweb_core.SharedAction
import uniffi.kweb_core.SharedMailbox
import uniffi.kweb_core.SharedMessage
import uniffi.kweb_core.SharedMessages

/**
 * What the "Shared with me" screens ask of the core. Every call blocks and asks the server (nothing here is
 * cached): run it off the main thread. Ids of mail, folders, events and contacts are only unique within one
 * account, so every call but [accounts] takes it. The tests use a fake.
 */
interface SharedBackend {
    fun accounts(): List<SharedAccount>
    fun mailboxes(account: String): List<SharedMailbox>
    fun messages(account: String, mailboxId: String, offset: Int, limit: Int): SharedMessages
    fun message(account: String, messageId: String): SharedMessage
    fun act(account: String, messageId: String, action: SharedAction)
    fun calendars(account: String): List<Calendar>
    fun agenda(account: String, from: Long, to: Long, timeZone: String): Agenda
    fun event(account: String, eventId: String, recurrenceId: String?): EventDetail
    fun contacts(account: String, query: String): List<ContactRow>
    fun contact(account: String, contactId: String): ContactDetail
}

class CoreSharedBackend(private val core: () -> Core) : SharedBackend {
    override fun accounts() = core().sharedAccounts()
    override fun mailboxes(account: String) = core().sharedMailboxes(account)
    override fun messages(account: String, mailboxId: String, offset: Int, limit: Int) =
        core().sharedMessages(account, mailboxId, offset.toUInt(), limit.toUInt())
    override fun message(account: String, messageId: String) = core().sharedMessage(account, messageId)
    override fun act(account: String, messageId: String, action: SharedAction) = core().sharedAct(account, messageId, action)
    override fun calendars(account: String) = core().sharedCalendars(account)
    override fun agenda(account: String, from: Long, to: Long, timeZone: String) = core().sharedAgenda(account, from, to, timeZone)
    override fun event(account: String, eventId: String, recurrenceId: String?) = core().sharedEvent(account, eventId, recurrenceId)
    override fun contacts(account: String, query: String) = core().sharedContacts(account, query)
    override fun contact(account: String, contactId: String) = core().sharedContact(account, contactId)
}
