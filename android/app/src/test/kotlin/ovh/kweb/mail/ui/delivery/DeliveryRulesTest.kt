package ovh.kweb.mail.ui.delivery

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import ovh.kweb.mail.background.CheckRecord
import ovh.kweb.mail.background.DeliveryMode
import ovh.kweb.mail.ui.delivery.NotificationFix.Step

class DeliveryRulesTest {
    private val nowMillis = 1_800_000_000_000L
    private val nowSeconds = nowMillis / 1000

    @Test fun secondsAgo() = assertEquals(Age(12, AgeUnit.Seconds), LastPush.age(nowSeconds - 12, nowMillis))

    @Test fun justNow() = assertEquals(Age(0, AgeUnit.Seconds), LastPush.age(nowSeconds, nowMillis))

    @Test fun aMinuteIsAMinute() = assertEquals(Age(1, AgeUnit.Minutes), LastPush.age(nowSeconds - 60, nowMillis))

    @Test fun minutesAreRoundedDown() = assertEquals(Age(59, AgeUnit.Minutes), LastPush.age(nowSeconds - 3599, nowMillis))

    @Test fun hoursAndDays() {
        assertEquals(Age(5, AgeUnit.Hours), LastPush.age(nowSeconds - 5 * 3600 - 10, nowMillis))
        assertEquals(Age(2, AgeUnit.Days), LastPush.age(nowSeconds - 2 * 86_400, nowMillis))
    }

    @Test fun anEventFromTheFutureIsNotNegative() =
        assertEquals(Age(0, AgeUnit.Seconds), LastPush.age(nowSeconds + 30, nowMillis))

    @Test fun withoutTheExemptionTheConnectionIsAtRiskButPushThroughGoogleIsNot() {
        fun ok(exempt: Boolean, mode: DeliveryMode, working: Boolean = true, restricted: Boolean = false) =
            BatteryRule.isOk(exempt, mode, pushWorking = working, backgroundRestricted = restricted)
        assertFalse(ok(exempt = false, mode = DeliveryMode.Connection))
        assertTrue(ok(exempt = false, mode = DeliveryMode.Google))
        assertTrue(ok(exempt = true, mode = DeliveryMode.Connection))
        assertTrue(ok(exempt = true, mode = DeliveryMode.Google))
    }

    @Test fun pushThroughGoogleNeedsNoExemptionOnlyWhileItCanArrive() {
        fun ok(working: Boolean, restricted: Boolean) =
            BatteryRule.isOk(exempt = false, mode = DeliveryMode.Google, pushWorking = working, backgroundRestricted = restricted)
        // An app Android keeps from the background gets no push from Google: a fault, with the way to fix it.
        assertFalse(ok(working = true, restricted = true))
        // Key changed, subscription gone, still verifying: nothing wakes the app, so the row must not say it does.
        assertFalse(ok(working = false, restricted = false))
        assertTrue(ok(working = true, restricted = false))
        // The exemption settles it in every case.
        assertTrue(BatteryRule.isOk(exempt = true, mode = DeliveryMode.Google, pushWorking = false, backgroundRestricted = false))
    }

    @Test fun aRecentSuccessIsOnTime() =
        assertTrue(SafetyCheck.isOnTime(CheckRecord(nowMillis - 20 * 60_000, null), nowMillis))

    @Test fun anOldSuccessIsLate() =
        assertFalse(SafetyCheck.isOnTime(CheckRecord(nowMillis - 36 * 60_000, null), nowMillis))

    @Test fun theLimitItselfIsLate() =
        assertFalse(SafetyCheck.isOnTime(CheckRecord(nowMillis - SafetyCheck.LATE_AFTER_MILLIS, null), nowMillis))

    @Test fun aRecentFailureIsLate() =
        assertFalse(SafetyCheck.isOnTime(CheckRecord(nowMillis - 60_000, "timed out"), nowMillis))

    @Test fun noCheckYetIsLate() = assertFalse(SafetyCheck.isOnTime(null, nowMillis))

    @Test fun beforeAndroid13OnlyTheSettingsHelp() =
        assertEquals(Step.OpenSettings, NotificationFix.nextStep(32, false, false, false))

    @Test fun grantedButOffGoesToSettings() =
        assertEquals(Step.OpenSettings, NotificationFix.nextStep(34, true, true, false))

    @Test fun neverAskedAsksNow() = assertEquals(Step.AskPermission, NotificationFix.nextStep(34, false, false, false))

    @Test fun deniedOnceCanAskAgain() = assertEquals(Step.AskPermission, NotificationFix.nextStep(34, false, true, true))

    @Test fun deniedForGoodGoesToSettings() =
        assertEquals(Step.OpenSettings, NotificationFix.nextStep(34, false, true, false))

    @Test fun arrivalAsksOnlyOnceOnAndroid13Plus() {
        assertTrue(NotificationFix.shouldAskOnArrival(33, false, false))
        assertFalse(NotificationFix.shouldAskOnArrival(33, false, true))
        assertFalse(NotificationFix.shouldAskOnArrival(33, true, false))
        assertFalse(NotificationFix.shouldAskOnArrival(32, false, false))
    }

    @Test fun theOtherAccountsOnTheConnectionAreOffered() {
        val accounts = listOf(
            OtherAccounts.Other("a", "mail@example.org", DeliveryMode.Google),
            OtherAccounts.Other("b", "admin@example.org", DeliveryMode.Connection),
            OtherAccounts.Other("c", "work@example.org", DeliveryMode.Google),
        )
        // The account just switched on is never offered, nor one already on push through Google.
        assertEquals(listOf("b"), OtherAccounts.toOffer("a", accounts).map { it.id })
        assertEquals(emptyList<String>(), OtherAccounts.toOffer("b", accounts).map { it.id })
    }
}
