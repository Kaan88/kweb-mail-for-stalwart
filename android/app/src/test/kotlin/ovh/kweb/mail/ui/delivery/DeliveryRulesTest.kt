package ovh.kweb.mail.ui.delivery

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import ovh.kweb.mail.background.CheckRecord
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
}
