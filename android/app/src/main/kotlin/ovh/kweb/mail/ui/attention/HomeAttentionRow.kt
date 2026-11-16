package ovh.kweb.mail.ui.attention

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.KeyboardArrowRight
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.accounts.LocalAccountSession
import ovh.kweb.mail.ui.list.Groups
import uniffi.kweb_core.AttentionSettings

/** The families that are switched on, "Calendar, Tasks, Mail", as the settings say them. */
internal fun familiesOn(s: AttentionSettings, names: Triple<String, String, String>): String =
    listOfNotNull(names.first.takeIf { s.calendar }, names.second.takeIf { s.tasks }, names.third.takeIf { s.mail }).joinToString(", ")

/**
 * One row in the AI assistant's settings that leads to Home and attention (a small "New" label), saying which families
 * are on and whether each run is asked for first.
 */
@Composable
fun HomeAttentionRow(onClick: () -> Unit, modifier: Modifier = Modifier) {
    val session = LocalAccountSession.current
    var settings by remember { mutableStateOf<AttentionSettings?>(null) }
    LaunchedEffect(session) { settings = withContext(Dispatchers.IO) { runCatching { session.core.attentionSettings() }.getOrNull() } }
    val colors = MaterialTheme.colorScheme
    val s = settings
    val on = s?.let { familiesOn(it, Triple(stringResource(R.string.att_settings_cal), stringResource(R.string.att_settings_tasks), stringResource(R.string.att_settings_mail))) }.orEmpty()
    val detail = when {
        s == null -> null
        on.isEmpty() -> stringResource(R.string.att_settings_ai_detail_off)
        s.askEachRun -> stringResource(R.string.att_settings_ai_detail, on)
        else -> stringResource(R.string.att_settings_ai_detail_noask, on)
    }
    Column(modifier.padding(horizontal = 12.dp)) {
        Row(
            Modifier.fillMaxWidth().clip(Groups.shape(0, 1)).background(colors.surfaceContainerLow)
                .clickable(role = Role.Button, onClick = onClick).heightIn(min = 56.dp).padding(horizontal = 20.dp, vertical = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Column(Modifier.weight(1f)) {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Text(stringResource(R.string.att_settings_title), style = MaterialTheme.typography.bodyLarge)
                    Text(
                        stringResource(R.string.att_settings_new),
                        modifier = Modifier.clip(RoundedCornerShape(8.dp)).background(colors.tertiaryContainer).padding(horizontal = 7.dp, vertical = 1.dp),
                        style = MaterialTheme.typography.labelSmall,
                        color = colors.onTertiaryContainer,
                    )
                }
                if (detail != null) Text(detail, style = MaterialTheme.typography.bodyMedium, color = colors.onSurfaceVariant)
            }
            Icon(Icons.AutoMirrored.Filled.KeyboardArrowRight, contentDescription = null, tint = colors.onSurfaceVariant)
        }
    }
}
