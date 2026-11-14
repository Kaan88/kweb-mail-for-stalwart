package ovh.kweb.mail.ui.search

/** When and how much to search. */
object SearchRules {
    const val MIN_LENGTH = 2
    const val DEBOUNCE_MILLIS = 400L
    const val LIMIT = 50u

    /** Blanks around the query do not count; one letter would match everything. */
    fun isSearchable(query: String): Boolean = query.trim().codePointCount(0, query.trim().length) >= MIN_LENGTH
}
