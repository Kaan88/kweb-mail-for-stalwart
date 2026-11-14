package ovh.kweb.mail.ui.mailextras

import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.withStyle
import uniffi.kweb_core.MarkRange
import uniffi.kweb_core.Marked

/** A text with the stretches to draw as found, already made safe for that text. */
data class Highlight(val text: String, val ranges: List<IntRange>) {
    /** [style] on every range; the plain text when there is none. */
    fun annotated(style: SpanStyle): AnnotatedString = buildAnnotatedString {
        append(text)
        ranges.forEach { addStyle(style, it.first, it.last + 1) }
    }

    companion object {
        /** From what the core gave; null when there is no text. */
        fun of(marked: Marked?): Highlight? {
            if (marked == null || marked.text.isEmpty()) return null
            return Highlight(marked.text, clamp(marked.text, marked.marks))
        }

        /**
         * The marks as ranges that lie inside [text] (UTF-16 units, as Kotlin counts): out-of-range ends are pulled in,
         * empty and inverted ones dropped, a range that would cut a character in two (a surrogate pair) takes the whole
         * character, and ranges that touch or overlap become one. Sorted, never overlapping.
         */
        fun clamp(text: String, marks: List<MarkRange>): List<IntRange> {
            val length = text.length
            val raw = marks.mapNotNull { mark ->
                val start = mark.start.toLong().coerceIn(0L, length.toLong()).toInt()
                val end = mark.end.toLong().coerceIn(0L, length.toLong()).toInt()
                if (end <= start) return@mapNotNull null
                val s = if (start in 1 until length && text[start].isLowSurrogate() && text[start - 1].isHighSurrogate()) start - 1 else start
                val e = if (end in 1 until length && text[end].isLowSurrogate() && text[end - 1].isHighSurrogate()) end + 1 else end
                s until e
            }.sortedBy { it.first }
            val merged = ArrayList<IntRange>()
            for (range in raw) {
                val last = merged.lastOrNull()
                if (last != null && range.first <= last.last + 1) {
                    if (range.last > last.last) merged[merged.size - 1] = last.first..range.last
                } else {
                    merged += range
                }
            }
            return merged
        }
    }
}
