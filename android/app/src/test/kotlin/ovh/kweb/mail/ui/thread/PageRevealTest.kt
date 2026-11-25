package ovh.kweb.mail.ui.thread

import org.junit.Assert.assertEquals
import org.junit.Test

class PageRevealTest {
    private var shown = 0
    private var hidden = 0
    private val reveal = PageReveal(show = { shown++ }, hide = { hidden++ })

    @Test fun aPageThatFitsIsShownOnceDrawn() {
        val page = reveal.newPage(measure = true)
        reveal.drawn(reveal.finished()) { false }
        assertEquals(1, shown)
        reveal.timedOut(page)
        assertEquals(2, shown) // harmless: it is shown already
    }

    @Test fun aWidePageIsHiddenAndTheNextLoadIsShown() {
        reveal.newPage(measure = true)
        reveal.drawn(reveal.finished()) { true }
        assertEquals(0, shown)
        assertEquals(1, hidden)
        reveal.newPage(measure = false)
        reveal.drawn(reveal.finished()) { error("the second layout is not measured") }
        assertEquals(1, shown)
    }

    @Test fun aLateFinishOfAReplacedLoadDoesNotStopTheNewOneBeingMeasured() {
        reveal.newPage(measure = true)
        reveal.newPage(measure = true)
        // The replaced load finishes after the new one started: it counts for the new number.
        reveal.drawn(reveal.finished()) { false }
        var measured = false
        reveal.drawn(reveal.finished()) { measured = true; true }
        assertEquals(true, measured)
        assertEquals(1, hidden)
    }

    @Test fun aCallbackOfAnEarlierLoadIsIgnored() {
        reveal.newPage(measure = true)
        val old = reveal.finished()
        reveal.newPage(measure = true)
        reveal.drawn(old) { error("an old load is not measured") }
        reveal.timedOut(old)
        assertEquals(0, shown)
    }

    @Test fun aSecondFinishAfterTheMeasurementAskedForASecondLayoutDoesNotShowTheSmallOne() {
        reveal.newPage(measure = true)
        val first = reveal.finished()
        val second = reveal.finished()
        reveal.drawn(first) { true }
        // The fit is set by now, so a second measurement would answer false.
        reveal.drawn(second) { false }
        assertEquals(0, shown)
        assertEquals(1, hidden)
    }

    @Test fun aViewMadeAgainAfterACrashHidesBeforeItsSecondLayoutThoughItWasNeverShown() {
        // The new view knows nothing of the old one, which may have been shown already.
        reveal.newPage(measure = true)
        reveal.drawn(reveal.finished()) { true }
        assertEquals(1, hidden)
        assertEquals(0, shown)
    }

    @Test fun aPageThatNeverReportsIsShownAfterAll() {
        val page = reveal.newPage(measure = true)
        reveal.timedOut(page)
        assertEquals(1, shown)
    }
}
