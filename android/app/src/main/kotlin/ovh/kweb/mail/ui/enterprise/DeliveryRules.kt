package ovh.kweb.mail.ui.enterprise

import androidx.annotation.StringRes
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.util.Locale
import ovh.kweb.mail.R
import uniffi.kweb_core.Delivery
import uniffi.kweb_core.DeliveryDetail
import uniffi.kweb_core.DeliveryStatus

/** How far back the delivery history looks (the chips under the search field). */
enum class HistoryRange(@StringRes val label: Int) {
    TODAY(R.string.ent_range_today),
    WEEK(R.string.ent_range_7d),
    MONTH(R.string.ent_range_30d),
}

/** What the delivery history and one delivery show, worked out from the core's values. Pure, so it is tested. */
object DeliveryRules {
    /** Unix seconds of the start of the range: the start of today (local time) or that many days back from [nowMillis]. */
    fun since(range: HistoryRange, nowMillis: Long, zone: ZoneId = ZoneId.systemDefault()): Long = when (range) {
        HistoryRange.TODAY -> Instant.ofEpochMilli(nowMillis).atZone(zone).toLocalDate().atStartOfDay(zone).toEpochSecond()
        HistoryRange.WEEK -> nowMillis / 1000 - 7 * 86_400L
        HistoryRange.MONTH -> nowMillis / 1000 - 30 * 86_400L
    }

    /** The word in the pill. Only [DeliveryStatus.FAILED] is drawn in the error colour (see [isBad]). */
    @StringRes fun statusLabel(status: DeliveryStatus): Int = when (status) {
        DeliveryStatus.DELIVERED -> R.string.ent_status_delivered
        DeliveryStatus.RETRYING -> R.string.ent_status_retrying
        DeliveryStatus.FAILED -> R.string.ent_status_failed
        DeliveryStatus.UNKNOWN -> R.string.ent_status_unknown
    }

    fun isBad(status: DeliveryStatus): Boolean = status == DeliveryStatus.FAILED

    /** The time of a row: the hour for today, else the date and hour. */
    fun rowTime(at: Long, nowMillis: Long, zone: ZoneId = ZoneId.systemDefault(), locale: Locale = Locale.getDefault()): String {
        val day = Instant.ofEpochSecond(at).atZone(zone).toLocalDate()
        val today = Instant.ofEpochMilli(nowMillis).atZone(zone).toLocalDate()
        val pattern = if (day == today) "HH:mm" else "d MMM HH:mm"
        return DateTimeFormatter.ofPattern(pattern, locale).withZone(zone).format(Instant.ofEpochSecond(at))
    }

    /** The time of a step on the timeline, with seconds. */
    fun stepTime(at: Long, zone: ZoneId = ZoneId.systemDefault(), locale: Locale = Locale.getDefault()): String =
        DateTimeFormatter.ofPattern("HH:mm:ss", locale).withZone(zone).format(Instant.ofEpochSecond(at))

    /** "to bob@example.org" or "to bob@example.org and 2 more" is built by the screen; this is the first and the count of the rest. */
    fun firstAndRest(to: List<String>): Pair<String, Int> = (to.firstOrNull() ?: "") to (to.size - 1).coerceAtLeast(0)

    /**
     * The step that went wrong: the server stores the steps, not a verdict, so for a failed delivery it is the last
     * one. -1 when nothing failed or there are no steps.
     */
    fun failedStep(detail: DeliveryDetail): Int =
        if (detail.delivery.status == DeliveryStatus.FAILED) detail.steps.lastIndex else -1

    /** The server's own lines for one step ("show the server's own lines"). */
    fun rawLines(detail: DeliveryDetail): List<String> = detail.steps.map { it.raw }.filter { it.isNotBlank() }

    /** Whether more of the list can be asked for: the rows held are fewer than the total. */
    fun hasMore(held: Int, total: UInt): Boolean = held.toLong() < total.toLong()

}
