package ovh.kweb.mail.ui.enterprise

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Check
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import java.util.Locale
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.calendar.GroupColumn
import ovh.kweb.mail.ui.calendar.NoticeBar
import ovh.kweb.mail.ui.list.Groups
import uniffi.kweb_core.ChartPoint
import uniffi.kweb_core.DashboardHistory

/**
 * How the server is doing: four live figures, the range chooser and two charts of the past. The stream runs while the
 * screen is shown: it opens when the screen starts and closes when the screen stops (it leaves the view or the app
 * goes to the background) or is left.
 */
@Composable
fun DashboardScreen(onBack: () -> Unit, vm: DashboardViewModel = viewModel()) {
    val c = vm.controller
    val state by c.state.collectAsStateWithLifecycle()
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    LaunchedEffect(vm) { c.start() }
    DisposableEffect(lifecycle, vm) {
        val observer = LifecycleEventObserver { _, event ->
            when (event) {
                Lifecycle.Event.ON_START -> c.startLive()
                Lifecycle.Event.ON_STOP -> c.stopLive()
                else -> Unit
            }
        }
        lifecycle.addObserver(observer)
        onDispose {
            lifecycle.removeObserver(observer)
            c.stopLive()
        }
    }

    EnterpriseScaffold(
        title = stringResource(R.string.ent_dash_title),
        onBack = onBack,
        actions = {
            StatusPill(
                stringResource(
                    when (state.live) {
                        LiveStatus.LIVE -> R.string.ent_pill_live
                        LiveStatus.PAUSED -> R.string.ent_pill_paused
                        LiveStatus.CONNECTING -> R.string.ent_pill_connecting
                    },
                ),
                bad = false,
                dot = state.live == LiveStatus.LIVE,
                modifier = Modifier.padding(end = 16.dp),
            )
        },
    ) { padding ->
        Column(Modifier.padding(padding).fillMaxSize().verticalScroll(rememberScrollState()).padding(bottom = 32.dp)) {
            val error = state.error
            if (error != null && state.history != null) {
                NoticeBar(errorText(error), error = true, action = stringResource(R.string.ent_dismiss) to c::dismissError)
            }
            Figures(state.figures)
            Row(Modifier.fillMaxWidth().padding(horizontal = 12.dp).padding(top = 12.dp, bottom = 10.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                ChartRange.entries.forEach { range ->
                    FilterChip(
                        selected = state.range == range,
                        onClick = { c.setRange(range) },
                        label = { Text(stringResource(range.label)) },
                        leadingIcon = if (state.range == range) ({ Icon(Icons.Filled.Check, contentDescription = null, modifier = Modifier.size(18.dp)) }) else null,
                    )
                }
            }
            val history = state.history
            when {
                history != null -> Charts(history, state.range)
                state.historyPhase == Phase.FAILED -> FailedBox(error, R.string.ent_dash_history_failed, c::loadHistory, Modifier.fillMaxWidth().height(220.dp))
                else -> LoadingBox(Modifier.fillMaxWidth().height(220.dp))
            }
        }
    }
}

@Composable
private fun Figures(figures: uniffi.kweb_core.LiveFigures?) {
    val none = stringResource(R.string.ent_none)
    val rows = listOf(
        stringResource(R.string.ent_fig_queue) to (figures?.queue?.toString() ?: none),
        stringResource(R.string.ent_fig_connections) to (figures?.connections?.toString() ?: none),
        stringResource(R.string.ent_fig_users) to (figures?.users?.toString() ?: none),
        stringResource(R.string.ent_fig_memory) to (figures?.memoryBytes?.let { SizeText.bytes(it) } ?: none),
    )
    GroupColumn {
        rows.forEachIndexed { index, (label, value) ->
            GroupRow(Groups.shape(index, rows.size)) {
                Text(label, modifier = Modifier.weight(1f), style = MaterialTheme.typography.bodyLarge)
                Text(value, style = MaterialTheme.typography.titleMedium)
            }
        }
    }
}

@Composable
private fun Charts(history: DashboardHistory, range: ChartRange) {
    val locale = remember { Locale.getDefault() }
    val resources = LocalResources.current
    val (times, received, delivered) = remember(history) { ChartRules.align(history.received, history.delivered) }
    val labels = remember(times, range) { times.map { ChartRules.timeLabel(it, range) } }
    val receivedTotal = ChartRules.tickLabel(received.sum(), locale)
    val deliveredTotal = ChartRules.tickLabel(delivered.sum(), locale)
    val perUnit = stringResource(if (range == ChartRange.DAY) R.string.ent_chart_messages_hour else R.string.ent_chart_messages_day)
    GroupColumn {
        Column(
            Modifier.fillMaxWidth().groupSurface(Groups.shape(0, 2)).padding(horizontal = 20.dp, vertical = 14.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Text(perUnit, style = MaterialTheme.typography.titleMedium)
            Row(horizontalArrangement = Arrangement.spacedBy(16.dp), verticalAlignment = Alignment.CenterVertically) {
                Legend(MaterialTheme.colorScheme.primary, stringResource(R.string.ent_legend_received, receivedTotal), bar = true)
                Legend(MaterialTheme.colorScheme.onSurface, stringResource(R.string.ent_legend_delivered, deliveredTotal), bar = false)
            }
            if (times.isEmpty()) {
                Text(stringResource(R.string.ent_chart_empty), color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.bodySmall)
            } else {
                HistoryChart(
                    labels, received, delivered, fill = false,
                    valueText = { ChartRules.tickLabel(it, locale) },
                    description = resources.getString(R.string.ent_chart_messages_description, perUnit, receivedTotal, deliveredTotal),
                )
            }
        }
        val seconds = history.deliverySeconds
        val average = ChartRules.average(seconds)
        Column(
            Modifier.fillMaxWidth().groupSurface(Groups.shape(1, 2)).padding(horizontal = 20.dp, vertical = 14.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(stringResource(R.string.ent_chart_delivery_time), style = MaterialTheme.typography.titleMedium, modifier = Modifier.weight(1f, fill = false))
                if (average != null) {
                    Text(
                        stringResource(R.string.ent_chart_average, ChartRules.duration(average, locale)),
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        style = MaterialTheme.typography.bodySmall,
                    )
                }
            }
            if (seconds.isEmpty()) {
                Text(stringResource(R.string.ent_chart_empty), color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.bodySmall)
            } else {
                val values = remember(seconds) { seconds.map(ChartPoint::value) }
                val secondLabels = remember(seconds, range) { seconds.map { ChartRules.timeLabel(it.at, range) } }
                HistoryChart(
                    secondLabels, emptyList(), values, fill = true,
                    valueText = { ChartRules.duration(it, locale) },
                    description = resources.getString(R.string.ent_chart_time_description, average?.let { ChartRules.duration(it, locale) } ?: resources.getString(R.string.ent_none)),
                )
            }
        }
    }
}

@Composable
private fun Legend(color: androidx.compose.ui.graphics.Color, text: String, bar: Boolean) {
    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
        androidx.compose.foundation.layout.Box(
            Modifier.then(if (bar) Modifier.size(10.dp) else Modifier.size(width = 12.dp, height = 2.dp))
                .background(color, RoundedCornerShape(2.dp)),
        )
        Text(text, style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}
