package ovh.kweb.mail.ui.admin

import android.content.ActivityNotFoundException
import android.content.Intent
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Lock
import androidx.compose.material.icons.filled.Warning
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalClipboard
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.core.net.toUri
import androidx.lifecycle.compose.LifecycleResumeEffect
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import kotlinx.coroutines.launch
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.accounts.LocalAccountSession
import ovh.kweb.mail.ui.list.Groups

/** "Administration" of one account: what its key opens, and Forget this key. */
@Composable
fun AdminAccessScreen(onBack: () -> Unit, vm: AdminAccessViewModel = viewModel()) {
    val c = vm.access
    val state by c.state.collectAsStateWithLifecycle()
    LifecycleResumeEffect(vm) {
        c.load()
        onPauseOrDispose { }
    }
    AdminScaffold(title = stringResource(R.string.admin_access_title), onBack = onBack, secure = true) { padding ->
        Column(Modifier.padding(padding).fillMaxSize()) {
            AccessContent(c, state)
        }
    }
}

/**
 * The access card and what goes with it, in its three states: locked (three steps and "Paste an API key"), key not
 * accepted, and unlocked (the edition, what the key may do, "Remove key from this phone"). Home shows it too while no
 * key is kept.
 */
@Composable
internal fun AccessContent(c: AccessController, state: AccessState) {
    when {
        state.access == null && state.phase == AccessPhase.LOADING -> LoadingBox()
        state.access == null -> FailureBox(stringResource(R.string.admin_load_failed), onRetry = c::load)
        else -> Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(bottom = 32.dp)) {
            Notices(state.error, null, c::dismissError) {}
            when (state.kind) {
                AccessKind.Open -> UnlockedCard(c, state, state.access)
                AccessKind.Refused -> LockedCard(c, state, refused = true)
                AccessKind.Locked -> LockedCard(c, state, refused = false)
            }
        }
    }
    AccessDialogs(c, state)
}

/** "ada on mail.example.org": whose admin area it is. */
@Composable
internal fun rememberAccountLine(): String {
    val record = LocalAccountSession.current.record
    return remember(record) {
        val host = runCatching { java.net.URI(record.serverUrl).host }.getOrNull() ?: record.serverUrl
        "${record.username.substringBefore('@')} · $host"
    }
}

@Composable
private fun Hero(icon: ImageVector, title: String, line: String, bad: Boolean = false, ok: Boolean = false) {
    val colors = MaterialTheme.colorScheme
    Column(Modifier.fillMaxWidth().padding(top = 8.dp, bottom = 16.dp), horizontalAlignment = Alignment.CenterHorizontally) {
        Box(
            Modifier.size(72.dp).clip(CircleShape).background(if (bad) colors.errorContainer else if (ok) colors.tertiaryContainer else colors.secondaryContainer),
            contentAlignment = Alignment.Center,
        ) {
            Icon(
                icon,
                contentDescription = null,
                modifier = Modifier.size(34.dp),
                tint = if (bad) colors.onErrorContainer else if (ok) colors.onTertiaryContainer else colors.onSecondaryContainer,
            )
        }
        Text(title, style = MaterialTheme.typography.headlineSmall, modifier = Modifier.padding(top = 12.dp))
        Text(line, style = MaterialTheme.typography.bodyMedium, color = colors.onSurfaceVariant)
    }
}

