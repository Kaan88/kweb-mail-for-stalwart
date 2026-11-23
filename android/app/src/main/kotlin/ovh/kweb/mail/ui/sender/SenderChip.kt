package ovh.kweb.mail.ui.sender

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Info
import androidx.compose.material.icons.filled.Warning
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import ovh.kweb.mail.R
import uniffi.kweb_core.MethodResult
import uniffi.kweb_core.SenderCheck

/** The small mark under the sender line: verified, not verified or failed. A tap opens the explanation. [compact]: the height of a line of small text, for the thread header's row of marks. */
@Composable
fun SenderChip(check: SenderCheck, onClick: () -> Unit, modifier: Modifier = Modifier, compact: Boolean = false) {
    val colors = MaterialTheme.colorScheme
    val tone = SenderRules.tone(check)
    val (container, content) = when (tone) {
        SenderRules.Tone.GOOD -> colors.secondaryContainer to colors.onSecondaryContainer
        SenderRules.Tone.BAD -> colors.errorContainer to colors.onErrorContainer
        SenderRules.Tone.NEUTRAL -> colors.surfaceContainerHighest to colors.onSurfaceVariant
    }
    val icon = when (tone) {
        SenderRules.Tone.GOOD -> Icons.Filled.Check
        SenderRules.Tone.BAD -> Icons.Filled.Warning
        SenderRules.Tone.NEUTRAL -> Icons.Filled.Info
    }
    Row(
        modifier
            .clip(RoundedCornerShape(50))
            .background(container)
            .clickable(role = Role.Button, onClick = onClick)
            .padding(horizontal = if (compact) 8.dp else 12.dp, vertical = if (compact) 2.dp else 6.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(if (compact) 4.dp else 6.dp),
    ) {
        Icon(icon, contentDescription = null, modifier = Modifier.size(if (compact) 14.dp else 16.dp), tint = content)
        Text(
            stringResource(SenderRules.chipText(check)),
            color = content,
            style = if (compact) MaterialTheme.typography.labelMedium else MaterialTheme.typography.labelLarge,
            fontWeight = FontWeight.Medium,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
    }
}

/** The line under the chip of a failed check: what to do about it, in one sentence. */
@Composable
fun SenderWarning(check: SenderCheck, modifier: Modifier = Modifier) {
    Text(
        stringResource(R.string.sender_warning, check.fromDomain),
        color = MaterialTheme.colorScheme.error,
        style = MaterialTheme.typography.bodyMedium,
        modifier = modifier,
    )
}

/** The explanation in plain words: what the server found, what each check said, and what it does not prove. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SenderSheet(check: SenderCheck, onDismiss: () -> Unit) {
    ModalBottomSheet(onDismissRequest = onDismiss) {
        Column(
            Modifier.padding(horizontal = 24.dp).padding(bottom = 24.dp).navigationBarsPadding(),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Text(stringResource(R.string.sender_sheet_title), style = MaterialTheme.typography.titleLarge)
            Text(stringResource(SenderRules.why(check), check.fromDomain, check.signedBy), style = MaterialTheme.typography.bodyLarge)
            if (check.dmarc != MethodResult.NOT_RUN || check.dkim != MethodResult.NOT_RUN || check.spf != MethodResult.NOT_RUN) {
                MethodLine(R.string.sender_method_dmarc, check.dmarc)
                MethodLine(R.string.sender_method_dkim, check.dkim)
                MethodLine(R.string.sender_method_spf, check.spf)
            }
            Text(
                stringResource(R.string.sender_sheet_limits),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

@Composable
private fun MethodLine(label: Int, result: MethodResult) {
    Row(horizontalArrangement = Arrangement.SpaceBetween, modifier = Modifier.padding(end = 4.dp)) {
        Text(stringResource(label), style = MaterialTheme.typography.bodyMedium, modifier = Modifier.padding(end = 16.dp))
        Text(
            stringResource(SenderRules.method(result)),
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}
