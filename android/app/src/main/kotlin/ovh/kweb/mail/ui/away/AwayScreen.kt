package ovh.kweb.mail.ui.away

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Close
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DatePicker
import androidx.compose.material3.DatePickerDialog
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TextField
import androidx.compose.material3.TextFieldDefaults
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.material3.rememberDatePickerState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import java.time.LocalDate
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.calendar.FormChoices
import ovh.kweb.mail.ui.calendar.GroupColumn
import ovh.kweb.mail.ui.calendar.NoticeBar
import ovh.kweb.mail.ui.calendar.rememberLocale
import ovh.kweb.mail.ui.list.Groups

/** Which date picker is on screen. */
private enum class Picking { FROM, UNTIL }

/**
 * The away message: a switch, the days it is for (both optional), the subject and the text, and Save.
 * [onBack] leaves, after a save and when the user lets changes go.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun AwayScreen(onBack: () -> Unit, vm: AwayViewModel = viewModel()) {
    val state by vm.state.collectAsStateWithLifecycle()
    var confirmDiscard by rememberSaveable { mutableStateOf(false) }

    LaunchedEffect(vm) { vm.load() }
    LaunchedEffect(vm) { vm.saved.collect { onBack() } }

    // While a save runs back does nothing: leaving would cancel the coroutine that reports a failure.
    val leave = { if (state.saving) Unit else if (state.dirty) confirmDiscard = true else onBack() }
    BackHandler(onBack = leave)

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(stringResource(R.string.away_title)) },
                navigationIcon = {
                    IconButton(onClick = leave, enabled = !state.saving) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = stringResource(R.string.back))
                    }
                },
                colors = TopAppBarDefaults.topAppBarColors(containerColor = MaterialTheme.colorScheme.background),
            )
        },
        containerColor = MaterialTheme.colorScheme.background,
    ) { padding ->
        Column(Modifier.padding(padding).fillMaxSize()) {
            state.error?.let {
                NoticeBar(it.resolve(LocalResources.current), error = true, action = stringResource(R.string.contacts_dismiss) to vm::dismissError)
            }
            when (state.phase) {
                AwayPhase.READY -> Form(state, vm)
                AwayPhase.LOADING -> Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) { CircularProgressIndicator() }
                AwayPhase.FAILED -> Column(
                    Modifier.fillMaxSize().padding(32.dp),
                    verticalArrangement = Arrangement.Center,
                    horizontalAlignment = Alignment.CenterHorizontally,
                ) {
                    Text(stringResource(R.string.away_load_failed), color = MaterialTheme.colorScheme.onSurfaceVariant, textAlign = TextAlign.Center)
                    TextButton(onClick = vm::load) { Text(stringResource(R.string.retry)) }
                }
            }
        }
    }

    if (confirmDiscard) {
        AlertDialog(
            onDismissRequest = { confirmDiscard = false },
            title = { Text(stringResource(R.string.away_discard_title)) },
            text = { Text(stringResource(R.string.away_discard_text)) },
            confirmButton = {
                TextButton(onClick = {
                    confirmDiscard = false
                    onBack()
                }) { Text(stringResource(R.string.away_discard)) }
            },
            dismissButton = { TextButton(onClick = { confirmDiscard = false }) { Text(stringResource(R.string.away_keep_editing)) } },
        )
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun Form(state: AwayState, vm: AwayViewModel) {
    val form = state.form
    val locale = rememberLocale()
    val dateFormat = remember(locale) { DateTimeFormatter.ofLocalizedDate(FormatStyle.MEDIUM).withLocale(locale) }
    var picking by rememberSaveable { mutableStateOf<Picking?>(null) }

    Column(Modifier.fillMaxSize().imePadding().verticalScroll(rememberScrollState()).padding(bottom = 32.dp)) {
        GroupColumn {
            val shape = Groups.shape(0, 1)
            RowFrame(shape, Modifier.toggleable(value = form.enabled, role = Role.Switch, onValueChange = { v -> vm.edit { it.copy(enabled = v) } })) {
                Column(Modifier.weight(1f)) {
                    Text(stringResource(R.string.away_switch), style = MaterialTheme.typography.bodyLarge)
                    Text(
                        stringResource(R.string.away_switch_detail),
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        style = MaterialTheme.typography.bodyMedium,
                    )
                }
                Switch(checked = form.enabled, onCheckedChange = null)
            }
        }
        if (state.filtersOn) {
            Text(
                stringResource(R.string.filters_away_line),
                modifier = Modifier.padding(horizontal = 28.dp, vertical = 8.dp),
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                style = MaterialTheme.typography.bodyMedium,
            )
        }
        GroupColumn(Modifier.padding(top = 16.dp)) {
            DateRow(Groups.shape(0, 2), stringResource(R.string.away_from), form.from, stringResource(R.string.away_from_unset), dateFormat, { picking = Picking.FROM }) {
                vm.setFrom(null)
            }
            DateRow(Groups.shape(1, 2), stringResource(R.string.away_until), form.until, stringResource(R.string.away_until_unset), dateFormat, { picking = Picking.UNTIL }) {
                vm.setUntil(null)
            }
        }
        GroupColumn(Modifier.padding(top = 16.dp)) {
            FieldRow(Groups.shape(0, 2), stringResource(R.string.away_subject), form.subject, { v -> vm.edit { it.copy(subject = v) } }, singleLine = true)
            FieldRow(Groups.shape(1, 2), stringResource(R.string.away_text), form.text, { v -> vm.edit { it.copy(text = v) } }, singleLine = false)
        }
        // The main action: a filled pill, 52 dp high.
        Button(
            onClick = vm::save,
            enabled = !state.saving,
            shape = CircleShape,
            modifier = Modifier.fillMaxWidth().padding(horizontal = 12.dp).padding(top = 24.dp).height(52.dp),
        ) {
            if (state.saving) {
                CircularProgressIndicator(Modifier.size(20.dp), strokeWidth = 2.dp, color = MaterialTheme.colorScheme.onPrimary)
            } else {
                Text(stringResource(R.string.away_save), style = MaterialTheme.typography.labelLarge)
            }
        }
    }

    when (picking) {
        Picking.FROM -> DatePickerSheet(form.from ?: LocalDate.now(state.zone), { picking = null }) { vm.setFrom(it) }
        Picking.UNTIL -> DatePickerSheet(form.until ?: form.from ?: LocalDate.now(state.zone), { picking = null }) { vm.setUntil(it) }
        null -> Unit
    }
}

@Composable
private fun RowFrame(shape: Shape, modifier: Modifier = Modifier, content: @Composable androidx.compose.foundation.layout.RowScope.() -> Unit) {
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

/** A label, the day as a button that opens the picker, and a button that clears it (shown only when a day is set). */
@Composable
private fun DateRow(shape: Shape, label: String, date: LocalDate?, unset: String, format: DateTimeFormatter, onPick: () -> Unit, onClear: () -> Unit) {
    RowFrame(shape) {
        Text(label, modifier = Modifier.weight(1f), style = MaterialTheme.typography.bodyLarge)
        Surface(
            onClick = onPick,
            shape = RoundedCornerShape(12.dp),
            color = MaterialTheme.colorScheme.surfaceContainerHighest,
            modifier = Modifier.heightIn(min = 40.dp),
        ) {
            Box(Modifier.padding(horizontal = 12.dp), contentAlignment = Alignment.Center) {
                Text(date?.let(format::format) ?: unset, style = MaterialTheme.typography.bodyLarge)
            }
        }
        if (date != null) {
            IconButton(onClick = onClear) { Icon(Icons.Filled.Close, contentDescription = stringResource(R.string.away_clear_date, label)) }
        }
    }
}

@Composable
private fun FieldRow(shape: Shape, label: String, value: String, onValue: (String) -> Unit, singleLine: Boolean) {
    val container = MaterialTheme.colorScheme.surfaceContainerLow
    TextField(
        value = value,
        onValueChange = onValue,
        label = { Text(label) },
        singleLine = singleLine,
        minLines = if (singleLine) 1 else 6,
        shape = shape,
        keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Sentences),
        colors = TextFieldDefaults.colors(
            focusedContainerColor = container,
            unfocusedContainerColor = container,
            focusedIndicatorColor = Color.Transparent,
            unfocusedIndicatorColor = Color.Transparent,
        ),
        modifier = Modifier.fillMaxWidth(),
    )
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
            ) { Text(stringResource(R.string.away_ok)) }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.cancel)) } },
    ) { DatePicker(state) }
}
