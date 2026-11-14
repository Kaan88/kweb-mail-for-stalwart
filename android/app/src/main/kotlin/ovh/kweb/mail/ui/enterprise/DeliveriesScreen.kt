package ovh.kweb.mail.ui.enterprise

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Clear
import androidx.compose.material.icons.filled.Search
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.calendar.NoticeBar
import ovh.kweb.mail.ui.list.Groups
import uniffi.kweb_core.Delivery

/** "Where is my mail": the search field, the range chips and what the server handled, newest first, with paging. */
@Composable
fun DeliveriesScreen(onBack: () -> Unit, onOpen: (String) -> Unit, vm: DeliveriesViewModel = viewModel()) {
    val c = vm.controller
    val state by c.state.collectAsStateWithLifecycle()
    LaunchedEffect(vm) { c.start() }

    EnterpriseScaffold(stringResource(R.string.ent_deliveries_title), onBack) { padding ->
        Column(Modifier.padding(padding).fillMaxSize()) {
            OutlinedTextField(
                value = state.text,
                onValueChange = c::setText,
                modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp),
                singleLine = true,
                shape = RoundedCornerShape(28.dp),
                placeholder = { Text(stringResource(R.string.ent_deliveries_hint)) },
                leadingIcon = { Icon(Icons.Filled.Search, contentDescription = null) },
                trailingIcon = {
                    if (state.text.isNotEmpty()) {
                        IconButton(onClick = { c.setText(""); c.search() }) {
                            Icon(Icons.Filled.Clear, contentDescription = stringResource(R.string.ent_clear))
                        }
                    }
                },
                keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search),
                keyboardActions = KeyboardActions(onSearch = { c.search() }),
            )
            Row(Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 10.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                HistoryRange.entries.forEach { range ->
                    FilterChip(
                        selected = state.range == range,
                        onClick = { c.setRange(range) },
                        label = { Text(stringResource(range.label)) },
                        leadingIcon = if (state.range == range) ({ Icon(Icons.Filled.Check, contentDescription = null, modifier = Modifier.size(18.dp)) }) else null,
                    )
                }
            }
            val error = state.error
            if (error != null && state.phase == Phase.READY) {
                NoticeBar(errorText(error), error = true, action = stringResource(R.string.ent_dismiss) to c::dismissError)
            }
            when {
                state.phase == Phase.LOADING -> LoadingBox()
                state.phase == Phase.FAILED -> FailedBox(error, R.string.ent_deliveries_failed, c::search)
                state.deliveries.isEmpty() -> EmptyState(R.drawable.ic_schedule, stringResource(R.string.ent_deliveries_none), stringResource(R.string.ent_deliveries_none_body))
                else -> DeliveryList(state, onOpen, c)
            }
        }
    }
}

@Composable
private fun DeliveryList(state: DeliveriesState, onOpen: (String) -> Unit, c: DeliveriesController) {
    val now = System.currentTimeMillis()
    val rows = state.deliveries
    LazyColumn(Modifier.fillMaxSize().padding(horizontal = 12.dp), verticalArrangement = Arrangement.spacedBy(3.dp)) {
        items(rows.size, key = { rows[it].id }) { index ->
            DeliveryRow(Groups.shape(index, rows.size), rows[index], now, onOpen)
        }
        item {
            Column(Modifier.fillMaxWidth().padding(vertical = 8.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                QuietNote(pluralStringResource(R.plurals.ent_deliveries_count, state.total.toInt().coerceAtLeast(0), state.total.toInt()))
                if (state.loadingMore) {
                    LoadingBox(Modifier.padding(8.dp))
                } else if (state.hasMore) {
                    TextButton(onClick = c::loadMore) { Text(stringResource(R.string.ent_load_more)) }
                }
            }
        }
    }
}

@Composable
private fun DeliveryRow(shape: androidx.compose.ui.graphics.Shape, d: Delivery, now: Long, onOpen: (String) -> Unit) {
    GroupRow(shape, onClick = { onOpen(d.id) }) {
        Column(Modifier.weight(1f)) {
            Text(d.from, maxLines = 1, overflow = TextOverflow.Ellipsis, style = MaterialTheme.typography.bodyLarge)
            Text(
                toLine(d.to),
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        Column(horizontalAlignment = Alignment.End, verticalArrangement = Arrangement.spacedBy(3.dp)) {
            StatusPill(stringResource(DeliveryRules.statusLabel(d.status)), bad = DeliveryRules.isBad(d.status))
            Text(DeliveryRules.rowTime(d.at, now), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
    }
}

@Composable
internal fun toLine(to: List<String>): String {
    val (first, rest) = DeliveryRules.firstAndRest(to)
    return if (rest == 0) stringResource(R.string.ent_to, first) else stringResource(R.string.ent_to_more, first, rest)
}
