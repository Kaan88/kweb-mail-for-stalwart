package ovh.kweb.mail.ui.calendar

import ovh.kweb.mail.ui.accounts.LocalAccountSession
import android.app.Application
import android.text.format.DateFormat
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Close
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DatePicker
import androidx.compose.material3.DatePickerDialog
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TextField
import androidx.compose.material3.TextFieldDefaults
import androidx.compose.material3.TimePicker
import androidx.compose.material3.rememberDatePickerState
import androidx.compose.material3.rememberTimePickerState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import java.time.DayOfWeek
import java.time.LocalDate
import java.time.LocalTime
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle
import java.util.Locale
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.list.Groups
import uniffi.kweb_core.Frequency

/** What a date or time picker on screen is for. */
private enum class Picking { START_DATE, START_TIME, END_DATE, END_TIME, UNTIL }

/**
 * The form for a new event or task ([eventId] null; [startHint] the day it is for) or for
 * changing one. [onBack] leaves, after a save and when the user lets changes go.
 */
@Composable
fun EditEventScreen(eventId: String?, startHint: String?, onBack: () -> Unit, onSaved: () -> Unit, account: String? = null) {
    // The view model belongs to this visit (the store of its route, see ScreenStores): it survives a rotation and goes with the screen.
    val app = LocalContext.current.applicationContext as Application
    val accountSession = LocalAccountSession.current
    val vm: EditEventViewModel = viewModel(
        factory = viewModelFactory { initializer { EditEventViewModel(app, accountSession, eventId, startHint, account) } },
    )
    val state by vm.state.collectAsStateWithLifecycle()
    var confirmDiscard by remember { mutableStateOf(false) }
    val form = state.form

    LaunchedEffect(vm) { vm.load() }
    LaunchedEffect(vm) { vm.saved.collect { onSaved() } }

    // While a save runs back does nothing: leaving would cancel the coroutine that reports a failure.
    val leave = { if (state.saving) Unit else if (state.dirty) confirmDiscard = true else onBack() }
    BackHandler(onBack = leave)

    Scaffold(
        topBar = {
            Row(
                Modifier.fillMaxWidth().background(MaterialTheme.colorScheme.background).statusBarsPadding().padding(horizontal = 8.dp, vertical = 6.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                IconButton(onClick = leave, enabled = !state.saving) {
                    Icon(Icons.Filled.Close, contentDescription = stringResource(R.string.cal_close))
                }
                Text(
                    stringResource(if (state.creating) R.string.cal_new else R.string.cal_edit_title),
                    modifier = Modifier.weight(1f).padding(start = 8.dp),
                    style = MaterialTheme.typography.titleLarge,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                // A filled pill, as the Compose screen has its Send.
                Button(
                    onClick = vm::save,
                    enabled = state.phase == EditPhase.READY && !state.saving,
                    shape = CircleShape,
                    modifier = Modifier.height(40.dp),
                    contentPadding = androidx.compose.foundation.layout.PaddingValues(horizontal = 22.dp),
                ) {
                    if (state.saving) {
                        CircularProgressIndicator(Modifier.size(18.dp), strokeWidth = 2.dp, color = MaterialTheme.colorScheme.onPrimary)
                    } else {
                        Text(stringResource(R.string.cal_save), style = MaterialTheme.typography.labelLarge)
                    }
                }
            }
        },
        containerColor = MaterialTheme.colorScheme.background,
    ) { padding ->
        Column(Modifier.padding(padding).fillMaxSize()) {
            state.error?.let {
                NoticeBar(it.resolve(LocalResources.current), error = true, action = stringResource(R.string.cal_dismiss) to vm::dismissError)
            }
            when {
                form != null && state.phase == EditPhase.READY -> Form(form, state, vm::edit, vm::setAllDay)
                state.phase == EditPhase.LOADING -> Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) { CircularProgressIndicator() }
                else -> Column(
                    Modifier.fillMaxSize().padding(32.dp),
                    verticalArrangement = Arrangement.Center,
                    horizontalAlignment = Alignment.CenterHorizontally,
                ) {
                    Text(
                        stringResource(
                            when (state.phase) {
                                EditPhase.MISSING -> R.string.cal_event_missing
                                EditPhase.READ_ONLY -> R.string.cal_read_only
                                EditPhase.NO_CALENDAR -> R.string.cal_no_calendar
                                else -> R.string.cal_event_failed
                            },
                        ),
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        textAlign = TextAlign.Center,
                    )
                    if (state.phase == EditPhase.FAILED) TextButton(onClick = vm::load) { Text(stringResource(R.string.retry)) }
                }
            }
        }
    }

    if (confirmDiscard) {
        AlertDialog(
            onDismissRequest = { confirmDiscard = false },
            title = { Text(stringResource(R.string.cal_discard_title)) },
            text = { Text(stringResource(R.string.cal_discard_text)) },
            confirmButton = {
                TextButton(onClick = {
                    confirmDiscard = false
                    onBack()
                }) { Text(stringResource(R.string.cal_discard)) }
            },
            dismissButton = { TextButton(onClick = { confirmDiscard = false }) { Text(stringResource(R.string.cal_keep_editing)) } },
        )
    }
}

@Composable
private fun Form(form: EventForm, state: EditState, edit: ((EventForm) -> EventForm) -> Unit, onAllDay: (Boolean) -> Unit) {
    val locale = rememberLocale()
    val words = rememberWords()
    var picking by remember { mutableStateOf<Picking?>(null) }
    val dateFormat = remember(locale) { DateTimeFormatter.ofLocalizedDate(FormatStyle.MEDIUM).withLocale(locale) }
    val timeFormat = remember(locale) { DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT).withLocale(locale) }

    Column(Modifier.fillMaxSize().imePadding().verticalScroll(rememberScrollState()).padding(bottom = 32.dp)) {
        Group {
            add { shape -> TextRow(shape, stringResource(R.string.cal_title_label), form.title, { v -> edit { it.copy(title = v) } }, singleLine = true) }
        }

        if (state.creating) {
            val kinds = listOf(false to R.string.cal_kind_event, true to R.string.cal_kind_task)
            SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 12.dp)) {
                kinds.forEachIndexed { index, (task, label) ->
                    SegmentedButton(
                        selected = form.isTask == task,
                        onClick = { edit { it.withTask(task) } },
                        shape = SegmentedButtonDefaults.itemShape(index, kinds.size),
                    ) { Text(stringResource(label)) }
                }
            }
        } else {
            Box(Modifier.height(12.dp))
        }

        Group {
            if (state.calendars.size > 1 || state.calendars.none { it.id == form.calendarId }) {
                add { shape ->
                    val name = state.calendars.firstOrNull { it.id == form.calendarId }?.name.orEmpty()
                    MenuRow(shape, stringResource(R.string.cal_calendar), name) { close ->
                        state.calendars.forEach { calendar ->
                            DropdownMenuItem(
                                text = { Text(calendar.name) },
                                onClick = {
                                    edit { it.copy(calendarId = calendar.id) }
                                    close()
                                },
                            )
                        }
                    }
                }
            }
            add { shape ->
                SwitchRow(shape, stringResource(R.string.cal_all_day_switch), form.allDay, onAllDay)
            }
            add { shape ->
                PickerRow(
                    shape,
                    stringResource(if (form.isTask) R.string.cal_due else R.string.cal_starts),
                    dateFormat.format(form.startDate),
                    if (form.allDay) null else timeFormat.format(form.startTime),
                    onDate = { picking = Picking.START_DATE },
                    onTime = { picking = Picking.START_TIME },
                )
            }
            if (!form.isTask) {
                add { shape ->
                    PickerRow(
                        shape,
                        stringResource(R.string.cal_ends),
                        dateFormat.format(form.endDate),
                        if (form.allDay) null else timeFormat.format(form.endTime),
                        onDate = { picking = Picking.END_DATE },
                        onTime = { picking = Picking.END_TIME },
                    )
                }
            }
        }

        // The times are wall-clock times in the entry's own zone; the agenda shows the phone's.
        val zoneNote = remember(form.zone) { EventForms.zoneNote(form.zone, ZoneId.systemDefault()) }
        if (zoneNote != null && !form.allDay) {
            Text(
                stringResource(R.string.cal_form_zone, zoneNote),
                modifier = Modifier.fillMaxWidth().padding(start = 32.dp, end = 20.dp, top = 8.dp),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }

        Group(Modifier.padding(top = 16.dp)) {
            if (form.repeatLocked) {
                add { shape -> StaticRow(shape, stringResource(R.string.cal_repeat), stringResource(R.string.cal_repeat_locked)) }
            } else {
                val rule = form.repeat
                add { shape ->
                    val value = if (rule == null) stringResource(R.string.cal_repeat_never) else words.every(rule.frequency, rule.interval)
                    MenuRow(shape, stringResource(R.string.cal_repeat), value) { close ->
                        DropdownMenuItem(
                            text = { Text(stringResource(R.string.cal_repeat_never)) },
                            onClick = {
                                edit { it.withFrequency(null) }
                                close()
                            },
                        )
                        Frequency.entries.forEach { frequency ->
                            DropdownMenuItem(
                                text = { Text(words.every(frequency, 1)) },
                                onClick = {
                                    edit { it.withFrequency(frequency) }
                                    close()
                                },
                            )
                        }
                    }
                }
                if (rule != null && rule.frequency == Frequency.WEEKLY) {
                    add { shape -> WeekdayRow(shape, rule.days, locale) { day, on -> edit { it.withWeekday(day, on) } } }
                }
                if (rule != null) {
                    add { shape ->
                        val value = stringResource(
                            when (rule.endKind) {
                                EndKind.NEVER -> R.string.cal_ends_never
                                EndKind.ON_DATE -> R.string.cal_ends_on_date
                                EndKind.AFTER_COUNT -> R.string.cal_ends_after
                            },
                        )
                        MenuRow(shape, stringResource(R.string.cal_repeat_ends), value) { close ->
                            EndKind.entries.forEach { kind ->
                                DropdownMenuItem(
                                    text = {
                                        Text(
                                            stringResource(
                                                when (kind) {
                                                    EndKind.NEVER -> R.string.cal_ends_never
                                                    EndKind.ON_DATE -> R.string.cal_ends_on_date
                                                    EndKind.AFTER_COUNT -> R.string.cal_ends_after
                                                },
                                            ),
                                        )
                                    },
                                    onClick = {
                                        edit { it.withEndKind(kind) }
                                        close()
                                    },
                                )
                            }
                        }
                    }
                    if (rule.endKind == EndKind.ON_DATE) {
                        add { shape ->
                            PickerRow(
                                shape,
                                stringResource(R.string.cal_repeat_until),
                                (rule.until ?: form.startDate).let(dateFormat::format),
                                null,
                                onDate = { picking = Picking.UNTIL },
                                onTime = {},
                            )
                        }
                    }
                    if (rule.endKind == EndKind.AFTER_COUNT) {
                        add { shape ->
                            TextRow(
                                shape,
                                stringResource(R.string.cal_repeat_count),
                                rule.countText,
                                { v -> edit { it.withCountText(v) } },
                                singleLine = true,
                                number = true,
                            )
                        }
                    }
                }
            }
        }

        Group(Modifier.padding(top = 16.dp)) {
            if (form.alertsLocked) {
                add { shape ->
                    StaticRow(shape, stringResource(R.string.cal_reminder), pluralStringResource(R.plurals.cal_reminders_locked, form.alerts.size, form.alerts.size))
                }
            } else {
                add { shape ->
                    val current = form.alerts.singleOrNull()
                    val value = if (current == null) stringResource(R.string.cal_reminder_none) else EventWording.reminder(current, words)
                    MenuRow(shape, stringResource(R.string.cal_reminder), value) { close ->
                        DropdownMenuItem(
                            text = { Text(stringResource(R.string.cal_reminder_none)) },
                            onClick = {
                                edit { it.copy(alerts = emptyList()) }
                                close()
                            },
                        )
                        EventForms.alertOptions(form.alerts).forEach { minutes ->
                            DropdownMenuItem(
                                text = { Text(EventWording.reminder(minutes, words)) },
                                onClick = {
                                    edit { it.copy(alerts = listOf(minutes)) }
                                    close()
                                },
                            )
                        }
                    }
                }
            }
        }

        Group(Modifier.padding(top = 16.dp)) {
            add { shape -> TextRow(shape, stringResource(R.string.cal_place), form.location, { v -> edit { it.copy(location = v) } }, singleLine = true) }
            add { shape -> TextRow(shape, stringResource(R.string.cal_notes), form.description, { v -> edit { it.copy(description = v) } }, singleLine = false) }
        }
    }

    when (picking) {
        Picking.START_DATE -> DatePickerSheet(form.startDate, { picking = null }) { d -> edit { it.withStart(date = d) } }
        Picking.END_DATE -> DatePickerSheet(form.endDate, { picking = null }) { d -> edit { it.withEnd(date = d) } }
        Picking.UNTIL -> DatePickerSheet(form.repeat?.until ?: form.startDate, { picking = null }) { d -> edit { it.withUntil(d) } }
        Picking.START_TIME -> TimePickerSheet(form.startTime, { picking = null }) { t -> edit { it.withStart(time = t) } }
        Picking.END_TIME -> TimePickerSheet(form.endTime, { picking = null }) { t -> edit { it.withEnd(time = t) } }
        null -> Unit
    }
}

