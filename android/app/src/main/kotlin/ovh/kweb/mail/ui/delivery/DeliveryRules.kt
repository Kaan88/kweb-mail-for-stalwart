package ovh.kweb.mail.ui.delivery

import ovh.kweb.mail.background.CheckRecord

enum class AgeUnit { Seconds, Minutes, Hours, Days }

/** A whole number of [unit]s, rounded down. */
data class Age(val count: Int, val unit: AgeUnit)

/** "Last push N ago". */
object LastPush {
    /** How long ago the server last sent anything, given in Unix seconds. Never negative. */
    fun age(lastEventAtSeconds: Long, nowMillis: Long): Age {
        val seconds = (nowMillis / 1000 - lastEventAtSeconds).coerceAtLeast(0)
        return when {
            seconds < 60 -> Age(seconds.toInt(), AgeUnit.Seconds)
            seconds < 3600 -> Age((seconds / 60).toInt(), AgeUnit.Minutes)
            seconds < 86_400 -> Age((seconds / 3600).toInt(), AgeUnit.Hours)
            else -> Age((seconds / 86_400).coerceAtMost(Int.MAX_VALUE.toLong()).toInt(), AgeUnit.Days)
        }
    }
}

/** The 15-minute safety check of the Delivery check screen. */
object SafetyCheck {
    /** The job runs every 15 minutes; Android may run it a little late, so twice that is the limit. */
    const val LATE_AFTER_MILLIS = 35 * 60 * 1000L

    /** True when the last check went well and is recent. No check yet is not on time. */
    fun isOnTime(last: CheckRecord?, nowMillis: Long): Boolean =
        last != null && last.error == null && nowMillis - last.atMillis < LATE_AFTER_MILLIS
}

/** What to do when the user taps the notification permission row, or whether to ask on arrival. */
object NotificationFix {
    enum class Step { AskPermission, OpenSettings }

    private const val RUNTIME_PERMISSION_SDK = 33

    /** `Manifest.permission.POST_NOTIFICATIONS`, spelled out because the constant is new in API 33. */
    const val PERMISSION = "android.permission.POST_NOTIFICATIONS"

    /**
     * Before Android 13 there is no permission to ask for, and when it is granted the switch that is
     * off is the app's own in the system settings. After a permanent denial Android shows no dialog
     * (the request returns at once), which is recognised by: asked before, and no rationale to show.
     */
    fun nextStep(sdk: Int, permissionGranted: Boolean, askedBefore: Boolean, canShowRationale: Boolean): Step = when {
        sdk < RUNTIME_PERMISSION_SDK || permissionGranted -> Step.OpenSettings
        askedBefore && !canShowRationale -> Step.OpenSettings
        else -> Step.AskPermission
    }

    /** The one-time request when the Inbox first opens. */
    fun shouldAskOnArrival(sdk: Int, permissionGranted: Boolean, askedBefore: Boolean): Boolean =
        sdk >= RUNTIME_PERMISSION_SDK && !permissionGranted && !askedBefore
}
