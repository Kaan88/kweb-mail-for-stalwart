package ovh.kweb.mail.ui.admin

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material3.Button
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.LifecycleResumeEffect
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.list.EmptyNote
import ovh.kweb.mail.ui.list.Groups
import uniffi.kweb_core.QueuedMessage

private fun nowSeconds() = System.currentTimeMillis() / 1000

/** The mail queue: what waits to be delivered, with Retry and Cancel on each message and Pause for all. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun QueueScreen(onBack: () -> Unit, onOpen: (AdminScreen) -> Unit, vm: AdminQueueViewModel = viewModel()) {
    val c = vm.queue
    val list by c.list.state.collectAsStateWithLifecycle()
    val state by c.state.collectAsStateWithLifecycle()
    LifecycleResumeEffect(vm) {
        c.refresh()
        onPauseOrDispose { }
    }
    var menu by remember { mutableStateOf(false) }
    AdminScaffold(
        title = stringResource(R.string.admin_area_queue),
        onBack = onBack,
        actions = {
            if (!state.paused) TextButton(onClick = c::askPause, enabled = !state.busy) { Text(stringResource(R.string.admin_pause)) }
            IconButton(onClick = { menu = true }) { Icon(Icons.Filled.MoreVert, contentDescription = stringResource(R.string.more_options)) }
            DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                DropdownMenuItem(text = { Text(stringResource(R.string.admin_resume_menu)) }, onClick = { menu = false; c.resume() })
                DropdownMenuItem(text = { Text(stringResource(R.string.admin_refresh)) }, onClick = { menu = false; c.refresh() })
            }
        },
    ) { padding ->
        Column(Modifier.padding(padding).fillMaxSize()) {
            Notices(state.error ?: list.error, state.info, { c.dismissError(); c.list.dismissError() }, c::dismissInfo)
            if (state.paused) PausedBar(onResume = c::resume, enabled = !state.busy)
            when (list.phase) {
                ListPhase.LOADING -> LoadingBox()
                ListPhase.FAILED -> FailureBox(stringResource(R.string.admin_queue_load_failed), onRetry = { c.list.reload(quiet = false) })
                ListPhase.READY -> PullToRefreshBox(isRefreshing = list.refreshing, onRefresh = c::refresh, modifier = Modifier.fillMaxSize()) {
                    QueueList(list, state, c, onOpen)
                }
            }
        }
    }
    when (val dialog = state.dialog) {
        is QueueDialog.Cancel -> ConfirmDialog(
            title = stringResource(R.string.admin_cancel_title),
            text = stringResource(R.string.admin_cancel_text, dialog.from),
            confirm = stringResource(R.string.admin_cancel_confirm),
            onConfirm = c::confirmCancel,
            onDismiss = c::dismissDialog,
            danger = true,
        )
        QueueDialog.Pause -> ConfirmDialog(
            title = stringResource(R.string.admin_pause_title),
            text = stringResource(R.string.admin_pause_text),
            confirm = stringResource(R.string.admin_pause_confirm),
            onConfirm = c::confirmPause,
            onDismiss = c::dismissDialog,
        )
        null -> Unit
    }
}

/** "Sending is paused" with the way back one tap away; in the tertiary colour so it cannot be overlooked. */
@Composable
private fun PausedBar(onResume: () -> Unit, enabled: Boolean) {
    val colors = MaterialTheme.colorScheme
    Row(
        Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 4.dp).clip(RoundedCornerShape(16.dp)).background(colors.tertiaryContainer).padding(start = 16.dp, end = 8.dp, top = 4.dp, bottom = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.SpaceBetween,
    ) {
        Text(stringResource(R.string.admin_paused_bar), color = colors.onTertiaryContainer, style = MaterialTheme.typography.bodyLarge)
        TextButton(onClick = onResume, enabled = enabled) { Text(stringResource(R.string.admin_resume), color = colors.onTertiaryContainer) }
    }
}

@Composable
private fun QueueList(list: PagedState<QueuedMessage>, state: QueueState, c: QueueController, onOpen: (AdminScreen) -> Unit) {
    val now = nowSeconds()
    if (list.items.isEmpty()) {
        Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.Center) {
            EmptyNote(
                icon = R.drawable.ic_sent,
                text = stringResource(R.string.admin_queue_empty),
                hint = stringResource(R.string.admin_queue_empty_hint),
                modifier = Modifier.fillMaxWidth(),
            )
        }
        return
    }
    val line = remember(list.items, list.total, state.paused) {
        AdminRules.queueLine(list.total ?: list.items.size, list.items.map { it.nextTryAt }, now, state.paused)
    }
    LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(bottom = 32.dp), verticalArrangement = Arrangement.spacedBy(3.dp)) {
        item(key = "line") { Note(queueLineText(line)) }
        itemsIndexed(list.items, key = { _, m -> m.id }) { index, message ->
            Box12 { QueueCard(Groups.shape(index, list.items.size), message, now, state.busy, c, onOpen) }
        }
        if (list.more || list.loadingMore) {
            item(key = "more") {
                TextButton(onClick = c.list::loadMore, enabled = !list.loadingMore, modifier = Modifier.fillMaxWidth()) {
                    Text(stringResource(if (list.loadingMore) R.string.admin_loading else R.string.admin_load_more))
                }
            }
        }
    }
}

@Composable
private fun Box12(content: @Composable () -> Unit) {
    Column(Modifier.padding(horizontal = 12.dp)) { content() }
}

