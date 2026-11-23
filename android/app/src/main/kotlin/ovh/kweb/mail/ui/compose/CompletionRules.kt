package ovh.kweb.mail.ui.compose

import java.util.Locale
import uniffi.kweb_core.Address

/**
 * Smart completion while writing, as Gmail does it: the first name of the first recipient after a greeting, and the
 * rest of a sign-off. Pure and on the device only: text, cursor, recipients and the sender's name in, one suggestion
 * (or none) out. The screen renders it and applies it on a tap; nothing is sent anywhere.
 */
object CompletionRules {
    enum class Kind { Name, SignOff }

    /**
     * Replace `text[start, end)` with [replacement]; [label] is what the chip shows. [kind] says which of the two it is.
     */
    data class Suggestion(val kind: Kind, val start: Int, val end: Int, val replacement: String, val label: String)

    /** Greetings (folded: lower case, no accents) that a first name may follow. */
    private val greetings = setOf(
        "hi", "hello", "hey", "dear", "hola", "hallo", "bonjour", "merhaba", "selam", "sayin", "sevgili",
    )

    /** Sign-offs in the order they are offered; the first one that begins like the typed fragment wins. */
    private val signOffs = listOf(
        "Best regards,", "Best wishes,", "Kind regards,", "Warm regards,", "Regards,", "Thanks,", "Thank you,",
        "Sincerely,", "Yours sincerely,", "Cheers,", "Saygılarımla,", "Saygılar,", "İyi günler,", "İyi çalışmalar,",
        "Teşekkürler,", "Sevgiler,",
    )

    /** Words that mark an address or a display name as a role or a company, never a person. */
    private val roles = setOf(
        "info", "noreply", "no-reply", "donotreply", "admin", "administrator", "support", "sales", "contact", "hello",
        "office", "mail", "team", "billing", "help", "hr", "jobs", "press", "news", "newsletter", "postmaster",
        "webmaster", "service", "services", "accounts", "account", "enquiries", "inquiries", "reply", "notifications",
        "notification", "root", "abuse", "marketing", "orders", "order", "careers", "security", "feedback", "hi",
        "customer", "care", "desk", "bot", "system", "alerts", "alert", "updates", "ops", "legal", "finance",
        "invoice", "invoices", "payments", "mailer", "daemon", "bounce", "bounces", "list", "digest",
    )

    /** The shortest fragment of a sign-off that is offered a completion. */
    private const val MIN_FRAGMENT = 3

    /**
     * The suggestion for the body [text] with the cursor at [selStart] (and [selEnd] when text is selected), or null.
     * [recipients] are the To addresses in order; [identityName] is the sending identity's display name.
     */
    fun suggest(
        text: String,
        selStart: Int,
        selEnd: Int,
        recipients: List<Address>,
        identityName: String,
        locale: Locale = Locale.getDefault(),
    ): Suggestion? {
        if (selStart != selEnd) return null
        val cursor = selStart
        if (cursor < 0 || cursor > text.length) return null
        // Only at the end of a line: what follows on the line is not ours to rewrite.
        val lineEnd = text.indexOf('\n', cursor).let { if (it < 0) text.length else it }
        if (text.substring(cursor, lineEnd).isNotBlank()) return null
        val lineStart = text.lastIndexOf('\n', cursor - 1).let { if (it < 0 || cursor == 0) 0 else it + 1 }
        val prefix = text.substring(lineStart, cursor)
        if (prefix.isEmpty() || prefix.first().isWhitespace()) return null
        if (prefix.startsWith(">")) return null
        if (inQuotedPart(text.substring(0, lineStart))) return null

        greeting(prefix, lineStart, cursor, recipients, locale)?.let { return it }
        return signOff(text, prefix, lineStart, cursor, lineEnd, identityName)
    }

    /** The new text and where the cursor goes once [s] is applied to [text]. */
    fun apply(text: String, s: Suggestion): Pair<String, Int> {
        val start = s.start.coerceIn(0, text.length)
        val end = s.end.coerceIn(start, text.length)
        val done = text.substring(0, start) + s.replacement + text.substring(end)
        return done to start + s.replacement.length
    }

