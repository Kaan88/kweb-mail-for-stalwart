package ovh.kweb.mail.ui.settings

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.ExitToApp
import androidx.compose.material.icons.filled.Notifications
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Text
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
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.intl.Locale as ComposeLocale
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.LifecycleResumeEffect
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import java.util.Locale
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.assistant.AssistantRules
import ovh.kweb.mail.ui.away.AwayLine
import ovh.kweb.mail.ui.away.AwayRules
import ovh.kweb.mail.ui.delivery.DeliveryViewModel
import ovh.kweb.mail.ui.delivery.SignOutDialog
import ovh.kweb.mail.ui.list.Groups
import ovh.kweb.mail.ui.theme.ThemePickerDialog
import ovh.kweb.mail.ui.theme.ThemeState
import ovh.kweb.mail.ui.theme.themeName
import uniffi.kweb_core.Quota

/**
 * The settings of the app in grouped rows: the account, how much storage is used, the theme, the
 * Delivery check and Sign out. Signing out is the Delivery check's own flow ([DeliveryViewModel]),
 * not a copy of it.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SettingsScreen(
    onBack: () -> Unit,
    onOpenDeliveryCheck: () -> Unit,
    onOpenAway: () -> Unit,
    onOpenFilters: () -> Unit,
    onOpenAssistant: () -> Unit,
    vm: SettingsViewModel = viewModel(),
    delivery: DeliveryViewModel = viewModel(),
) {
    val state by vm.state.collectAsStateWithLifecycle()
    val deliveryState by delivery.state.collectAsStateWithLifecycle()
    val theme by ThemeState.choice.collectAsStateWithLifecycle()
    val snackbar = remember { SnackbarHostState() }
    val resources = LocalResources.current
    var pickingTheme by remember { mutableStateOf(false) }
    val signOutPrompt by delivery.signOutPrompt.collectAsStateWithLifecycle()

    LifecycleResumeEffect(vm) {
        vm.load()
        onPauseOrDispose { }
    }
    // "Signing out did not work" comes back from the shared flow.
    LaunchedEffect(delivery) { delivery.messages.collect { snackbar.showSnackbar(resources.getString(it)) } }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(stringResource(R.string.settings_title)) },
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
            SectionTitle(stringResource(R.string.settings_account))
            Group {
                SettingsRow(Groups.shape(0, 2), stringResource(R.string.settings_name), state.name.ifEmpty { "…" })
                SettingsRow(Groups.shape(1, 2), stringResource(R.string.settings_server), state.server.ifEmpty { "…" })
            }

            SectionTitle(stringResource(R.string.settings_storage))
            StorageGroup(state.storage)

            SectionTitle(stringResource(R.string.settings_mail))
            Group {
                SettingsRow(
                    Groups.shape(0, 3),
                    stringResource(R.string.away_title),
                    awayLine(state.away),
                    icon = { Icon(painterResource(R.drawable.ic_mail), contentDescription = null) },
                    onClick = onOpenAway,
                )
                SettingsRow(
                    Groups.shape(1, 3),
                    stringResource(R.string.filters_title),
                    filtersLine(state.filters),
                    icon = { Icon(painterResource(R.drawable.ic_mail), contentDescription = null) },
                    onClick = onOpenFilters,
                )
                SettingsRow(
                    Groups.shape(2, 3),
                    stringResource(R.string.assistant_title),
                    assistantLine(state.assistant),
                    icon = { Icon(painterResource(R.drawable.ic_assistant), contentDescription = null) },
                    onClick = onOpenAssistant,
                )
            }

            SectionTitle(stringResource(R.string.settings_app))
            Group {
                SettingsRow(
                    Groups.shape(0, 2),
                    stringResource(R.string.theme_title),
                    stringResource(themeName(theme)),
                    icon = { Icon(painterResource(R.drawable.ic_palette), contentDescription = null) },
                    onClick = { pickingTheme = true },
                )
                SettingsRow(
                    Groups.shape(1, 2),
                    stringResource(R.string.delivery_check_title),
                    stringResource(R.string.settings_delivery_detail),
                    icon = { Icon(Icons.Filled.Notifications, contentDescription = null) },
                    onClick = onOpenDeliveryCheck,
                )
            }

            Group(Modifier.padding(top = 16.dp)) {
                SettingsRow(
                    Groups.shape(0, 1),
                    stringResource(R.string.sign_out),
                    if (deliveryState.signingOut) stringResource(R.string.settings_signing_out) else null,
                    icon = { Icon(Icons.AutoMirrored.Filled.ExitToApp, contentDescription = null) },
                    titleColor = MaterialTheme.colorScheme.error,
                    onClick = if (deliveryState.signingOut) null else ({ delivery.askSignOut() }),
                )
            }
        }
    }

    if (pickingTheme) ThemePickerDialog(onDismiss = { pickingTheme = false })
    signOutPrompt?.let { SignOutDialog(it, onConfirm = delivery::signOut, onDismiss = delivery::dismissSignOutPrompt) }
}

/** The quiet line of the away message row: "Off", "On", "On until 21 Nov"; nothing while it is read or when it cannot be. */
@Composable
private fun awayLine(state: AwayRowState): String? {
    val line = (state as? AwayRowState.Known)?.line ?: return null
    val locale = remember { Locale.getDefault() }
    return when (line) {
        AwayLine.Off -> stringResource(R.string.away_state_off)
        AwayLine.On -> stringResource(R.string.away_state_on)
        is AwayLine.Ended -> stringResource(R.string.away_state_ended, AwayRules.shortDate(line.until, locale))
        is AwayLine.OnUntil -> stringResource(R.string.away_state_on_until, AwayRules.shortDate(line.until, locale))
        is AwayLine.OnFrom -> stringResource(R.string.away_state_on_from, AwayRules.shortDate(line.from, locale))
        is AwayLine.OnFromUntil ->
            stringResource(R.string.away_state_on_from_until, AwayRules.shortDate(line.from, locale), AwayRules.shortDate(line.until, locale))
    }
}

