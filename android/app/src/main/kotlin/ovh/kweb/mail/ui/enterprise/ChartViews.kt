package ovh.kweb.mail.ui.enterprise

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextMeasurer
import androidx.compose.ui.text.drawText
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.unit.dp

/**
 * A chart drawn on a Canvas in the theme's colours: gridlines with a scale at the left (so a value can be read), a
 * label under some of the points, [bars] as bars in the primary colour, [line] as a line (filled underneath when
 * [fill]). [labels] has one entry per point. [valueText] writes a scale value ("2 s"), [description] is what a screen
 * reader says.
 */
@Composable
internal fun HistoryChart(
    labels: List<String>,
    bars: List<Double>,
    line: List<Double>,
    fill: Boolean,
    valueText: (Double) -> String,
    description: String,
    modifier: Modifier = Modifier,
) {
    val colors = MaterialTheme.colorScheme
    val grid = colors.outlineVariant
    val barColor = colors.primary
    val lineColor = if (fill) colors.primary else colors.onSurface
    val fillColor = colors.primary.copy(alpha = 0.18f)
    val textColor = colors.onSurfaceVariant
    val style = MaterialTheme.typography.labelSmall.copy(color = textColor)
    val measurer = rememberTextMeasurer()
    val top = remember(bars, line) { ChartRules.niceMax((bars + line).maxOrNull() ?: 0.0) }
    val ticks = remember(top) { ChartRules.ticks(top) }
    val tickTexts = remember(ticks) { ticks.map(valueText) }
    val shown = remember(labels) { ChartRules.labelIndices(labels.size).toSet() }
    val n = labels.size

    Canvas(modifier.fillMaxWidth().height(180.dp).semantics { contentDescription = description }) {
        val bottomGutter = 22.dp.toPx()
        val topPad = 6.dp.toPx()
        val gutter = (tickTexts.maxOfOrNull { measurer.measure(it, style).size.width } ?: 0) + 8.dp.toPx()
        val plotLeft = gutter
        val plotRight = size.width
        val plotTop = topPad
        val plotBottom = size.height - bottomGutter
        val plotH = plotBottom - plotTop
        val plotW = plotRight - plotLeft
        if (plotW <= 0f || plotH <= 0f) return@Canvas

        // The scale: a line and a value at each tick.
        ticks.forEachIndexed { i, tick ->
            val y = plotBottom - ChartRules.barHeight(tick, top, plotH)
            drawLine(grid, Offset(plotLeft, y), Offset(plotRight, y), strokeWidth = 1.dp.toPx())
            val laid = measurer.measure(tickTexts[i], style)
            drawText(laid, topLeft = Offset(plotLeft - 6.dp.toPx() - laid.size.width, y - laid.size.height / 2f))
        }
        if (n == 0) return@Canvas
        val slot = plotW / n

        bars.forEachIndexed { i, v ->
            val h = ChartRules.barHeight(v, top, plotH)
            if (h > 0f) {
                val w = slot * 0.62f
                drawRect(barColor, Offset(plotLeft + slot * i + (slot - w) / 2f, plotBottom - h), Size(w, h))
            }
        }
        if (line.isNotEmpty()) {
            val path = Path()
            line.forEachIndexed { i, v ->
                val x = plotLeft + slot * (i + 0.5f)
                val y = plotBottom - ChartRules.barHeight(v, top, plotH)
                if (i == 0) path.moveTo(x, y) else path.lineTo(x, y)
            }
            if (fill) {
                val area = Path().apply {
                    addPath(path)
                    lineTo(plotLeft + slot * (line.size - 0.5f), plotBottom)
                    lineTo(plotLeft + slot * 0.5f, plotBottom)
                    close()
                }
                drawPath(area, fillColor)
            }
            drawPath(path, lineColor, style = Stroke(width = 2.dp.toPx(), cap = StrokeCap.Round))
        }
        labels.forEachIndexed { i, text ->
            if (i in shown) drawLabel(measurer, text, style, plotLeft + slot * (i + 0.5f), plotBottom + 4.dp.toPx(), plotLeft, plotRight)
        }
    }
}

private fun androidx.compose.ui.graphics.drawscope.DrawScope.drawLabel(
    measurer: TextMeasurer,
    text: String,
    style: androidx.compose.ui.text.TextStyle,
    centerX: Float,
    y: Float,
    minX: Float,
    maxX: Float,
) {
    val laid = measurer.measure(text, style)
    val x = (centerX - laid.size.width / 2f).coerceIn(minX, (maxX - laid.size.width).coerceAtLeast(minX))
    drawText(laid, topLeft = Offset(x, y))
}
