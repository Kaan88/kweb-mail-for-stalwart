package ovh.kweb.mail.ui.assistant

import android.content.ClipData
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.ClipEntry
import androidx.compose.ui.platform.LocalClipboard
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.launch
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.assistant.AssistantRules.Kind
import ovh.kweb.mail.ui.calendar.NoticeBar
import ovh.kweb.mail.ui.calendar.rememberLocale
import uniffi.kweb_core.AiTask

/**
 * The sheet of one task: its title, the answer as it streams (selectable), a quiet line naming the model, and what
 * can be done with it. [onReplace] is given only where an answer can replace text (the message being written).
 * Nothing here sends mail.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun AssistantSheet(session: AssistSession, vm: AssistantViewModel, onReplace: ((String) -> Unit)? = null) {
    ModalBottomSheet(
        onDismissRequest = vm::close,
        sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true),
        containerColor = MaterialTheme.colorScheme.surfaceContainerLow,
    ) {
        Column(
            Modifier.fillMaxWidth().imePadding().padding(horizontal = 20.dp).padding(bottom = 16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Text(
                stringResource(
                    when (session.kind) {
                        Kind.SUMMARY -> R.string.assistant_sheet_summary
                        Kind.DRAFT -> R.string.assistant_sheet_draft
                        Kind.REWRITE -> R.string.assistant_sheet_rewrite
                    },
                ),
                style = MaterialTheme.typography.titleLarge,
            )
            Body(session, vm, onReplace)
        }
    }
    (session.phase as? AssistPhase.Consent)?.let { ConsentDialog(it, vm) }
}

@Composable
private fun Body(session: AssistSession, vm: AssistantViewModel, onReplace: ((String) -> Unit)?) {
    val colors = MaterialTheme.colorScheme
    val phase = session.phase
    if (phase == AssistPhase.Input) {
        // The wish is the user's own text and not a secret; it survives a rotation.
        var instruction by rememberSaveable(session.id) { mutableStateOf((session.task as? AiTask.DraftReply)?.instruction.orEmpty()) }
        OutlinedTextField(
            value = instruction,
            onValueChange = { instruction = it },
            label = { Text(stringResource(R.string.assistant_instruction)) },
            modifier = Modifier.fillMaxWidth(),
            keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Sentences),
            minLines = 1,
            maxLines = 4,
        )
        Buttons(
            secondary = stringResource(R.string.cancel) to vm::close,
            main = stringResource(R.string.assistant_write_draft) to { vm.start(instruction) },
        )
        return
    }

    if (session.text.isNotEmpty()) AnswerText(session.text, streaming = phase == AssistPhase.Running)
    if (phase is AssistPhase.Failed) {
        NoticeBar(phase.message.resolve(LocalResources.current), error = true)
    }
    session.notice?.let { NoticeBar(it.resolve(LocalResources.current), error = true) }
    if (phase == AssistPhase.Stopped) {
        Text(stringResource(R.string.assistant_stopped), color = colors.onSurfaceVariant, style = MaterialTheme.typography.bodyMedium)
    }
    if (session.model.isNotBlank() && phase != AssistPhase.Checking) {
        Text(
            stringResource(R.string.assistant_model_line, session.model),
            color = colors.onSurfaceVariant,
            style = MaterialTheme.typography.bodySmall,
        )
    }

    when (phase) {
        AssistPhase.Input -> Unit
        AssistPhase.Checking, is AssistPhase.Consent, AssistPhase.Running -> {
            LinearProgressIndicator(Modifier.fillMaxWidth())
            // While the settings are read the task is not running at the service yet: Stop is still the way out.
            Buttons(main = stringResource(R.string.assistant_stop) to (if (phase == AssistPhase.Running) vm::stop else vm::close), mainTonal = true)
        }
        AssistPhase.Done -> when (session.kind) {
            Kind.SUMMARY -> {
                val copy = rememberCopy(session.text)
                Buttons(
                    secondary = stringResource(R.string.assistant_close) to vm::close,
                    main = stringResource(if (copy.done) R.string.assistant_copied else R.string.assistant_copy) to copy.action,
                )
            }
            Kind.DRAFT -> Buttons(
                secondary = stringResource(R.string.assistant_close) to vm::close,
                main = stringResource(R.string.assistant_use_as_reply) to vm::useAsReply,
                mainEnabled = !session.busy,
            )
            Kind.REWRITE -> Buttons(
                secondary = stringResource(R.string.cancel) to vm::close,
                main = stringResource(R.string.assistant_replace) to { onReplace?.invoke(session.text) },
                mainEnabled = onReplace != null,
            )
        }
        AssistPhase.Stopped, is AssistPhase.Failed -> Buttons(
            secondary = stringResource(if (session.kind == Kind.REWRITE) R.string.cancel else R.string.assistant_close) to vm::close,
            main = stringResource(if (phase is AssistPhase.Failed) R.string.retry else R.string.assistant_try_again) to vm::retry,
        )
    }
}

/** The answer, selectable, in a box that scrolls and follows the text while it streams. */
@Composable
private fun AnswerText(text: String, streaming: Boolean) {
    val scroll = rememberScrollState()
    LaunchedEffect(text, streaming) {
        if (streaming) scroll.scrollTo(scroll.maxValue)
    }
    Column(Modifier.fillMaxWidth().heightIn(max = 360.dp).verticalScroll(scroll)) {
        SelectionContainer {
            Text(text, style = MaterialTheme.typography.bodyLarge)
        }
    }
}

