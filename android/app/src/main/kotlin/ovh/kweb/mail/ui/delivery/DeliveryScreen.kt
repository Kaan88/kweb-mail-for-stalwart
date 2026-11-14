package ovh.kweb.mail.ui.delivery

import android.annotation.SuppressLint
import android.app.Activity
import android.content.ActivityNotFoundException
import android.content.Context
import android.content.ContextWrapper
import android.content.Intent
import android.os.Build
import android.provider.Settings
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.Notifications
import androidx.compose.material.icons.filled.Warning
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.SwitchDefaults
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.State
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.core.net.toUri
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LifecycleResumeEffect
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.repeatOnLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle
import java.util.Locale
import kotlinx.coroutines.delay
import androidx.compose.ui.text.toUpperCase
import ovh.kweb.mail.R
import ovh.kweb.mail.background.CheckRecord
import ovh.kweb.mail.ui.UiPrefs
import uniffi.kweb_core.ConnectionStatus

/** One row of the screen. [hint] and [onClick] are set only when there is something to do. */
private class CheckItem(
    val title: String,
    val detail: String,
    val pill: String,
    val ok: Boolean,
    val extra: String? = null,
    val hint: String? = null,
    val onClick: (() -> Unit)? = null,
)

/**
 * "Will I get my notifications?": real checks with a pill each, and for a failed one a line that
 * says what to do and a tap that does it.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun DeliveryScreen(onBack: () -> Unit, onSignInAgain: () -> Unit, vm: DeliveryViewModel = viewModel()) {
    val state by vm.state.collectAsStateWithLifecycle()
    val now by rememberNowMillis()
    val resources = LocalResources.current
    val snackbar = remember { SnackbarHostState() }
    var confirmSignOut by remember { mutableStateOf(false) }
    var menuOpen by remember { mutableStateOf(false) }

    // The user comes back from the system settings: read the permission and battery state again.
    LifecycleResumeEffect(vm) {
        vm.refreshSystem()
        onPauseOrDispose { }
    }
    LaunchedEffect(vm) { vm.messages.collect { snackbar.showSnackbar(resources.getString(it)) } }

    val permissionLauncher = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) {
        vm.refreshSystem()
    }
    val actions = rememberActions(vm, onSignInAgain, permissionLauncher::launch)
    val items = checkItems(state, now, actions)

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(stringResource(R.string.delivery_check_title)) },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = stringResource(R.string.back))
                    }
                },
                actions = {
                    IconButton(onClick = { menuOpen = true }) {
                        Icon(Icons.Filled.MoreVert, contentDescription = stringResource(R.string.more_options))
                    }
                    DropdownMenu(expanded = menuOpen, onDismissRequest = { menuOpen = false }) {
                        DropdownMenuItem(
                            text = { Text(stringResource(R.string.sign_out)) },
                            enabled = !state.signingOut,
                            onClick = { menuOpen = false; confirmSignOut = true },
                        )
                    }
                },
                colors = TopAppBarDefaults.topAppBarColors(containerColor = MaterialTheme.colorScheme.background),
            )
        },
        snackbarHost = { SnackbarHost(snackbar) },
    ) { padding ->
        Column(Modifier.padding(padding).verticalScroll(rememberScrollState())) {
            if (state.signingOut) LinearProgressIndicator(Modifier.fillMaxWidth())
            Verdict(items)
            items.forEach { CheckRow(it) }
            SectionTitle(stringResource(R.string.delivery_what_to_show))
            SwitchRow(stringResource(R.string.delivery_show_mail), state.mailEnabled, vm::setMailEnabled)
            SwitchRow(stringResource(R.string.delivery_show_reminders), state.remindersEnabled, vm::setRemindersEnabled)
            SwitchRow(stringResource(R.string.delivery_remove_elsewhere), state.removeWhenReadElsewhere, vm::setRemoveWhenReadElsewhere)
            Button(
                onClick = vm::sendTestNotification,
                modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 16.dp).height(52.dp),
            ) {
                Icon(Icons.Filled.Notifications, contentDescription = null, modifier = Modifier.size(20.dp))
                Spacer(Modifier.width(8.dp))
                Text(stringResource(R.string.delivery_send_test), style = MaterialTheme.typography.titleSmall)
            }
            state.coreVersion?.let {
                Text(
                    it,
                    modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp),
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }

    if (confirmSignOut) {
        SignOutDialog(
            onConfirm = { confirmSignOut = false; vm.signOut() },
            onDismiss = { confirmSignOut = false },
        )
    }
}

/** What a tap on a failed row does. */
private class Actions(
    val signInAgain: () -> Unit,
    val retryConnection: () -> Unit,
    val fixNotifications: () -> Unit,
    val fixBattery: () -> Unit,
    val runCheck: () -> Unit,
)

