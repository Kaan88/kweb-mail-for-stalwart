package ovh.kweb.mail.ui.contacts

import uniffi.kweb_core.Contact
import uniffi.kweb_core.Labeled

/** One line of a list in the form. [id] is only for the screen to tell the lines apart while they change. */
data class FormRow(val id: Int, val label: String, val value: String)

/**
 * What the edit screen holds. The three `loaded` fields are what the contact had when the form was
 * opened (empty for a new contact): they decide whether the name is sent as it was.
 */
data class ContactForm(
    val addressBookId: String,
    val givenName: String = "",
    val familyName: String = "",
    val organization: String = "",
    val emails: List<FormRow> = emptyList(),
    val phones: List<FormRow> = emptyList(),
    val addresses: List<FormRow> = emptyList(),
    val notes: String = "",
    val loadedName: String = "",
    val loadedGivenName: String = "",
    val loadedFamilyName: String = "",
)

/** Which list of the form a change is about. */
enum class FormList { EMAILS, PHONES, ADDRESSES }

/** The form and the core's [Contact], both ways, and the changes of the form. All plain functions. */
object ContactForms {
    /** A new contact in [addressBookId], with one empty line for a mail address. */
    fun blank(addressBookId: String): ContactForm = ContactForm(addressBookId, emails = listOf(FormRow(0, "", "")))

    fun fromContact(contact: Contact): ContactForm = ContactForm(
        addressBookId = contact.addressBookId,
        givenName = contact.givenName,
        familyName = contact.familyName,
        organization = contact.organization,
        emails = rows(contact.emails),
        phones = rows(contact.phones),
        addresses = rows(contact.addresses),
        notes = contact.notes,
        loadedName = contact.name,
        loadedGivenName = contact.givenName,
        loadedFamilyName = contact.familyName,
    )

    private fun rows(values: List<Labeled>): List<FormRow> = values.mapIndexed { index, it -> FormRow(index, it.label, it.value) }

    /**
     * The name the core gets: "given family", trimmed. A contact whose name fields the user did not change
     * keeps the name it had, since the card may have a name the two fields do not make (a nickname, a company).
     */
    fun name(form: ContactForm): String {
        val untouched = form.loadedName.isNotEmpty() &&
            form.givenName.trim() == form.loadedGivenName.trim() && form.familyName.trim() == form.loadedFamilyName.trim()
        if (untouched) return form.loadedName
        return listOf(form.givenName, form.familyName).map { it.trim() }.filter { it.isNotEmpty() }.joinToString(" ")
    }

    /** What is sent: lines with no value are left out, everything else is trimmed. */
    fun toContact(form: ContactForm): Contact = Contact(
        addressBookId = form.addressBookId,
        name = name(form),
        givenName = form.givenName.trim(),
        familyName = form.familyName.trim(),
        organization = form.organization.trim(),
        emails = labeled(form.emails),
        phones = labeled(form.phones),
        addresses = labeled(form.addresses),
        notes = form.notes.trim(),
    )

    private fun labeled(rows: List<FormRow>): List<Labeled> =
        rows.filter { it.value.isNotBlank() }.map { Labeled(it.label.trim(), it.value.trim()) }

    /**
     * What is saved for a card that is open for change: what the user changed, put on [current] (the card as the
     * server has it now). Everything the user did not touch stays as [current] has it, so an entry added elsewhere
     * while the form was open (or while it showed an old copy) is not deleted. [base] is the contact the form was
     * opened from and [wanted] what the form holds; all three are as [toContact] writes them (see [normalized]).
     * A list the user changed loses what the user removed and gains what the user added.
     */
    fun merge(base: Contact, wanted: Contact, current: Contact): Contact = Contact(
        addressBookId = current.addressBookId,
        name = pick(base.name, wanted.name, current.name),
        givenName = pick(base.givenName, wanted.givenName, current.givenName),
        familyName = pick(base.familyName, wanted.familyName, current.familyName),
        organization = pick(base.organization, wanted.organization, current.organization),
        emails = mergeList(base.emails, wanted.emails, current.emails),
        phones = mergeList(base.phones, wanted.phones, current.phones),
        addresses = mergeList(base.addresses, wanted.addresses, current.addresses),
        notes = pick(base.notes, wanted.notes, current.notes),
    )

    /** [contact] the way [toContact] would send it from a form: trimmed, lines without a value gone. */
    fun normalized(contact: Contact): Contact = toContact(fromContact(contact))

    private fun pick(base: String, wanted: String, current: String): String = if (wanted == base) current else wanted

    private fun mergeList(base: List<Labeled>, wanted: List<Labeled>, current: List<Labeled>): List<Labeled> {
        if (wanted == base) return current
        val removed = base.filter { it !in wanted }
        val added = wanted.filter { it !in base && it !in current }
        return current.filter { it !in removed } + added
    }

    /** Whether the user changed anything: an empty line that was added and stray blanks do not count. */
    fun isChanged(form: ContactForm, initial: ContactForm): Boolean = toContact(form) != toContact(initial)

    fun rowsOf(form: ContactForm, list: FormList): List<FormRow> = when (list) {
        FormList.EMAILS -> form.emails
        FormList.PHONES -> form.phones
        FormList.ADDRESSES -> form.addresses
    }

    private fun withRows(form: ContactForm, list: FormList, rows: List<FormRow>): ContactForm = when (list) {
        FormList.EMAILS -> form.copy(emails = rows)
        FormList.PHONES -> form.copy(phones = rows)
        FormList.ADDRESSES -> form.copy(addresses = rows)
    }

    /** Adds an empty line at the end of [list]. */
    fun addRow(form: ContactForm, list: FormList): ContactForm {
        val rows = rowsOf(form, list)
        return withRows(form, list, rows + FormRow((rows.maxOfOrNull { it.id } ?: -1) + 1, "", ""))
    }

    fun removeRow(form: ContactForm, list: FormList, id: Int): ContactForm =
        withRows(form, list, rowsOf(form, list).filterNot { it.id == id })

    fun changeRow(form: ContactForm, list: FormList, id: Int, change: (FormRow) -> FormRow): ContactForm =
        withRows(form, list, rowsOf(form, list).map { if (it.id == id) change(it) else it })
}
