package ovh.kweb.mail.ui.calendar

import android.app.Application
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.Edit
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import java.time.ZoneId
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.list.Groups
import uniffi.kweb_core.EventDetail
import uniffi.kweb_core.Participant

/**
 * One event or task: when, where, notes, reminders, the guests and, for an invitation, the three
 * answers. [onEdit] opens the form; [onBack] leaves, also after a delete.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun EventScreen(eventId: String, recurrenceId: String?, onBack: () -> Unit, onEdit: (eventId: String) -> Unit) {
    // The view model belongs to this visit (the store of its route, see ScreenStores), so a new visit asks the core again.
    val app = LocalContext.current.applicationContext as Application
    val vm: EventViewModel = viewModel(
        factory = viewModelFactory { initializer { EventViewModel(app, eventId, recurrenceId) } },
    )
    val state by vm.state.collectAsStateWithLifecycle()
    val detail = state.detail
    var asking by remember { mutableStateOf(false) }

    LaunchedEffect(vm) { vm.load() }
    LaunchedEffect(vm) { vm.left.collect { onBack() } }

    Scaffold(
        topBar = {
            TopAppBar(
                title = {},
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = stringResource(R.string.back))
                    }
                },
                actions = {
                    if (detail != null && detail.mayWrite) {
                        IconButton(onClick = { onEdit(detail.eventId) }, enabled = !state.busy) {
                            Icon(Icons.Filled.Edit, contentDescription = stringResource(R.string.cal_edit))
                        }
                        IconButton(onClick = { asking = true }, enabled = !state.busy) {
                            Icon(Icons.Filled.Delete, contentDescription = stringResource(R.string.cal_delete))
                        }
                    }
                },
                colors = TopAppBarDefaults.topAppBarColors(containerColor = MaterialTheme.colorScheme.background),
            )
        },
        containerColor = MaterialTheme.colorScheme.background,
    ) { padding ->
        Column(Modifier.padding(padding).fillMaxSize()) {
            state.message?.let {
                NoticeBar(it.resolve(LocalResources.current), error = true, action = stringResource(R.string.cal_dismiss) to vm::dismissMessage)
            }
            when {
                detail != null -> Details(detail, recurrenceId, state, onAnswer = vm::respond)
                state.phase == EventPhase.LOADING -> Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) { CircularProgressIndicator() }
                else -> Column(
                    Modifier.fillMaxSize().padding(32.dp),
                    verticalArrangement = Arrangement.Center,
                    horizontalAlignment = Alignment.CenterHorizontally,
                ) {
                    val missing = state.phase == EventPhase.MISSING
                    Text(
                        stringResource(if (missing) R.string.cal_event_missing else R.string.cal_event_failed),
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        textAlign = TextAlign.Center,
                    )
                    if (!missing) TextButton(onClick = vm::retry) { Text(stringResource(R.string.retry)) }
                }
            }
        }
    }

    if (asking && detail != null) {
        DeleteDialog(
            ask = EventRules.deleteAsk(detail.input.repeat != null || detail.hasComplexRepeat, recurrenceId),
            isTask = detail.input.isTask,
            onDelete = { scope ->
                asking = false
                vm.delete(scope)
            },
            onDismiss = { asking = false },
        )
    }
}

@Composable
private fun DeleteDialog(ask: EventRules.DeleteAsk, isTask: Boolean, onDelete: (DeleteScope) -> Unit, onDismiss: () -> Unit) {
    val title = stringResource(if (isTask) R.string.cal_delete_task_title else R.string.cal_delete_title)
    when (ask) {
        EventRules.DeleteAsk.ONE_OR_ALL -> AlertDialog(
            onDismissRequest = onDismiss,
            title = { Text(title) },
            text = { Text(stringResource(R.string.cal_delete_repeating_text)) },
            confirmButton = {
                Column(horizontalAlignment = Alignment.End) {
                    TextButton(onClick = { onDelete(DeleteScope.THIS_ONE) }) { Text(stringResource(R.string.cal_delete_this_one)) }
                    TextButton(onClick = { onDelete(DeleteScope.ALL) }) { Text(stringResource(R.string.cal_delete_all)) }
                    TextButton(onClick = onDismiss) { Text(stringResource(R.string.cancel)) }
                }
            },
        )
        else -> AlertDialog(
            onDismissRequest = onDismiss,
            title = { Text(title) },
            text = if (ask == EventRules.DeleteAsk.WHOLE_SERIES) {
                { Text(stringResource(R.string.cal_delete_series_text)) }
            } else {
                null
            },
            confirmButton = { TextButton(onClick = { onDelete(DeleteScope.ALL) }) { Text(stringResource(R.string.action_delete)) } },
            dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.cancel)) } },
        )
    }
}

private data class InfoRow(val icon: Int, val primary: String, val secondary: String? = null)

@Composable
private fun Details(detail: EventDetail, recurrenceId: String?, state: EventState, onAnswer: (String) -> Unit) {
    val locale = rememberLocale()
    val words = rememberWords()
    val input = detail.input
    val span = remember(input) { runCatching { Span.ofInput(input, ZoneId.systemDefault()) }.getOrNull() }
    val whenText = remember(span, locale) { span?.let { EventWording.whenOf(it, locale, words) } }
    val rule = input.repeat
    val repeatText = remember(rule, detail.hasComplexRepeat, locale) {
        when {
            detail.hasComplexRepeat -> words.repeats
            rule != null -> EventWording.repeat(rule, locale, words)
            else -> null
        }
    }
    val rows = buildList {
        add(InfoRow(R.drawable.ic_schedule, whenText?.date ?: input.start, whenText?.time))
        repeatText?.let { add(InfoRow(R.drawable.ic_repeat, it)) }
        state.calendarName?.let { add(InfoRow(R.drawable.ic_calendar, it)) }
        if (input.location.isNotBlank()) add(InfoRow(R.drawable.ic_place, input.location.trim()))
        if (input.alertMinutes.isNotEmpty()) {
            add(InfoRow(R.drawable.ic_alarm, input.alertMinutes.joinToString("\n") { EventWording.reminder(it.toInt(), words) }))
        }
        if (input.description.isNotBlank()) add(InfoRow(R.drawable.ic_notes, input.description.trim()))
    }
    val guests = remember(detail.participants) { EventRules.sorted(detail.participants) }

    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(bottom = 32.dp)) {
        Text(
            input.title.ifBlank { stringResource(R.string.cal_untitled) },
            modifier = Modifier.padding(start = 24.dp, end = 24.dp, top = 4.dp, bottom = 16.dp).semantics { heading() },
            style = MaterialTheme.typography.headlineMedium,
            textDecoration = if (input.isTask && detail.isDone) TextDecoration.LineThrough else null,
        )
        if (EventRules.isInvitation(detail.participants, detail.myStatus)) {
            AnswerButtons(current = detail.myStatus, busy = state.busy, onAnswer = onAnswer)
        }
        GroupColumn {
            rows.forEachIndexed { index, row -> InfoCard(Groups.shape(index, rows.size), row.icon, row.primary, row.secondary) }
        }
        if (guests.isNotEmpty()) {
            SectionTitle(stringResource(R.string.cal_guests))
            GroupColumn {
                guests.forEachIndexed { index, guest ->
                    InfoCard(Groups.shape(index, guests.size), R.drawable.ic_group, EventRules.label(guest), guestLine(guest, words))
                }
            }
        }
    }
}

@Composable
private fun guestLine(guest: Participant, words: Words): String {
    val name = EventRules.label(guest)
    return listOfNotNull(
        guest.email.trim().takeIf { it.isNotEmpty() && it != name },
        words.status(guest.status),
        stringResource(R.string.cal_organizer).takeIf { guest.isOrganizer },
        stringResource(R.string.cal_you).takeIf { guest.isMe },
    ).joinToString(" · ")
}

@Composable
private fun SectionTitle(text: String) {
    Text(
        text.uppercase(rememberLocale()),
        modifier = Modifier.fillMaxWidth().padding(start = 32.dp, end = 20.dp, top = 24.dp, bottom = 8.dp).semantics { heading() },
        style = MaterialTheme.typography.labelMedium,
        fontWeight = FontWeight.Bold,
        letterSpacing = 0.8.sp,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
    )
}

@Composable
private fun InfoCard(shape: Shape, icon: Int, primary: String, secondary: String?) {
    val colors = MaterialTheme.colorScheme
    Row(
        Modifier.fillMaxWidth().clip(shape).background(colors.surfaceContainerLow).heightIn(min = 56.dp).padding(horizontal = 20.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        Icon(painterResource(icon), contentDescription = null, tint = colors.onSurfaceVariant, modifier = Modifier.size(24.dp))
        Column(Modifier.weight(1f)) {
            Text(primary, style = MaterialTheme.typography.bodyLarge)
            if (!secondary.isNullOrEmpty()) {
                Text(secondary, color = colors.onSurfaceVariant, style = MaterialTheme.typography.bodyMedium)
            }
        }
    }
}

/** Accept, Maybe and Decline as one connected group, the current answer filled. */
@Composable
private fun AnswerButtons(current: String?, busy: Boolean, onAnswer: (String) -> Unit) {
    val answers = listOf(
        "accepted" to R.string.cal_accept,
        "tentative" to R.string.cal_maybe,
        "declined" to R.string.cal_decline,
    )
    Row(Modifier.padding(horizontal = 12.dp).padding(bottom = 12.dp), horizontalArrangement = Arrangement.spacedBy(3.dp)) {
        answers.forEachIndexed { index, (status, label) ->
            val outer = 26.dp
            val inner = 8.dp
            val first = index == 0
            val last = index == answers.lastIndex
            val shape = RoundedCornerShape(
                topStart = if (first) outer else inner,
                bottomStart = if (first) outer else inner,
                topEnd = if (last) outer else inner,
                bottomEnd = if (last) outer else inner,
            )
            val modifier = Modifier.weight(1f).height(52.dp).semantics { selected = current == status }
            val padding = PaddingValues(horizontal = 8.dp)
            if (current == status) {
                Button(onClick = { onAnswer(status) }, enabled = !busy, shape = shape, modifier = modifier, contentPadding = padding) {
                    Text(stringResource(label), maxLines = 1)
                }
            } else {
                FilledTonalButton(onClick = { onAnswer(status) }, enabled = !busy, shape = shape, modifier = modifier, contentPadding = padding) {
                    Text(stringResource(label), maxLines = 1)
                }
            }
        }
    }
}
