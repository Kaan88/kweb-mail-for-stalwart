package ovh.kweb.mail.ui.thread

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
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
}
