package ovh.kweb.mail.ui

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.size
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp

/**
 * The Ward mark of branding/ward/ward-mark.svg, drawn in the 64-unit box of that file. It is
 * decoration: the app name is always written next to it.
 */
@Composable
fun WardMark(modifier: Modifier = Modifier, size: Dp = 72.dp) {
    val background = MaterialTheme.colorScheme.primary
    val foreground = MaterialTheme.colorScheme.onPrimary
    Canvas(modifier.size(size).clearAndSetSemantics {}) {
        val unit = this.size.width / 64f
        drawRoundRect(
            color = background,
            topLeft = Offset(4 * unit, 4 * unit),
            size = Size(56 * unit, 56 * unit),
            cornerRadius = CornerRadius(16 * unit),
        )
        val strokes = Path().apply {
            moveTo(24 * unit, 17 * unit); lineTo(24 * unit, 47 * unit)
            moveTo(24 * unit, 33 * unit); lineTo(39 * unit, 17 * unit)
            moveTo(29 * unit, 28 * unit); lineTo(41 * unit, 47 * unit)
        }
        drawPath(strokes, foreground, style = Stroke(6 * unit, cap = StrokeCap.Round, join = StrokeJoin.Round))
    }
}
