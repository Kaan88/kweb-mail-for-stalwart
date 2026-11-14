package ovh.kweb.mail.ui.account

import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle
import java.util.Locale
import uniffi.kweb_core.AccountFeatures

/** What Settings shows of the "Security and server" group. */
sealed interface FeaturesRow {
    /** Not read yet, or never readable: the group is not shown. */
    data object Unknown : FeaturesRow

    /** The server offers nothing of it to this account: the group is not shown. */
    data object None : FeaturesRow

    data class Offered(val masked: Boolean, val security: Boolean) : FeaturesRow

    /** The server offers it but the sign-in is older than the feature: one row that signs in again. */
    data object SignInAgain : FeaturesRow
}

/** Pure decisions shared by the masked-address and security screens. */
object AccountRules {
    const val SECONDS_PER_DAY = 86_400L

    fun row(features: AccountFeatures): FeaturesRow = when {
        features.needsSignIn -> FeaturesRow.SignInAgain
        features.maskedEmail || features.security -> FeaturesRow.Offered(features.maskedEmail, features.security)
        else -> FeaturesRow.None
    }

    /** A date as the lists write it ("Nov 14, 2026"); empty when the server gave none. */
    fun date(seconds: Long?, locale: Locale, zone: ZoneId = ZoneId.systemDefault()): String? =
        seconds?.let { DateTimeFormatter.ofLocalizedDate(FormatStyle.MEDIUM).withLocale(locale).format(Instant.ofEpochSecond(it).atZone(zone)) }

    /** A short date for a crowded row: "Oct 2" this year, the full date otherwise. */
    fun shortDate(seconds: Long?, locale: Locale, zone: ZoneId = ZoneId.systemDefault(), now: Instant = Instant.now()): String? {
        seconds ?: return null
        val day = Instant.ofEpochSecond(seconds).atZone(zone)
        if (day.year != now.atZone(zone).year) return date(seconds, locale, zone)
        return DateTimeFormatter.ofPattern("MMM d", locale).format(day)
    }

    /** The end date has passed. */
    fun isEnded(expiresAt: Long?, now: Long): Boolean = expiresAt != null && expiresAt <= now
}
