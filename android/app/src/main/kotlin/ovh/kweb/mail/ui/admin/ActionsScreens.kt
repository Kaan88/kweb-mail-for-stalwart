package ovh.kweb.mail.ui.admin

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.LifecycleResumeEffect
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.list.Groups
import uniffi.kweb_core.AuthResult
import uniffi.kweb_core.DmarcCheckResult
import uniffi.kweb_core.SpamCheckResult

/**
 * Actions: what the server's web interface lists under Actions. Each simple action asks first and then shows what the
 * server returned; the two checks open a form of their own.
 */
@Composable
fun ActionsScreen(onBack: () -> Unit, onOpen: (AdminScreen) -> Unit, vm: AdminActionsViewModel = viewModel()) {
    val c = vm.actions
    val state by c.state.collectAsStateWithLifecycle()
    LifecycleResumeEffect(vm) {
        c.start()
        onPauseOrDispose { }
    }
    AdminScaffold(title = stringResource(R.string.admin_area_maintenance), onBack = onBack) { padding ->
        when (state.phase) {
            ActionsPhase.LOADING -> LoadingBox(Modifier.padding(padding))
            ActionsPhase.FAILED -> FailureBox(stringResource(R.string.admin_maintenance_failed), onRetry = c::start, modifier = Modifier.padding(padding))
            ActionsPhase.READY -> Column(Modifier.padding(padding).fillMaxSize().verticalScroll(rememberScrollState())) {
                Notices(state.error, state.info, c::dismissError, c::dismissInfo)
                if (state.warnings.isNotEmpty()) {
                    SectionLabel(pluralStringResource(R.plurals.admin_actions_warnings, state.warnings.size))
                    AdminGroup {
                        state.warnings.forEachIndexed { index, warning -> AdminRow(Groups.shape(index, state.warnings.size), warning) }
                    }
                }
                Note(stringResource(R.string.admin_actions_note))
                for (group in ServerActionRules.visible(state.allowed)) {
                    SectionLabel(stringResource(group.label))
                    AdminGroup {
                        group.actions.forEachIndexed { index, what ->
                            val words = ServerActionRules.words(what)
                            AdminRow(
                                Groups.shape(index, group.actions.size),
                                stringResource(words.name),
                                detail = stringResource(words.detail),
                                trailing = { if (state.running == what) CircularProgressIndicator(Modifier.size(20.dp), strokeWidth = 2.dp) },
                                onClick = if (state.running == null) ({ c.ask(what) }) else null,
                            )
                        }
                    }
                }
                val checks = listOfNotNull(
                    if (state.mayDmarc) Triple(AdminScreen.DmarcCheck, R.string.admin_dmarc_title, R.string.admin_dmarc_detail) else null,
                    if (state.maySpam) Triple(AdminScreen.SpamCheck, R.string.admin_spam_title, R.string.admin_spam_detail) else null,
                )
                if (checks.isNotEmpty()) {
                    SectionLabel(stringResource(R.string.admin_actions_checks))
                    AdminGroup {
                        checks.forEachIndexed { index, (screen, title, detail) ->
                            AdminRow(Groups.shape(index, checks.size), stringResource(title), detail = stringResource(detail), onClick = { onOpen(screen) })
                        }
                    }
                }
                if (state.allowed.isEmpty() && checks.isEmpty()) Note(stringResource(R.string.admin_actions_none))
            }
        }
    }
    state.asking?.let { what ->
        val words = ServerActionRules.words(what)
        ConfirmDialog(
            title = stringResource(R.string.admin_action_confirm_title, stringResource(words.name)),
            text = stringResource(words.detail) + "\n\n" + stringResource(R.string.admin_action_confirm_note),
            confirm = stringResource(R.string.admin_action_run),
            onConfirm = c::confirm,
            onDismiss = c::dismissDialog,
        )
    }
}

