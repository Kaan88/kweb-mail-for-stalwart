package ovh.kweb.mail.ui.calendar

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalLocale
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp

/** The locale the user has now; the screens redraw when it changes. */
@Composable
internal fun rememberLocale(): java.util.Locale = LocalLocale.current.platformLocale

@Composable
internal fun rememberWords(): Words {
    val resources = LocalResources.current
    return remember(resources) { ResourceWords(resources) }
}

/** The container colour and the colour of what is on it for the calendar tone 0, 1 or 2 (see [AgendaRules.tone]). */
@Composable
internal fun toneColors(tone: Int): Pair<Color, Color> {
    val colors = MaterialTheme.colorScheme
    return when (tone) {
        0 -> colors.primaryContainer to colors.onPrimaryContainer
        1 -> colors.secondaryContainer to colors.onSecondaryContainer
        else -> colors.tertiaryContainer to colors.onTertiaryContainer
    }
}

/**
 * A tonal bar for something the user should know: the tertiary container for a notice, the error
 * container for a failure, with an action at its right end when there is one.
 */
@Composable
internal fun NoticeBar(text: String, error: Boolean, action: Pair<String, () -> Unit>? = null) {
    val colors = MaterialTheme.colorScheme
    val content = if (error) colors.onErrorContainer else colors.onTertiaryContainer
    Row(
        Modifier
            .fillMaxWidth()
            .padding(horizontal = 14.dp, vertical = 4.dp)
            .clip(RoundedCornerShape(14.dp))
            .background(if (error) colors.errorContainer else colors.tertiaryContainer)
            .padding(start = 14.dp, end = if (action != null) 6.dp else 14.dp, top = 8.dp, bottom = 8.dp)
            .semantics { liveRegion = LiveRegionMode.Polite },
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Text(text, modifier = Modifier.weight(1f), color = content, style = MaterialTheme.typography.labelLarge)
        if (action != null) {
            TextButton(onClick = action.second) { Text(action.first, color = content) }
        }
    }
}

/** A column of rows that belong together: a small gap between them (see `ui/list/Groups.kt` for the corners). */
@Composable
internal fun GroupColumn(modifier: Modifier = Modifier, content: @Composable () -> Unit) {
    Column(modifier.padding(horizontal = 12.dp), verticalArrangement = Arrangement.spacedBy(3.dp)) { content() }
}
