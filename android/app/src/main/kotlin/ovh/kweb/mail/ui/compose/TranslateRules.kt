package ovh.kweb.mail.ui.compose

import java.util.Locale

/** The languages Translate offers, and the recent ones. The name the model gets is the English one. */
object TranslateRules {
    /** How many recent languages are kept and shown first. */
    const val RECENT = 4

    /** One choice: [name] (English, what the assistant is asked for) and [native] (how speakers write it; null when the same). */
    data class Language(val name: String, val native: String?)

    /** Language tags offered, roughly by how many people write mail in them, then the rest. */
    private val tags = listOf(
        "en", "zh-Hans", "zh-Hant", "es", "ar", "hi", "pt", "fr", "de", "ja", "ru", "tr", "ko", "it", "nl", "pl", "uk",
        "id", "vi", "th", "fa", "he", "el", "sv", "nb", "da", "fi", "cs", "sk", "hu", "ro", "bg", "sr", "hr", "sl", "lt",
        "lv", "et", "ms", "bn", "ur", "ta", "sw", "ca", "eu", "gl", "ga", "is", "sq", "mk", "ka", "hy", "az", "kk", "uz",
        "tl", "af",
    )

    /** Every language offered, with its English and own name, in the order above. */
    val all: List<Language> by lazy {
        tags.map { tag ->
            val locale = Locale.forLanguageTag(tag)
            val name = locale.getDisplayName(Locale.ENGLISH)
            val native = locale.getDisplayName(locale).replaceFirstChar { it.titlecase(locale) }
            Language(name, native.takeUnless { it.equals(name, ignoreCase = true) })
        }
    }

    /** The list shown for [query]: every language whose English or own name contains it (all of them for an empty query). */
    fun filter(query: String, languages: List<Language> = all): List<Language> {
        val q = query.trim()
        if (q.isEmpty()) return languages
        return languages.filter { it.name.contains(q, ignoreCase = true) || it.native?.contains(q, ignoreCase = true) == true }
    }

    /** [query] as a language of its own, offered when nothing in the list has exactly that name; null otherwise. */
    fun typed(query: String, languages: List<Language> = all): String? {
        val q = query.trim()
        if (q.isEmpty()) return null
        return q.takeUnless { languages.any { it.name.equals(q, ignoreCase = true) || it.native.equals(q, ignoreCase = true) } }
    }

    /** The recent list after [chosen] was used: it first, no repeat, at most [RECENT]. */
    fun remember(recent: List<String>, chosen: String): List<String> =
        (listOf(chosen) + recent.filterNot { it.equals(chosen, ignoreCase = true) }).take(RECENT)
}
