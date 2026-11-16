package ovh.kweb.mail.ui.devcal

import android.content.Intent
import android.net.Uri
import android.provider.Settings
import android.text.format.DateUtils
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.shape.CircleShape
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
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
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
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.intl.Locale
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.LifecycleResumeEffect
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import ovh.kweb.mail.R
import ovh.kweb.mail.devcal.DeviceCalendarRules
import ovh.kweb.mail.devcal.DeviceRun
import ovh.kweb.mail.ui.list.Groups
import ovh.kweb.mail.ui.theme.KwebSwitch

/**
 * Device calendar, for the account that is shown: one switch for showing the account's own calendars in the phone's calendar
 * apps, one switch for each calendar, and how the last sync went. The calendar permission is asked for when the switch is
 * turned on, after a dialog that says what will be written where; nothing is copied before.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun DeviceCalendarScreen(onBack: () -> Unit, vm: DeviceCalendarViewModel = viewModel()) {
    val state by vm.state.collectAsStateWithLifecycle()
    val blocked by vm.blocked.collectAsStateWithLifecycle()
    val snackbar = remember { SnackbarHostState() }
    val resources = LocalResources.current
    val context = LocalContext.current
    var explaining by remember { mutableStateOf(false) }
    var denied by remember { mutableStateOf(false) }
    val permissionLauncher = rememberLauncherForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) { result ->
        val granted = DeviceCalendarRules.PERMISSIONS.all { result[it] == true }
        denied = !granted
        vm.permissionResult(granted)
    }

    LifecycleResumeEffect(vm) {
        vm.load()
        onPauseOrDispose { }
    }
    LaunchedEffect(vm) { vm.messages.collect { snackbar.showSnackbar(resources.getString(it)) } }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(stringResource(R.string.devcal_title)) },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = stringResource(R.string.back))
                    }
                },
                colors = TopAppBarDefaults.topAppBarColors(containerColor = MaterialTheme.colorScheme.background),
            )
        },
        snackbarHost = { SnackbarHost(snackbar) },
        containerColor = MaterialTheme.colorScheme.background,
    ) { padding ->
        Column(Modifier.padding(padding).fillMaxWidth().verticalScroll(rememberScrollState()).padding(bottom = 24.dp)) {
            state.account?.let { Title(stringResource(R.string.devcal_for, it)) }
            Column(Modifier.padding(horizontal = 16.dp, vertical = 8.dp), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                RowBox(
                    Groups.shape(0, 1),
                    Modifier.toggleable(
                        value = state.on,
                        enabled = !state.busy,
                        role = Role.Switch,
                        onValueChange = { on ->
                            if (!on) {
                                vm.turnOff()
                            } else if (state.permission) {
                                vm.turnOn()
                            } else {
                                explaining = true
                            }
                        },
                    ),
                ) {
                    TwoLines(stringResource(R.string.devcal_switch), stringResource(R.string.devcal_switch_detail), Modifier.weight(1f))
                    if (state.busy) CircularProgressIndicator(Modifier.size(24.dp), strokeWidth = 2.dp) else KwebSwitch(checked = state.on, onCheckedChange = null)
                }
            }
            if (denied && !state.permission) {
                Text(
                    stringResource(R.string.devcal_permission_denied),
                    modifier = Modifier.padding(horizontal = 32.dp, vertical = 4.dp),
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.error,
                )
                TextButton(onClick = { context.startActivity(appSettings(context.packageName)) }, modifier = Modifier.padding(horizontal = 24.dp)) {
                    Text(stringResource(R.string.devcal_permission_settings))
                }
            }

            if (state.on) {
                Title(stringResource(R.string.devcal_calendars))
                Text(
                    stringResource(R.string.devcal_calendars_text),
                    modifier = Modifier.padding(horizontal = 32.dp, vertical = 4.dp),
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                val calendars = state.calendars
                Column(Modifier.padding(horizontal = 16.dp, vertical = 8.dp), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                    when {
                        calendars == null -> LineRow(Groups.shape(0, 1), stringResource(R.string.devcal_calendars_loading))
                        calendars.isEmpty() -> LineRow(Groups.shape(0, 1), stringResource(R.string.devcal_calendars_none))
                        else -> calendars.forEachIndexed { index, calendar ->
                            RowBox(
                                Groups.shape(index, calendars.size),
                                Modifier.toggleable(value = calendar.on, role = Role.Switch, onValueChange = { vm.setCalendar(calendar.id, it) }),
                            ) {
                                Dot(calendar.color)
                                Text(calendar.name, Modifier.weight(1f), style = MaterialTheme.typography.bodyLarge, maxLines = 2, overflow = TextOverflow.Ellipsis)
                                KwebSwitch(checked = calendar.on, onCheckedChange = null)
                            }
                        }
                    }
                }

                Title(stringResource(R.string.devcal_last))
                Column(Modifier.padding(horizontal = 16.dp, vertical = 8.dp), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                    RowBox(Groups.shape(0, 2), Modifier) {
                        Column(Modifier.weight(1f)) {
                            Text(lastTime(state.lastRunAt), style = MaterialTheme.typography.bodyLarge)
                            val run = state.lastRun
                            if (run != null) {
                                Text(
                                    runLine(run),
                                    style = MaterialTheme.typography.bodyMedium,
                                    color = if (run.good || run.kind == DeviceRun.Kind.Failed) MaterialTheme.colorScheme.onSurfaceVariant else MaterialTheme.colorScheme.error,
                                )
                                extraLines(run).forEach {
                                    Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                                }
                            }
                            if (state.pausedInAndroid) {
                                Text(stringResource(R.string.devcal_android_off), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.error)
                            }
                        }
                        if (state.syncing) CircularProgressIndicator(Modifier.size(24.dp), strokeWidth = 2.dp)
                    }
                    RowBox(Groups.shape(1, 2), Modifier.clickable(enabled = !state.syncing, role = Role.Button, onClick = vm::syncNow)) {
                        Text(
                            stringResource(if (state.syncing) R.string.devcal_syncing else R.string.devcal_sync_now),
                            style = MaterialTheme.typography.bodyLarge,
                            color = MaterialTheme.colorScheme.primary,
                        )
                    }
                }
            }

            Title(stringResource(R.string.devcal_how))
            Text(
                stringResource(R.string.devcal_how_text),
                modifier = Modifier.padding(horizontal = 32.dp, vertical = 4.dp),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }

    blocked?.let { stuck ->
        AlertDialog(
            onDismissRequest = vm::keepOn,
            title = { Text(stringResource(R.string.devcal_block_title)) },
            text = {
                Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    Text(
                        if (stuck.waiting > 0) {
                            pluralStringResource(R.plurals.devcal_block_text, stuck.waiting, stuck.waiting)
                        } else {
                            stringResource(R.string.devcal_block_text_unknown)
                        },
                    )
                    Text(blockReason(stuck.run), color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
            },
            confirmButton = { TextButton(onClick = vm::keepOn) { Text(stringResource(R.string.devcal_block_keep)) } },
            dismissButton = { TextButton(onClick = { vm.turnOff(force = true) }) { Text(stringResource(R.string.devcal_block_remove)) } },
        )
    }

    if (explaining) {
        AlertDialog(
            onDismissRequest = { explaining = false },
            title = { Text(stringResource(R.string.devcal_permission_title)) },
            text = { Text(stringResource(R.string.devcal_permission_text)) },
            confirmButton = {
                TextButton(onClick = {
                    explaining = false
                    permissionLauncher.launch(DeviceCalendarRules.PERMISSIONS)
                }) { Text(stringResource(R.string.devcal_permission_continue)) }
            },
            dismissButton = { TextButton(onClick = { explaining = false }) { Text(stringResource(R.string.cancel)) } },
        )
    }
}

private fun appSettings(packageName: String): Intent =
    Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.fromParts("package", packageName, null)).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)

@Composable
private fun lastTime(at: Long): String =
    if (at <= 0L) stringResource(R.string.devcal_never) else DateUtils.getRelativeTimeSpanString(at, System.currentTimeMillis(), DateUtils.MINUTE_IN_MILLIS).toString()

/** The first line a run leaves: what it did, or why it stopped. */
@Composable
private fun runLine(run: DeviceRun): String = when (run.kind) {
    DeviceRun.Kind.Ok -> {
        val (written, sent) = run.numbers.getOrElse(0) { 0 } to run.numbers.getOrElse(1) { 0 }
        if (written == 0 && sent == 0) stringResource(R.string.devcal_result_ok) else stringResource(R.string.devcal_result_ok_counts, written, sent)
    }
    DeviceRun.Kind.StoppedDeletions -> stringResource(R.string.devcal_result_deletions, run.numbers.getOrElse(0) { 0 })
    DeviceRun.Kind.StoppedEmpty -> stringResource(R.string.devcal_result_empty, run.numbers.getOrElse(0) { 0 })
    DeviceRun.Kind.Denied -> stringResource(R.string.devcal_result_denied)
    DeviceRun.Kind.SignIn -> stringResource(R.string.devcal_result_signin)
    DeviceRun.Kind.Failed -> stringResource(R.string.devcal_result_failed)
    DeviceRun.Kind.Other -> stringResource(R.string.devcal_result_other, stringResource(R.string.devcal_result_failed))
}

