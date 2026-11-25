package ovh.kweb.mail.ui.thread

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class FitRulesTest {
    @Test fun aMailThatFitsOrNearlyFitsIsLeftAlone() {
        assertNull(FitRules.layoutWidth(360f, 360f))
        assertNull(FitRules.layoutWidth(390f, 360f))
        assertNull(FitRules.layoutWidth(200f, 360f))
    }

    @Test fun aWideSignatureOrANewsletterIsLaidOutAtItsOwnWidth() {
        assertEquals(560, FitRules.layoutWidth(559.2f, 360f))
        assertEquals(640, FitRules.layoutWidth(640f, 360f))
    }

    @Test fun aPageFarWiderThanTheWindowIsOnlyScaled() {
        assertNull(FitRules.layoutWidth(360f * 4, 360f))
    }

    @Test fun nothingMeasuredGivesNothing() {
        assertNull(FitRules.layoutWidth(0f, 360f))
        assertNull(FitRules.layoutWidth(500f, 0f))
    }

    @Test fun theSecondLayoutKnowsHowFarThePageWasScaled() {
        val fit = FitRules.fit(550f, 411f)!!
        assertEquals(550, fit.width)
        assertEquals(550f / 411f, fit.shrink, 0.0001f)
        assertNull(FitRules.fit(411f, 411f))
        assertNull(FitRules.fit(411f * 4, 411f))
    }

    @Test fun theUsersTextSizeIsAppliedOnceAndScaledBackUp() {
        assertEquals(130, FitRules.textZoom(130, null))
        val fit = FitRules.Fit(600, 1.5f)
        assertEquals(150, FitRules.textZoom(100, fit))
        assertEquals(195, FitRules.textZoom(130, fit))
        assertEquals(225, FitRules.textZoom(150, fit))
    }

    @Test fun aVeryWidePageIsMadeLargerByNoMoreThanTheCap() {
        val fit = FitRules.fit(360f * 3, 360f)!!
        assertEquals(3f, fit.shrink, 0.0001f)
        assertEquals(200, FitRules.textZoom(100, fit))
        assertEquals(300, FitRules.textZoom(150, fit))
        // Just under the cap the whole factor is used.
        assertEquals(199, FitRules.textZoom(100, FitRules.Fit(718, 1.99f)))
    }

    @Test fun aSecondLayoutWhoseTablesGrewWithTheTextGetsAThird() {
        // The owner's mail: a table of short cells, 520 px wide in a 385 px window; with the text 1.35 times larger it
        // grew to about 700 px and the page was scaled down again.
        val second = FitRules.fit(520f, 385f)!!
        assertNull(FitRules.tableZoom(second))
        val third = FitRules.refit(second, 702f)!!
        assertTrue(third.tablesBack)
        assertEquals(second.width, third.width)
        assertEquals(1f / second.shrink, FitRules.tableZoom(third)!!, 0.0001f)
        // Never a fourth.
        assertNull(FitRules.refit(third, 900f))
    }

    @Test fun aSecondLayoutThatFitsIsKept() {
        val second = FitRules.fit(550f, 411f)!!
        assertNull(FitRules.refit(second, 550f))
        assertNull(FitRules.refit(second, 570f))
    }

    @Test fun theTableZoomFollowsTheCappedTextZoom() {
        assertEquals(0.5f, FitRules.tableZoom(FitRules.Fit(1080, 3f, tablesBack = true))!!, 0.0001f)
    }
}
