package ovh.kweb.mail.ui.mailextras

import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.font.FontWeight
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import uniffi.kweb_core.MarkRange
import uniffi.kweb_core.Marked

class HighlightTest {
    private fun m(start: Int, end: Int) = MarkRange(start.toUInt(), end.toUInt())

    @Test fun rangesInsideTheTextStayAsTheyAre() {
        assertEquals(listOf(0..2, 6..10), Highlight.clamp("hello world", listOf(m(0, 3), m(6, 11))))
    }

    @Test fun endsOutsideTheTextArePulledIn() {
        assertEquals(listOf(6..10), Highlight.clamp("hello world", listOf(m(6, 99))))
        assertEquals(emptyList<IntRange>(), Highlight.clamp("hello", listOf(m(50, 60))))
        assertEquals(listOf(0..4), Highlight.clamp("hello", listOf(MarkRange(0u, UInt.MAX_VALUE))))
    }

    @Test fun emptyAndInvertedRangesAreDropped() {
        assertEquals(emptyList<IntRange>(), Highlight.clamp("hello", listOf(m(2, 2), m(4, 1))))
    }

    @Test fun overlappingAndTouchingRangesBecomeOne() {
        assertEquals(listOf(0..5), Highlight.clamp("hello world", listOf(m(3, 6), m(0, 4))))
        assertEquals(listOf(0..4), Highlight.clamp("hello world", listOf(m(0, 2), m(2, 5))))
    }

    @Test fun rangesAreSortedAndAnEmptyTextHasNone() {
        assertEquals(listOf(0..1, 4..5), Highlight.clamp("hello world", listOf(m(4, 6), m(0, 2))))
        assertEquals(emptyList<IntRange>(), Highlight.clamp("", listOf(m(0, 3))))
    }

    @Test fun aRangeNeverCutsACharacterInTwo() {
        // "a😀b": the emoji is units 1 and 2.
        val text = "a😀b"
        assertEquals(listOf(1..2), Highlight.clamp(text, listOf(m(1, 2))))
        assertEquals(listOf(1..2), Highlight.clamp(text, listOf(m(2, 3))))
        assertEquals(listOf(0..3), Highlight.clamp(text, listOf(m(0, 4))))
    }

    @Test fun nothingMarkedIsNoHighlight() {
        assertNull(Highlight.of(null))
        assertNull(Highlight.of(Marked("", listOf(m(0, 1)))))
    }

    @Test fun aMarkedTextWithoutMarksIsPlain() {
        assertEquals(Highlight("hello", emptyList()), Highlight.of(Marked("hello", emptyList())))
    }

    @Test fun theStyleLandsOnTheMarkedStretchesOnly() {
        val style = SpanStyle(fontWeight = FontWeight.Bold)
        val annotated = Highlight.of(Marked("hello world", listOf(m(6, 11))))!!.annotated(style)
        assertEquals("hello world", annotated.text)
        val span = annotated.spanStyles.single()
        assertEquals(6, span.start)
        assertEquals(11, span.end)
        assertEquals(style, span.item)
    }
}
