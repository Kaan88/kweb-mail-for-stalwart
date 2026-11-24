package ovh.kweb.mail.ui.thread

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class PdfViewRulesTest {
    @Test
    fun zoom_stays_between_fit_and_the_most() {
        assertEquals(2f, PdfViewRules.zoom(1f, 2f), 0f)
        assertEquals(1f, PdfViewRules.zoom(1.5f, 0.1f), 0f)
        assertEquals(PdfViewRules.MAX_ZOOM, PdfViewRules.zoom(3f, 10f), 0f)
    }

    @Test
    fun the_page_moves_only_by_what_sticks_out() {
        // At zoom 1 nothing sticks out.
        assertEquals(0f, PdfViewRules.offset(50f, 1f, 1000f), 0f)
        // At zoom 2 a 1000 px page sticks out 500 px on each side.
        assertEquals(300f, PdfViewRules.offset(300f, 2f, 1000f), 0f)
        assertEquals(500f, PdfViewRules.offset(900f, 2f, 1000f), 0f)
        assertEquals(-500f, PdfViewRules.offset(-900f, 2f, 1000f), 0f)
    }

    @Test
    fun bitmaps_are_twice_the_view_and_capped() {
        assertEquals(2160, PdfViewRules.bitmapWidth(1080))
        assertEquals(PdfViewRules.MAX_BITMAP_WIDTH, PdfViewRules.bitmapWidth(1440))
        assertEquals(1, PdfViewRules.bitmapWidth(0))
    }

    @Test
    fun a_very_tall_page_is_made_smaller_as_a_whole() {
        // A4 at 2160 px wide stays as it is.
        assertEquals(2160 to 3056, PdfViewRules.bitmapSize(2160, 595, 842))
        // A receipt ten times as tall as wide would be 46 million pixels: it keeps its shape under the cap.
        val (w, h) = PdfViewRules.bitmapSize(2160, 200, 2000)
        assertTrue(w.toLong() * h <= PdfViewRules.MAX_PIXELS)
        assertEquals(10f, h.toFloat() / w, 0.01f)
        // A page with no size does not divide by zero.
        assertEquals(PdfViewRules.DEFAULT_RATIO, PdfViewRules.ratio(0, 842), 0f)
    }
}
