package ovh.kweb.mail.ui.notifications

import android.content.ActivityNotFoundException
import android.content.Context
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.RadioButton
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
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalResources
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
import ovh.kweb.mail.ui.list.Groups
import ovh.kweb.mail.ui.theme.KwebSwitch
import uniffi.kweb_core.AddressChoice
import uniffi.kweb_core.AddressMode

/**
 * What notifies, and how, for the account that is shown: new mail on or off, the sound (Android keeps that
 * per channel, so the row opens the system's own screen for the account's channel), what happens to mail by
 * the address it came to, and the reminders. The switches are the ones the Delivery check had; there is one
 * state and this is its only screen.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun NotificationsScreen(onBack: () -> Unit, onOpenFolders: () -> Unit, vm: NotificationsViewModel = viewModel()) {
    val state by vm.state.collectAsStateWithLifecycle()
    val snackbar = remember { SnackbarHostState() }
    val resources = LocalResources.current
    val context = LocalContext.current
    var choosing by remember { mutableStateOf<AddressChoice?>(null) }

    LifecycleResumeEffect(vm) {
        vm.load()
        onPauseOrDispose { }
    }
    LaunchedEffect(vm) { vm.messages.collect { snackbar.showSnackbar(resources.getString(it)) } }
    val openChannel = { kind: ChannelKind -> if (!startSafely(context, vm, kind)) vm.noChannelScreen() }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(stringResource(R.string.notifications_title)) },
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
            state.account?.let { Title(stringResource(R.string.notifications_for, it)) }

            Title(stringResource(R.string.notifications_mail))
            Column(Modifier.padding(horizontal = 16.dp), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                SwitchRow(Groups.shape(0, 4), stringResource(R.string.delivery_show_mail), null, state.mailEnabled, vm::setMailEnabled)
                SwitchRow(
                    Groups.shape(1, 4),
                    stringResource(R.string.delivery_remove_elsewhere),
                    stringResource(R.string.notifications_remove_detail),
                    state.removeWhenReadElsewhere,
                    vm::setRemoveWhenReadElsewhere,
                )
                LinkRow(Groups.shape(2, 4), stringResource(R.string.notifications_sound), stringResource(R.string.notifications_sound_detail)) {
                    openChannel(ChannelKind.Mail)
                }
                LinkRow(Groups.shape(3, 4), stringResource(R.string.notifications_folders), stringResource(R.string.notifications_folders_detail), onOpenFolders)
            }

            Title(stringResource(R.string.notifications_addresses))
            Text(
                stringResource(R.string.notifications_addresses_text),
                modifier = Modifier.padding(horizontal = 32.dp, vertical = 4.dp),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            val addresses = state.addresses
            Column(Modifier.padding(horizontal = 16.dp, vertical = 8.dp), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                if (addresses.isNullOrEmpty()) {
                    LinkRow(
                        Groups.shape(0, 1),
                        stringResource(if (addresses == null) R.string.notifications_addresses_loading else R.string.notifications_addresses_none),
                        null,
                        null,
                    )
                } else {
                    addresses.forEachIndexed { i, choice ->
                        LinkRow(
                            Groups.shape(i, addresses.size + 1),
                            choice.name.ifBlank { choice.address },
                            (if (choice.name.isBlank()) "" else choice.address + " · ") + stringResource(modeName(choice.mode)),
                        ) { choosing = choice }
                    }
                    LinkRow(Groups.shape(addresses.size, addresses.size + 1), stringResource(R.string.notifications_quiet_sound), stringResource(R.string.notifications_quiet_detail)) {
                        openChannel(ChannelKind.Quiet)
                    }
                }
            }

            Title(stringResource(R.string.notifications_reminders))
            Column(Modifier.padding(horizontal = 16.dp), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                SwitchRow(Groups.shape(0, 2), stringResource(R.string.delivery_show_reminders), null, state.remindersEnabled, vm::setRemindersEnabled)
                LinkRow(Groups.shape(1, 2), stringResource(R.string.notifications_reminder_sound), stringResource(R.string.notifications_reminder_sound_detail)) {
                    openChannel(ChannelKind.Reminders)
                }
            }
        }
    }

    choosing?.let { choice ->
        AlertDialog(
            onDismissRequest = { choosing = null },
            title = { Text(choice.name.ifBlank { choice.address }) },
            text = {
                Column {
                    Text(
                        choice.address,
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.padding(bottom = 8.dp),
                    )
                    NotificationsRules.modes.forEach { mode ->
                        Row(
                            Modifier
                                .fillMaxWidth()
                                .selectable(selected = choice.mode == mode, role = Role.RadioButton, onClick = {
                                    vm.setAddress(choice.address, mode)
                                    choosing = null
                                })
                                .heightIn(min = 48.dp),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            RadioButton(selected = choice.mode == mode, onClick = null)
                            Column(Modifier.padding(start = 12.dp)) {
                                Text(stringResource(modeName(mode)), style = MaterialTheme.typography.bodyLarge)
                                Text(
                                    stringResource(modeDetail(mode)),
                                    style = MaterialTheme.typography.bodySmall,
                                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                                )
                            }
                        }
                    }
                }
            },
            confirmButton = { TextButton(onClick = { choosing = null }) { Text(stringResource(R.string.cancel)) } },
        )
    }
}

private fun modeName(mode: AddressMode): Int = when (mode) {
    AddressMode.NOTIFY -> R.string.notifications_mode_notify
    AddressMode.SILENT -> R.string.notifications_mode_silent
    AddressMode.OFF -> R.string.notifications_mode_off
}

private fun modeDetail(mode: AddressMode): Int = when (mode) {
    AddressMode.NOTIFY -> R.string.notifications_mode_notify_detail
    AddressMode.SILENT -> R.string.notifications_mode_silent_detail
    AddressMode.OFF -> R.string.notifications_mode_off_detail
}

/** Opens the system's settings for a channel; false when the phone has no such screen. */
private fun startSafely(context: Context, vm: NotificationsViewModel, kind: ChannelKind): Boolean = try {
    context.startActivity(vm.channelSettings(kind))
    true
} catch (_: ActivityNotFoundException) {
    false
} catch (_: SecurityException) {
    false
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
private fun TwoLines(title: String, detail: String?, modifier: Modifier = Modifier) {
    Column(modifier) {
        Text(title, style = MaterialTheme.typography.bodyLarge, color = MaterialTheme.colorScheme.onSurface)
        if (detail != null) {
            Text(detail, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 2, overflow = TextOverflow.Ellipsis)
        }
    }
}

@Composable
private fun SwitchRow(shape: Shape, title: String, detail: String?, checked: Boolean, onChange: (Boolean) -> Unit) {
    RowBox(shape, Modifier.toggleable(value = checked, role = Role.Switch, onValueChange = onChange)) {
        TwoLines(title, detail, Modifier.weight(1f))
        KwebSwitch(checked = checked, onCheckedChange = null)
    }
}

/** A row that opens something; [onClick] null makes it a plain line. */
@Composable
private fun LinkRow(shape: Shape, title: String, detail: String?, onClick: (() -> Unit)?) {
    RowBox(shape, if (onClick != null) Modifier.clickable(role = Role.Button, onClick = onClick) else Modifier) {
        TwoLines(title, detail, Modifier.weight(1f))
    }
}
