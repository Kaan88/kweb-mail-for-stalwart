package ovh.kweb.mail.ui.calendar

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.KeyboardArrowLeft
import androidx.compose.material.icons.automirrored.filled.KeyboardArrowRight
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Check
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.FloatingActionButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.LifecycleResumeEffect
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import java.time.LocalDate
import java.time.ZoneId
import java.util.Locale
import kotlin.math.abs
import ovh.kweb.mail.R
import uniffi.kweb_core.Occurrence

/**
 * The agenda of milestone M5: the month as the title, the week strip, then the days that have
 * something on them, each entry a rounded container in its calendar's tone.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun CalendarScreen(
    onOpenEvent: (eventId: String, recurrenceId: String?) -> Unit,
    onAddEvent: (startHint: String) -> Unit,
    bottomBar: @Composable () -> Unit,
    appSnackbar: SnackbarHostState,
    vm: CalendarViewModel = viewModel(),
) {
    val state by vm.state.collectAsStateWithLifecycle()
    val snackbar = remember { SnackbarHostState() }
    val resources = LocalResources.current
    val locale = rememberLocale()
    val words = rememberWords()

    // Every time the screen comes up, also from the background: it refreshes the agenda, today and the zone.
    LifecycleResumeEffect(vm) {
        vm.load()
        onPauseOrDispose { }
    }
    LaunchedEffect(vm) {
        vm.messages.collect {
            snackbar.currentSnackbarData?.dismiss()
            snackbar.showSnackbar(it.resolve(resources))
        }
    }
    LaunchedEffect(vm) {
        vm.scrollTo.collect { date ->
            AgendaRules.indexOfDay(vm.state.value.rows, date)?.let { vm.listState.animateScrollToItem(it) }
        }
    }

    Scaffold(
        topBar = {
            Column(Modifier.background(MaterialTheme.colorScheme.background).statusBarsPadding()) {
                TitleRow(
                    title = WeekRules.title(state.selected, locale),
                    todaySelected = state.selected == state.today,
                    onPrevious = { vm.moveWeek(-1) },
                    onNext = { vm.moveWeek(1) },
                    onToday = vm::goToToday,
                )
                WeekStrip(
                    selected = state.selected,
                    today = state.today,
                    locale = locale,
                    onSelect = vm::select,
                    onWeek = vm::moveWeek,
                )
            }
        },
        bottomBar = bottomBar,
        snackbarHost = {
            Column {
                SnackbarHost(snackbar)
                SnackbarHost(appSnackbar)
            }
        },
        floatingActionButton = {
            FloatingActionButton(
                onClick = { onAddEvent(state.selected.toString()) },
                shape = CircleShape,
                containerColor = MaterialTheme.colorScheme.primaryContainer,
                contentColor = MaterialTheme.colorScheme.onPrimaryContainer,
            ) { Icon(Icons.Filled.Add, contentDescription = stringResource(R.string.cal_add)) }
        },
        containerColor = MaterialTheme.colorScheme.background,
    ) { padding ->
        Column(Modifier.padding(padding).fillMaxSize()) {
            if (state.fromCache) NoticeBar(stringResource(R.string.cal_offline), error = false)
            if (state.failed && state.loaded) {
                NoticeBar(stringResource(R.string.cal_agenda_failed), error = true, action = stringResource(R.string.retry) to vm::refresh)
            }
            PullToRefreshBox(
                isRefreshing = state.refreshing,
                onRefresh = vm::refresh,
                modifier = Modifier.weight(1f).fillMaxWidth(),
            ) {
                LazyColumn(
                    Modifier.fillMaxSize(),
                    state = vm.listState,
                    contentPadding = PaddingValues(bottom = 96.dp),
                ) {
                    when {
                        !state.loaded && !state.failed -> item(key = "loading") { Centered { CircularProgressIndicator() } }
                        !state.loaded -> item(key = "failed") {
                            Centered {
                                Text(
                                    stringResource(R.string.cal_load_failed),
                                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                                    textAlign = TextAlign.Center,
                                )
                                TextButton(onClick = vm::refresh) { Text(stringResource(R.string.retry)) }
                            }
                        }
                        state.rows.isEmpty() -> item(key = "empty") {
                            Centered {
                                Text(
                                    stringResource(R.string.cal_empty),
                                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                                    textAlign = TextAlign.Center,
                                )
                            }
                        }
                        else -> items(state.rows, key = { row -> rowKey(row) }) { row ->
                            when (row) {
                                is AgendaRules.Row.Header -> DayHeading(
                                    AgendaRules.heading(row.date, state.today, locale, stringResource(R.string.cal_today)).uppercase(locale),
                                )
                                is AgendaRules.Row.Entry -> EntryCard(
                                    entry = row.occurrence,
                                    continues = row.continues,
                                    tone = AgendaRules.tone(row.occurrence.calendarId, state.calendarIds),
                                    zone = state.zone,
                                    locale = locale,
                                    words = words,
                                    onOpen = { onOpenEvent(row.occurrence.eventId, row.occurrence.recurrenceId) },
                                    onToggleDone = { vm.setTaskDone(row.occurrence, it) },
                                )
                            }
                        }
                    }
                }
            }
        }
    }
}

private fun rowKey(row: AgendaRules.Row): String = when (row) {
    is AgendaRules.Row.Header -> "day/${row.date}"
    is AgendaRules.Row.Entry -> "entry/${row.key}"
}

/** Content in the middle of the list area, with room to pull down on. */
@Composable
private fun androidx.compose.foundation.lazy.LazyItemScope.Centered(content: @Composable () -> Unit) {
    Column(
        Modifier.fillParentMaxSize().padding(32.dp),
        verticalArrangement = Arrangement.Center,
        horizontalAlignment = Alignment.CenterHorizontally,
    ) { content() }
}