/** Collects the rows of one group, so that each knows its place and so its corners. */
private class GroupScope {
    val rows = mutableListOf<@Composable (Shape) -> Unit>()

    fun add(row: @Composable (Shape) -> Unit) {
        rows += row
    }
}

@Composable
private fun Group(modifier: Modifier = Modifier, content: GroupScope.() -> Unit) {
    val scope = GroupScope().apply(content)
    GroupColumn(modifier) {
        scope.rows.forEachIndexed { index, row -> row(Groups.shape(index, scope.rows.size)) }
    }
}

@Composable
private fun RowFrame(shape: Shape, modifier: Modifier = Modifier, content: @Composable RowScope.() -> Unit) {
    Row(
        modifier
            .fillMaxWidth()
            .clip(shape)
            .background(MaterialTheme.colorScheme.surfaceContainerLow)
            .heightIn(min = 56.dp)
            .padding(horizontal = 20.dp, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) { content() }
}

@Composable
private fun TextRow(shape: Shape, label: String, value: String, onValue: (String) -> Unit, singleLine: Boolean, number: Boolean = false) {
    val container = MaterialTheme.colorScheme.surfaceContainerLow
    TextField(
        value = value,
        onValueChange = onValue,
        label = { Text(label) },
        singleLine = singleLine,
        minLines = if (singleLine) 1 else 3,
        shape = shape,
        keyboardOptions = KeyboardOptions(
            capitalization = if (number) KeyboardCapitalization.None else KeyboardCapitalization.Sentences,
            keyboardType = if (number) KeyboardType.Number else KeyboardType.Text,
        ),
        colors = TextFieldDefaults.colors(
            focusedContainerColor = container,
            unfocusedContainerColor = container,
            focusedIndicatorColor = Color.Transparent,
            unfocusedIndicatorColor = Color.Transparent,
        ),
        modifier = Modifier.fillMaxWidth(),
    )
}

/** A row that shows its current value and opens a menu of the others. [items] gets a function that closes the menu. */
@Composable
private fun MenuRow(shape: Shape, label: String, value: String, items: @Composable (close: () -> Unit) -> Unit) {
    var open by remember { mutableStateOf(false) }
    Box {
        RowFrame(shape, Modifier.clickable(role = Role.DropdownList, onClick = { open = true })) {
            Text(label, modifier = Modifier.weight(1f), style = MaterialTheme.typography.bodyLarge)
            Text(value, color = MaterialTheme.colorScheme.primary, style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) { items { open = false } }
    }
}

@Composable
private fun StaticRow(shape: Shape, label: String, value: String) {
    RowFrame(shape) {
        Text(label, style = MaterialTheme.typography.bodyLarge)
        Text(
            value,
            modifier = Modifier.weight(1f),
            textAlign = TextAlign.End,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            style = MaterialTheme.typography.bodyMedium,
        )
    }
}

@Composable
private fun SwitchRow(shape: Shape, label: String, checked: Boolean, onChecked: (Boolean) -> Unit) {
    RowFrame(shape, Modifier.clickable(role = Role.Switch) { onChecked(!checked) }) {
        Text(label, modifier = Modifier.weight(1f), style = MaterialTheme.typography.bodyLarge)
        Switch(checked = checked, onCheckedChange = null)
    }
}

/** A label with the date, and the time when there is one, each a button that opens its picker. */
@Composable
private fun PickerRow(shape: Shape, label: String, date: String, time: String?, onDate: () -> Unit, onTime: () -> Unit) {
    RowFrame(shape) {
        Text(label, modifier = Modifier.weight(1f), style = MaterialTheme.typography.bodyLarge)
        PickerButton(date, onDate)
        if (time != null) PickerButton(time, onTime)
    }
}

@Composable
private fun PickerButton(text: String, onClick: () -> Unit) {
    Surface(
        onClick = onClick,
        shape = RoundedCornerShape(12.dp),
        color = MaterialTheme.colorScheme.surfaceContainerHighest,
        modifier = Modifier.heightIn(min = 40.dp),
    ) {
        Box(Modifier.padding(horizontal = 12.dp), contentAlignment = Alignment.Center) {
            Text(text, style = MaterialTheme.typography.bodyLarge)
        }
    }
}

/** Seven round buttons, the weekdays of a weekly rule, the week starting where the locale starts it. */
@Composable
private fun WeekdayRow(shape: Shape, selected: Set<DayOfWeek>, locale: Locale, onToggle: (DayOfWeek, Boolean) -> Unit) {
    val first = remember(locale) { WeekRules.firstDayOfWeek(locale) }
    val colors = MaterialTheme.colorScheme
    RowFrame(shape) {
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
            for (offset in 0L..6L) {
                val day = first.plus(offset)
                val on = day in selected
                val name = WeekRules.fullName(day, locale)
                Box(
                    Modifier
                        .size(40.dp)
                        .clip(CircleShape)
                        .background(if (on) colors.primary else colors.surfaceContainerHighest)
                        .clickable(role = Role.Checkbox) { onToggle(day, !on) }
                        .semantics {
                            contentDescription = name
                            role = Role.Checkbox
                            this.selected = on
                        },
                    contentAlignment = Alignment.Center,
                ) {
                    Text(
                        WeekRules.initial(day, locale),
                        color = if (on) colors.onPrimary else colors.onSurface,
                        fontWeight = if (on) FontWeight.Bold else FontWeight.Medium,
                        style = MaterialTheme.typography.labelLarge,
                    )
                }
            }
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun DatePickerSheet(initial: LocalDate, onDismiss: () -> Unit, onPick: (LocalDate) -> Unit) {
    val state = rememberDatePickerState(initialSelectedDateMillis = FormChoices.toPickerMillis(initial))
    DatePickerDialog(
        onDismissRequest = onDismiss,
        confirmButton = {
            TextButton(
                onClick = {
                    state.selectedDateMillis?.let { onPick(FormChoices.fromPickerMillis(it)) }
                    onDismiss()
                },
            ) { Text(stringResource(R.string.cal_ok)) }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.cancel)) } },
    ) { DatePicker(state) }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun TimePickerSheet(initial: LocalTime, onDismiss: () -> Unit, onPick: (LocalTime) -> Unit) {
    val is24 = DateFormat.is24HourFormat(LocalContext.current)
    val state = rememberTimePickerState(initialHour = initial.hour, initialMinute = initial.minute, is24Hour = is24)
    Dialog(onDismissRequest = onDismiss) {
        Surface(shape = MaterialTheme.shapes.extraLarge, color = MaterialTheme.colorScheme.surfaceContainerHigh) {
            Column(Modifier.padding(24.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                TimePicker(state)
                Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
                    TextButton(onClick = onDismiss) { Text(stringResource(R.string.cancel)) }
                    TextButton(
                        onClick = {
                            onPick(LocalTime.of(state.hour, state.minute))
                            onDismiss()
                        },
                    ) { Text(stringResource(R.string.cal_ok)) }
                }
            }
        }
    }
}
