package ovh.kweb.mail.ui.contacts

import uniffi.kweb_core.AddressBook
import uniffi.kweb_core.Contact
import uniffi.kweb_core.ContactDetail
import uniffi.kweb_core.Core

/**
 * What the contact form asks of the core: the user's own address books, or the ones a user shared (one account).
 * One form, two backends. Every call blocks: run it off the main thread.
 */
interface ContactFormBackend {
    fun addressBooks(): List<AddressBook>
    fun contact(contactId: String): ContactDetail
    fun save(contactId: String?, contact: Contact): String
}

class CoreContactFormBackend(private val core: () -> Core) : ContactFormBackend {
    override fun addressBooks() = core().addressBooks()
    override fun contact(contactId: String) = core().contact(contactId)
    override fun save(contactId: String?, contact: Contact) = core().saveContact(contactId, contact)
}

/** The address books [account] shares. */
class SharedContactFormBackend(private val core: () -> Core, private val account: String) : ContactFormBackend {
    override fun addressBooks() = core().sharedAddressBooks(account)
    override fun contact(contactId: String) = core().sharedContact(account, contactId)
    override fun save(contactId: String?, contact: Contact) = core().sharedSaveContact(account, contactId, contact)
}

/** The backend of the form for [account] (null: the user's own). */
fun contactFormBackend(core: () -> Core, account: String?): ContactFormBackend =
    if (account == null) CoreContactFormBackend(core) else SharedContactFormBackend(core, account)
