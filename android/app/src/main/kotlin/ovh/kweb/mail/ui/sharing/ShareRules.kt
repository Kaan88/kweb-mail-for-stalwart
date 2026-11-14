package ovh.kweb.mail.ui.sharing

import uniffi.kweb_core.FileEntry
import uniffi.kweb_core.FileKind
import uniffi.kweb_core.Person
import uniffi.kweb_core.Share
import uniffi.kweb_core.ShareLevel

/** The pure rules of the Share screen. */
object ShareRules {
    /** A lookup of people starts with this many characters. */
    const val MIN_QUERY = 2

    /** The wait after the last key before the lookup is asked for. */
    const val DEBOUNCE_MILLIS = 300L

    /** The levels in the order they are offered. */
    val LEVELS = listOf(ShareLevel.READ, ShareLevel.WRITE)

    /** The special folders that are never shared: the drafts, the bin and the spam folder. */
    private val PRIVATE_ROLES = setOf("drafts", "trash", "junk")

    fun mailboxShareable(role: String?): Boolean = role == null || role.lowercase() !in PRIVATE_ROLES

    /** Only a folder of the user's own space can be shared; the entries of a shared space are not the user's to share. */
    fun fileFolderShareable(entry: FileEntry): Boolean = entry.accountId == null && entry.kind == FileKind.FOLDER

    fun canSearch(raw: String): Boolean = raw.trim().length >= MIN_QUERY

    /** The people of an answer that the thing is not already shared with, each once. */
    fun candidates(people: List<Person>, shares: List<Share>): List<Person> {
        val taken = shares.mapTo(HashSet()) { it.person.id }
        return people.filter { taken.add(it.id) }
    }

    /** By name, then id, so a change of one level does not move the rows. */
    fun ordered(shares: List<Share>): List<Share> = shares.sortedWith(compareBy({ label(it.person).lowercase() }, { it.person.id }))

    /** The name, else the address, else the id. */
    fun label(person: Person): String = person.name.trim().ifEmpty { person.email.trim() }.ifEmpty { person.id }

    /** The address under the name, when there is one that the name does not already say. */
    fun secondLine(person: Person): String? = person.email.trim().takeIf { it.isNotEmpty() && it != label(person) }
}