@Composable
private fun TitleRow(title: String, todaySelected: Boolean, onPrevious: () -> Unit, onNext: () -> Unit, onToday: () -> Unit) {
    Row(
        Modifier.fillMaxWidth().padding(start = 24.dp, end = 12.dp, top = 8.dp, bottom = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            title,
            modifier = Modifier.weight(1f).semantics { heading() },
            style = MaterialTheme.typography.headlineSmall,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
        IconButton(onClick = onPrevious) {
            Icon(Icons.AutoMirrored.Filled.KeyboardArrowLeft, contentDescription = stringResource(R.string.cal_previous_week))
        }
        IconButton(onClick = onNext) {
            Icon(Icons.AutoMirrored.Filled.KeyboardArrowRight, contentDescription = stringResource(R.string.cal_next_week))
        }
        FilterChip(
            selected = todaySelected,
            onClick = onToday,
            label = { Text(stringResource(R.string.cal_today)) },
            shape = RoundedCornerShape(17.dp),
        )
    }
}

/** The seven days of the selected week. A swipe or the arrows change the week; today is the filled circle when it is selected, a ring otherwise. */
@Composable
private fun WeekStrip(selected: LocalDate, today: LocalDate, locale: Locale, onSelect: (LocalDate) -> Unit, onWeek: (Int) -> Unit) {
    val first = remember(locale) { WeekRules.firstDayOfWeek(locale) }
    val days = WeekRules.days(selected, first)
    val threshold = with(LocalDensity.current) { 48.dp.toPx() }
    val rtl = LocalLayoutDirection.current == LayoutDirection.Rtl
    Row(
        Modifier
            .fillMaxWidth()
            .padding(horizontal = 12.dp)
            .padding(bottom = 10.dp)
            .pointerInput(rtl) {
                var total = 0f
                detectHorizontalDragGestures(
                    onDragStart = { total = 0f },
                    onDragEnd = {
                        if (abs(total) > threshold) onWeek(if ((total < 0) != rtl) 1 else -1)
                    },
                    onDragCancel = { total = 0f },
                    onHorizontalDrag = { _, amount -> total += amount },
                )
            },
    ) {
        for (day in days) {
            DayCell(day, isSelected = day == selected, isToday = day == today, locale = locale, onClick = { onSelect(day) }, modifier = Modifier.weight(1f))
        }
    }
}

@Composable
private fun DayCell(day: LocalDate, isSelected: Boolean, isToday: Boolean, locale: Locale, onClick: () -> Unit, modifier: Modifier) {
    val colors = MaterialTheme.colorScheme
    val description = remember(day, locale) { "${WeekRules.fullName(day.dayOfWeek, locale)} ${day.dayOfMonth}" }
    Column(
        modifier.clickable(role = Role.Button, onClick = onClick).semantics(mergeDescendants = true) { contentDescription = description },
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        Text(WeekRules.initial(day.dayOfWeek, locale), style = MaterialTheme.typography.labelSmall, color = colors.onSurfaceVariant)
        Box(
            Modifier
                .size(36.dp)
                .clip(CircleShape)
                .then(if (isSelected) Modifier.background(colors.primary) else Modifier)
                .then(if (isToday && !isSelected) Modifier.border(BorderStroke(1.5.dp, colors.primary), CircleShape) else Modifier),
            contentAlignment = Alignment.Center,
        ) {
            Text(
                day.dayOfMonth.toString(),
                color = if (isSelected) colors.onPrimary else colors.onSurface,
                fontWeight = if (isSelected || isToday) FontWeight.Bold else FontWeight.Medium,
                style = MaterialTheme.typography.bodyMedium,
            )
        }
    }
}

@Composable
internal fun DayHeading(text: String) {
    Text(
        text,
        modifier = Modifier.fillMaxWidth().padding(start = 20.dp, end = 20.dp, top = 12.dp, bottom = 4.dp).semantics { heading() },
        style = MaterialTheme.typography.labelMedium,
        fontWeight = FontWeight.Bold,
        letterSpacing = 0.7.sp,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
    )
}

/** An entry: its title, a line with time, place and "Invitation", in the container of its calendar. A task has a round box. */
@Composable
internal fun EntryCard(
    entry: Occurrence,
    continues: Boolean,
    tone: Int,
    zone: ZoneId,
    locale: Locale,
    words: Words,
    onOpen: () -> Unit,
    /** Null where a task cannot be ticked (a shared calendar): its box is only drawn. */
    onToggleDone: ((Boolean) -> Unit)?,
) {
    val (container, content) = toneColors(tone)
    val span = remember(entry, zone) { Span.ofOccurrence(entry.startsAt, entry.endsAt, entry.allDay, entry.isTask, zone) }
    val continuesWord = stringResource(R.string.cal_continues)
    val line = remember(entry, span, locale, continues) {
        if (continues) {
            listOfNotNull(continuesWord, entry.location.trim().ifEmpty { null }).joinToString(" · ")
        } else {
            EventWording.agendaLine(span, entry.location, EventWording.needsAnswer(entry.myStatus), locale, words)
        }
    }
    val title = entry.title.ifBlank { stringResource(R.string.cal_untitled) }
    Row(
        Modifier
            .fillMaxWidth()
            .padding(horizontal = 14.dp, vertical = 3.dp)
            .clip(RoundedCornerShape(20.dp))
            .background(container)
            .clickable(onClick = onOpen)
            .padding(start = if (entry.isTask) 6.dp else 14.dp, end = 14.dp, top = 10.dp, bottom = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (entry.isTask) {
            TaskBox(
                done = entry.isDone,
                color = content,
                checkColor = container,
                onToggle = onToggleDone?.let { toggle -> { toggle(!entry.isDone) } },
            )
        }
        Column(Modifier.weight(1f)) {
            Text(
                title,
                color = content,
                style = MaterialTheme.typography.bodyMedium,
                fontWeight = FontWeight.Bold,
                textDecoration = if (entry.isTask && entry.isDone) TextDecoration.LineThrough else null,
            )
            if (line.isNotEmpty()) {
                Text(line, color = content.copy(alpha = 0.8f), style = MaterialTheme.typography.bodySmall)
            }
        }
    }
}

/** The round check box of a task. */
@Composable
private fun TaskBox(done: Boolean, color: Color, checkColor: Color, onToggle: (() -> Unit)?) {
    val description = stringResource(if (done) R.string.cal_task_mark_open else R.string.cal_task_mark_done)
    Box(
        Modifier
            .size(44.dp)
            .clip(CircleShape)
            .then(if (onToggle != null) Modifier.clickable(role = Role.Checkbox, onClickLabel = description, onClick = onToggle) else Modifier)
            .semantics { contentDescription = description },
        contentAlignment = Alignment.Center,
    ) {
        Box(
            Modifier
                .size(22.dp)
                .clip(CircleShape)
                .then(if (done) Modifier.background(color) else Modifier.border(BorderStroke(2.dp, color), CircleShape)),
            contentAlignment = Alignment.Center,
        ) {
            if (done) {
                Icon(Icons.Filled.Check, contentDescription = null, tint = checkColor, modifier = Modifier.size(16.dp))
            }
        }
    }
}
