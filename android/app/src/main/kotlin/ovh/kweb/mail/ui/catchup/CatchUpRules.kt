package ovh.kweb.mail.ui.catchup

import java.text.NumberFormat
import java.util.Locale
import ovh.kweb.mail.ui.attention.SparkleWords
import uniffi.kweb_core.AiCatchUp
import uniffi.kweb_core.AiCatchUpItem
import uniffi.kweb_core.AiSettings

/** What the Catch me up screens decide without Android: whether the entry is shown, and how a result is read. */
object CatchUpRules {
    /** The id of the finding the result's "Find to-dos and dates" runs under; not a real finding, never kept. */
    const val FINDER_ID = "catchup-finder"

    /** The entry is shown when the assistant is on and may summarize (the run is a summary of unread mail). */
    fun offered(settings: AiSettings): Boolean = settings.enabled && settings.allowSummaries

    /** "Sent 12 mails to OpenAI · gpt-5 at 09:41 · 2,412 tokens in, 318 out"; the tokens are left out when the service reported none. */
    fun costNote(result: AiCatchUp, service: String, time: String, words: SparkleWords, locale: Locale): String {
        val note = words.cost(words.sentMails(result.mails.toInt()), service, result.model, time)
        val usage = result.usage ?: return note
        val format = NumberFormat.getIntegerInstance(locale)
        val tokens = words.tokens(usage.inputTokens?.let { format.format(it.toLong()) }, usage.outputTokens?.let { format.format(it.toLong()) })
        return if (tokens == null) note else words.withTokens(note, tokens)
    }

    /** The items that ask for an answer, for the line above the list. */
    fun needingAnswer(items: List<AiCatchUpItem>): Int = items.count { it.needsAnswer }
}
