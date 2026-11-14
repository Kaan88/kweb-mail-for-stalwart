package ovh.kweb.mail.ui.account

import android.content.ActivityNotFoundException
import android.content.Intent
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.RadioButton
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.DialogProperties
import androidx.compose.ui.window.SecureFlagPolicy
import androidx.core.net.toUri
import kotlinx.coroutines.launch
import ovh.kweb.mail.R

/**
 * The questions and forms of the security screen. Anything typed that is a secret (a password, a code) lives only
 * in `remember` here, never in saved state and never in the view model; it is gone when the dialog closes.
 * A dialog with a secret on it is its own secure window.
 */
@Composable
fun SecurityDialogs(state: SecurityState, c: SecurityController, snackbar: SnackbarHostState) {
    val dialog = state.dialog ?: return
    val secure = if (dialog.showsSecret) SecureFlagPolicy.SecureOn else SecureFlagPolicy.Inherit
    val props = DialogProperties(dismissOnBackPress = !state.busy, dismissOnClickOutside = false, securePolicy = secure)
    val error = state.dialogError?.resolve(LocalResources.current)
    val busy = state.busy
    // With two-step codes on, the server wants a current code for these changes.
    val needsCode = state.security?.twoStep == true
    val cancel: @Composable () -> Unit = { TextButton(onClick = c::dismissDialog, enabled = !busy) { Text(stringResource(R.string.cancel)) } }
    when (dialog) {
        SecurityDialog.ChangePassword -> ChangePasswordDialog(c, needsCode, busy, error, props, cancel)
        is SecurityDialog.TwoStep -> TwoStepDialog(dialog, c, needsCode, busy, error, props, cancel, snackbar)
        SecurityDialog.TwoStepOff -> {
            var password by remember { mutableStateOf("") }
            var code by remember { mutableStateOf("") }
            val codeOk = !needsCode || SecurityRules.codeIsComplete(code)
            AlertDialog(
                onDismissRequest = c::dismissDialog,
                properties = props,
                title = { Text(stringResource(R.string.security_two_step_off_title)) },
                text = {
                    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                        Text(stringResource(R.string.security_two_step_off_text))
                        PasswordField(password, { password = it }, stringResource(R.string.security_pw_current), enabled = !busy)
                        if (needsCode) CodeField(code, { code = it }, stringResource(R.string.security_two_step_current_code), enabled = !busy)
                        ErrorLine(error)
                    }
                },
                confirmButton = {
                    TextButton(onClick = { c.disableTwoStep(password, code) }, enabled = password.isNotEmpty() && codeOk && !busy) {
                        Text(stringResource(R.string.security_two_step_switch_off), color = MaterialTheme.colorScheme.error)
                    }
                },
                dismissButton = cancel,
            )
        }
        is SecurityDialog.NewCredential -> NewCredentialDialog(dialog.kind, c, busy, error, props, cancel)
        is SecurityDialog.Secret -> SecretDialog(
            dialog, c,
            // The secret is shown once: Back must not drop it (a press meant for the form before it appeared).
            DialogProperties(dismissOnBackPress = false, dismissOnClickOutside = false, securePolicy = SecureFlagPolicy.SecureOn),
        )
        is SecurityDialog.DeleteCredential -> ConfirmDialog(
            title = stringResource(if (dialog.kind == CredentialKind.APP_PASSWORD) R.string.security_delete_app_password_title else R.string.security_delete_api_key_title),
            text = stringResource(R.string.security_delete_credential_text, dialog.credential.description.ifBlank { stringResource(R.string.security_no_note) }),
            action = stringResource(R.string.account_delete),
            error = error, busy = busy, props = props, onConfirm = c::deleteCredential, cancel = cancel, onDismiss = c::dismissDialog,
        )
        is SecurityDialog.RenamePasskey -> {
            var name by rememberSaveable(dialog.passkey.id) { mutableStateOf(dialog.passkey.name) }
            AlertDialog(
                onDismissRequest = c::dismissDialog,
                properties = props,
                title = { Text(stringResource(R.string.security_rename_title)) },
                text = {
                    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                        OutlinedTextField(name, { name = it }, label = { Text(stringResource(R.string.security_rename_label)) }, singleLine = true, enabled = !busy, modifier = Modifier.fillMaxWidth())
                        ErrorLine(error)
                    }
                },
                confirmButton = { TextButton(onClick = { c.renamePasskey(name) }, enabled = name.isNotBlank() && !busy) { Text(stringResource(R.string.account_save)) } },
                dismissButton = cancel,
            )
        }
        is SecurityDialog.DeletePasskey -> ConfirmDialog(
            title = stringResource(R.string.security_delete_passkey_title),
            text = stringResource(if (dialog.last) R.string.security_delete_passkey_last else R.string.security_delete_passkey_text, dialog.passkey.name),
            action = stringResource(R.string.account_delete),
            error = error, busy = busy, props = props, onConfirm = c::deletePasskey, cancel = cancel, onDismiss = c::dismissDialog,
        )
        is SecurityDialog.Revoke -> ConfirmDialog(
            title = stringResource(R.string.security_revoke_title),
            text = stringResource(R.string.security_revoke_text, dialog.app.name),
            action = stringResource(R.string.security_revoke),
            error = error, busy = busy, props = props, onConfirm = c::revoke, cancel = cancel, onDismiss = c::dismissDialog,
        )
    }
}

