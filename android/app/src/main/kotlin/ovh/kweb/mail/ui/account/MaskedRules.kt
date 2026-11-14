package ovh.kweb.mail.ui.account

/** Pure decisions of the masked-address screen. */
object MaskedRules {
    const val MAX_PREFIX = 32

    /** Nothing more can be made: the server's limit counts switched-off addresses too. */
    fun atLimit(count: Int, limit: Int?): Boolean = limit != null && count >= limit

    /** "Starts with" is empty or letters, digits, dot, hyphen and underscore, at most [MAX_PREFIX]. Spaces around it are ignored. */
    fun prefixIsValid(prefix: String): Boolean {
        val p = prefix.trim()
        return p.length <= MAX_PREFIX && p.all { it in 'a'..'z' || it in 'A'..'Z' || it in '0'..'9' || it == '.' || it == '-' || it == '_' }
    }

    /** A site as typed: the scheme, a path and spaces are not part of it. */
    fun cleanSite(site: String): String =
        site.trim().removePrefix("https://").removePrefix("http://").substringBefore('/').trim()

    /** The note and the site of a created address, as the sheet says them ("Bookshop · example.com"); null when both are empty. */
    fun purpose(description: String, forDomain: String): String? =
        listOf(description.trim(), forDomain.trim()).filter { it.isNotEmpty() }.joinToString(" · ").ifEmpty { null }

    /** An address with the switch set to [enabled]; the list otherwise unchanged. */
    fun withEnabled(list: List<uniffi.kweb_core.MaskedAddress>, id: String, enabled: Boolean) =
        list.map { if (it.id == id) it.copy(enabled = enabled) else it }
}
