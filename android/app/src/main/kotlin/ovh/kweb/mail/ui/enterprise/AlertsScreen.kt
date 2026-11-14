package ovh.kweb.mail.ui.enterprise

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.calendar.GroupColumn
import ovh.kweb.mail.ui.calendar.NoticeBar
import ovh.kweb.mail.ui.list.Groups

/** The server's alerts, each with a switch. Alerts arrive as mail or webhooks, so the phone notifies for one that is mailed. */
@Composable
fun AlertsScreen(onBack: () -> Unit, vm: AlertsViewModel = viewModel()) {
    val c = vm.controller
    val state by c.state.collectAsStateWithLifecycle()
    LaunchedEffect(vm) { c.start() }

    EnterpriseScaffold(stringResource(R.string.ent_alerts_title), onBack) { padding ->
        Column(Modifier.padding(padding).fillMaxSize()) {
            val error = state.error
            if (error != null && state.phase == Phase.READY) {
                NoticeBar(errorText(error), error = true, action = stringResource(R.string.ent_dismiss) to c::dismissError)
            }
            when {
                state.phase == Phase.LOADING -> LoadingBox()
                state.phase == Phase.FAILED -> FailedBox(error, R.string.ent_alerts_failed, c::load)
                state.alerts.isEmpty() -> EmptyState(R.drawable.ic_alarm, stringResource(R.string.ent_alerts_none), stringResource(R.string.ent_alerts_none_body))
                else -> Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(bottom = 32.dp)) {
                    GroupColumn {
                        state.alerts.forEachIndexed { index, alert ->
                            GroupRow(Groups.shape(index, state.alerts.size)) {
                                Column(Modifier.weight(1f)) {
                                    Text(alert.name, maxLines = 2, overflow = TextOverflow.Ellipsis, style = MaterialTheme.typography.bodyLarge)
                                    if (alert.summary.isNotBlank()) {
                                        Text(alert.summary, maxLines = 3, overflow = TextOverflow.Ellipsis, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                                    }
                                }
                                Switch(
                                    checked = alert.isEnabled,
                                    onCheckedChange = { c.setEnabled(alert.id, it) },
                                    enabled = alert.id !in state.pending,
                                )
                            }
                        }
                    }
                    QuietNote(stringResource(R.string.ent_alerts_note))
                }
            }
        }
    }
}
