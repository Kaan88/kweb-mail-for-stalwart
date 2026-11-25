package ovh.kweb.mail.ui.thread

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class FitScriptTest {
    /** The mail can rename what `document.body` or an element's own properties point to: only prototypes are used. */
    @Test fun usesNoPropertyTheMailCouldClobber() {
        val js = FitScript.JS
        assertFalse(js.contains("document.body"))
        assertFalse(js.contains(".children"))
        assertFalse(js.contains(".getBoundingClientRect("))
        assertTrue(js.contains("Element.prototype"))
    }

    @Test fun fitsAFoldedQuoteWhenItIsOpened() = assertTrue(FitScript.JS.contains("'toggle'"))

    @Test fun reachesNothingOutsideThePage() {
        for (word in listOf("fetch", "XMLHttpRequest", "location", "textContent", "innerHTML", "innerText", "cookie", "open(")) {
            assertFalse(word, FitScript.JS.contains(word))
        }
    }
}
