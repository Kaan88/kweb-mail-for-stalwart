package ovh.kweb.mail.ui.enterprise

import java.time.ZoneId
import java.util.Locale
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import ovh.kweb.mail.R
import uniffi.kweb_core.DeliveryDetail
import uniffi.kweb_core.DeliveryStatus
import uniffi.kweb_core.DeliveryStep

class DeliveryRulesTest {
    private val utc = ZoneId.of("UTC")

    @Test fun todayStartsAtMidnightLocalTime() {
        // 2023-11-14 22:13:20 UTC
        val now = 1_700_000_000_000L
        assertEquals(1_699_920_000L, DeliveryRules.since(HistoryRange.TODAY, now, utc))
        assertEquals(1_700_000_000L - 7 * 86_400, DeliveryRules.since(HistoryRange.WEEK, now, utc))
        assertEquals(1_700_000_000L - 30 * 86_400, DeliveryRules.since(HistoryRange.MONTH, now, utc))
    }

    @Test fun onlyAFailureIsBad() {
        assertTrue(DeliveryRules.isBad(DeliveryStatus.FAILED))
        assertFalse(DeliveryRules.isBad(DeliveryStatus.RETRYING))
        assertFalse(DeliveryRules.isBad(DeliveryStatus.DELIVERED))
        assertFalse(DeliveryRules.isBad(DeliveryStatus.UNKNOWN))
        assertEquals(R.string.ent_status_retrying, DeliveryRules.statusLabel(DeliveryStatus.RETRYING))
    }

    @Test fun aRowShowsTheHourTodayAndTheDateBefore() {
        val now = 1_700_000_000_000L
        assertEquals("22:13", DeliveryRules.rowTime(1_700_000_000L, now, utc, Locale.ROOT))
        assertEquals("12 Nov 10:00", DeliveryRules.rowTime(1_699_783_200L, now, utc, Locale.ROOT))
        assertEquals("22:13:20", DeliveryRules.stepTime(1_700_000_000L, utc, Locale.ROOT))
    }

    @Test fun theFirstRecipientAndTheRest() {
        assertEquals("a" to 0, DeliveryRules.firstAndRest(listOf("a")))
        assertEquals("a" to 2, DeliveryRules.firstAndRest(listOf("a", "b", "c")))
        assertEquals("" to 0, DeliveryRules.firstAndRest(emptyList()))
    }

    @Test fun aFailedDeliveryMarksItsLastStep() {
        val steps = listOf(DeliveryStep(1, "a", "", "r1"), DeliveryStep(2, "b", "", ""), DeliveryStep(3, "c", "", "r3"))
        assertEquals(2, DeliveryRules.failedStep(DeliveryDetail(sampleDelivery("x", DeliveryStatus.FAILED), steps)))
        assertEquals(-1, DeliveryRules.failedStep(DeliveryDetail(sampleDelivery("x"), steps)))
        assertEquals(-1, DeliveryRules.failedStep(DeliveryDetail(sampleDelivery("x", DeliveryStatus.FAILED), emptyList())))
        assertEquals(listOf("r1", "r3"), DeliveryRules.rawLines(DeliveryDetail(sampleDelivery("x"), steps)))
    }

    @Test fun moreCanBeAskedForWhileRowsAreMissing() {
        assertTrue(DeliveryRules.hasMore(50, 51u))
        assertFalse(DeliveryRules.hasMore(50, 50u))
        assertFalse(DeliveryRules.hasMore(0, 0u))
    }
}
