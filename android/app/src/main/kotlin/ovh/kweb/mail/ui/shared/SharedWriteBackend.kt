package ovh.kweb.mail.ui.shared

import uniffi.kweb_core.AddressBook
import uniffi.kweb_core.Contact
import uniffi.kweb_core.Core
import uniffi.kweb_core.DraftKind
import uniffi.kweb_core.EventInput

/**
 * What the "Shared with me" screens ask of the core to write: answer or forward a message, change events and
 * contacts where the rights allow. Every call blocks and asks the server: run it off the main thread. The tests use a fake.
 */
interface SharedWriteBackend {
    /** The id of a new draft of the user's own that answers or forwards a message of [account]. */
    fun newDraftFromShared(account: String, kind: DraftKind): String
    fun discardDraft(draftId: String)
    fun deleteEvent(account: String, eventId: String, recurrenceId: String?)
    fun addressBooks(account: String): List<AddressBook>
    fun deleteContact(account: String, contactId: String)
}

class CoreSharedWriteBackend(private val core: () -> Core) : SharedWriteBackend {
    override fun newDraftFromShared(account: String, kind: DraftKind) = core().newDraftFromShared(account, kind).id
    override fun discardDraft(draftId: String) = core().discardDraft(draftId)
    override fun deleteEvent(account: String, eventId: String, recurrenceId: String?) = core().sharedDeleteEvent(account, eventId, recurrenceId)
    override fun addressBooks(account: String) = core().sharedAddressBooks(account)
    override fun deleteContact(account: String, contactId: String) = core().sharedDeleteContact(account, contactId)
}