@Composable
private fun rememberActions(
    vm: DeliveryViewModel,
    onSignInAgain: () -> Unit,
    requestPermission: (String) -> Unit,
): Actions {
    val context = LocalContext.current
    return Actions(
        signInAgain = onSignInAgain,
        retryConnection = vm::retryConnection,
        fixNotifications = { fixNotifications(context, requestPermission) },
        fixBattery = { openBatterySettings(context) },
        runCheck = vm::runCheck,
    )
}

@Composable
private fun checkItems(state: DeliveryState, now: Long, actions: Actions): List<CheckItem> = listOf(
    connectionItem(state, now, actions),
    notificationItem(state, actions),
    batteryItem(state, actions),
    safetyItem(state, now, actions),
    signInItem(state, actions),
)

@Composable
private fun connectionItem(state: DeliveryState, now: Long, actions: Actions): CheckItem {
    val title = state.host?.let { stringResource(R.string.delivery_connection_title, it) }
        ?: stringResource(R.string.delivery_connection_title_unknown)
    val failure = state.startFailure?.let { stringResource(R.string.delivery_start_failed, it.text) }
    val retry = stringResource(R.string.delivery_hint_retry)
    return when (val status = state.status) {
        is ConnectionStatus.Live -> CheckItem(
            title,
            lastPushText(status.lastEventAt, now),
            stringResource(R.string.pill_live),
            ok = failure == null,
            extra = failure,
            hint = retry.takeIf { failure != null },
            onClick = actions.retryConnection.takeIf { failure != null },
        )
        ConnectionStatus.Connecting, ConnectionStatus.SignedOut -> CheckItem(
            title,
            stringResource(R.string.delivery_connecting),
            stringResource(R.string.pill_connecting),
            ok = false,
            extra = failure,
        )
        is ConnectionStatus.Offline -> CheckItem(
            title,
            status.reason,
            stringResource(R.string.pill_offline),
            ok = false,
            extra = failure,
            hint = retry,
            onClick = actions.retryConnection,
        )
        ConnectionStatus.SignInNeeded -> CheckItem(
            title,
            stringResource(R.string.delivery_server_refused),
            stringResource(R.string.pill_sign_in_needed),
            ok = false,
            extra = failure,
            hint = stringResource(R.string.delivery_hint_sign_in),
            onClick = actions.signInAgain,
        )
    }
}

@Composable
private fun notificationItem(state: DeliveryState, actions: Actions): CheckItem {
    val ok = state.notificationsAllowed
    return CheckItem(
        stringResource(R.string.delivery_notifications_title),
        stringResource(R.string.delivery_notifications_detail),
        stringResource(if (ok) R.string.pill_allowed else R.string.pill_off),
        ok,
        hint = stringResource(R.string.delivery_hint_notifications).takeIf { !ok },
        onClick = actions.fixNotifications.takeIf { !ok },
    )
}

@Composable
private fun batteryItem(state: DeliveryState, actions: Actions): CheckItem {
    val ok = state.batteryExempt
    return CheckItem(
        stringResource(R.string.delivery_battery_title),
        stringResource(R.string.delivery_battery_detail),
        stringResource(if (ok) R.string.pill_exempt else R.string.pill_not_exempt),
        ok,
        hint = stringResource(R.string.delivery_hint_battery).takeIf { !ok },
        onClick = actions.fixBattery.takeIf { !ok },
    )
}

@Composable
private fun safetyItem(state: DeliveryState, now: Long, actions: Actions): CheckItem {
    val last = state.lastCheck
    val onTime = SafetyCheck.isOnTime(last, now)
    val detail = if (last == null) {
        stringResource(R.string.delivery_safety_never)
    } else {
        stringResource(R.string.delivery_safety_detail, clockTime(last.atMillis))
    }
    return CheckItem(
        stringResource(R.string.delivery_safety_title),
        detail,
        stringResource(if (onTime) R.string.pill_on_time else R.string.pill_late),
        onTime,
        extra = last?.error?.let { stringResource(R.string.delivery_safety_failed, it) },
        hint = stringResource(R.string.delivery_hint_check).takeIf { !onTime },
        onClick = actions.runCheck,
    )
}

