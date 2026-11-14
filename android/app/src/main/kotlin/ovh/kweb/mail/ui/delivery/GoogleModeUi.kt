package ovh.kweb.mail.ui.delivery

import androidx.annotation.StringRes
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle
import java.util.Locale
import ovh.kweb.mail.R
import ovh.kweb.mail.background.DeliveryMode
import ovh.kweb.mail.background.FallbackReason
import ovh.kweb.mail.background.ModeRules
import ovh.kweb.mail.background.PushFallback

/** What the screens know about one account's push through Google. */
data class PushView(
    val mode: DeliveryMode = DeliveryMode.Connection,
    val line: ModeRules.Line = ModeRules.Line.Off,
    val fallback: PushFallback? = null,
    val busy: Boolean = false,
)

/** Which text says what (as resource ids, so a test pins the mapping without Compose). */
object PushTexts {
    @StringRes
    fun reason(reason: FallbackReason): Int = when (reason) {
        FallbackReason.NoPlayServices -> R.string.push_reason_no_play_services
        FallbackReason.NoServerKey -> R.string.push_reason_no_server_key
        FallbackReason.RegistrationRefused -> R.string.push_reason_registration_refused
        FallbackReason.NotVerified -> R.string.push_reason_not_verified
        FallbackReason.SubscriptionGone -> R.string.push_reason_subscription_gone
        FallbackReason.Unreachable -> R.string.push_reason_unreachable
    }

    /** The snackbar text after a switch; null for nothing to say. */
    @StringRes
    fun result(result: GoogleModeResult): Int? = when (result) {
        GoogleModeResult.SwitchedOn -> R.string.push_done_on
        GoogleModeResult.SwitchedOff -> R.string.push_done_off
        GoogleModeResult.ConnectionNotUp -> R.string.push_failed_connection
        GoogleModeResult.Busy -> null
        is GoogleModeResult.Failed -> when (result.reason) {
            FallbackReason.NoPlayServices -> R.string.push_failed_no_play_services
            FallbackReason.NoServerKey -> R.string.push_failed_no_server_key
            FallbackReason.RegistrationRefused -> R.string.push_failed_registration_refused
            FallbackReason.NotVerified -> R.string.push_failed_not_verified
            FallbackReason.SubscriptionGone -> R.string.push_failed_subscription_gone
            FallbackReason.Unreachable -> R.string.push_failed_unreachable
        }
    }

    /** Whether the push of a Google-mode account works: only a registered one does. */
    fun working(view: PushView): Boolean = view.mode == DeliveryMode.Google && view.line is ModeRules.Line.Registered
}

private fun dateTime(seconds: Long): String =
    DateTimeFormatter.ofLocalizedDateTime(FormatStyle.MEDIUM, FormatStyle.SHORT).withLocale(Locale.getDefault())
        .format(Instant.ofEpochSecond(seconds).atZone(ZoneId.systemDefault()))

/** The quiet line of the Settings row. */
@Composable
fun pushRowLine(view: PushView): String = when {
    view.busy -> stringResource(R.string.push_row_busy)
    view.mode == DeliveryMode.Google -> stringResource(R.string.push_row_on)
    view.fallback != null -> stringResource(R.string.push_row_fell_back)
    else -> stringResource(R.string.push_row_off)
}

/** "Registered until 21 Nov, 14:00, last push 3 minutes ago" and the other states, for the Delivery check. */
@Composable
fun pushDetail(view: PushView, nowMillis: Long): String = when (val line = view.line) {
    is ModeRules.Line.Registered -> {
        val until = dateTime(line.expiresSeconds)
        val last = line.lastMessageAtSeconds
        if (last == null) {
            stringResource(R.string.push_detail_registered, until)
        } else {
            val age = LastPush.age(last, nowMillis)
            val plural = when (age.unit) {
                AgeUnit.Seconds -> R.plurals.last_push_seconds
                AgeUnit.Minutes -> R.plurals.last_push_minutes
                AgeUnit.Hours -> R.plurals.last_push_hours
                AgeUnit.Days -> R.plurals.last_push_days
            }
            stringResource(R.string.push_detail_registered_last, until, pluralStringResource(plural, age.count, age.count))
        }
    }
    is ModeRules.Line.Verifying -> stringResource(R.string.push_detail_verifying)
    ModeRules.Line.KeyChanged -> stringResource(R.string.push_detail_key_changed)
    ModeRules.Line.Gone -> stringResource(R.string.push_detail_gone)
    ModeRules.Line.Off -> stringResource(R.string.push_detail_off)
}

/** The one line that says why the account is back on the connection, with the error name from Play services if any. */
@Composable
fun pushFallbackLine(fallback: PushFallback): String {
    val reason = stringResource(PushTexts.reason(fallback.reason))
    val text = fallback.detail?.let { stringResource(R.string.push_fallback_detail, reason, it) } ?: reason
    return stringResource(R.string.push_fallback_line, text)
}

/** The notes of the warning, one per line; the Delivery check shows them too while the mode is on. */
@Composable
fun PushNotes(modifier: Modifier = Modifier) {
    Column(modifier, verticalArrangement = Arrangement.spacedBy(8.dp)) {
        listOf(
            R.string.push_note_privacy,
            R.string.push_note_mail,
            R.string.push_note_late,
            R.string.push_note_play,
            R.string.push_note_force_stop,
        ).forEach { Text("• " + stringResource(it), style = MaterialTheme.typography.bodyMedium) }
    }
}

/** The question before switching, shared by Settings and the Delivery check. [turningOn] true carries the warning. */
@Composable
fun GoogleModeDialog(turningOn: Boolean, onConfirm: () -> Unit, onDismiss: () -> Unit) {
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(if (turningOn) R.string.push_on_title else R.string.push_off_title)) },
        text = {
            if (turningOn) {
                Column(Modifier.fillMaxWidth().verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                    Text(stringResource(R.string.push_on_intro), style = MaterialTheme.typography.bodyMedium)
                    PushNotes(Modifier.padding(top = 4.dp))
                }
            } else {
                Text(stringResource(R.string.push_off_text))
            }
        },
        confirmButton = {
            TextButton(onClick = onConfirm) { Text(stringResource(if (turningOn) R.string.push_on_confirm else R.string.push_off_confirm)) }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.cancel)) } },
    )
}

/**
 * The mode group of the Delivery check: why the account went back to the connection (if it did), the notes while the
 * mode is on, and the button that asks to switch. [now] is the screen's clock.
 */
@Composable
fun GoogleModeSection(view: PushView, @Suppress("UNUSED_PARAMETER") now: Long, onAsk: () -> Unit) {
    Column(Modifier.fillMaxWidth().padding(horizontal = 20.dp, vertical = 8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        view.fallback?.takeIf { view.mode != DeliveryMode.Google }?.let {
            Text(pushFallbackLine(it), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.error)
        }
        if (view.mode == DeliveryMode.Google) PushNotes()
        TextButton(onClick = onAsk, enabled = !view.busy) {
            Text(stringResource(if (view.mode == DeliveryMode.Google) R.string.push_mode_on else R.string.push_mode_off))
        }
    }
}
