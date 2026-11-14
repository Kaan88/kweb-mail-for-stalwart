package ovh.kweb.mail.ui.enterprise

import androidx.annotation.StringRes
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.util.Locale
import kotlin.math.abs
import kotlin.math.floor
import kotlin.math.log10
import kotlin.math.pow
import ovh.kweb.mail.R
import uniffi.kweb_core.ChartPoint

/** How far back the dashboard looks. [hours] is what the core takes. */
enum class ChartRange(val hours: UInt, @StringRes val label: Int) {
    DAY(24u, R.string.ent_range_24h),
    WEEK(168u, R.string.ent_range_7d),
    MONTH(720u, R.string.ent_range_30d),
}

/** The numbers behind the two charts: a scale a value can be read from, and where things go. Pure, so it is tested. */
object ChartRules {
    /** A rounded top for the scale, at least [value] (and 1 when there is nothing, so an empty chart has a scale too). */
    fun niceMax(value: Double): Double {
        if (!value.isFinite() || value <= 0.0) return 1.0
        val exp = floor(log10(value))
        val base = 10.0.pow(exp)
        val fraction = value / base
        val step = when {
            fraction <= 1.0 -> 1.0
            fraction <= 2.0 -> 2.0
            fraction <= 2.5 -> 2.5
            fraction <= 5.0 -> 5.0
            else -> 10.0
        }
        return step * base
    }

    /** Evenly spaced tick values from 0 up to [top] (included): [segments] + 1 of them. */
    fun ticks(top: Double, segments: Int = 4): List<Double> = (0..segments).map { top * it / segments }

    /** The tick label: whole numbers without a decimal point, small values with one; thousands as "1.2k". */
    fun tickLabel(value: Double, locale: Locale = Locale.getDefault()): String = when {
        abs(value) >= 1000 -> String.format(locale, "%.1fk", value / 1000).replace(Regex("[.,]0k$"), "k")
        value == floor(value) -> String.format(locale, "%d", value.toLong())
        else -> String.format(locale, "%.1f", value)
    }

    /** Which of [count] points get an axis label: at most [max], first and last included, evenly spread. */
    fun labelIndices(count: Int, max: Int = 5): List<Int> {
        if (count <= 0) return emptyList()
        if (count <= max) return (0 until count).toList()
        return (0 until max).map { Math.round(it * (count - 1).toFloat() / (max - 1)) }.distinct()
    }

    /** The height of [value] on a scale from 0 to [top] drawn [height] high, 0 at the bottom: the distance from the bottom. */
    fun barHeight(value: Double, top: Double, height: Float): Float =
        if (top <= 0.0 || !value.isFinite()) 0f else (value.coerceIn(0.0, top) / top * height).toFloat()

    /**
     * Puts two series on one time axis: the times of [first] (in order), and for each the value of [second] at that
     * time (0 when it has none). The server gives both with the same hours, but one that skips an empty hour must not shift the line.
     */
    fun align(first: List<ChartPoint>, second: List<ChartPoint>): Triple<List<Long>, List<Double>, List<Double>> {
        val times = (first.map { it.at } + second.map { it.at }).distinct().sorted()
        val a = first.associate { it.at to it.value }
        val b = second.associate { it.at to it.value }
        return Triple(times, times.map { a[it] ?: 0.0 }, times.map { b[it] ?: 0.0 })
    }

    fun total(points: List<ChartPoint>): Double = points.sumOf { it.value }

    /** The average of the non-empty hours: a quiet hour with nothing delivered says nothing about the time. */
    fun average(points: List<ChartPoint>): Double? {
        val real = points.filter { it.value > 0.0 }
        return if (real.isEmpty()) null else real.sumOf { it.value } / real.size
    }

    /** The label under a point: the hour for a day, the date for longer. */
    fun timeLabel(at: Long, range: ChartRange, zone: ZoneId = ZoneId.systemDefault(), locale: Locale = Locale.getDefault()): String {
        val pattern = if (range == ChartRange.DAY) "HH:mm" else "d MMM"
        return DateTimeFormatter.ofPattern(pattern, locale).withZone(zone).format(Instant.ofEpochSecond(at))
    }

    /** "1.2 s", "340 ms", "2 min 5 s". */
    fun duration(seconds: Double, locale: Locale = Locale.getDefault()): String = when {
        !seconds.isFinite() || seconds < 0 -> "-"
        seconds < 1.0 -> String.format(locale, "%d ms", Math.round(seconds * 1000))
        seconds < 60.0 -> String.format(locale, "%.1f s", seconds)
        else -> {
            val whole = Math.round(seconds)
            String.format(locale, "%d min %d s", whole / 60, whole % 60)
        }
    }
}

/** Sizes written the way the dashboard shows them. */
object SizeText {
    private val units = listOf("B", "KB", "MB", "GB", "TB")

    fun bytes(value: ULong, locale: Locale = Locale.getDefault()): String {
        var v = value.toDouble()
        var i = 0
        while (v >= 1024.0 && i < units.size - 1) {
            v /= 1024.0
            i++
        }
        return when {
            i == 0 -> String.format(locale, "%d B", value.toLong())
            v >= 100.0 || v == floor(v) -> String.format(locale, "%d %s", Math.round(v), units[i])
            else -> String.format(locale, "%.1f %s", v, units[i])
        }
    }

    /** A share of [quota] that [used] is, 0 to 1; null without a limit (or a limit of nothing). */
    fun fraction(used: ULong?, quota: ULong?): Float? {
        if (quota == null || quota == 0UL) return null
        return ((used ?: 0UL).toDouble() / quota.toDouble()).coerceIn(0.0, 1.0).toFloat()
    }

    /** Nine tenths of the limit used is a warning, in the error colour. */
    fun isNearlyFull(fraction: Float?): Boolean = fraction != null && fraction >= 0.9f
}