@Composable
private fun ErrorLine(error: String?) {
    if (error != null) Text(error, color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodyMedium)
}

@Composable
private fun ConfirmDialog(
    title: String,
    text: String,
    action: String,
    error: String?,
    busy: Boolean,
    props: DialogProperties,
    onConfirm: () -> Unit,
    cancel: @Composable () -> Unit,
    onDismiss: () -> Unit,
) {
    AlertDialog(
        onDismissRequest = onDismiss,
        properties = props,
        title = { Text(title) },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                Text(text)
                ErrorLine(error)
            }
        },
        confirmButton = { TextButton(onClick = onConfirm, enabled = !busy) { Text(action, color = MaterialTheme.colorScheme.error) } },
        dismissButton = cancel,
    )
}

@Composable
private fun ChangePasswordDialog(c: SecurityController, needsCode: Boolean, busy: Boolean, error: String?, props: DialogProperties, cancel: @Composable () -> Unit) {
    // Typed passwords are kept here only: not saved with the screen, not in the view model.
    var current by remember { mutableStateOf("") }
    var new by remember { mutableStateOf("") }
    var again by remember { mutableStateOf("") }
    var code by remember { mutableStateOf("") }
    val ready = SecurityRules.passwordProblem(current, new, again) == null && (!needsCode || SecurityRules.codeIsComplete(code))
    AlertDialog(
        onDismissRequest = c::dismissDialog,
        properties = props,
        title = { Text(stringResource(R.string.security_pw_title)) },
        text = {
            Column(Modifier.verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                PasswordField(current, { current = it }, stringResource(R.string.security_pw_current), enabled = !busy)
                PasswordField(new, { new = it }, stringResource(R.string.security_pw_new), enabled = !busy)
                PasswordField(
                    again, { again = it }, stringResource(R.string.security_pw_again), enabled = !busy,
                    isError = SecurityRules.showsMismatch(new, again),
                    supporting = if (SecurityRules.showsMismatch(new, again)) stringResource(R.string.security_pw_mismatch) else null,
                )
                if (needsCode) CodeField(code, { code = it }, stringResource(R.string.security_two_step_current_code), enabled = !busy)
                ErrorLine(error)
            }
        },
        confirmButton = { TextButton(onClick = { c.changePassword(current, new, code) }, enabled = ready && !busy) { Text(stringResource(R.string.account_save)) } },
        dismissButton = cancel,
    )
}

@Composable
private fun TwoStepDialog(
    dialog: SecurityDialog.TwoStep,
    c: SecurityController,
    needsCode: Boolean,
    busy: Boolean,
    error: String?,
    props: DialogProperties,
    cancel: @Composable () -> Unit,
    snackbar: SnackbarHostState,
) {
    var password by remember { mutableStateOf("") }
    var code by remember { mutableStateOf("") }
    var oldCode by remember { mutableStateOf("") }
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val resources = LocalResources.current
    var copied by remember { mutableStateOf(false) }
    var noApp by remember { mutableStateOf(false) }
    val copy = rememberCopier(sensitive = true) { copied = true }
    AlertDialog(
        onDismissRequest = c::dismissDialog,
        properties = props,
        title = { Text(stringResource(R.string.security_two_step_title)) },
        text = {
            Column(Modifier.verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                Text(stringResource(R.string.security_two_step_text))
                Text(stringResource(R.string.security_two_step_secret), style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                Text(dialog.setup.secret, fontFamily = FontFamily.Monospace, style = MaterialTheme.typography.titleMedium)
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    FilledTonalButton(onClick = { copy(SecurityRules.plainSecret(dialog.setup.secret)) }) { Text(stringResource(if (copied) R.string.security_copied else R.string.account_copy)) }
                    OutlinedButton(onClick = {
                        try {
                            context.startActivity(Intent(Intent.ACTION_VIEW, dialog.setup.otpauthUrl.toUri()))
                        } catch (_: ActivityNotFoundException) {
                            noApp = true
                        }
                    }) { Text(stringResource(R.string.security_two_step_open)) }
                }
                if (noApp) Text(stringResource(R.string.security_two_step_no_app), color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodyMedium)
                PasswordField(password, { password = it }, stringResource(R.string.security_pw_current), enabled = !busy)
                if (needsCode) CodeField(oldCode, { oldCode = it }, stringResource(R.string.security_two_step_current_code), enabled = !busy)
                CodeField(code, { code = it }, stringResource(R.string.security_two_step_code), enabled = !busy)
                ErrorLine(error)
            }
        },
        confirmButton = {
            TextButton(onClick = { c.enableTwoStep(password, code, oldCode) },
                enabled = password.isNotEmpty() && SecurityRules.codeIsComplete(code) && (!needsCode || SecurityRules.codeIsComplete(oldCode)) && !busy,
            ) {
                Text(stringResource(R.string.security_two_step_enable))
            }
        },
        dismissButton = cancel,
    )
}