/** The quiet line of the filters row: "On" or "Off"; nothing while it is read or when it cannot be. */
@Composable
private fun filtersLine(state: FiltersRowState): String? = when (state) {
    FiltersRowState.On -> stringResource(R.string.filters_state_on)
    FiltersRowState.Off -> stringResource(R.string.filters_state_off)
    else -> null
}

/** The quiet line of the assistant row: "Off", "On", or the model's name; nothing while it is read or when it cannot be. */
@Composable
private fun assistantLine(state: AssistantRowState): String? {
    val line = (state as? AssistantRowState.Known)?.line ?: return null
    return when (line) {
        AssistantRules.StateLine.Off -> stringResource(R.string.assistant_state_off)
        AssistantRules.StateLine.On -> stringResource(R.string.assistant_state_on)
        is AssistantRules.StateLine.Model -> line.name
    }
}

@Composable
private fun StorageGroup(storage: StorageState) {
    when (storage) {
        StorageState.Loading -> Group {
            SettingsRow(Groups.shape(0, 1), stringResource(R.string.settings_storage_loading), trailing = {
                CircularProgressIndicator(Modifier.size(20.dp), strokeWidth = 2.dp)
            })
        }
        StorageState.Unavailable -> Group {
            SettingsRow(Groups.shape(0, 1), stringResource(R.string.settings_storage_unavailable), quiet = true)
        }
        is StorageState.Loaded -> Group {
            if (storage.quotas.isEmpty()) {
                SettingsRow(Groups.shape(0, 1), stringResource(R.string.settings_storage_none), quiet = true)
            }
            storage.quotas.forEachIndexed { index, quota -> QuotaRow(Groups.shape(index, storage.quotas.size), quota) }
        }
    }
}

/** One quota: its name, "x of y used" and a bar that turns to the warning colour above 90 percent. */
@Composable
private fun QuotaRow(shape: Shape, quota: Quota) {
    val colors = MaterialTheme.colorScheme
    val locale = remember { Locale.getDefault() }
    val kind = QuotaRules.kind(quota)
    val title = QuotaRules.name(quota)
        ?: stringResource(if (kind == QuotaRules.Kind.Bytes) R.string.settings_storage_mail else R.string.settings_storage_count)
    val used = QuotaRules.amount(quota.used, kind, locale)
    val limit = quota.limit
    val fraction = QuotaRules.fraction(quota.used, limit)
    val warning = QuotaRules.isWarning(quota.used, limit)
    val text = if (limit == null) {
        stringResource(R.string.quota_no_limit, used)
    } else {
        stringResource(R.string.quota_used_of, used, QuotaRules.amount(limit, kind, locale))
    }
    Column(
        Modifier.fillMaxWidth().clip(shape).background(colors.surfaceContainerLow).padding(horizontal = 20.dp, vertical = 14.dp),
        verticalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        Text(title, style = MaterialTheme.typography.bodyLarge)
        Text(
            text,
            color = if (warning) colors.error else colors.onSurfaceVariant,
            fontWeight = if (warning) FontWeight.Bold else null,
            style = MaterialTheme.typography.bodyMedium,
        )
        if (fraction != null) {
            LinearProgressIndicator(
                progress = { fraction },
                modifier = Modifier.fillMaxWidth().padding(top = 6.dp).height(8.dp).clip(CircleShape),
                color = if (warning) colors.error else colors.primary,
                trackColor = colors.surfaceContainerHighest,
                gapSize = 0.dp,
                drawStopIndicator = {},
            )
        }
    }
}

/** The rows of one group, a small gap apart. */
@Composable
private fun Group(modifier: Modifier = Modifier, content: @Composable () -> Unit) {
    Column(modifier.padding(horizontal = 12.dp), verticalArrangement = Arrangement.spacedBy(3.dp)) { content() }
}

/** One row of a group. [onClick] null: not a button. [quiet]: only a muted line. */
@Composable
private fun SettingsRow(
    shape: Shape,
    title: String,
    detail: String? = null,
    icon: (@Composable () -> Unit)? = null,
    titleColor: Color = Color.Unspecified,
    quiet: Boolean = false,
    onClick: (() -> Unit)? = null,
    trailing: (@Composable () -> Unit)? = null,
) {
    val colors = MaterialTheme.colorScheme
    Row(
        Modifier
            .fillMaxWidth()
            .clip(shape)
            .background(colors.surfaceContainerLow)
            .then(if (onClick != null) Modifier.clickable(role = Role.Button, onClick = onClick) else Modifier)
            .heightIn(min = 56.dp)
            .padding(horizontal = 20.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        if (icon != null) icon()
        Column(Modifier.weight(1f)) {
            Text(
                title,
                color = if (quiet) colors.onSurfaceVariant else if (titleColor == Color.Unspecified) colors.onSurface else titleColor,
                style = MaterialTheme.typography.bodyLarge,
            )
            if (detail != null) {
                Text(detail, color = colors.onSurfaceVariant, style = MaterialTheme.typography.bodyMedium)
            }
        }
        if (trailing != null) trailing()
    }
}

@Composable
private fun SectionTitle(text: String) {
    Text(
        text.uppercase(ComposeLocale.current.platformLocale),
        modifier = Modifier.fillMaxWidth().padding(start = 32.dp, end = 20.dp, top = 20.dp, bottom = 8.dp),
        style = MaterialTheme.typography.labelMedium,
        fontWeight = FontWeight.Bold,
        letterSpacing = 0.8.sp,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
    )
}
