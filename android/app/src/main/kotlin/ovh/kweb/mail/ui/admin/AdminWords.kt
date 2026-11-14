package ovh.kweb.mail.ui.admin

import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle
import java.util.Locale
import ovh.kweb.mail.R

/** Words for the plain values the rules make. Text comes from resources; the decisions are in [AdminRules]. */
object AdminFormat {
    /** A time of the server: only the clock for today, the date with it for any other day. */
    fun whenText(epochSeconds: Long, nowSeconds: Long, zone: ZoneId, locale: Locale): String {
        val at = Instant.ofEpochSecond(epochSeconds).atZone(zone)
        val today = Instant.ofEpochSecond(nowSeconds).atZone(zone).toLocalDate()
        val style = if (at.toLocalDate() == today) {
            DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT)
        } else {
            DateTimeFormatter.ofLocalizedDateTime(FormatStyle.SHORT)
        }
        return style.withLocale(locale).format(at)
    }
}

@Composable
internal fun spanIn(span: Span): String = when (span.unit) {
    SpanUnit.Minutes -> pluralStringResource(R.plurals.admin_in_minutes, span.value, span.value)
    SpanUnit.Hours -> pluralStringResource(R.plurals.admin_in_hours, span.value, span.value)
    SpanUnit.Days -> pluralStringResource(R.plurals.admin_in_days, span.value, span.value)
}

@Composable
internal fun whenText(epochSeconds: Long, nowSeconds: Long): String {
    val locale = remember { Locale.getDefault() }
    return remember(epochSeconds, nowSeconds / 60) { AdminFormat.whenText(epochSeconds, nowSeconds, ZoneId.systemDefault(), locale) }
}

@Composable
internal fun tryLineText(line: TryLine): String = when (line) {
    TryLine.NotTried -> stringResource(R.string.admin_not_tried)
    is TryLine.Tried -> if (line.next != null) {
        pluralStringResource(R.plurals.admin_tried_next, line.tries, line.tries, spanIn(line.next))
    } else {
        pluralStringResource(R.plurals.admin_tried, line.tries, line.tries)
    }
}

@Composable
internal fun queueLineText(line: QueueLine): String = when (line) {
    QueueLine.Empty -> ""
    is QueueLine.Paused -> pluralStringResource(R.plurals.admin_queue_paused_line, line.count, line.count)
    is QueueLine.Waiting -> if (line.next != null) {
        pluralStringResource(R.plurals.admin_queue_waiting_next, line.count, line.count, spanIn(line.next))
    } else {
        pluralStringResource(R.plurals.admin_queue_waiting, line.count, line.count)
    }
}

@Composable
internal fun areaTitle(area: AdminArea): String = stringResource(
    when (area) {
        AdminArea.Queue -> R.string.admin_area_queue
        AdminArea.Accounts -> R.string.admin_area_accounts
        AdminArea.Blocked -> R.string.admin_area_blocked
        AdminArea.Logs -> R.string.admin_area_logs
        AdminArea.Maintenance -> R.string.admin_area_maintenance
        AdminArea.Dashboard -> R.string.admin_area_dashboard
        AdminArea.History -> R.string.admin_area_history
        AdminArea.Recovery -> R.string.admin_area_recovery
        AdminArea.Alerts -> R.string.admin_area_alerts
        AdminArea.Tenants -> R.string.admin_area_tenants
    },
)

@Composable
internal fun editionName(edition: String): String = when (edition) {
    "enterprise" -> stringResource(R.string.admin_edition_enterprise)
    "community" -> stringResource(R.string.admin_edition_community)
    "oss" -> stringResource(R.string.admin_edition_oss)
    else -> stringResource(R.string.admin_edition_unknown)
}

@Composable
internal fun mayTitle(kind: MayKind): String = stringResource(
    when (kind) {
        MayKind.Queue -> R.string.admin_may_queue
        MayKind.Accounts -> R.string.admin_may_accounts
        MayKind.Blocked -> R.string.admin_may_blocked
        MayKind.Logs -> R.string.admin_may_logs
        MayKind.Maintenance -> R.string.admin_may_maintenance
        MayKind.Enterprise -> R.string.admin_may_enterprise
    },
)

@Composable
internal fun mayDetail(kind: MayKind, allowed: Boolean): String = stringResource(
    if (!allowed) {
        R.string.admin_may_not
    } else {
        when (kind) {
            MayKind.Queue -> R.string.admin_may_queue_detail
            MayKind.Accounts -> R.string.admin_may_accounts_detail
            MayKind.Blocked -> R.string.admin_may_blocked_detail
            MayKind.Logs -> R.string.admin_may_logs_detail
            MayKind.Maintenance -> R.string.admin_may_maintenance_detail
            MayKind.Enterprise -> R.string.admin_may_enterprise_detail
        }
    },
)
