package ovh.kweb.mail.ui.thread

/**
 * Folds the quoted part of a message (what the sender replied to), the way Gmail does in a long conversation. No Android and
 * no scripts: HTML quotes are wrapped in `<details>`, which the WebView opens and closes by itself, and plain-text quotes are
 * cut into parts the screen shows behind a toggle. Nothing is ever removed or changed: the folded text is the original, only
 * wrapped, and a message that is nothing but a quote is left open.
 */
object QuoteFolding {
    /** In a conversation of this many messages every quote is folded; in a shorter one only a long quote is. */
    const val LONG_CONVERSATION = 3

    /** A quote this long (characters of HTML) is folded even in a short conversation. */
    const val LONG_HTML_QUOTE = 1200

    /** A plain-text quote of this many lines is folded even in a short conversation. */
    const val LONG_TEXT_QUOTE = 8

    /** A message longer than this (characters of HTML) is not folded: a hostile one could make the work slow. */
    private const val MAX_FOLDED = 1_000_000

    fun foldAll(messageCount: Int): Boolean = messageCount >= LONG_CONVERSATION

    private val opening = Regex("<(blockquote|div)\\b[^<>]*>", RegexOption.IGNORE_CASE)

    // Two linear steps (the attribute with a possessive value, then the word inside it): one pattern that tried both would
    // rescan a very long tag once for every hit.
    private val classAttribute = Regex("class\\s*=\\s*([\"'])([^\"']*+)\\1", RegexOption.IGNORE_CASE)
    private val quoteWord = Regex("\\b(gmail_quote|yahoo_quoted)\\b", RegexOption.IGNORE_CASE)
    private fun quoteClass(tag: String): Boolean = classAttribute.findAll(tag).any { quoteWord.containsMatchIn(it.groupValues[2]) }
    private val hidden = listOf("style", "head", "script", "title")

    /**
     * True when [s] has nothing a reader would see: only tags, comments, the contents of style, head, script and title,
     * `&nbsp;` and white space. One pass: a closing tag or `-->` that is not there is looked for once, never again, so a
     * hostile mail with thousands of unclosed tags cannot make this slow.
     */
    internal fun nothingToRead(s: String): Boolean {
        val missing = HashSet<String>()
        var i = 0
        while (i < s.length) {
            val c = s[i]
            if (c == '<') {
                if (s.startsWith("<!--", i)) {
                    val end = if ("-->" in missing) -1 else s.indexOf("-->", i + 4)
                    if (end >= 0) {
                        i = end + 3
                        continue
                    }
                    missing.add("-->")
                }
                val close = s.indexOf('>', i)
                if (close < 0) return false
                val name = hidden.firstOrNull { h ->
                    s.regionMatches(i + 1, h, 0, h.length, ignoreCase = true) && !s.getOrElse(i + 1 + h.length) { ' ' }.isLetterOrDigit()
                }
                if (name != null && name !in missing) {
                    val end = s.indexOf("</" + name, close, ignoreCase = true)
                    if (end >= 0) {
                        val endTag = s.indexOf('>', end)
                        if (endTag >= 0) {
                            i = endTag + 1
                            continue
                        }
                    }
                    missing.add(name)
                }
                i = close + 1
            } else if (c.isWhitespace()) {
                i++
            } else if (s.startsWith("&nbsp;", i) || s.startsWith("&#160;", i)) {
                i += 6
            } else {
                return false
            }
        }
        return true
    }

    // What mail programs write at the start of a forwarded message ("---------- Forwarded message ---------", Apple Mail's
    // "Begin forwarded message:", and the same in German, French, Spanish, Italian and Turkish).
    private val forwardMark = Regex(
        "forwarded message|weitergeleitete nachricht|message transféré|mensaje reenviado|messaggio inoltrato|iletilen ileti",
        RegexOption.IGNORE_CASE,
    )

    /** How far before a quote and into it a forward's mark is looked for (characters of HTML). */
    private const val FORWARD_LOOK = 400

    /**
     * True when the quote at [r] is a forwarded message: the mark is in its first characters or just before it. A forward is
     * what the mail is about, so its length does not fold it (only a long conversation's trailing-quote rule does).
     */
    private fun isForward(html: String, r: IntRange): Boolean {
        val from = (r.first - FORWARD_LOOK).coerceAtLeast(0)
        val to = (r.first + FORWARD_LOOK).coerceAtMost(r.last + 1)
        return forwardMark.containsMatchIn(html.substring(from, to))
    }

    /** True for the start tag of something that is a quote: a `blockquote`, or a `div` of Gmail's or Yahoo's quote. */
    private fun isQuote(tag: String, name: String): Boolean = name.equals("blockquote", ignoreCase = true) || quoteClass(tag)

    /** The end (exclusive) of the element whose start tag ends at [from], counting elements of its own name inside it; -1 if never closed. */
    private fun closeOf(html: String, name: String, from: Int): Int {
        val tags = tagRegex(name)
        var depth = 1
        var at = from
        while (true) {
            val m = tags.find(html, at) ?: return -1
            at = m.range.last + 1
            if (m.groupValues[1].isEmpty()) {
                if (!m.value.endsWith("/>")) depth++
            } else if (--depth == 0) {
                return at
            }
        }
    }

    private val tagRegexes = HashMap<String, Regex>()

    private fun tagRegex(name: String): Regex = synchronized(tagRegexes) {
        tagRegexes.getOrPut(name.lowercase()) { Regex("<(/?)" + name.lowercase() + "\\b[^<>]*>", RegexOption.IGNORE_CASE) }
    }