@Composable
private fun signInItem(state: DeliveryState, actions: Actions): CheckItem {
    val needed = state.status == ConnectionStatus.SignInNeeded
    return CheckItem(
        stringResource(R.string.delivery_sign_in_title),
        stringResource(R.string.delivery_sign_in_detail),
        stringResource(if (needed) R.string.pill_sign_in_needed else R.string.pill_valid),
        ok = !needed,
        hint = stringResource(R.string.delivery_hint_sign_in).takeIf { needed },
        onClick = actions.signInAgain.takeIf { needed },
    )
}

@Composable
private fun lastPushText(lastEventAtSeconds: Long, now: Long): String {
    val age = LastPush.age(lastEventAtSeconds, now)
    val plural = when (age.unit) {
        AgeUnit.Seconds -> R.plurals.last_push_seconds
        AgeUnit.Minutes -> R.plurals.last_push_minutes
        AgeUnit.Hours -> R.plurals.last_push_hours
        AgeUnit.Days -> R.plurals.last_push_days
    }
    return pluralStringResource(plural, age.count, age.count)
}

private fun clockTime(millis: Long): String =
    DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT).withLocale(Locale.getDefault())
        .format(Instant.ofEpochMilli(millis).atZone(ZoneId.systemDefault()))

/**
 * The answer first, as the mock-up has it: one card that says whether mail will arrive, in the
 * primary colours when every check passes and in the error colours when one does not.
 */
@Composable
private fun Verdict(items: List<CheckItem>) {
    val failed = items.count { !it.ok }
    val ok = failed == 0
    val colors = MaterialTheme.colorScheme
    Row(
        Modifier
            .fillMaxWidth()
            .padding(start = 14.dp, end = 14.dp, top = 6.dp, bottom = 8.dp)
            .clip(RoundedCornerShape(30.dp))
            .background(if (ok) colors.primaryContainer else colors.errorContainer)
            .semantics(mergeDescendants = true) {}
            .padding(16.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        // The uneven corners are the mock-up's soft "blob", as far as plain corners can draw it.
        Box(
            Modifier
                .size(58.dp)
                .clip(RoundedCornerShape(topStartPercent = 50, topEndPercent = 38, bottomEndPercent = 50, bottomStartPercent = 42))
                .background(if (ok) colors.primary else colors.error),
            contentAlignment = Alignment.Center,
        ) {
            Icon(
                if (ok) Icons.Filled.Check else Icons.Filled.Warning,
                contentDescription = null,
                modifier = Modifier.size(28.dp),
                tint = if (ok) colors.onPrimary else colors.onError,
            )
        }
        val content = if (ok) colors.onPrimaryContainer else colors.onErrorContainer
        Column(Modifier.weight(1f)) {
            Text(
                stringResource(if (ok) R.string.delivery_verdict_ok else R.string.delivery_verdict_bad),
                color = content,
                style = MaterialTheme.typography.titleMedium,
                fontWeight = FontWeight.SemiBold,
            )
            Text(
                if (ok) items.first().detail else pluralStringResource(R.plurals.delivery_verdict_to_fix, failed, failed),
                color = content.copy(alpha = 0.8f),
                style = MaterialTheme.typography.bodyMedium,
            )
        }
    }
}

@Composable
private fun CheckRow(item: CheckItem) {
    val click = item.onClick
    Row(
        Modifier
            .fillMaxWidth()
            .then(if (click != null) Modifier.clickable(role = Role.Button, onClick = click) else Modifier.semantics(mergeDescendants = true) {})
            .heightIn(min = 56.dp)
            .padding(horizontal = 20.dp, vertical = 9.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        Column(Modifier.weight(1f)) {
            Text(item.title, style = MaterialTheme.typography.bodyLarge)
            Text(item.detail, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
            val alert = MaterialTheme.colorScheme.error
            item.extra?.let { Text(it, style = MaterialTheme.typography.bodyMedium, color = alert) }
            item.hint?.let { Text(it, style = MaterialTheme.typography.bodyMedium, color = alert, fontWeight = FontWeight.Medium) }
        }
        StatusPill(item.pill, item.ok)
        // The whole row is the button; this says so where there is something to fix.
        if (!item.ok && click != null) {
            Surface(color = MaterialTheme.colorScheme.primary, contentColor = MaterialTheme.colorScheme.onPrimary, shape = CircleShape) {
                Text(
                    stringResource(R.string.delivery_fix),
                    modifier = Modifier.padding(horizontal = 14.dp, vertical = 6.dp),
                    style = MaterialTheme.typography.labelLarge,
                )
            }
        }
    }
}

/**
 * The status pill of the mock-up: a passed check in the tertiary container, a failed one in the
 * error container. Its text is real text, so a screen reader reads the verdict.
 */
@Composable
private fun StatusPill(text: String, ok: Boolean) {
    val colors = MaterialTheme.colorScheme
    Surface(
        color = if (ok) colors.tertiaryContainer else colors.errorContainer,
        contentColor = if (ok) colors.onTertiaryContainer else colors.onErrorContainer,
        shape = RoundedCornerShape(10.dp),
    ) {
        Text(
            text,
            modifier = Modifier.padding(horizontal = 10.dp, vertical = 3.dp),
            style = MaterialTheme.typography.labelMedium,
            fontWeight = FontWeight.Bold,
        )
    }
}

@Composable
private fun SectionTitle(text: String) {
    Text(
        text.toUpperCase(androidx.compose.ui.text.intl.Locale.current),
        modifier = Modifier.fillMaxWidth().padding(start = 20.dp, end = 20.dp, top = 16.dp, bottom = 4.dp),
        style = MaterialTheme.typography.labelMedium,
        fontWeight = FontWeight.Bold,
        letterSpacing = 0.8.sp,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
    )
}

@Composable
private fun SwitchRow(label: String, checked: Boolean, onChange: (Boolean) -> Unit) {
    Row(
        Modifier
            .fillMaxWidth()
            .toggleable(value = checked, role = Role.Switch, onValueChange = onChange)
            .heightIn(min = 52.dp)
            .padding(horizontal = 20.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Text(label, Modifier.weight(1f), style = MaterialTheme.typography.bodyLarge)
        Switch(
            checked = checked,
            onCheckedChange = null,
            thumbContent = if (checked) {
                { Icon(Icons.Filled.Check, contentDescription = null, modifier = Modifier.size(SwitchDefaults.IconSize)) }
            } else {
                null
            },
        )
    }
}

@Composable
private fun SignOutDialog(onConfirm: () -> Unit, onDismiss: () -> Unit) {
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.sign_out_title)) },
        text = { Text(stringResource(R.string.sign_out_text)) },
        confirmButton = { TextButton(onClick = onConfirm) { Text(stringResource(R.string.sign_out)) } },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.cancel)) } },
    )
}