/** Why edits made on the phone were not sent, in a line for the dialog that offers to remove the calendars anyway. */
@Composable
private fun blockReason(run: DeviceRun): String = stringResource(
    when (run.kind) {
        DeviceRun.Kind.Failed -> R.string.devcal_block_failed
        DeviceRun.Kind.SignIn -> R.string.devcal_block_signin
        DeviceRun.Kind.Denied -> R.string.devcal_block_denied
        DeviceRun.Kind.StoppedDeletions, DeviceRun.Kind.StoppedEmpty -> R.string.devcal_block_stopped
        DeviceRun.Kind.Ok, DeviceRun.Kind.Other -> R.string.devcal_block_other
    },
)

/** What a good run did that the user should know: edits that were put back and changes taken from the server. */
@Composable
private fun extraLines(run: DeviceRun): List<String> {
    if (run.kind != DeviceRun.Kind.Ok) return emptyList()
    val (conflicts, reverted, notSent) = Triple(run.numbers.getOrElse(2) { 0 }, run.numbers.getOrElse(3) { 0 }, run.numbers.getOrElse(4) { 0 })
    return listOfNotNull(
        stringResource(R.string.devcal_result_conflicts, conflicts).takeIf { conflicts > 0 },
        stringResource(R.string.devcal_result_reverted, reverted).takeIf { reverted > 0 },
        stringResource(R.string.devcal_result_not_sent, notSent).takeIf { notSent > 0 },
    )
}

