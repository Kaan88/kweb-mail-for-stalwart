package ovh.kweb.mail.ui.enterprise

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.calendar.GroupColumn
import ovh.kweb.mail.ui.calendar.NoticeBar
import ovh.kweb.mail.ui.list.Groups
import uniffi.kweb_core.DeliveryDetail

/** One message followed through the server: who, to whom and the outcome, then the steps as a timeline. */
@Composable
fun DeliveryDetailScreen(deliveryId: String, onBack: () -> Unit, vm: DeliveryDetailViewModel = viewModel()) {
    val c = vm.controller
    val state by c.state.collectAsStateWithLifecycle()
    LaunchedEffect(vm, deliveryId) { c.start(deliveryId) }

    EnterpriseScaffold(stringResource(R.string.ent_delivery_title), onBack) { padding ->
        val detail = state.detail
        Column(Modifier.padding(padding).fillMaxSize()) {
            val error = state.error
            if (error != null && detail != null) {
                NoticeBar(errorText(error), error = true, action = stringResource(R.string.ent_dismiss) to c::dismissError)
            }
            when {
                detail != null -> Detail(detail, state.showRaw, c::toggleRaw)
                state.phase == Phase.FAILED -> FailedBox(error, R.string.ent_delivery_failed, c::load)
                else -> LoadingBox()
            }
        }
    }
}

@Composable
private fun Detail(detail: DeliveryDetail, showRaw: Boolean, onToggleRaw: () -> Unit) {
    val d = detail.delivery
    val failed = DeliveryRules.failedStep(detail)
    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(bottom = 32.dp)) {
        GroupColumn {
            GroupRow(Groups.shape(0, 1)) {
                Column(Modifier.weight(1f)) {
                    Text(d.from, style = MaterialTheme.typography.bodyLarge)
                    Text(toLine(d.to), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
                StatusPill(stringResource(DeliveryRules.statusLabel(d.status)), bad = DeliveryRules.isBad(d.status))
            }
        }
        if (detail.steps.isEmpty()) QuietNote(stringResource(R.string.ent_delivery_no_steps))
        Column(Modifier.padding(start = 20.dp, end = 20.dp, top = 16.dp)) {
            detail.steps.forEachIndexed { index, step ->
                TimelineStep(
                    time = DeliveryRules.stepTime(step.at),
                    title = step.title,
                    detail = step.detail,
                    bad = index == failed,
                    first = index == 0,
                    last = index == detail.steps.lastIndex,
                )
            }
        }
        if (detail.steps.isNotEmpty()) {
            TextButton(onClick = onToggleRaw, modifier = Modifier.padding(start = 28.dp)) {
                Text(stringResource(if (showRaw) R.string.ent_hide_raw else R.string.ent_show_raw))
            }
        }
        if (showRaw) {
            SelectionContainer {
                Column(
                    Modifier.fillMaxWidth().padding(horizontal = 12.dp)
                        .background(MaterialTheme.colorScheme.surfaceContainerHighest, RoundedCornerShape(16.dp))
                        .padding(14.dp),
                    verticalArrangement = Arrangement.spacedBy(6.dp),
                ) {
                    DeliveryRules.rawLines(detail).forEach {
                        Text(it, fontFamily = FontFamily.Monospace, style = MaterialTheme.typography.bodySmall)
                    }
                }
            }
        }
    }
}

/** One step: its time, a dot on the line and what happened; a step that went wrong is in the error colour. */
@Composable
private fun TimelineStep(time: String, title: String, detail: String, bad: Boolean, first: Boolean, last: Boolean) {
    val colors = MaterialTheme.colorScheme
    val dot = if (bad) colors.error else colors.primary
    val line = colors.outlineVariant
    Row(Modifier.fillMaxWidth().height(IntrinsicSize.Min), horizontalArrangement = Arrangement.spacedBy(10.dp)) {
        Text(time, modifier = Modifier.width(62.dp).padding(top = 2.dp), style = MaterialTheme.typography.labelSmall, color = colors.onSurfaceVariant, fontFamily = FontFamily.Monospace)
        Canvas(Modifier.width(14.dp).fillMaxSize()) {
            val x = size.width / 2f
            val y = 11.dp.toPx()
            if (!first) drawLine(line, Offset(x, 0f), Offset(x, y), strokeWidth = 2.dp.toPx())
            if (!last) drawLine(line, Offset(x, y), Offset(x, size.height), strokeWidth = 2.dp.toPx())
            drawCircle(dot, radius = 5.dp.toPx(), center = Offset(x, y))
        }
        Column(Modifier.weight(1f).padding(bottom = 14.dp)) {
            Text(title, style = MaterialTheme.typography.bodyLarge, color = if (bad) colors.error else colors.onSurface)
            if (detail.isNotBlank()) Text(detail, style = MaterialTheme.typography.bodySmall, color = colors.onSurfaceVariant)
        }
    }
}
