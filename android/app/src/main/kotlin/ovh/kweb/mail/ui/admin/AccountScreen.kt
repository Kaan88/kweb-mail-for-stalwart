package ovh.kweb.mail.ui.admin

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Edit
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.LifecycleResumeEffect
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import java.util.Locale
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.list.Groups
import uniffi.kweb_core.ServerAccount
import uniffi.kweb_core.ServerAccountKind

/** One server account as the administrator sees it, with what is done to it (each risky change asks first). */
@Composable
fun AccountScreen(id: String, onBack: () -> Unit, vm: AdminAccountViewModel = viewModel()) {
    val c = vm.forAccount(id)
    val state by c.state.collectAsStateWithLifecycle()
    LifecycleResumeEffect(vm) {
        c.refresh()
        onPauseOrDispose { }
    }
    LaunchedEffect(state.gone) { if (state.gone) onBack() }
    AdminScaffold(
        title = stringResource(R.string.admin_account_title),
        onBack = onBack,
        secure = state.dialog is AccountDialog.Reset,
        actions = {
            if (state.account != null) IconButton(onClick = c::openEdit, enabled = !state.busy) { Icon(Icons.Filled.Edit, contentDescription = stringResource(R.string.admin_edit)) }
        },
    ) { padding ->
        Column(Modifier.padding(padding).fillMaxSize()) {
            Notices(state.error.takeIf { state.dialog !is AccountDialog.Reset }, state.info, c::dismissError, c::dismissInfo)
            val account = state.account
            when {
                account != null -> AccountBody(account, state.busy, c)
                state.phase == AccountPhase.FAILED -> FailureBox(stringResource(R.string.admin_load_failed), onRetry = c::refresh)
                else -> LoadingBox()
            }
        }
    }
    AccountDialogs(state, c)
}

@Composable
private fun AccountBody(account: ServerAccount, busy: Boolean, c: AccountController) {
    val colors = MaterialTheme.colorScheme
    val locale = remember { Locale.getDefault() }
    val isGroup = account.kind == ServerAccountKind.GROUP
    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(bottom = 32.dp)) {
        Row(Modifier.fillMaxWidth().padding(horizontal = 28.dp, vertical = 12.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(14.dp)) {
            RoundMark(AdminRules.initial(account.name), 56)
            Column(Modifier.weight(1f)) {
                Text(account.name, style = MaterialTheme.typography.titleLarge)
                Text(
                    listOf(AdminRules.secondLine(account), stringResource(if (isGroup) R.string.admin_kind_group else R.string.admin_kind_user)).filter { it.isNotBlank() }.joinToString(" · "),
                    color = colors.onSurfaceVariant,
                    style = MaterialTheme.typography.bodyMedium,
                )
            }
            Column(horizontalAlignment = Alignment.End, verticalArrangement = Arrangement.spacedBy(4.dp)) {
                for (pill in AdminRules.pills(account).filter { it != AccountPill.Group }) Pill(pillWord(pill), error = pill == AccountPill.Suspended)
            }
        }
        if (account.description.isNotBlank()) Note(account.description)
        AdminGroup {
            Column(
                Modifier.fillMaxWidth().clip(Groups.shape(0, 1)).background(colors.surfaceContainerLow).padding(horizontal = 20.dp, vertical = 14.dp),
                verticalArrangement = Arrangement.spacedBy(4.dp),
            ) {
                Text(stringResource(R.string.admin_storage_limit), style = MaterialTheme.typography.bodyLarge)
                val quota = account.quota
                val used = account.used
                Text(
                    when {
                        quota == null && used == null -> stringResource(R.string.admin_no_limit)
                        quota == null -> stringResource(R.string.admin_used_no_limit, AdminRules.formatBytes(used ?: 0uL, locale))
                        used == null -> AdminRules.formatBytes(quota, locale)
                        else -> stringResource(R.string.admin_used_of, AdminRules.formatBytes(used, locale), AdminRules.formatBytes(quota, locale))
                    },
                    color = if (AdminRules.nearlyFull(used, quota)) colors.error else colors.onSurfaceVariant,
                    style = MaterialTheme.typography.bodyMedium,
                )
                AdminRules.usedFraction(used, quota)?.let { fraction ->
                    LinearProgressIndicator(
                        progress = { fraction },
                        modifier = Modifier.fillMaxWidth().padding(top = 6.dp).height(8.dp).clip(CircleShape),
                        color = if (AdminRules.nearlyFull(used, quota)) colors.error else colors.primary,
                        trackColor = colors.surfaceContainerHighest,
                        gapSize = 0.dp,
                        drawStopIndicator = {},
                    )
                }
            }
        }
        if (account.aliases.isNotEmpty()) {
            SectionLabel(stringResource(R.string.admin_other_addresses))
            AdminGroup { account.aliases.forEachIndexed { i, alias -> AdminRow(Groups.shape(i, account.aliases.size), alias) } }
        }
        if (account.groups.isNotEmpty()) {
            SectionLabel(stringResource(R.string.admin_groups))
            AdminGroup { AdminRow(Groups.shape(0, 1), account.groups.joinToString(", ")) }
        }
        val actions = buildList<@Composable (androidx.compose.ui.graphics.Shape) -> Unit> {
            if (!isGroup) add { shape -> AdminRow(shape, stringResource(R.string.admin_reset_password), onClick = if (busy) null else c::openReset) }
            if (!isGroup) {
                add { shape ->
                    AdminRow(
                        shape,
                        stringResource(if (account.isSuspended) R.string.admin_lift_suspension else R.string.admin_suspend),
                        onClick = if (busy) null else c::toggleSuspend,
                    )
                }
            }
            add { shape -> AdminRow(shape, stringResource(R.string.admin_delete_account), titleColor = colors.error, onClick = if (busy) null else c::askDelete) }
        }
        AdminGroup(Modifier.padding(top = 16.dp)) { actions.forEachIndexed { i, row -> row(Groups.shape(i, actions.size)) } }
    }
}

