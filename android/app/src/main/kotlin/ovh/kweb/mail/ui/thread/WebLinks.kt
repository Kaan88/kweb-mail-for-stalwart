package ovh.kweb.mail.ui.thread

/**
 * Web addresses written as plain text in a mail (`https://…`, `www.…`) become links, as Gmail shows them (owner,
 * 2026-11-25). Only `http`, `https` and `www.` are recognised, so a link made here can only be a web address; a tap still
 * goes through [UrlPolicy] like any other link. Nothing else in the mail is changed, and no script is involved.
 */
object WebLinks {
    // Possessive: the run of address characters is taken once, never rescanned.
    private val ADDRESS = Regex("""(?<![\w@./])(?:https?://|www\.)[^\s<>"'`]++""", RegexOption.IGNORE_CASE)

    /** Characters that end a sentence rather than an address when they come last. */
    private const val TRAILING = ".,;:!?'\""

    /** HTML entities that end an address in HTML text (a space, a bracket or a quote written as an entity). */
    private val ENTITY_STOPS = listOf("&nbsp;", "&#160;", "&lt;", "&gt;", "&quot;", "&#39;", "&#34;", "&apos;")

    /** A mail larger than this (characters of HTML) is left as it is: a hostile one could make the work slow. */
    private const val MAX_HTML = 1_000_000

    /** The address to open for [found] (one of [find]'s results): `www.` gets `https://`. */
    fun href(found: String): String = if (found.startsWith("www.", ignoreCase = true)) "https://$found" else found

    /** An address longer than this is not made a link (no real one is; a hostile one could make the work slow). */
    private const val MAX_ADDRESS = 2_000

    /** Where the web addresses in plain [text] are. */
    fun find(text: String, html: Boolean = false): List<IntRange> = ADDRESS.findAll(text).mapNotNull { match ->
        val start = match.range.first
        var end = match.range.last + 1
        if (end - start > MAX_ADDRESS) return@mapNotNull null
        if (html) {
            // Only inside the match: searching the rest of the text for every address would be quadratic.
            val found = text.substring(start, end)
            val cut = ENTITY_STOPS.mapNotNull { stop -> found.indexOf(stop, ignoreCase = true).takeIf { it >= 0 } }.minOrNull()
            if (cut != null) end = start + cut
        }
        end = trimEnd(text, start, end)
        // Little more than the prefix ("https://", "www.") is left: not a link.
        val prefix = if (text.startsWith("www.", start, ignoreCase = true)) 4 else text.indexOf("://", start) - start + 3
        if (end - start < prefix + 3) null else start until end
    }.toList()

    /** Leaves out what ends a sentence, and a closing bracket the address did not open. Brackets are counted once. */
    private fun trimEnd(text: String, start: Int, endIn: Int): Int {
        var end = endIn
        var round = 0
        var square = 0
        for (k in start until end) {
            when (text[k]) {
                '(' -> round++
                ')' -> round--
                '[' -> square++
                ']' -> square--
            }
        }
        while (end > start) {
            val c = text[end - 1]
            val drop = when (c) {
                in TRAILING -> true
                ')' -> round < 0
                ']' -> square < 0
                else -> false
            }
            if (!drop) break
            if (c == ')') round++ else if (c == ']') square++
            end--
        }
        return end
    }

    /** Elements whose text is never made a link: links themselves, and what is not shown as text. */
    private val SKIPPED = setOf("a", "style", "script", "title", "head", "textarea", "button", "select", "option", "svg")

    /**
     * [html] with the web addresses in its text made links. Tags, comments, attribute values and the contents of
     * [SKIPPED] elements are copied unchanged. One pass: an end that is missing is looked for once and the rest is copied.
     */
    fun html(html: String): String {
        if (html.length > MAX_HTML || !html.contains("http", ignoreCase = true) && !html.contains("www.", ignoreCase = true)) {
            return html
        }
        val out = StringBuilder(html.length + 64)
        var i = 0
        val n = html.length
        while (i < n) {
            val lt = html.indexOf('<', i)
            val textEnd = if (lt < 0) n else lt
            if (textEnd > i) linkText(html, i, textEnd, out)
            if (lt < 0) break
            if (html.startsWith("<!--", lt)) {
                val close = html.indexOf("-->", lt + 4)
                val end = if (close < 0) n else close + 3
                out.append(html, lt, end)
                i = end
                continue
            }
            val tagEnd = tagEnd(html, lt)
            out.append(html, lt, tagEnd)
            val name = tagName(html, lt)
            i = tagEnd
            if (name != null && name in SKIPPED && !html.startsWith("/", lt + 1) && html[tagEnd - 2] != '/') {
                val end = closeTag(html, name, tagEnd)
                out.append(html, tagEnd, end)
                i = end
            }
        }
        return out.toString()
    }

    private fun linkText(html: String, from: Int, to: Int, out: StringBuilder) {
        val text = html.substring(from, to)
        var last = 0
        for (range in find(text, html = true)) {
            out.append(text, last, range.first)
            val found = text.substring(range)
            out.append("<a href=\"").append(href(found)).append("\">").append(found).append("</a>")
            last = range.last + 1
        }
        out.append(text, last, text.length)
    }

    /** The index after the `>` of the tag at [lt], skipping quoted attribute values; the end of [s] when there is none. */
    private fun tagEnd(s: String, lt: Int): Int {
        var i = lt + 1
        var quote = 0.toChar()
        while (i < s.length) {
            val c = s[i]
            if (quote != 0.toChar()) {
                if (c == quote) quote = 0.toChar()
            } else if (c == '"' || c == '\'') {
                quote = c
            } else if (c == '>') {
                return i + 1
            }
            i++
        }
        return s.length
    }

    /** Where the end tag of [name] starts after [from] (the name exactly: `</a` must not match `</abbr>`); the end of [s] when there is none. */
    private fun closeTag(s: String, name: String, from: Int): Int {
        var i = from
        while (true) {
            val close = s.indexOf("</$name", i, ignoreCase = true)
            if (close < 0) return s.length
            val after = close + 2 + name.length
            if (after >= s.length || !s[after].isLetterOrDigit()) return close
            i = after
        }
    }

    /** The lower-case name of the tag at [lt] (without a leading `/`), or null. */
    private fun tagName(s: String, lt: Int): String? {
        var i = lt + 1
        if (i < s.length && s[i] == '/') i++
        val start = i
        while (i < s.length && (s[i].isLetterOrDigit())) i++
        return if (i > start) s.substring(start, i).lowercase() else null
    }
}
