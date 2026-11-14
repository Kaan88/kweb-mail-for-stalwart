package ovh.kweb.mail.ui.sharing

import uniffi.kweb_core.AddressBook
import uniffi.kweb_core.Calendar
import uniffi.kweb_core.Core
import uniffi.kweb_core.Person
import uniffi.kweb_core.Share
import uniffi.kweb_core.ShareKind
import uniffi.kweb_core.ShareLevel

/** What the Share screen asks of the core. Every call blocks and asks the server: run it off the main thread. The tests use a fake. */
interface SharingBackend {
    fun people(text: String): List<Person>
    fun shares(kind: ShareKind, id: String): List<Share>
    fun setShare(kind: ShareKind, id: String, personId: String, level: ShareLevel?): List<Share>

    /** The user's own calendars and address books, for choosing which one to share. */
    fun calendars(): List<Calendar>
    fun addressBooks(): List<AddressBook>
}

class CoreSharingBackend(private val core: () -> Core) : SharingBackend {
    override fun people(text: String) = core().sharePeople(text)
    override fun shares(kind: ShareKind, id: String) = core().shares(kind, id)
    override fun setShare(kind: ShareKind, id: String, personId: String, level: ShareLevel?) = core().setShare(kind, id, personId, level)
    override fun calendars() = core().calendars()
    override fun addressBooks() = core().addressBooks()
}
