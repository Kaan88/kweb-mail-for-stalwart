package ovh.kweb.mail.ui.settings

import java.text.NumberFormat
import java.util.Locale
import ovh.kweb.mail.ui.thread.SizeFormat
import uniffi.kweb_core.Quota

/** What the storage section says about a quota, in plain Kotlin. */
object QuotaRules {
    /** Above this share of the limit the bar and the text are drawn as a warning. */
    const val WARNING_PERCENT = 90

    /** What a quota counts, for the words around it. */
    enum class Kind { Bytes, Count }

    fun kind(quota: Quota): Kind = if (quota.resource == "octets") Kind.Bytes else Kind.Count

    /** The name the server gave, or null when it gave none (the screen then names the kind). */
    fun name(quota: Quota): String? = quota.name.trim().ifEmpty { null }

    /** How full the bar is, 0 to 1; null when there is no limit. A limit of zero is full as soon as anything is used. */
    fun fraction(used: ULong, limit: ULong?): Float? = when {
        limit == null -> null
        limit == 0uL -> if (used > 0uL) 1f else 0f
        else -> (used.toDouble() / limit.toDouble()).coerceIn(0.0, 1.0).toFloat()
    }

    /** More than [WARNING_PERCENT] percent of the limit is used. */
    fun isWarning(used: ULong, limit: ULong?): Boolean = when {
        limit == null -> false
        limit == 0uL -> used > 0uL
        else -> used.toDouble() * 100 > limit.toDouble() * WARNING_PERCENT
    }

    /** An amount as the quota counts it: bytes as a size ("1.5 GB"), objects as a plain number ("12,034"). */
    fun amount(value: ULong, kind: Kind, locale: Locale): String {
        val clamped = minOf(value, Long.MAX_VALUE.toULong()).toLong()
        return when (kind) {
            Kind.Bytes -> SizeFormat.format(clamped, locale)
            Kind.Count -> NumberFormat.getIntegerInstance(locale).format(clamped)
        }
    }
}