    /** "Hi " (or "Hi Jo") at the start of a line: the first name of the first recipient. */
    private fun greeting(prefix: String, lineStart: Int, cursor: Int, recipients: List<Address>, locale: Locale): Suggestion? {
        val word = prefix.takeWhile { it.isLetter() }
        if (word.isEmpty() || fold(word) !in greetings) return null
        val rest = prefix.substring(word.length)
        if (rest.isEmpty() || !rest.first().isWhitespace() || rest.first() == '\n') return null
        val spaces = rest.takeWhile { it == ' ' || it == '\t' }
        val partial = rest.substring(spaces.length)
        if (partial.any { !(it.isLetter() || it == '-' || it == '\'') }) return null
        val name = recipients.firstOrNull()?.let { firstName(it, locale) } ?: return null
        val foldedName = fold(name)
        val foldedPartial = fold(partial)
        if (foldedPartial.length >= foldedName.length || !foldedName.startsWith(foldedPartial)) return null
        return Suggestion(Kind.Name, cursor - partial.length, cursor, name, name)
    }

    /** "Best" at the start of a line: the whole sign-off, then the sender's name unless it is already below. */
    private fun signOff(text: String, prefix: String, lineStart: Int, cursor: Int, lineEnd: Int, identityName: String): Suggestion? {
        // A sign-off ends something: there has to be text above it.
        if (text.substring(0, lineStart).isBlank()) return null
        val fragment = fold(prefix)
        if (fragment.length < MIN_FRAGMENT) return null
        val full = signOffs.firstOrNull { fold(it).startsWith(fragment) } ?: return null
        if (fold(full) == fragment) return null
        val name = identityName.trim()
        // Only what the user writes counts, not the quoted reply under it.
        val belowLines = text.substring(lineEnd).split('\n')
        val cut = belowLines.indexOfFirst { l ->
            val t = l.trim()
            t.startsWith(">") || fold(t).let { it.startsWith("on ") && it.endsWith("wrote:") }
        }
        val below = (if (cut < 0) belowLines else belowLines.take(cut)).joinToString("\n")
        val withName = name.isNotEmpty() && !below.contains(name, ignoreCase = true)
        val replacement = if (withName) full + "\n" + name else full
        return Suggestion(Kind.SignOff, lineStart, cursor, replacement, full)
    }

    /** The cursor is below a quoted reply, a forwarded message header or the signature mark: no suggestions there. */
    private fun inQuotedPart(before: String): Boolean {
        for (line in before.split('\n')) {
            val t = line.trim()
            if (t.isEmpty()) continue
            val f = fold(t)
            if (f.startsWith(">")) continue
            if (f.startsWith("on ") && f.endsWith("wrote:")) return true
            if (f.endsWith("yazdi:")) return true
            if (f.contains("original message") && f.startsWith("---")) return true
            if (f.contains("forwarded message") && f.startsWith("---")) return true
            if (line == "-- " || t == "--") return true
        }
        return false
    }

    /** The first name to offer for [a]: from its display name, or a clear first.last address; never from a role address. */
    fun firstName(a: Address, locale: Locale = Locale.getDefault()): String? {
        val display = a.name.trim().trim('"', '\'').trim()
        if (display.isNotEmpty() && !display.contains('@')) {
            val part = if (display.contains(',')) display.substringAfter(',') else display
            val token = part.trim().split(Regex("\\s+")).firstOrNull().orEmpty()
            val tokens = display.split(Regex("[\\s,]+"))
            if (tokens.any { fold(it) in roles }) return null
            return token.takeIf { nameLike(it) }
        }
        if (display.contains('@')) return null
        val local = a.email.substringBefore('@').substringBefore('+')
        val parts = local.split('.', '_', '-')
        if (parts.size !in 2..3) return null
        if (parts.any { p -> p.length < 2 || p.any { !it.isLetter() } || fold(p) in roles }) return null
        return parts.first().lowercase(locale).replaceFirstChar { it.titlecase(locale) }
    }

    private fun nameLike(s: String): Boolean =
        s.length in 2..30 && s.first().isLetter() && s.all { it.isLetter() || it == '-' || it == '\'' }

    /** Lower case without accents or the dotted and dotless i, so "İyi", "iyi" and "Iyi" meet. */
    private fun fold(s: String): String {
        val b = StringBuilder(s.length)
        for (c in s) {
            b.append(
                when (c) {
                    'İ', 'ı', 'I' -> 'i'
                    'ş', 'Ş' -> 's'
                    'ç', 'Ç' -> 'c'
                    'ğ', 'Ğ' -> 'g'
                    'ö', 'Ö' -> 'o'
                    'ü', 'Ü' -> 'u'
                    else -> c.lowercaseChar()
                },
            )
        }
        return b.toString()
    }
}