/** DMARC check: the server tests one SMTP session for SPF, DKIM and DMARC. */
@Composable
fun DmarcCheckScreen(onBack: () -> Unit, vm: AdminDmarcCheckViewModel = viewModel()) {
    val c = vm.check
    val state by c.state.collectAsStateWithLifecycle()
    val form = state.form
    AdminScaffold(title = stringResource(R.string.admin_dmarc_title), onBack = onBack) { padding ->
        Column(Modifier.padding(padding).fillMaxSize().verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Notices(state.error, null, c::dismissError) {}
            Note(stringResource(R.string.admin_dmarc_note))
            val side = Modifier.padding(horizontal = 12.dp)
            AdminField(form.remoteIp, { v -> c.edit { it.copy(remoteIp = v) } }, stringResource(R.string.admin_check_remote_ip), side, keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri, imeAction = ImeAction.Next))
            AdminField(form.ehloDomain, { v -> c.edit { it.copy(ehloDomain = v) } }, stringResource(R.string.admin_check_ehlo), side, keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri, imeAction = ImeAction.Next))
            AdminField(form.mailFrom, { v -> c.edit { it.copy(mailFrom = v) } }, stringResource(R.string.admin_check_mail_from), side, keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Email, imeAction = ImeAction.Next))
            AdminField(
                form.spfEhloDomain, { v -> c.edit { it.copy(spfEhloDomain = v) } }, stringResource(R.string.admin_dmarc_spf_ehlo), side,
                supporting = stringResource(R.string.admin_dmarc_spf_ehlo_hint), keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri, imeAction = ImeAction.Next),
            )
            AdminField(
                form.spfMailFromDomain, { v -> c.edit { it.copy(spfMailFromDomain = v) } }, stringResource(R.string.admin_dmarc_spf_mail_from), side,
                supporting = stringResource(R.string.admin_dmarc_spf_mail_from_hint), keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri, imeAction = ImeAction.Next),
            )
            AdminField(form.rcptTo, { v -> c.edit { it.copy(rcptTo = v) } }, stringResource(R.string.admin_dmarc_rcpt), side, keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Email, imeAction = ImeAction.Next))
            AdminField(
                form.message, { v -> c.edit { it.copy(message = v) } }, stringResource(R.string.admin_dmarc_message), side,
                singleLine = false, minLines = 4, keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Text, imeAction = ImeAction.Default),
            )
            RunButton(enabled = form.canRun && !state.running, running = state.running, onClick = c::run)
            state.result?.let { DmarcResult(it) }
        }
    }
}

@Composable
private fun RunButton(enabled: Boolean, running: Boolean, onClick: () -> Unit) {
    FilledTonalButton(onClick = onClick, enabled = enabled, modifier = Modifier.fillMaxWidth().padding(horizontal = 12.dp)) {
        if (running) CircularProgressIndicator(Modifier.size(18.dp), strokeWidth = 2.dp)
        else Text(stringResource(R.string.admin_check_run))
    }
}

@Composable
private fun AuthPill(result: AuthResult) {
    Pill(result.outcome.replaceFirstChar { it.uppercase() }, error = !result.good)
}

@Composable
private fun DmarcResult(r: DmarcCheckResult) {
    SectionLabel(stringResource(R.string.admin_check_result))
    val rows: List<Triple<String, String?, @Composable () -> Unit>> = listOf(
        Triple(stringResource(R.string.admin_dmarc_r_spf_ehlo), r.spfEhlo.details.ifEmpty { null }) { AuthPill(r.spfEhlo) },
        Triple(stringResource(R.string.admin_dmarc_r_spf_mail_from), r.spfMailFrom.details.ifEmpty { null }) { AuthPill(r.spfMailFrom) },
        Triple(stringResource(R.string.admin_dmarc_r_dkim), null) {
            Pill(stringResource(if (r.dkimPass) R.string.admin_check_pass else R.string.admin_check_fail), error = !r.dkimPass)
        },
        Triple(stringResource(R.string.admin_dmarc_r_dmarc), r.dmarc.details.ifEmpty { null }) {
            Pill(stringResource(if (r.dmarcPass) R.string.admin_check_pass else R.string.admin_check_fail), error = !r.dmarcPass)
        },
        Triple(stringResource(R.string.admin_dmarc_r_policy), null) { Text(stringResource(ServerActionRules.policyWords(r.dmarcPolicy))) },
        Triple(stringResource(R.string.admin_check_time), null) { Text(stringResource(R.string.admin_check_ms, r.elapsedMs.toLong())) },
    )
    AdminGroup {
        rows.forEachIndexed { index, (title, detail, trailing) ->
            AdminRow(Groups.shape(index, rows.size), title, detail = detail, trailing = trailing)
        }
    }
    Spacer(Modifier.height(32.dp))
}

