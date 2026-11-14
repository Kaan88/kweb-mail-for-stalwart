package ovh.kweb.mail.ui.sending

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.LifecycleResumeEffect
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import java.time.ZoneId
import java.util.Locale
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.calendar.GroupColumn
import ovh.kweb.mail.ui.calendar.NoticeBar
import ovh.kweb.mail.ui.list.Groups
import uniffi.kweb_core.ScheduledSend

/**
 * The messages the server holds back for later. Each can be taken back: it asks, then the message is a draft
 * again and opens in the compose screen ([onOpenDraft]). Pull to refresh.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ScheduledScreen(onBack: () -> Unit, onOpenDraft: (String) -> Unit, vm: ScheduledViewModel = viewModel()) {
    val c = vm.controller
    val state by c.state.collectAsStateWithLifecycle()
    LaunchedEffect(vm) { c.start() }
    // The list is read again each time the screen comes up (also after the draft of a taken-back message is closed).
    LifecycleResumeEffect(vm) {
        c.refresh()
        onPauseOrDispose { }
    }
    LaunchedEffect(vm) { c.openDraft.collect(onOpenDraft) }
    // Leaving while a message is being taken back would drop the answer: stay until the core has answered.
    BackHandler(enabled = state.busyId != null) { }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(stringResource(R.string.scheduled_title)) },
                navigationIcon = {
                    IconButton(onClick = onBack, enabled = state.busyId == null) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = stringResource(R.string.back))
                    }
                },
                colors = TopAppBarDefaults.topAppBarColors(containerColor = MaterialTheme.colorScheme.background),
            )
        },
        containerColor = MaterialTheme.colorScheme.background,
    ) { padding ->
        Column(Modifier.padding(padding).fillMaxSize()) {
            state.error?.let { NoticeBar(it.resolve(LocalResources.current), error = true, action = stringResource(R.string.compose_dismiss) to c::dismissError) }
            when (state.phase) {
                ScheduledPhase.LOADING -> Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) { CircularProgressIndicator() }
                ScheduledPhase.FAILED -> Column(
                    Modifier.fillMaxSize().padding(32.dp),
                    verticalArrangement = Arrangement.Center,
                    horizontalAlignment = Alignment.CenterHorizontally,
                ) {
                    Text(stringResource(R.string.scheduled_load_failed), color = MaterialTheme.colorScheme.onSurfaceVariant, textAlign = TextAlign.Center)
                    TextButton(onClick = { c.refresh() }) { Text(stringResource(R.string.retry)) }
                }
                ScheduledPhase.READY -> PullToRefreshBox(isRefreshing = state.refreshing, onRefresh = { c.refresh(pull = true) }, modifier = Modifier.fillMaxSize()) {
                    // Scrollable even when empty, so that the pull works on the empty state too.
                    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(bottom = 24.dp)) {
                        if (state.items.isEmpty()) {
                            Text(
                                stringResource(R.string.scheduled_empty),
                                modifier = Modifier.fillMaxWidth().padding(horizontal = 32.dp, vertical = 48.dp),
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                                textAlign = TextAlign.Center,
                            )
                        } else {
                            Text(
                                stringResource(R.string.scheduled_hint),
                                modifier = Modifier.padding(horizontal = 28.dp, vertical = 8.dp),
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                                style = MaterialTheme.typography.bodyMedium,
                            )
                            GroupColumn(Modifier.padding(top = 8.dp)) {
                                state.items.forEachIndexed { index, item ->
                                    ScheduledRow(Groups.shape(index, state.items.size), item, busy = state.busyId == item.id, enabled = state.busyId == null) { c.ask(item) }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    state.asking?.let { item ->
        AlertDialog(
            onDismissRequest = c::dismissAsk,
            title = { Text(stringResource(R.string.scheduled_cancel_title)) },
            text = { Text(stringResource(R.string.scheduled_cancel_text, item.subject.ifBlank { stringResource(R.string.scheduled_no_subject) })) },
            confirmButton = { TextButton(onClick = c::confirmCancel) { Text(stringResource(R.string.scheduled_cancel)) } },
            dismissButton = { TextButton(onClick = c::dismissAsk) { Text(stringResource(R.string.scheduled_keep)) } },
        )
    }
}

@Composable
private fun ScheduledRow(shape: Shape, item: ScheduledSend, busy: Boolean, enabled: Boolean, onCancel: () -> Unit) {
    val colors = MaterialTheme.colorScheme
    val resources = LocalResources.current
    val zone = remember { ZoneId.systemDefault() }
    val locale = remember { Locale.getDefault() }
    val whenText = remember(item.sendAt) { SendLaterWords.whenText(resources, item.sendAt, System.currentTimeMillis() / 1000, zone, locale) }
    val recipients = remember(item.to) { ScheduledRules.recipients(item.to) }
    Row(
        Modifier.fillMaxWidth().clip(shape).background(colors.surfaceContainerLow).heightIn(min = 72.dp).padding(start = 20.dp, end = 8.dp, top = 10.dp, bottom = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Column(Modifier.weight(1f)) {
            Text(
                item.subject.ifBlank { stringResource(R.string.scheduled_no_subject) },
                style = MaterialTheme.typography.bodyLarge,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            val to = if (recipients.shown.isEmpty()) {
                stringResource(R.string.scheduled_no_recipients)
            } else if (recipients.more > 0) {
                stringResource(R.string.scheduled_recipients_more, recipients.shown.joinToString(", "), recipients.more)
            } else {
                recipients.shown.joinToString(", ")
            }
            Text(to, color = colors.onSurfaceVariant, style = MaterialTheme.typography.bodyMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(whenText, color = colors.primary, style = MaterialTheme.typography.labelLarge)
        }
        if (busy) {
            CircularProgressIndicator(Modifier.padding(end = 12.dp).size(24.dp), strokeWidth = 2.dp)
        } else {
            TextButton(onClick = onCancel, enabled = enabled) { Text(stringResource(R.string.scheduled_cancel)) }
        }
    }
}
