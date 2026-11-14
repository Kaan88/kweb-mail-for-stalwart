package ovh.kweb.mail.ui.settings

import java.util.Locale
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.kweb_core.Quota

class QuotaRulesTest {
    private fun quota(resource: String, used: ULong, limit: ULong?, name: String = "Storage") = Quota(name, resource, used, limit)

    @Test fun halfFullIsHalf() = assertEquals(0.5f, QuotaRules.fraction(50u, 100u)!!, 0.0001f)

    @Test fun noLimitHasNoBar() = assertNull(QuotaRules.fraction(50u, null))

    @Test fun overTheLimitIsAFullBar() = assertEquals(1f, QuotaRules.fraction(150u, 100u)!!, 0.0001f)

    @Test fun aLimitOfZeroIsFullOnlyOnceSomethingIsUsed() {
        assertEquals(1f, QuotaRules.fraction(1u, 0u)!!, 0.0001f)
        assertEquals(0f, QuotaRules.fraction(0u, 0u)!!, 0.0001f)
    }

    @Test fun theWarningStartsAboveNinetyPercent() {
        assertFalse(QuotaRules.isWarning(90u, 100u))
        assertTrue(QuotaRules.isWarning(91u, 100u))
        assertTrue(QuotaRules.isWarning(100u, 100u))
        assertTrue(QuotaRules.isWarning(120u, 100u))
        assertFalse(QuotaRules.isWarning(10u, 100u))
    }

    @Test fun noLimitIsNeverAWarning() = assertFalse(QuotaRules.isWarning(ULong.MAX_VALUE, null))

    @Test fun hugeNumbersDoNotOverflow() {
        val max = ULong.MAX_VALUE
        assertTrue(QuotaRules.isWarning(max, max))
        assertEquals(1f, QuotaRules.fraction(max, max)!!, 0.0001f)
        assertFalse(QuotaRules.isWarning(max / 2u, max))
        assertEquals(0.5f, QuotaRules.fraction(max / 2u, max)!!, 0.0001f)
        assertTrue(QuotaRules.isWarning(max / 100u * 95u, max))
    }

    @Test fun bytesAreShownAsASize() {
        assertEquals("1.5 MB", QuotaRules.amount(1_500_000u, QuotaRules.Kind.Bytes, Locale.US))
        assertEquals("812 B", QuotaRules.amount(812u, QuotaRules.Kind.Bytes, Locale.US))
    }

    @Test fun objectsAreShownAsANumber() = assertEquals("12,034", QuotaRules.amount(12_034u, QuotaRules.Kind.Count, Locale.US))

    @Test fun anAmountBeyondALongIsClamped() =
        assertEquals(
            QuotaRules.amount(Long.MAX_VALUE.toULong(), QuotaRules.Kind.Bytes, Locale.US),
            QuotaRules.amount(ULong.MAX_VALUE, QuotaRules.Kind.Bytes, Locale.US),
        )

    @Test fun octetsAreBytesAndAnythingElseIsACount() {
        assertEquals(QuotaRules.Kind.Bytes, QuotaRules.kind(quota("octets", 1u, 2u)))
        assertEquals(QuotaRules.Kind.Count, QuotaRules.kind(quota("count", 1u, 2u)))
        assertEquals(QuotaRules.Kind.Count, QuotaRules.kind(quota("other", 1u, 2u)))
    }

    @Test fun aBlankNameFallsBackToNull() {
        assertNull(QuotaRules.name(quota("octets", 1u, 2u, name = "  ")))
        assertEquals("Mail", QuotaRules.name(quota("octets", 1u, 2u, name = " Mail ")))
    }
}