/** Two pills 52 dp high: [secondary] tonal at the start, [main] filled; or [main] alone, full width. */
@Composable
private fun Buttons(
    main: Pair<String, () -> Unit>,
    secondary: Pair<String, () -> Unit>? = null,
    mainEnabled: Boolean = true,
    mainTonal: Boolean = false,
) {
    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
        if (secondary != null) {
            FilledTonalButton(onClick = secondary.second, shape = CircleShape, modifier = Modifier.weight(1f).height(52.dp)) {
                Text(secondary.first, style = MaterialTheme.typography.labelLarge)
            }
        }
        val modifier = Modifier.weight(1f).height(52.dp)
        if (mainTonal) {
            FilledTonalButton(onClick = main.second, enabled = mainEnabled, shape = CircleShape, modifier = modifier) {
                Text(main.first, style = MaterialTheme.typography.labelLarge)
            }
        } else {
            Button(onClick = main.second, enabled = mainEnabled, shape = CircleShape, modifier = modifier) {
                Text(main.first, style = MaterialTheme.typography.labelLarge)
            }
        }
    }
}

private class CopyState(val done: Boolean, val action: () -> Unit)

/** Puts [text] on the clipboard; [CopyState.done] turns true afterwards so the button can say so. */
@Composable
private fun rememberCopy(text: String): CopyState {
    val clipboard = LocalClipboard.current
    val scope = rememberCoroutineScope()
    var done by remember(text) { mutableStateOf(false) }
    return CopyState(done) {
        scope.launch {
            clipboard.setClipEntry(ClipEntry(ClipData.newPlainText("", text)))
            done = true
        }
    }
}

/** The first task for a service and model: what is sent and where, before anything is. */
@Composable
private fun ConsentDialog(phase: AssistPhase.Consent, vm: AssistantViewModel) {
    val resources = LocalResources.current
    val locale = rememberLocale()
    val words = remember(resources) { ResourceAssistantWords(resources) }
    val scope = remember(phase.preview, locale, words) { ConsentRules.scope(phase.preview, words, locale) }
    val host = phase.preview.host
    AlertDialog(
        onDismissRequest = vm::declineConsent,
        title = { Text(stringResource(R.string.assistant_consent_title, host)) },
        text = { Text(stringResource(R.string.assistant_consent_text, scope, host, phase.preview.model)) },
        confirmButton = { TextButton(onClick = vm::confirmConsent) { Text(stringResource(R.string.assistant_consent_send)) } },
        dismissButton = { TextButton(onClick = vm::declineConsent) { Text(stringResource(R.string.cancel)) } },
    )
}
