package ovh.kweb.mail.ui.contacts

import java.net.URLEncoder
import java.util.Locale
import uniffi.kweb_core.ContactRow

/** The pure rules of the contact list and of the contact screen. */
object ContactRules {
    /** The heading of the names that do not start with a letter. */
    const val OTHER = "#"

    /** How long after the last key the list is asked for the query. */
    const val DEBOUNCE_MILLIS = 300L

    /** How many messages "Recent mail" shows. */
    const val RECENT_LIMIT = 5

    private fun firstCodePoint(text: String): Int? = if (text.isEmpty()) null else text.codePointAt(0)

    private fun upper(codePoint: Int, locale: Locale): String =
        String(Character.toChars(codePoint)).uppercase(locale).let { it.substring(0, it.offsetByCodePoints(0, 1)) }

    /**
     * The letter a name is filed under: its first letter, upper-cased the way [locale] does it (in Turkish
     * "i" is "İ" and "ı" is "I"), or [OTHER] when the name does not start with a letter.
     */
    fun letter(name: String, locale: Locale): String {
        val cp = firstCodePoint(name.trim()) ?: return OTHER
        return if (Character.isLetter(cp)) upper(cp, locale) else OTHER
    }

    /** The letter or digit in the round avatar: the first one of the name, upper-cased; "?" when there is none. */
    fun initial(name: String, locale: Locale): String {
        val text = name.trim()
        var i = 0
        while (i < text.length) {
            val cp = text.codePointAt(i)
            if (Character.isLetterOrDigit(cp)) return upper(cp, locale)
            i += Character.charCount(cp)
        }
        return "?"
    }

    /** One line of the list: what to draw. */
    sealed interface Entry {
        data class Heading(val letter: String) : Entry

        /** A contact, with its place in the group under its heading (for the corners). */
        data class Item(val row: ContactRow, val indexInGroup: Int, val groupSize: Int) : Entry
    }

    /**
     * The rows under their letters. The rows come sorted by the core; the headings follow the order in which
     * their letters first appear, except that [OTHER] goes last, so a letter never has two headings.
     */
    fun group(rows: List<ContactRow>, locale: Locale): List<Entry> {
        val byLetter = LinkedHashMap<String, MutableList<ContactRow>>()
        for (row in rows) byLetter.getOrPut(letter(row.name, locale)) { mutableListOf() }.add(row)
        val order = byLetter.keys.filter { it != OTHER } + listOfNotNull(OTHER.takeIf { it in byLetter })
        return buildList {
            for (letter in order) {
                val members = byLetter.getValue(letter)
                add(Entry.Heading(letter))
                members.forEachIndexed { index, row -> add(Entry.Item(row, index, members.size)) }
            }
        }
    }

    /** The quiet line under a name: the first mail address and the organization, as far as there are any. */
    fun subtitle(row: ContactRow): String = listOf(row.email, row.organization).map { it.trim() }.filter { it.isNotEmpty() }.joinToString(" · ")

    /** The first mail address of a contact that has a value, the one "Recent mail" is searched by. */
    fun firstEmail(emails: List<uniffi.kweb_core.Labeled>): String? = emails.map { it.value.trim() }.firstOrNull { it.isNotEmpty() }

    /**
     * The name put in front of [email] when a message is started to it. The core names a card that has none
     * after its first mail address, and that is no name for another address of the card: only a name that
     * is not one of the card's own addresses is used.
     */
    fun recipientName(name: String, emails: List<uniffi.kweb_core.Labeled>, email: String): String {
        val trimmed = name.trim()
        return if (emails.any { it.value.trim() == trimmed } || trimmed == email.trim()) "" else trimmed
    }

    /** The address a tap on a phone number dials: only what a dialler reads, spaces and punctuation left out. */
    fun dialTarget(number: String): String {
        val kept = number.filter { it.isDigit() || it == '+' || it == '*' || it == '#' || it == ',' || it == ';' }
        return kept.ifEmpty { number.trim() }
    }

    /** `tel:` for [number]. */
    fun dialUri(number: String): String = "tel:" + encode(dialTarget(number))

    /** `geo:` with a query for [address], which a map app searches for; line breaks become spaces. */
    fun geoUri(address: String): String = "geo:0,0?q=" + encode(address.replace(Regex("\\s*\\R\\s*"), ", ").trim())

    private fun encode(text: String): String = URLEncoder.encode(text, "UTF-8").replace("+", "%20")

    /** The labels the menu offers. A word of the user's own that came with a contact is kept as it is until another is picked. */
    enum class LabelKind { WORK, PRIVATE, OTHER, CUSTOM }

    fun labelKind(label: String): LabelKind = when (label.trim().lowercase(Locale.ROOT)) {
        "work" -> LabelKind.WORK
        "private", "home" -> LabelKind.PRIVATE
        "" , "other" -> LabelKind.OTHER
        else -> LabelKind.CUSTOM
    }

    /** What is stored for a label picked from the menu: the core's words "work" and "private", nothing for Other. */
    fun labelValue(kind: LabelKind, current: String): String = when (kind) {
        LabelKind.WORK -> "work"
        LabelKind.PRIVATE -> "private"
        LabelKind.OTHER -> ""
        LabelKind.CUSTOM -> current
    }
}