    /** The outermost quotes of [html], as ranges (start inclusive, end exclusive as `first until end`), in order. */
    internal fun quoteRanges(html: String): List<IntRange> {
        val found = mutableListOf<IntRange>()
        // A quote that is never closed: no later start tag of that name can be closed either, so it is not looked for again.
        val unclosed = HashSet<String>()
        var at = 0
        while (true) {
            val m = opening.find(html, at) ?: break
            val name = m.groupValues[1].lowercase()
            if (name in unclosed || !isQuote(m.value, name)) {
                at = m.range.last + 1
                continue
            }
            val end = closeOf(html, name, m.range.last + 1)
            if (end < 0) {
                unclosed.add(name)
                at = m.range.last + 1
                continue
            }
            found.add(m.range.first until end)
            at = end
        }
        return found
    }

    /**
     * [html] with its quotes wrapped in `<details class="kq"><summary>...</summary>...</details>`, those that [foldAll] says
     * or that are long. Unchanged when there is no quote to fold or when nothing but quotes would be left to read.
     */
    fun foldHtml(html: String, label: String, foldAll: Boolean): String {
        if (html.length > MAX_FOLDED) return html
        val all = quoteRanges(html)
        if (all.isEmpty()) return html
        // Where the last thing to read outside the quotes ends: a quote after it is a trailing quote (the reply's own text is
        // above it), and only such a quote is folded for being in a long conversation. A quote with text after it (a pull
        // quote in a newsletter, an answer written between the lines) stays open. A long quote is folded wherever it is.
        var lastText = -1
        var segEnd = html.length
        for (r in all.asReversed()) {
            if (r.last + 1 < segEnd && !nothingToRead(html.substring(r.last + 1, segEnd))) {
                lastText = segEnd
                break
            }
            segEnd = r.first
        }
        if (lastText < 0 && !nothingToRead(html.substring(0, segEnd))) lastText = segEnd
        // Nothing to read outside the quotes: leave the message open.
        if (lastText < 0) return html
        val ranges = all.filter { (foldAll && it.first >= lastText) || (it.last + 1 - it.first >= LONG_HTML_QUOTE && !isForward(html, it)) }
        if (ranges.isEmpty()) return html
        val summary = "<summary>\u2026 " + label.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;") + "</summary>"
        return buildString {
            var from = 0
            for (r in ranges) {
                append(html, from, r.first)
                append("<details class=\"kq\">").append(summary).append(html, r.first, r.last + 1).append("</details>")
                from = r.last + 1
            }
            append(html, from, html.length)
        }
    }

    /** A piece of a plain-text message. The pieces joined with a newline are the text again. */
    data class Part(val text: String, val quoted: Boolean)

    private const val WROTE = "(wrote|a \u00e9crit|schrieb|escribi\u00f3|scrisse|yazd\u0131):\\s*$"
    private val attribution = Regex("(?i)^\\s*(on|le|am|el|il)\\b.*$WROTE")
    private val attributionTail = Regex("(?i)^.*$WROTE")
    private val attributionHead = Regex("(?i)^\\s*(on|le|am|el|il)\\b.*$")

    private fun isQuoteLine(line: String) = line.trimStart().startsWith(">")

    /**
     * [text] cut into open and quoted parts: runs of lines that start with ">" (blank lines between two quoted lines belong to
     * the run), with the "On ... wrote:" line before a run (one line, or two when the sender's mail program wrapped it). A run is
     * quoted when [foldAll] says or when it is long; otherwise it stays in the open text. One open part when there is nothing to
     * fold or nothing but quotes.
     */
    fun splitText(text: String, foldAll: Boolean): List<Part> {
        val lines = text.split("\n")
        val runs = mutableListOf<IntRange>()
        var i = 0
        while (i < lines.size) {
            if (!isQuoteLine(lines[i])) {
                i++
                continue
            }
            var last = i
            var j = i + 1
            while (j < lines.size) {
                if (isQuoteLine(lines[j])) {
                    last = j
                    j++
                } else if (lines[j].isBlank() && j + 1 < lines.size && isQuoteLine(lines[j + 1])) {
                    j++
                } else {
                    break
                }
            }
            var first = i
            val floor = (runs.lastOrNull()?.last ?: -1) + 1
            if (first - 1 >= floor && attribution.matches(lines[first - 1])) {
                first -= 1
            } else if (first - 2 >= floor && attributionTail.matches(lines[first - 1]) && attributionHead.matches(lines[first - 2])) {
                first -= 2
            }
            runs.add(first..last)
            i = last + 1
        }
        // As for HTML: in a long conversation only a quote with nothing to read after it is folded for that; a long one always.
        val inRun = BooleanArray(lines.size)
        for (r in runs) for (k in r) inRun[k] = true
        val lastOpen = lines.indices.lastOrNull { !inRun[it] && lines[it].isNotBlank() } ?: -1
        val folded = runs.filter { run -> (foldAll && run.first > lastOpen) || run.count { isQuoteLine(lines[it]) } >= LONG_TEXT_QUOTE }
        if (folded.isEmpty()) return listOf(Part(text, false))
        val parts = mutableListOf<Part>()
        var from = 0
        for (run in folded) {
            if (run.first > from) parts.add(Part(lines.subList(from, run.first).joinToString("\n"), false))
            parts.add(Part(lines.subList(run.first, run.last + 1).joinToString("\n"), true))
            from = run.last + 1
        }
        if (from < lines.size) parts.add(Part(lines.subList(from, lines.size).joinToString("\n"), false))
        // Nothing to read outside the quotes: leave the message open.
        if (parts.none { !it.quoted && it.text.isNotBlank() }) return listOf(Part(text, false))
        return parts
    }
}
