package ovh.kweb.mail.ui.enterprise

import java.time.ZoneId
import java.util.Locale
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.kweb_core.ChartPoint

class ChartRulesTest {
    private val en = Locale.ROOT

    @Test fun theScaleTopIsRoundAndNeverBelowTheValue() {
        assertEquals(1.0, ChartRules.niceMax(0.0), 0.0)
        assertEquals(1.0, ChartRules.niceMax(Double.NaN), 0.0)
        assertEquals(20.0, ChartRules.niceMax(18.0), 0.0)
        assertEquals(50.0, ChartRules.niceMax(37.0), 0.0)
        assertEquals(100.0, ChartRules.niceMax(100.0), 0.0)
        assertEquals(2.5, ChartRules.niceMax(2.4), 0.0)
        assertEquals(10.0, ChartRules.niceMax(6.0), 0.0)
        for (v in listOf(0.3, 1.1, 7.0, 99.0, 1234.0)) assertTrue(ChartRules.niceMax(v) >= v)
    }

    @Test fun ticksRunFromZeroToTheTop() {
        assertEquals(listOf(0.0, 5.0, 10.0, 15.0, 20.0), ChartRules.ticks(20.0))
    }

    @Test fun tickLabelsAreShort() {
        assertEquals("0", ChartRules.tickLabel(0.0, en))
        assertEquals("15", ChartRules.tickLabel(15.0, en))
        assertEquals("0.5", ChartRules.tickLabel(0.5, en))
        assertEquals("1.2k", ChartRules.tickLabel(1200.0, en))
        assertEquals("2k", ChartRules.tickLabel(2000.0, en))
    }

    @Test fun axisLabelsKeepTheEndsAndAreFew() {
        assertEquals(emptyList<Int>(), ChartRules.labelIndices(0))
        assertEquals(listOf(0, 1, 2), ChartRules.labelIndices(3))
        val many = ChartRules.labelIndices(24)
        assertEquals(5, many.size)
        assertEquals(0, many.first())
        assertEquals(23, many.last())
        assertEquals(many.sorted(), many)
    }

    @Test fun aBarIsInProportionAndClamped() {
        assertEquals(50f, ChartRules.barHeight(5.0, 10.0, 100f), 0.001f)
        assertEquals(100f, ChartRules.barHeight(50.0, 10.0, 100f), 0.001f)
        assertEquals(0f, ChartRules.barHeight(-3.0, 10.0, 100f), 0.001f)
        assertEquals(0f, ChartRules.barHeight(3.0, 0.0, 100f), 0.001f)
    }

    @Test fun twoSeriesShareOneAxisAndAMissingHourIsZero() {
        val (times, a, b) = ChartRules.align(
            listOf(ChartPoint(100, 1.0), ChartPoint(200, 2.0), ChartPoint(300, 3.0)),
            listOf(ChartPoint(100, 5.0), ChartPoint(300, 7.0)),
        )
        assertEquals(listOf(100L, 200L, 300L), times)
        assertEquals(listOf(1.0, 2.0, 3.0), a)
        assertEquals(listOf(5.0, 0.0, 7.0), b)
    }

    @Test fun anAverageIgnoresHoursWithNothing() {
        assertNull(ChartRules.average(emptyList()))
        assertNull(ChartRules.average(listOf(ChartPoint(0, 0.0))))
        assertEquals(2.0, ChartRules.average(listOf(ChartPoint(0, 1.0), ChartPoint(1, 0.0), ChartPoint(2, 3.0)))!!, 0.0001)
        assertEquals(6.0, ChartRules.total(listOf(ChartPoint(0, 1.0), ChartPoint(1, 5.0))), 0.0)
    }

    @Test fun timeLabelsFollowTheRange() {
        val zone = ZoneId.of("UTC")
        assertEquals("22:13", ChartRules.timeLabel(1_700_000_000L, ChartRange.DAY, zone, en))
        assertEquals("14 Nov", ChartRules.timeLabel(1_700_000_000L, ChartRange.WEEK, zone, en))
    }

    @Test fun durationsAreWrittenInTheRightUnit() {
        assertEquals("340 ms", ChartRules.duration(0.34, en))
        assertEquals("1.2 s", ChartRules.duration(1.2, en))
        assertEquals("2 min 5 s", ChartRules.duration(125.0, en))
        assertEquals("-", ChartRules.duration(Double.NaN, en))
    }

    @Test fun rangesAskForTheHoursTheCoreTakes() {
        assertEquals(listOf(24u, 168u, 720u), ChartRange.entries.map { it.hours })
    }

    @Test fun sizesAreWrittenInBinaryUnits() {
        assertEquals("0 B", SizeText.bytes(0uL, en))
        assertEquals("512 B", SizeText.bytes(512uL, en))
        assertEquals("1.5 KB", SizeText.bytes(1536uL, en))
        assertEquals("412 MB", SizeText.bytes(412uL * 1024u * 1024u, en))
        assertEquals("2 GB", SizeText.bytes(2uL * 1024u * 1024u * 1024u, en))
    }

    @Test fun aStorageShareNeedsALimit() {
        assertNull(SizeText.fraction(5uL, null))
        assertNull(SizeText.fraction(5uL, 0uL))
        assertEquals(0.25f, SizeText.fraction(250uL, 1000uL)!!, 0.0001f)
        assertEquals(1f, SizeText.fraction(2000uL, 1000uL)!!, 0f)
        assertEquals(0f, SizeText.fraction(null, 1000uL)!!, 0f)
        assertTrue(SizeText.isNearlyFull(0.95f))
        assertFalse(SizeText.isNearlyFull(0.5f))
        assertFalse(SizeText.isNearlyFull(null))
    }
}