@Composable
private fun LockedCard(c: AccessController, state: AccessState, refused: Boolean) {
    val resources = LocalResources.current
    val line = rememberAccountLine()
    Hero(
        icon = if (refused) Icons.Filled.Warning else Icons.Filled.Lock,
        title = stringResource(if (refused) R.string.admin_key_not_accepted else R.string.admin_locked),
        line = line,
        bad = refused,
    )
    Note(
        when {
            state.refusal != null -> state.refusal.resolve(resources)
            refused -> stringResource(R.string.admin_key_refused_default)
            else -> stringResource(R.string.admin_locked_note)
        },
    )
    val server = LocalAccountSession.current.record.serverUrl
    val context = LocalContext.current
    AdminGroup(Modifier.padding(top = 8.dp)) {
        val steps = listOf(R.string.admin_step_1, R.string.admin_step_2, R.string.admin_step_3)
        steps.forEachIndexed { index, text ->
            AdminRow(
                Groups.shape(index, steps.size),
                stringResource(text),
                leading = { RoundMark((index + 1).toString(), 32) },
                onClick = if (index == 0) ({ openServerPage(context, server) }) else null,
            )
        }
    }
    val clipboard = LocalClipboard.current
    val scope = rememberCoroutineScope()
    Button(
        // The clipboard is read here, when the user taps, and never otherwise.
        onClick = {
            scope.launch {
                val text = clipboard.getClipEntry()?.clipData?.takeIf { it.itemCount > 0 }?.getItemAt(0)?.text
                c.paste(text)
            }
        },
        enabled = !state.busy,
        shape = CircleShape,
        modifier = Modifier.fillMaxWidth().padding(horizontal = 12.dp).padding(top = 16.dp).heightIn(min = 52.dp),
    ) {
        if (state.busy) {
            CircularProgressIndicator(Modifier.size(20.dp), strokeWidth = 2.dp, color = MaterialTheme.colorScheme.onPrimary)
        } else {
            Icon(Icons.Filled.Lock, contentDescription = null)
        }
        Text(stringResource(R.string.admin_paste_key), modifier = Modifier.padding(start = 8.dp), style = MaterialTheme.typography.labelLarge)
    }
    Note(stringResource(R.string.admin_key_storage_note), Modifier.padding(top = 8.dp))
    if (state.access?.keyRefused == true) {
        AdminGroup(Modifier.padding(top = 16.dp)) {
            AdminRow(
                Groups.shape(0, 1),
                stringResource(R.string.admin_forget_row),
                titleColor = MaterialTheme.colorScheme.error,
                onClick = if (state.busy) null else c::askForget,
            )
        }
    }
}

private fun openServerPage(context: android.content.Context, server: String) {
    val uri = runCatching { server.toUri() }.getOrNull() ?: return
    if (uri.scheme != "https" && uri.scheme != "http") return
    try {
        context.startActivity(Intent(Intent.ACTION_VIEW, uri).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
    } catch (_: ActivityNotFoundException) {
        // No browser: the step stays as words.
    }
}

@Composable
private fun UnlockedCard(c: AccessController, state: AccessState, access: uniffi.kweb_core.AdminAccess) {
    Hero(Icons.Filled.Check, stringResource(R.string.admin_unlocked), rememberAccountLine(), ok = true)
    AdminGroup {
        AdminRow(
            Groups.shape(0, 2),
            stringResource(R.string.admin_edition),
            trailing = { Pill(editionName(access.edition)) },
        )
        AdminRow(Groups.shape(1, 2), stringResource(R.string.admin_key_label), detail = stringResource(R.string.admin_key_kept))
    }
    SectionLabel(stringResource(R.string.admin_may_title))
    val rows = AdminRules.mayRows(access)
    AdminGroup {
        rows.forEachIndexed { index, row ->
            AdminRow(
                Groups.shape(index, rows.size),
                mayTitle(row.kind),
                detail = mayDetail(row.kind, row.allowed),
                leading = {
                    Icon(
                        if (row.allowed) Icons.Filled.Check else Icons.Filled.Close,
                        contentDescription = null,
                        tint = if (row.allowed) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                },
            )
        }
    }
    AdminGroup(Modifier.padding(top = 16.dp)) {
        AdminRow(
            Groups.shape(0, 1),
            stringResource(R.string.admin_forget_row),
            titleColor = MaterialTheme.colorScheme.error,
            onClick = if (state.busy) null else c::askForget,
        )
    }
}

@Composable
private fun AccessDialogs(c: AccessController, state: AccessState) {
    when (val dialog = state.dialog) {
        is AccessDialog.ConfirmKey -> ConfirmDialog(
            title = stringResource(R.string.admin_confirm_key_title),
            text = stringResource(R.string.admin_confirm_key_text),
            confirm = stringResource(R.string.admin_confirm_key_use),
            onConfirm = c::confirmKey,
            onDismiss = c::declineKey,
        ) {
            Text(dialog.shown, fontFamily = FontFamily.Monospace, style = MaterialTheme.typography.titleMedium, textAlign = TextAlign.Start)
        }
        AccessDialog.Forget -> ConfirmDialog(
            title = stringResource(R.string.admin_forget_title),
            text = stringResource(R.string.admin_forget_text),
            confirm = stringResource(R.string.admin_forget_confirm),
            onConfirm = c::forget,
            onDismiss = c::dismissDialog,
            danger = true,
        )
        null -> Unit
    }
}

