package ovh.kweb.mail.ui.thread

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import ovh.kweb.mail.R

/**
 * The floating connected button group of the Message figure: Reply (filled, with its icon), Reply
 * all, and Forward as an icon, inside one rounded bar. The outer ends are round, the joins small.
 */
@Composable
fun ReplyBar(enabled: Boolean, onReply: () -> Unit, onReplyAll: () -> Unit, onForward: () -> Unit, modifier: Modifier = Modifier) {
    val colors = MaterialTheme.colorScheme
    Surface(
        modifier = modifier.fillMaxWidth(),
        shape = RoundedCornerShape(32.dp),
        color = colors.surfaceContainerHigh,
        shadowElevation = 6.dp,
    ) {
        Row(Modifier.padding(8.dp), horizontalArrangement = Arrangement.spacedBy(2.dp)) {
            Segment(
                Modifier.weight(1.3f),
                RoundedCornerShape(topStart = 24.dp, bottomStart = 24.dp, topEnd = 8.dp, bottomEnd = 8.dp),
                colors.primary,
                colors.onPrimary,
                enabled,
                onReply,
            ) {
                Icon(painterResource(R.drawable.ic_reply), contentDescription = null, modifier = Modifier.size(20.dp))
                Text(stringResource(R.string.action_reply), style = MaterialTheme.typography.labelLarge)
            }
            Segment(Modifier.weight(1.3f), RoundedCornerShape(8.dp), colors.surfaceContainer, colors.onSurface, enabled, onReplyAll) {
                Text(stringResource(R.string.action_reply_all), style = MaterialTheme.typography.labelLarge)
            }
            Segment(
                Modifier.weight(0.7f),
                RoundedCornerShape(topStart = 8.dp, bottomStart = 8.dp, topEnd = 24.dp, bottomEnd = 24.dp),
                colors.surfaceContainer,
                colors.onSurface,
                enabled,
                onForward,
            ) {
                Icon(painterResource(R.drawable.ic_forward), contentDescription = stringResource(R.string.action_forward), modifier = Modifier.size(20.dp))
            }
        }
    }
}

@Composable
private fun Segment(
    modifier: Modifier,
    shape: androidx.compose.ui.graphics.Shape,
    container: Color,
    content: Color,
    enabled: Boolean,
    onClick: () -> Unit,
    body: @Composable () -> Unit,
) {
    Surface(
        onClick = onClick,
        enabled = enabled,
        modifier = modifier.height(48.dp).alpha(if (enabled) 1f else 0.38f),
        shape = shape,
        color = container,
        contentColor = content,
    ) {
        Row(
            Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(6.dp, Alignment.CenterHorizontally),
            verticalAlignment = Alignment.CenterVertically,
        ) { body() }
    }
}