/** Spam check: the server's spam filter judges a pasted message. */
@Composable
fun SpamCheckScreen(onBack: () -> Unit, vm: AdminSpamCheckViewModel = viewModel()) {
    val c = vm.check
    val state by c.state.collectAsStateWithLifecycle()
    val form = state.form
    AdminScaffold(title = stringResource(R.string.admin_spam_title), onBack = onBack) { padding ->
        Column(Modifier.padding(padding).fillMaxSize().verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Notices(state.error, null, c::dismissError) {}
            Note(stringResource(R.string.admin_spam_note))
            val side = Modifier.padding(horizontal = 12.dp)
            AdminField(
                form.message, { v -> c.edit { it.copy(message = v) } }, stringResource(R.string.admin_spam_message), side,
                singleLine = false, minLines = 6, keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Text, imeAction = ImeAction.Default),
            )
            AdminField(form.remoteIp, { v -> c.edit { it.copy(remoteIp = v) } }, stringResource(R.string.admin_check_remote_ip), side, keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri, imeAction = ImeAction.Next))
            AdminField(form.ehloDomain, { v -> c.edit { it.copy(ehloDomain = v) } }, stringResource(R.string.admin_check_ehlo), side, keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri, imeAction = ImeAction.Next))
            AdminField(form.mailFrom, { v -> c.edit { it.copy(mailFrom = v) } }, stringResource(R.string.admin_check_mail_from), side, keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Email, imeAction = ImeAction.Next))
            AdminField(form.rcptTo, { v -> c.edit { it.copy(rcptTo = v) } }, stringResource(R.string.admin_spam_rcpt), side, keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Email, imeAction = ImeAction.Next))
            AdminField(form.authenticatedAs, { v -> c.edit { it.copy(authenticatedAs = v) } }, stringResource(R.string.admin_spam_auth), side, keyboardOptions = KeyboardOptions(imeAction = ImeAction.Done))
            AdminGroup {
                AdminRow(
                    Groups.shape(0, 1),
                    stringResource(R.string.admin_spam_tls),
                    trailing = { Switch(checked = form.isTls, onCheckedChange = null) },
                    onClick = { c.edit { it.copy(isTls = !it.isTls) } },
                )
            }
            RunButton(enabled = form.canRun && !state.running, running = state.running, onClick = c::run)
            state.result?.let { SpamResult(it) }
        }
    }
}

@Composable
private fun SpamResult(r: SpamCheckResult) {
    SectionLabel(stringResource(R.string.admin_check_result))
    val extra = listOfNotNull(
        r.aiGate.takeIf { it.isNotBlank() }?.let { stringResource(R.string.admin_spam_ai_gate) to it },
        r.aiVerdict.takeIf { it.isNotBlank() }?.let { stringResource(R.string.admin_spam_ai_verdict) to it },
    )
    val size = 2 + extra.size
    AdminGroup {
        AdminRow(
            Groups.shape(0, size),
            stringResource(R.string.admin_spam_verdict),
            trailing = { Pill(stringResource(ServerActionRules.verdictWords(r.result)), error = !ServerActionRules.verdictGood(r.result)) },
        )
        AdminRow(Groups.shape(1, size), stringResource(R.string.admin_spam_score), trailing = { Text(ServerActionRules.scoreText(r.score)) })
        extra.forEachIndexed { i, (title, value) -> AdminRow(Groups.shape(2 + i, size), title, detail = value) }
    }
    val shown = ServerActionRules.mattering(r.tags)
    if (shown.isNotEmpty()) {
        SectionLabel(stringResource(R.string.admin_spam_tags))
        AdminGroup {
            shown.forEachIndexed { index, tag ->
                AdminRow(
                    Groups.shape(index, shown.size),
                    tag.name,
                    trailing = {
                        val words = ServerActionRules.dispositionWords(tag.disposition)
                        if (words != null) Pill(stringResource(words), error = true) else Text(ServerActionRules.tagScoreText(tag.score))
                    },
                )
            }
        }
    }
    val quiet = r.tags.size - shown.size
    if (quiet > 0) Note(pluralStringResource(R.plurals.admin_spam_quiet_tags, quiet, quiet))
    Spacer(Modifier.height(32.dp))
}