@Composable
private fun AccountDialogs(state: AccountState, c: AccountController) {
    val account = state.account
    when (val dialog = state.dialog) {
        is AccountDialog.Edit -> ConfirmDialog(
            title = stringResource(R.string.admin_edit_title),
            text = stringResource(R.string.admin_edit_text),
            confirm = stringResource(R.string.admin_save),
            onConfirm = c::saveEdit,
            onDismiss = c::dismissDialog,
            enabled = !dialog.limitInvalid && !state.busy,
        ) {
            AdminField(dialog.description, c::typeDescription, stringResource(R.string.admin_description))
            AdminField(
                dialog.limit,
                c::typeLimit,
                stringResource(R.string.admin_limit_field),
                isError = dialog.limitInvalid,
                supporting = stringResource(if (dialog.limitInvalid) R.string.admin_limit_invalid else R.string.admin_limit_hint),
            )
        }
        is AccountDialog.Reset -> ConfirmDialog(
            title = stringResource(R.string.admin_reset_title),
            text = stringResource(R.string.admin_reset_text, account?.name.orEmpty()),
            confirm = stringResource(R.string.admin_reset_confirm),
            onConfirm = c::confirmReset,
            onDismiss = c::dismissDialog,
            enabled = dialog.problem == null && !state.busy,
        ) {
            AdminField(dialog.password, c::typePassword, stringResource(R.string.admin_new_password), secret = true)
            AdminField(
                dialog.repeat,
                c::typeRepeat,
                stringResource(R.string.admin_repeat_password),
                secret = true,
                isError = dialog.problem == PasswordProblem.Different,
                supporting = passwordHint(dialog.problem, dialog.password.isNotEmpty() && dialog.repeat.isNotEmpty()),
            )
            state.error?.let { Text(it.resolve(androidx.compose.ui.platform.LocalResources.current), color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodyMedium) }
        }
        AccountDialog.Suspend -> ConfirmDialog(
            title = stringResource(R.string.admin_suspend_title, account?.name.orEmpty()),
            text = stringResource(R.string.admin_suspend_text),
            confirm = stringResource(R.string.admin_suspend),
            onConfirm = c::confirmSuspend,
            onDismiss = c::dismissDialog,
            danger = true,
        )
        is AccountDialog.Delete -> ConfirmDialog(
            title = stringResource(R.string.admin_delete_title, account?.name.orEmpty()),
            text = stringResource(R.string.admin_delete_text),
            confirm = stringResource(R.string.admin_delete),
            onConfirm = c::confirmDelete,
            onDismiss = c::dismissDialog,
            danger = true,
            enabled = account != null && AdminRules.deleteConfirmed(dialog.typed, account.name),
        ) {
            AdminField(dialog.typed, c::typeDelete, stringResource(R.string.admin_type_name))
        }
        null -> Unit
    }
}

@Composable
internal fun passwordHint(problem: PasswordProblem?, typed: Boolean): String? = when {
    problem == PasswordProblem.TooShort -> stringResource(R.string.admin_password_short, AdminRules.MIN_PASSWORD)
    problem == PasswordProblem.Different && typed -> stringResource(R.string.admin_password_different)
    else -> null
}

@Composable
internal fun pillWord(pill: AccountPill): String = stringResource(
    when (pill) {
        AccountPill.Admin -> R.string.admin_pill_admin
        AccountPill.Suspended -> R.string.admin_pill_suspended
        AccountPill.Group -> R.string.admin_pill_group
    },
)
