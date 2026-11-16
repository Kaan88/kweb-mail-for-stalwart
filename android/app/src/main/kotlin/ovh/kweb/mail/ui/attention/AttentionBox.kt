package ovh.kweb.mail.ui.attention

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Close
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import ovh.kweb.mail.R
import uniffi.kweb_core.Finding

/**
 * The findings of one place, inline: one outlined "Worth a look" box with a title row (sparkle, title, a close x) over the
 * cards. A card that is handled or dismissed drops out of [findings]; the caller closes the box when none is left.
 */
@Composable
fun AttentionBox(
    findings: List<Finding>,
    runner: FindingRunner,
    clock: WordClock,
    onClose: () -> Unit,
    onDismiss: (Finding) -> Unit,
    modifier: Modifier = Modifier,
    onService: (Finding) -> Unit = {},
) {
    val colors = MaterialTheme.colorScheme
    Column(
        modifier.fillMaxWidth().border(BorderStroke(1.dp, colors.outlineVariant), RoundedCornerShape(24.dp)).padding(start = 8.dp, end = 8.dp, bottom = 8.dp),
    ) {
        Row(Modifier.fillMaxWidth().padding(start = 8.dp), verticalAlignment = Alignment.CenterVertically) {
            Icon(painterResource(R.drawable.ic_sparkle), contentDescription = null, tint = colors.primary, modifier = Modifier.size(16.dp))
            Text(
                stringResource(R.string.att_box_title),
                modifier = Modifier.weight(1f).padding(start = 8.dp),
                style = MaterialTheme.typography.titleSmall,
                fontWeight = FontWeight.SemiBold,
            )
            IconButton(onClick = onClose) { Icon(Icons.Filled.Close, contentDescription = stringResource(R.string.att_box_close)) }
        }
        FindingList(findings, runner, clock, onDismiss = onDismiss, onService = onService)
    }
}
