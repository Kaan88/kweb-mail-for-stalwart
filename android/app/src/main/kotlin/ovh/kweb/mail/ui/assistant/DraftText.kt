package ovh.kweb.mail.ui.assistant

/** A stretch of a text: [start] up to, not including, [end]. */
data class Span(val start: Int, val end: Int) {
    val length: Int get() = end - start
}

/**
 * Which part of a draft is the user's own, and how a rewritten part goes back in. The assistant rewrites only
 * what the user wrote: not the quoted original of a reply, not a forwarded message, not the signature.
 */
object DraftText {
    private const val SIGNATURE_LINE = "-- "
    private const val ATTRIBUTION_END = "wrote:"

    /**
     * Where the user's own text ends: the start of the quote, or of the signature (the line "-- "), whichever comes
     * first; the end of the text when there is neither. A quote starts at its first line beginning with ">" (together
     * with a preceding attribution line ending in a colon, whatever language it is in, and the blank lines before that), at
     * a line ending in "wrote:", at a "-----Original Message-----" style separator, or at the header of a forwarded message.
     */
    fun ownEnd(body: String): Int {
        val lines = body.split('\n')
        val starts = IntArray(lines.size)
        var offset = 0
        for ((i, raw) in lines.withIndex()) {
            starts[i] = offset
            offset += raw.length + 1
        }
        for ((index, raw) in lines.withIndex()) {
            val line = raw.trimEnd('\r')
            if (line == SIGNATURE_LINE) return starts[index]
            if (line.startsWith(">")) return starts[quoteStart(lines, index)]
            if (line.trimEnd().endsWith(ATTRIBUTION_END) && lines.getOrNull(index + 1)?.startsWith(">") == true) return starts[index]
            if (isForwardHeader(line) || isOriginalMessage(line)) return starts[index]
        }
        return body.length
    }

    /** The line where the quote whose first ">" line is [quoted] really starts: its attribution line, if there is one. */
    private fun quoteStart(lines: List<String>, quoted: Int): Int {
        var i = quoted - 1
        while (i >= 0 && lines[i].isBlank()) i--
        // The line right before the quote (blank lines between are fine) is its attribution when it ends in a colon, as they do in every language.
        return if (i >= 0 && lines[i].trimEnd().endsWith(":") && !lines[i].startsWith(">")) i else quoted
    }

    private fun isOriginalMessage(line: String): Boolean {
        val trimmed = line.trim()
        return trimmed.startsWith("-----") && trimmed.endsWith("-----") && trimmed.contains("original message", ignoreCase = true)
    }

    private fun isForwardHeader(line: String): Boolean {
        val trimmed = line.trim()
        return trimmed.startsWith("-----") && trimmed.contains("forwarded message", ignoreCase = true)
    }

    /**
     * What a rewrite works on: the selection when something is selected ([selStart] to [selEnd], cut back to the
     * user's own text), otherwise all of it; without the blank space around it. Null when there is nothing of the
     * user's own to change.
     */
    fun target(body: String, selStart: Int, selEnd: Int): Span? {
        val limit = ownEnd(body)
        val from = minOf(selStart, selEnd).coerceIn(0, body.length)
        val to = maxOf(selStart, selEnd).coerceIn(0, body.length)
        var start = 0
        var end = limit
        if (from < to) {
            start = from
            end = minOf(to, limit)
        }
        while (start < end && body[start].isWhitespace()) start++
        while (end > start && body[end - 1].isWhitespace()) end--
        return if (start < end) Span(start, end) else null
    }

    /**
     * [body] with [result] in place of [original], which was at [span] when it was sent. The text may have
     * been edited meanwhile: when it is not at [span] any more, it is found again if it occurs exactly once.
     * Null when it cannot be placed or the result is blank: nothing is replaced then.
     */
    fun replace(body: String, span: Span, original: String, result: String): Span? {
        if (result.isBlank() || original.isEmpty()) return null
        if (span.start >= 0 && span.end <= body.length && span.length == original.length && body.regionMatches(span.start, original, 0, original.length)) {
            return span
        }
        val first = body.indexOf(original)
        if (first < 0 || body.indexOf(original, first + 1) >= 0) return null
        return Span(first, first + original.length)
    }

    /** The new body, and where the replacement ends (for the cursor). */
    data class Replaced(val body: String, val cursor: Int)

    fun apply(body: String, span: Span, original: String, result: String): Replaced? {
        val at = replace(body, span, original, result) ?: return null
        val text = result.trim()
        return Replaced(body.substring(0, at.start) + text + body.substring(at.end), at.start + text.length)
    }

    /** What an Undo puts back: [before], and only while the text is still what the replacement made ([after]). */
    fun undo(current: String, before: String, after: String): String? = if (current == after) before else null

    /**
     * A new reply's body with the assistant's [answer] above the quoted original (the body the core made starts
     * with a blank line, then the attribution).
     */
    fun withAnswer(body: String, answer: String): String {
        val text = answer.trim()
        return when {
            body.isEmpty() -> text
            body.startsWith("\n") -> text + body
            else -> text + "\n\n" + body
        }
    }
}