@Composable
private fun Title(text: String) {
    Text(
        text.uppercase(Locale.current.platformLocale),
        modifier = Modifier.fillMaxWidth().padding(start = 32.dp, end = 20.dp, top = 20.dp, bottom = 8.dp),
        style = MaterialTheme.typography.labelMedium,
        fontWeight = FontWeight.Bold,
        letterSpacing = 0.8.sp,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
    )
}

@Composable
private fun RowBox(shape: Shape, modifier: Modifier, content: @Composable RowScope.() -> Unit) {
    Row(
        Modifier
            .fillMaxWidth()
            .clip(shape)
            .background(MaterialTheme.colorScheme.surfaceContainerLow)
            .then(modifier)
            .heightIn(min = 56.dp)
            .padding(horizontal = 20.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(16.dp),
        content = content,
    )
}

@Composable
private fun LineRow(shape: Shape, text: String) {
    RowBox(shape, Modifier) { Text(text, style = MaterialTheme.typography.bodyLarge, color = MaterialTheme.colorScheme.onSurfaceVariant) }
}

@Composable
private fun TwoLines(title: String, detail: String?, modifier: Modifier = Modifier) {
    Column(modifier) {
        Text(title, style = MaterialTheme.typography.bodyLarge, color = MaterialTheme.colorScheme.onSurface)
        if (detail != null) {
            Text(detail, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
    }
}

/** The calendar's own colour, as the server has it: a CSS colour ("#3a7bd5") or none (then a neutral dot). */
@Composable
private fun Dot(css: String?) {
    val parsed = css?.let { parseCss(it) }
    Box(Modifier.size(14.dp).clip(CircleShape).background(parsed ?: MaterialTheme.colorScheme.outline))
}

private fun parseCss(css: String): Color? {
    val hex = css.removePrefix("#")
    val value = when (hex.length) {
        6 -> hex.toLongOrNull(16)
        3 -> hex.toLongOrNull(16)?.let { v -> ((v shr 8 and 0xF) * 17 shl 16) or ((v shr 4 and 0xF) * 17 shl 8) or ((v and 0xF) * 17) }
        else -> null
    } ?: return null
    return Color(0xFF000000L or value)
}
