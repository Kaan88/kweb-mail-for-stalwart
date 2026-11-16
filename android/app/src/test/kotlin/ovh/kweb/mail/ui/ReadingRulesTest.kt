package ovh.kweb.mail.ui

import ovh.kweb.mail.ui.ReadingRules.AfterAction
import ovh.kweb.mail.ui.ReadingRules.MarkRead
import ovh.kweb.mail.ui.ReadingRules.TextSize
import org.junit.Test
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull

class ReadingRulesTest {
    @Test
    fun unknownKeysFallBackToTodaysBehaviour() {
        assertEquals(MarkRead.ON_OPEN, MarkRead.of(null))
        assertEquals(MarkRead.ON_OPEN, MarkRead.of("nonsense"))
        assertEquals(AfterAction.LIST, AfterAction.of(null))
        assertEquals(TextSize.DEFAULT, TextSize.of("huge"))
        assertEquals(MarkRead.NEVER, MarkRead.of(MarkRead.NEVER.key))
    }

    @Test
    fun markReadWaitsAsChosen() {
        assertEquals(0L, MarkRead.ON_OPEN.delayMillis)
        assertEquals(2_000L, MarkRead.AFTER_DELAY.delayMillis)
        assertNull(MarkRead.NEVER.delayMillis)
    }

    @Test
    fun previewLinesStayInRange() {
        assertEquals(0, ReadingRules.clampPreviewLines(-3))
        assertEquals(3, ReadingRules.clampPreviewLines(9))
        assertEquals(1, ReadingRules.DEFAULT_PREVIEW_LINES)
    }

    @Test
    fun textSizeGrowsInSteps() {
        assertEquals(listOf(85, 100, 125, 150), TextSize.entries.map { it.percent })
    }

    @Test
    fun theNextPageAfterAnActionFollowsTheChoice() {
        assertNull(ReadingRules.pageAfter(AfterAction.LIST, 1, 3))
        assertEquals(2, ReadingRules.pageAfter(AfterAction.NEXT, 1, 3))
        assertEquals("the last one steps back", 1, ReadingRules.pageAfter(AfterAction.NEXT, 2, 3))
        assertEquals(0, ReadingRules.pageAfter(AfterAction.PREVIOUS, 1, 3))
        assertEquals("the first one steps on", 1, ReadingRules.pageAfter(AfterAction.PREVIOUS, 0, 3))
        assertNull("a single page goes back to the list", ReadingRules.pageAfter(AfterAction.NEXT, 0, 1))
    }
}