/** The current time in milliseconds, updated every second while the screen is on display. */
@Composable
private fun rememberNowMillis(): State<Long> {
    val owner = LocalLifecycleOwner.current
    val now = remember { mutableLongStateOf(System.currentTimeMillis()) }
    LaunchedEffect(owner) {
        owner.lifecycle.repeatOnLifecycle(Lifecycle.State.STARTED) {
            while (true) {
                now.longValue = System.currentTimeMillis()
                delay(1_000)
            }
        }
    }
    return now
}

/**
 * Tap on "Notification permission": ask for the permission when Android will still show its
 * dialog, otherwise open the app's notification settings.
 */
private fun fixNotifications(context: Context, requestPermission: (String) -> Unit) {
    val activity = context.findActivity()
    val prefs = UiPrefs(context)
    val step = NotificationFix.nextStep(
        sdk = Build.VERSION.SDK_INT,
        permissionGranted = DeliveryViewModel.permissionGranted(context),
        askedBefore = prefs.notificationPermissionAsked,
        canShowRationale = activity != null &&
            activity.shouldShowRequestPermissionRationale(NotificationFix.PERMISSION),
    )
    when (step) {
        NotificationFix.Step.AskPermission -> {
            prefs.notificationPermissionAsked = true
            requestPermission(NotificationFix.PERMISSION)
        }
        NotificationFix.Step.OpenSettings -> startSafely(
            context,
            Intent(Settings.ACTION_APP_NOTIFICATION_SETTINGS).putExtra(Settings.EXTRA_APP_PACKAGE, context.packageName),
        )
    }
}

/**
 * Tap on "Battery optimisation": the system's own "let this app run in the background" dialog.
 * Google Play does not allow this request for most apps; this app is distributed outside it, and a
 * push connection that Android may pause defeats the purpose of the app.
 */
@SuppressLint("BatteryLife")
private fun openBatterySettings(context: Context) {
    val request = Intent(Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS, "package:${context.packageName}".toUri())
    if (!startSafely(context, request)) {
        startSafely(context, Intent(Settings.ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS))
    }
}

private fun startSafely(context: Context, intent: Intent): Boolean = try {
    // From an activity the system dialog returns to the app on its own; only other contexts need a task.
    if (context !is Activity) intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
    context.startActivity(intent)
    true
} catch (_: ActivityNotFoundException) {
    false
}

private tailrec fun Context.findActivity(): Activity? = when (this) {
    is Activity -> this
    is ContextWrapper -> baseContext.findActivity()
    else -> null
}
