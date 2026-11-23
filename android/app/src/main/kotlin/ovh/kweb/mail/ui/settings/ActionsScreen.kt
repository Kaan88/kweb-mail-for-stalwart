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
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.ActionRules
import ovh.kweb.mail.ui.UiPrefs
import ovh.kweb.mail.ui.list.Groups
import ovh.kweb.mail.ui.theme.KwebSwitch

/**
 * "Swipes and actions": what a swipe on a conversation does, whether a delete asks first, how long Undo of a send lasts and
 * whether Reply all leads. Kept in the app's own preferences, the same for every account.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ActionsScreen(onBack: () -> Unit) {
    val prefs = UiPrefs(LocalContext.current)
    var right by remember { mutableStateOf(prefs.swipeRight) }
    var left by remember { mutableStateOf(prefs.swipeLeft) }
    var confirm by remember { mutableStateOf(prefs.confirmDelete) }
    var undo by remember { mutableStateOf(prefs.undoSeconds) }
    var replyAll by remember { mutableStateOf(prefs.replyAllDefault) }
    var completion by remember { mutableStateOf(prefs.smartCompletion) }
    var asking by remember { mutableStateOf<String?>(null) }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(stringResource(R.string.actions_title)) },
                navigationIcon = {
                    IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = stringResource(R.string.back)) }
                },
                colors = TopAppBarDefaults.topAppBarColors(containerColor = MaterialTheme.colorScheme.background),
            )
        },
        containerColor = MaterialTheme.colorScheme.background,
    ) { padding ->
        Column(Modifier.padding(padding).fillMaxWidth().verticalScroll(rememberScrollState()).padding(bottom = 24.dp)) {
            SectionTitle(stringResource(R.string.actions_swipes))
            Group {
                SettingsRow(Groups.shape(0, 2), stringResource(R.string.actions_swipe_right), stringResource(right.label), onClick = { asking = "right" })
                SettingsRow(Groups.shape(1, 2), stringResource(R.string.actions_swipe_left), stringResource(left.label), onClick = { asking = "left" })
            }
            Text(
                stringResource(R.string.actions_swipes_note),
                modifier = Modifier.fillMaxWidth().padding(horizontal = 32.dp, vertical = 12.dp),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )

            SectionTitle(stringResource(R.string.actions_deleting))
            Group {
                SettingsRow(
                    Groups.shape(0, 1),
                    stringResource(R.string.actions_confirm_delete),
                    stringResource(if (confirm) R.string.actions_confirm_delete_on else R.string.actions_confirm_delete_off),
                    trailing = { KwebSwitch(checked = confirm, onCheckedChange = { confirm = it; prefs.confirmDelete = it }) },
                    onClick = { confirm = !confirm; prefs.confirmDelete = confirm },
                )
            }

            SectionTitle(stringResource(R.string.actions_sending))
            Group {
                SettingsRow(
                    Groups.shape(0, 3),
                    stringResource(R.string.actions_undo_time),
                    stringResource(R.string.actions_undo_seconds, undo),
                    onClick = { asking = "undo" },
                )
                SettingsRow(
                    Groups.shape(1, 3),
                    stringResource(R.string.actions_reply_all),
                    stringResource(if (replyAll) R.string.actions_reply_all_on else R.string.actions_reply_all_off),
                    trailing = { KwebSwitch(checked = replyAll, onCheckedChange = { replyAll = it; prefs.replyAllDefault = it }) },
                    onClick = { replyAll = !replyAll; prefs.replyAllDefault = replyAll },
                )
                SettingsRow(
                    Groups.shape(2, 3),
                    stringResource(R.string.completion_setting),
                    stringResource(if (completion) R.string.completion_setting_on else R.string.completion_setting_off),
                    trailing = { KwebSwitch(checked = completion, onCheckedChange = { completion = it; prefs.smartCompletion = it }) },
                    onClick = { completion = !completion; prefs.smartCompletion = completion },
                )
            }

            Group(Modifier.padding(top = 24.dp)) {
                SettingsRow(
                    Groups.shape(0, 1),
                    stringResource(R.string.actions_reset),
                    stringResource(R.string.actions_reset_detail),
                    onClick = {
                        prefs.resetActions()
                        right = prefs.swipeRight
                        left = prefs.swipeLeft
                        confirm = prefs.confirmDelete
                        undo = prefs.undoSeconds
                        replyAll = prefs.replyAllDefault
                    },
                )
            }
        }
    }

    when (asking) {
        "right" -> ChoiceDialog(
            R.string.actions_swipe_right,
            ActionRules.Swipe.entries.map { Option(it, it.label) },
            right,
            onPick = { right = it; prefs.swipeRight = it },
            onDismiss = { asking = null },
        )
        "left" -> ChoiceDialog(
            R.string.actions_swipe_left,
            ActionRules.Swipe.entries.map { Option(it, it.label) },
            left,
            onPick = { left = it; prefs.swipeLeft = it },
            onDismiss = { asking = null },
        )
        "undo" -> ChoiceDialog(
            R.string.actions_undo_time,
            ActionRules.UNDO_CHOICES.map { Option(it, UNDO_LABELS.getValue(it)) },
            undo,
            onPick = { undo = it; prefs.undoSeconds = it },
            onDismiss = { asking = null },
        )
    }
}

private val UNDO_LABELS = mapOf(
    5 to R.string.actions_undo_5,
    10 to R.string.actions_undo_10,
    20 to R.string.actions_undo_20,
    30 to R.string.actions_undo_30,
)
