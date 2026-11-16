package ovh.kweb.mail.ui.settings

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
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
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.LifecycleResumeEffect
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.accounts.LocalAccountSession
import ovh.kweb.mail.ui.assistant.AssistantRules
import ovh.kweb.mail.ui.assistant.serviceName
import ovh.kweb.mail.ui.attention.AttentionViewModel
import ovh.kweb.mail.ui.list.Groups
import ovh.kweb.mail.ui.theme.KwebSwitch
import uniffi.kweb_core.AiSettings
import uniffi.kweb_core.AttentionSettings
import uniffi.kweb_core.MailScope
import uniffi.kweb_core.TileSwitches

/**
 * "Home and attention": the Home tab, its tiles, what the phone looks out for, and (only when an assistant is set up)
 * how the sparkle buttons ask. Kept per account in the core; attention works with Home off.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun HomeAttentionScreen(onBack: () -> Unit, onOpenAssistant: () -> Unit, vm: AttentionViewModel = viewModel()) {
    val state by vm.state.collectAsStateWithLifecycle()
    val session = LocalAccountSession.current
    val snackbar = remember { SnackbarHostState() }
    var ai by remember { mutableStateOf<AiSettings?>(null) }
    var asking by remember { mutableStateOf(false) }

    LifecycleResumeEffect(vm) {
        vm.load()
        onPauseOrDispose { }
    }
    LaunchedEffect(session) { ai = withContext(Dispatchers.IO) { runCatching { session.core.aiSettings() }.getOrNull() } }
    LaunchedEffect(vm) { vm.notices.collect { snackbar.showSnackbar(it.text) } }

    val s = state.settings
    fun change(next: AttentionSettings) = vm.saveSettings(next)
    fun tiles(t: TileSwitches) = s?.let { change(it.copy(tiles = t)) }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(stringResource(R.string.att_settings_title)) },
                navigationIcon = {
                    IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = stringResource(R.string.back)) }
                },
                colors = TopAppBarDefaults.topAppBarColors(containerColor = MaterialTheme.colorScheme.background),
            )
        },
        snackbarHost = { SnackbarHost(snackbar) },
        containerColor = MaterialTheme.colorScheme.background,
    ) { padding ->
        if (s == null) return@Scaffold
        Column(Modifier.padding(padding).fillMaxWidth().verticalScroll(rememberScrollState()).padding(bottom = 24.dp)) {
            Group {
                SwitchSettingsRow(
                    Groups.shape(0, 1), stringResource(R.string.att_settings_home),
                    stringResource(if (s.home) R.string.att_settings_home_on else R.string.att_settings_home_off), s.home,
                ) { change(s.copy(home = it)) }
            }

            SectionTitle(stringResource(R.string.att_settings_tiles))
            val t = s.tiles
            Group {
                SwitchSettingsRow(Groups.shape(0, 7), stringResource(R.string.home_tile_unread), null, t.unread) { tiles(t.copy(unread = it)) }
                SwitchSettingsRow(Groups.shape(1, 7), stringResource(R.string.home_tile_next_event), null, t.nextEvent) { tiles(t.copy(nextEvent = it)) }
                SwitchSettingsRow(Groups.shape(2, 7), stringResource(R.string.home_tile_due_today), null, t.dueToday) { tiles(t.copy(dueToday = it)) }
                SwitchSettingsRow(Groups.shape(3, 7), stringResource(R.string.home_tile_flagged), null, t.flagged) { tiles(t.copy(flagged = it)) }
                SwitchSettingsRow(Groups.shape(4, 7), stringResource(R.string.home_tile_files), null, t.recentFiles) { tiles(t.copy(recentFiles = it)) }
                SwitchSettingsRow(Groups.shape(5, 7), stringResource(R.string.att_settings_tile_delivery), stringResource(R.string.att_settings_delivery_detail), t.delivery) { tiles(t.copy(delivery = it)) }
                SwitchSettingsRow(Groups.shape(6, 7), stringResource(R.string.att_settings_tile_away), stringResource(R.string.att_settings_away_detail), t.away) { tiles(t.copy(away = it)) }
            }

            SectionTitle(stringResource(R.string.att_settings_attention))
            Group {
                SwitchSettingsRow(Groups.shape(0, 4), stringResource(R.string.att_settings_cal), stringResource(R.string.att_settings_cal_detail), s.calendar) { change(s.copy(calendar = it)) }
                SwitchSettingsRow(Groups.shape(1, 4), stringResource(R.string.att_settings_tasks), stringResource(R.string.att_settings_tasks_detail), s.tasks) { change(s.copy(tasks = it)) }
                SwitchSettingsRow(Groups.shape(2, 4), stringResource(R.string.att_settings_mail), stringResource(R.string.att_settings_mail_detail), s.mail) { change(s.copy(mail = it)) }
                SettingsRow(
                    Groups.shape(3, 4),
                    stringResource(R.string.att_settings_scope),
                    stringResource(if (s.mailScope == MailScope.INBOX) R.string.att_settings_scope_inbox else R.string.att_settings_scope_notified),
                    onClick = { asking = true },
                )
            }

            val assistant = ai
            if (assistant != null && assistant.enabled && assistant.hasKey) {
                SectionTitle(stringResource(R.string.att_settings_assistant))
                Group {
                    SettingsRow(
                        Groups.shape(0, 3),
                        serviceLine(assistant),
                        stringResource(R.string.att_settings_service_detail),
                        icon = { Icon(painterResource(R.drawable.ic_assistant), contentDescription = null) },
                        onClick = onOpenAssistant,
                    )
                    SwitchSettingsRow(Groups.shape(1, 3), stringResource(R.string.att_settings_ask), stringResource(R.string.att_settings_ask_detail), s.askEachRun) { change(s.copy(askEachRun = it)) }
                    SwitchSettingsRow(Groups.shape(2, 3), stringResource(R.string.att_settings_cost), stringResource(R.string.att_settings_cost_detail), s.showCost) { change(s.copy(showCost = it)) }
                }
            }
            Text(
                stringResource(R.string.att_settings_foot),
                modifier = Modifier.fillMaxWidth().padding(horizontal = 32.dp, vertical = 12.dp),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Group {
                SettingsRow(Groups.shape(0, 1), stringResource(R.string.att_settings_reset), onClick = vm::resetDismissed)
            }
        }
    }

    if (asking && s != null) {
        ChoiceDialog(
            R.string.att_settings_scope,
            listOf(Option(MailScope.INBOX, R.string.att_settings_scope_inbox), Option(MailScope.INBOX_AND_NOTIFIED, R.string.att_settings_scope_notified)),
            s.mailScope,
            onPick = { change(s.copy(mailScope = it)) },
            onDismiss = { asking = false },
        )
    }
}

/** "OpenAI · gpt-5": the service and model the sparkle buttons would use. */
@Composable
private fun serviceLine(ai: AiSettings): String {
    val name = AssistantRules.serviceOf(ai.baseUrl).let {
        if (it == AssistantRules.Service.OTHER) AssistantRules.hostOf(ai.baseUrl).ifEmpty { stringResource(R.string.assistant_svc_other) } else stringResource(serviceName(it))
    }
    return stringResource(R.string.assistant_switch_using, name, ai.model)
}

/** A settings row with a switch at its end; a tap anywhere on the row flips it. */
@Composable
private fun SwitchSettingsRow(shape: androidx.compose.ui.graphics.Shape, title: String, detail: String?, checked: Boolean, onChange: (Boolean) -> Unit) {
    SettingsRow(
        shape, title, detail,
        trailing = { KwebSwitch(checked = checked, onCheckedChange = onChange) },
        onClick = { onChange(!checked) },
    )
}
