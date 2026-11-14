package ovh.kweb.mail.ui.sending

import android.text.format.DateFormat
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ArrowDropDown
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.DatePicker
import androidx.compose.material3.DatePickerDialog
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.SelectableDates
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TimePicker
import androidx.compose.material3.rememberDatePickerState
import androidx.compose.material3.rememberTimePickerState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import java.time.Instant
import java.time.LocalDate
import java.time.ZoneId
import java.util.Locale
import ovh.kweb.mail.R

/**
 * The small arrow beside Send: a menu with the quick times and "Pick date and time", which asks for a day and
 * then a clock time. [onChoose] gets the Unix second. The step of the pickers and the day survive a rotation.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SendLaterMenu(enabled: Boolean, onChoose: (Long) -> Unit) {
    var open by remember { mutableStateOf(false) }
    // 0: no picker, 1: the day, 2: the time.
    var step by rememberSaveable { mutableIntStateOf(0) }
    var pickedDay by rememberSaveable { mutableLongStateOf(0L) }
    val zone = remember { ZoneId.systemDefault() }
    val locale = remember { Locale.getDefault() }
    val resources = LocalResources.current

    Box {
        IconButton(onClick = { open = true }, enabled = enabled) {
            Icon(Icons.Filled.ArrowDropDown, contentDescription = stringResource(R.string.send_later_menu))
        }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            // Worked out each time the menu opens, so "in 1 hour" is an hour from the moment it is shown.
            val now = remember(open) { Instant.now() }
            Text(
                stringResource(R.string.send_later_title),
                modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp),
                style = MaterialTheme.typography.labelLarge,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            for (choice in remember(open) { SendLaterRules.choices(now, zone) }) {
                DropdownMenuItem(
                    text = {
                        Column {
                            Text(
                                stringResource(
                                    when (choice.kind) {
                                        SendLaterRules.Kind.InOneHour -> R.string.send_later_in_one_hour
                                        SendLaterRules.Kind.ThisEvening -> R.string.send_later_this_evening
                                        SendLaterRules.Kind.TomorrowMorning -> R.string.send_later_tomorrow_morning
                                    },
                                ),
                            )
                            Text(
                                SendLaterWords.whenText(resources, choice.at, now.epochSecond, zone, locale),
                                style = MaterialTheme.typography.bodySmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                    },
                    onClick = {
                        open = false
                        onChoose(choice.at)
                    },
                )
            }
            HorizontalDivider()
            DropdownMenuItem(
                text = {
                    Column {
                        Text(stringResource(R.string.send_later_pick))
                        Text(
                            stringResource(R.string.send_later_limit),
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                },
                onClick = {
                    open = false
                    pickedDay = SendLaterRules.defaultPick(Instant.now(), zone).first.toEpochDay()
                    step = 1
                },
            )
        }
    }

    if (step == 1) {
        val today = remember { LocalDate.now(zone) }
        val state = rememberDatePickerState(
            initialSelectedDateMillis = pickedDay * MILLIS_PER_DAY,
            selectableDates = remember(today) {
                object : SelectableDates {
                    override fun isSelectableDate(utcTimeMillis: Long) =
                        SendLaterRules.dateAllowed(LocalDate.ofEpochDay(Math.floorDiv(utcTimeMillis, MILLIS_PER_DAY)), today)

                    override fun isSelectableYear(year: Int) = year in today.year..today.plusDays(SendLaterRules.MAX_DAYS).year
                }
            },
        )
        DatePickerDialog(
            onDismissRequest = { step = 0 },
            confirmButton = {
                TextButton(
                    onClick = {
                        state.selectedDateMillis?.let { pickedDay = Math.floorDiv(it, MILLIS_PER_DAY) }
                        step = if (state.selectedDateMillis != null) 2 else 0
                    },
                    enabled = state.selectedDateMillis != null,
                ) { Text(stringResource(R.string.send_later_next)) }
            },
            dismissButton = { TextButton(onClick = { step = 0 }) { Text(stringResource(R.string.cancel)) } },
        ) {
            DatePicker(state, title = {
                Text(stringResource(R.string.send_later_limit), modifier = Modifier.padding(start = 24.dp, end = 24.dp, top = 16.dp), style = MaterialTheme.typography.labelLarge)
            })
        }
    }
    if (step == 2) {
        val initial = remember { SendLaterRules.defaultPick(Instant.now(), zone).second }
        val context = LocalContext.current
        val time = rememberTimePickerState(initial.hour, initial.minute, is24Hour = DateFormat.is24HourFormat(context))
        var problem by rememberSaveable { mutableIntStateOf(0) }
        AlertDialog(
            onDismissRequest = { step = 0 },
            title = { Text(stringResource(R.string.send_later_title)) },
            text = {
                Column {
                    TimePicker(time)
                    if (problem != 0) Text(stringResource(problem), color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodyMedium)
                }
            },
            confirmButton = {
                TextButton(onClick = {
                    val at = SendLaterRules.at(LocalDate.ofEpochDay(pickedDay), time.hour, time.minute, zone)
                    when (SendLaterRules.check(at, System.currentTimeMillis() / 1000)) {
                        SendLaterRules.Check.Ok -> {
                            step = 0
                            onChoose(at)
                        }
                        SendLaterRules.Check.TooSoon -> problem = R.string.send_later_too_soon
                        SendLaterRules.Check.TooFar -> problem = R.string.send_later_too_far
                    }
                }) { Text(stringResource(R.string.send_later_schedule)) }
            },
            dismissButton = { TextButton(onClick = { step = 1 }) { Text(stringResource(R.string.back)) } },
        )
    }
}

private const val MILLIS_PER_DAY = 86_400_000L
