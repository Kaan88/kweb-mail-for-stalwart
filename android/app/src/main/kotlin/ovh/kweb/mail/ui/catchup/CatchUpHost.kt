package ovh.kweb.mail.ui.catchup

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Close
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import java.text.DateFormat
import java.util.Date
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.attention.AttentionViewModel
import ovh.kweb.mail.ui.attention.ResourceSparkleWords
import ovh.kweb.mail.ui.attention.SparkleActions
import ovh.kweb.mail.ui.attention.SparkleBoxes
import ovh.kweb.mail.ui.attention.SparkleDialog
import ovh.kweb.mail.ui.attention.SparkleEvents
import ovh.kweb.mail.ui.attention.SparkleRules
import ovh.kweb.mail.ui.attention.SparkleRunViewModel
import ovh.kweb.mail.ui.calendar.EventPrefill
import uniffi.kweb_core.AiCatchUp

/**
 * What "Catch me up" shows: the question before a run, the wait with Stop, and the result as a sheet of lines grouped by
 * sender. A line opens its mail. The result makes no change. Put this once on a screen that has the entry.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun CatchUpHost(
    vm: CatchUpViewModel,
    onOpenThread: (threadId: String, messageId: String) -> Unit,
    onAddToCalendar: (EventPrefill) -> Unit,
) {
    val state by vm.state.collectAsStateWithLifecycle()
    val colors = MaterialTheme.colorScheme
    when (val s = state) {
        CatchUpState.Idle -> Unit
        is CatchUpState.Asking -> AskDialog(vm, s.ask)
        else -> {
            val sheet = rememberModalBottomSheetState(skipPartiallyExpanded = s is CatchUpState.Done)
            ModalBottomSheet(
                onDismissRequest = { if (s is CatchUpState.Running) vm.stop() else vm.dismiss() },
                sheetState = sheet,
                containerColor = colors.surfaceContainerLow,
            ) {
                Column(Modifier.fillMaxWidth().navigationBarsPadding()) {
                    Row(Modifier.fillMaxWidth().padding(start = 24.dp, end = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                        Icon(painterResource(R.drawable.ic_sparkle), contentDescription = null, tint = colors.primary, modifier = Modifier.size(20.dp))
                        Text(
                            stringResource(R.string.catchup_title),
                            style = MaterialTheme.typography.titleLarge,
                            modifier = Modifier.weight(1f).padding(start = 12.dp),
                        )
                        IconButton(onClick = { if (s is CatchUpState.Running) vm.stop() else vm.dismiss() }) {
                            Icon(Icons.Filled.Close, contentDescription = stringResource(R.string.catchup_close))
                        }
                    }
                    when (s) {
                        is CatchUpState.Running -> RunningBody(vm)
                        is CatchUpState.Message -> MessageBody(vm, s)
                        is CatchUpState.Done -> DoneBody(vm, s, onOpenThread, onAddToCalendar)
                        else -> Unit
                    }
                }
            }
        }
    }
}

@Composable
private fun AskDialog(vm: CatchUpViewModel, ask: CatchUpAsk) {
    val resources = LocalResources.current
    val words = remember(resources) { ResourceSparkleWords(resources) }
    val preview = ask.preview
    val scope = remember(preview) {
        val mails = words.mails(preview.messages.toInt(), SparkleRules.senderNames(preview.senders, words))
        if (preview.isCut) words.shortened(mails) else mails
    }
    val text = if (ask.ask == SparkleRules.Ask.FIRST) {
        stringResource(R.string.assistant_consent_text, scope, preview.host, preview.model)
    } else {
        stringResource(R.string.att_run_ask_text, scope, preview.host, preview.model)
    }
    AlertDialog(
        onDismissRequest = vm::decline,
        icon = { Icon(painterResource(R.drawable.ic_sparkle), contentDescription = null) },
        title = { Text(stringResource(R.string.assistant_consent_title, preview.host)) },
        text = { Text(text) },
        confirmButton = { TextButton(onClick = vm::confirm) { Text(stringResource(R.string.assistant_consent_send)) } },
        dismissButton = { TextButton(onClick = vm::decline) { Text(stringResource(R.string.cancel)) } },
    )
}

@Composable
private fun RunningBody(vm: CatchUpViewModel) {
    Column(Modifier.fillMaxWidth().padding(24.dp), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(16.dp)) {
        CircularProgressIndicator()
        Text(stringResource(R.string.catchup_running), style = MaterialTheme.typography.bodyLarge)
        TextButton(onClick = vm::stop) { Text(stringResource(R.string.att_run_stop)) }
    }
}

@Composable
private fun MessageBody(vm: CatchUpViewModel, s: CatchUpState.Message) {
    val resources = LocalResources.current
    Column(Modifier.fillMaxWidth().padding(horizontal = 24.dp, vertical = 16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Text(s.text.resolve(resources), style = MaterialTheme.typography.bodyLarge)
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            if (s.canRetry) FilledTonalButton(onClick = vm::start) { Text(stringResource(R.string.retry)) }
            TextButton(onClick = vm::dismiss) { Text(stringResource(R.string.catchup_close)) }
        }
    }
}

@Composable
private fun DoneBody(
    vm: CatchUpViewModel,
    s: CatchUpState.Done,
    onOpenThread: (String, String) -> Unit,
    onAddToCalendar: (EventPrefill) -> Unit,
) {
    val resources = LocalResources.current
    val colors = MaterialTheme.colorScheme
    val result: AiCatchUp = s.result
    val words = remember(resources) { ResourceSparkleWords(resources) }
    val locale = LocalConfiguration.current.locales[0]
    val time = remember(s.at, locale) { DateFormat.getTimeInstance(DateFormat.SHORT, locale).format(Date(s.at * 1000)) }
    val since = remember(result.since, locale) {
        DateFormat.getDateTimeInstance(DateFormat.MEDIUM, DateFormat.SHORT, locale).format(Date(result.since * 1000))
    }
    val needing = remember(result) { CatchUpRules.needingAnswer(result.groups.flatMap { it.items }) }

    val sparkle: SparkleRunViewModel = viewModel()
    val attention: AttentionViewModel = viewModel()
    val snackbar = remember { SnackbarHostState() }
    SparkleEvents(sparkle, attention, snackbar, onOpenDraft = { vm.dismiss() })
    SparkleDialog(sparkle) { it == CatchUpRules.FINDER_ID }
    val finding = remember(result) { vm.finderFinding(result) }
    val actions = remember(onAddToCalendar, onOpenThread) {
        SparkleActions(
            editNew = { input -> vm.dismiss(); onAddToCalendar(SparkleRules.prefillOf(input)) },
            editEvent = { _, _, _ -> },
            openMail = { t, m -> vm.dismiss(); onOpenThread(t, m) },
        )
    }

    Column(
        Modifier.fillMaxWidth().verticalScroll(rememberScrollState()).padding(horizontal = 16.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Text(
            stringResource(R.string.catchup_since, since),
            style = MaterialTheme.typography.labelLarge,
            color = colors.onSurfaceVariant,
            modifier = Modifier.padding(horizontal = 8.dp),
        )
        if (needing > 0) {
            Text(
                pluralStringResource(R.plurals.catchup_needing, needing, needing),
                style = MaterialTheme.typography.bodyMedium,
                modifier = Modifier.padding(horizontal = 8.dp),
            )
        }
        for (group in result.groups) {
            Text(
                group.sender,
                style = MaterialTheme.typography.titleSmall,
                fontWeight = FontWeight.SemiBold,
                modifier = Modifier.padding(start = 8.dp, top = 8.dp),
            )
            for (item in group.items) {
                Surface(
                    color = colors.surfaceContainer,
                    shape = RoundedCornerShape(16.dp),
                    modifier = Modifier.fillMaxWidth().clickable {
                        vm.dismiss()
                        onOpenThread(item.message.threadId, item.message.id)
                    },
                ) {
                    Column(Modifier.padding(horizontal = 16.dp, vertical = 12.dp), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                        Text(item.message.subject, style = MaterialTheme.typography.labelLarge, color = colors.onSurfaceVariant)
                        Text(item.line, style = MaterialTheme.typography.bodyLarge)
                        if (item.needsAnswer) {
                            Text(stringResource(R.string.catchup_needs_answer), style = MaterialTheme.typography.labelMedium, color = colors.primary)
                        }
                    }
                }
            }
        }
        if (result.more > 0u) {
            Text(
                stringResource(R.string.catchup_more, result.more.toInt()),
                style = MaterialTheme.typography.bodySmall,
                color = colors.onSurfaceVariant,
                modifier = Modifier.padding(horizontal = 8.dp),
            )
        }
        if (finding != null && s.canFind) {
            FilledTonalButton(onClick = { sparkle.start(finding) }, modifier = Modifier.padding(top = 4.dp)) {
                Icon(painterResource(R.drawable.ic_sparkle), contentDescription = null, modifier = Modifier.size(18.dp))
                Text(stringResource(R.string.catchup_find), modifier = Modifier.padding(start = 8.dp))
            }
            SparkleBoxes(setOf(CatchUpRules.FINDER_ID), actions, sparkle)
        }
        if (s.showCost) {
            Text(
                CatchUpRules.costNote(result, s.service, time, words, locale),
                style = MaterialTheme.typography.bodySmall,
                color = colors.onSurfaceVariant,
                modifier = Modifier.padding(horizontal = 8.dp, vertical = 4.dp),
            )
        }
        SnackbarHost(snackbar)
    }
}