@Composable
private fun QueueCard(shape: androidx.compose.ui.graphics.Shape, m: QueuedMessage, now: Long, busy: Boolean, c: QueueController, onOpen: (AdminScreen) -> Unit) {
    val colors = MaterialTheme.colorScheme
    val (first, more) = AdminRules.recipientsSummary(m.recipients.map { it.address })
    Column(
        Modifier.fillMaxWidth().clip(shape).background(colors.surfaceContainerLow).clickable { onOpen(AdminScreen.QueuedMessage(m.id)) }.padding(horizontal = 20.dp, vertical = 14.dp),
        verticalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
            Text(m.from.ifBlank { stringResource(R.string.admin_no_sender) }, Modifier.weight(1f), style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
            m.queuedAt?.let { Text(stringResource(R.string.admin_since, whenText(it, now)), color = colors.onSurfaceVariant, style = MaterialTheme.typography.bodySmall) }
        }
        Text(
            if (more > 0) stringResource(R.string.admin_to_and_more, first, more) else stringResource(R.string.admin_to, first),
            color = colors.onSurfaceVariant,
            style = MaterialTheme.typography.bodyMedium,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
        Text(tryLineText(AdminRules.tryLine(m.tries, m.nextTryAt, now)), color = colors.onSurfaceVariant, style = MaterialTheme.typography.bodyMedium)
        if (m.error.isNotBlank()) Pill(AdminRules.clip(m.error, 200), error = true)
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
            TextButton(onClick = { c.askCancel(m) }, enabled = !busy) { Text(stringResource(R.string.admin_cancel_message), color = colors.error) }
            FilledTonalButton(onClick = { c.retry(m.id) }, enabled = !busy) { Text(stringResource(R.string.retry)) }
        }
    }
}

/** One message of the queue: what the server knows, every recipient with its state, Retry and Cancel message. */
@Composable
fun QueuedMessageScreen(id: String, onBack: () -> Unit, vm: AdminQueuedMessageViewModel = viewModel()) {
    val c = vm.forMessage(id)
    val state by c.state.collectAsStateWithLifecycle()
    LifecycleResumeEffect(vm) {
        c.refresh()
        onPauseOrDispose { }
    }
    androidx.compose.runtime.LaunchedEffect(state.gone) { if (state.gone) onBack() }
    AdminScaffold(title = stringResource(R.string.admin_queued_title), onBack = onBack) { padding ->
        Column(Modifier.padding(padding).fillMaxSize()) {
            Notices(state.error, state.info, c::dismissError, c::dismissInfo)
            val message = state.message
            when {
                message != null -> MessageBody(message, state.busy, c)
                state.phase == MessagePhase.FAILED -> FailureBox(stringResource(R.string.admin_load_failed), onRetry = c::refresh)
                else -> LoadingBox()
            }
        }
    }
    if (state.askCancel) {
        ConfirmDialog(
            title = stringResource(R.string.admin_cancel_title),
            text = stringResource(R.string.admin_cancel_text, state.message?.from.orEmpty()),
            confirm = stringResource(R.string.admin_cancel_confirm),
            onConfirm = c::confirmCancel,
            onDismiss = c::dismissDialog,
            danger = true,
        )
    }
}

@Composable
private fun MessageBody(m: QueuedMessage, busy: Boolean, c: QueuedMessageController) {
    val now = nowSeconds()
    val colors = MaterialTheme.colorScheme
    val rows = buildList {
        add(stringResource(R.string.admin_from) to m.from)
        m.queuedAt?.let { add(stringResource(R.string.admin_arrived) to whenText(it, now)) }
        add(stringResource(R.string.admin_size) to AdminRules.formatBytes(m.size))
        add(stringResource(R.string.admin_queue_id) to m.id)
        m.nextTryAt?.let { add(stringResource(R.string.admin_next_try) to if (AdminRules.isDue(it, now)) stringResource(R.string.admin_next_try_due) else whenText(it, now)) }
    }
    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(bottom = 32.dp)) {
        AdminGroup {
            rows.forEachIndexed { index, (label, value) -> AdminRow(Groups.shape(index, rows.size), label, detail = value) }
        }
        if (m.error.isNotBlank()) Note(m.error)
        SectionLabel(stringResource(R.string.admin_recipients))
        AdminGroup {
            m.recipients.forEachIndexed { index, r ->
                val state = AdminRules.recipientState(r.status)
                AdminRow(
                    Groups.shape(index, m.recipients.size),
                    r.address,
                    detail = r.detail.takeIf { it.isNotBlank() }?.let { AdminRules.clip(it, 300) },
                    trailing = { Pill(recipientWord(state, r.status), error = state == RecipientState.Failed) },
                )
            }
        }
        Button(
            onClick = c::retry,
            enabled = !busy,
            shape = CircleShape,
            modifier = Modifier.fillMaxWidth().padding(horizontal = 12.dp).padding(top = 16.dp),
        ) { Text(stringResource(R.string.retry)) }
        TextButton(onClick = c::askCancel, enabled = !busy, modifier = Modifier.fillMaxWidth().padding(horizontal = 12.dp)) {
            Text(stringResource(R.string.admin_cancel_message), color = colors.error)
        }
    }
}

@Composable
private fun recipientWord(state: RecipientState, raw: String): String = when (state) {
    RecipientState.Waiting -> stringResource(R.string.admin_rcpt_waiting)
    RecipientState.WillRetry -> stringResource(R.string.admin_rcpt_retry)
    RecipientState.Delivered -> stringResource(R.string.admin_rcpt_delivered)
    RecipientState.Failed -> stringResource(R.string.admin_rcpt_failed)
    RecipientState.Other -> raw.replaceFirstChar { it.uppercase() }
}