@Composable
private fun NewCredentialDialog(kind: CredentialKind, c: SecurityController, busy: Boolean, error: String?, props: DialogProperties, cancel: @Composable () -> Unit) {
    var note by rememberSaveable { mutableStateOf("") }
    var choice by rememberSaveable { mutableIntStateOf(0) }
    val choices = Expiry.entries
    AlertDialog(
        onDismissRequest = c::dismissDialog,
        properties = props,
        title = { Text(stringResource(if (kind == CredentialKind.APP_PASSWORD) R.string.security_new_app_password_title else R.string.security_new_api_key_title)) },
        text = {
            Column(Modifier.verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                if (kind == CredentialKind.API_KEY) Text(stringResource(R.string.security_api_key_text), color = MaterialTheme.colorScheme.onSurfaceVariant)
                OutlinedTextField(note, { note = it }, label = { Text(stringResource(R.string.security_new_note)) }, singleLine = true, enabled = !busy, modifier = Modifier.fillMaxWidth())
                Text(stringResource(R.string.security_expiry), style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(top = 4.dp))
                choices.forEachIndexed { i, option ->
                    Row(
                        Modifier.fillMaxWidth().selectable(selected = choice == i, enabled = !busy, role = Role.RadioButton, onClick = { choice = i }),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        RadioButton(selected = choice == i, onClick = null, enabled = !busy)
                        Text(stringResource(expiryText(option)), modifier = Modifier.padding(start = 12.dp, top = 8.dp, bottom = 8.dp))
                    }
                }
                ErrorLine(error)
            }
        },
        confirmButton = { TextButton(onClick = { c.createCredential(kind, note, choices[choice]) }, enabled = !busy) { Text(stringResource(R.string.security_make)) } },
        dismissButton = cancel,
    )
}

/** The new secret, once. It stays only as long as this dialog does. */
@Composable
private fun SecretDialog(dialog: SecurityDialog.Secret, c: SecurityController, props: DialogProperties) {
    var copied by remember { mutableStateOf(false) }
    // Done does not act on the first tap unless the secret was copied: a tap meant for the form must not drop it.
    var warned by remember { mutableStateOf(false) }
    val copy = rememberCopier(sensitive = true) { copied = true }
    AlertDialog(
        onDismissRequest = {},
        properties = props,
        title = { Text(stringResource(if (dialog.kind == CredentialKind.APP_PASSWORD) R.string.security_secret_title_app_password else R.string.security_secret_title_api_key)) },
        text = {
            Column(Modifier.verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                Text(dialog.secret.secret, fontFamily = FontFamily.Monospace, style = MaterialTheme.typography.titleMedium)
                Text(stringResource(R.string.security_secret_text), color = MaterialTheme.colorScheme.onSurfaceVariant)
                FilledTonalButton(onClick = { copy(dialog.secret.secret) }, modifier = Modifier.fillMaxWidth()) {
                    Text(stringResource(if (copied) R.string.security_copied else R.string.account_copy))
                }
                if (warned && !copied) Text(stringResource(R.string.security_secret_close_warning), color = MaterialTheme.colorScheme.error)
            }
        },
        confirmButton = { TextButton(onClick = { if (copied || warned) c.dismissDialog() else warned = true }) { Text(stringResource(R.string.account_done)) } },
    )
}

private fun expiryText(e: Expiry): Int = when (e) {
    Expiry.NEVER -> R.string.security_expiry_never
    Expiry.DAYS_30 -> R.string.security_expiry_30_days
    Expiry.YEAR_1 -> R.string.security_expiry_1_year
}
