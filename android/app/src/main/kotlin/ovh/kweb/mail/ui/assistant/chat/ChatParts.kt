package ovh.kweb.mail.ui.assistant.chat

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.Send
import androidx.compose.material.icons.filled.Close
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.FilledIconButton
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.FilledTonalIconButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TextField
import androidx.compose.material3.TextFieldDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import java.time.ZoneId
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.calendar.NoticeBar
import ovh.kweb.mail.ui.calendar.rememberLocale
import ovh.kweb.mail.ui.list.Groups
import uniffi.kweb_core.AiAbout
import uniffi.kweb_core.AiChange
import uniffi.kweb_core.AiLink
import uniffi.kweb_core.AiMessageRef
import uniffi.kweb_core.AiStep

@Composable
internal fun rememberChatWords(): ChatWords {
    val resources = LocalResources.current
    return remember(resources) { ResourceChatWords(resources) }
}

/** The user's own message: a bubble in the primary container at the end of the line. */
@Composable
internal fun MineBubble(text: String) {
    val colors = MaterialTheme.colorScheme
    Row(Modifier.fillMaxWidth().padding(start = 48.dp), horizontalArrangement = Arrangement.End) {
        Text(
            text,
            modifier = Modifier
                .clip(RoundedCornerShape(20.dp))
                .background(colors.primaryContainer)
                .padding(horizontal = 16.dp, vertical = 10.dp),
            color = colors.onPrimaryContainer,
            style = MaterialTheme.typography.bodyLarge,
        )
    }
}

/** The assistant's text: plain, selectable. A stopped answer says so under what came. */
@Composable
internal fun TheirsText(item: ChatItem.Theirs) {
    val colors = MaterialTheme.colorScheme
    Column(Modifier.fillMaxWidth().padding(horizontal = 4.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
        if (item.text.isNotEmpty()) {
            SelectionContainer {
                Text(item.text, color = colors.onSurface, style = MaterialTheme.typography.bodyLarge)
            }
        }
        if (item.stopped) {
            Text(stringResource(R.string.chat_stopped), color = colors.onSurfaceVariant, style = MaterialTheme.typography.bodyMedium)
        }
    }
}

/** The tools used in a row, as one group. [active]: the last one is being used now. */
@Composable
internal fun StepsGroup(lines: List<AiStep>, active: Boolean, words: ChatWords) {
    val colors = MaterialTheme.colorScheme
    Column(Modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(3.dp)) {
        lines.forEachIndexed { index, step ->
            Row(
                Modifier
                    .fillMaxWidth()
                    .clip(Groups.shape(index, lines.size))
                    .background(colors.surfaceContainerLow)
                    .padding(horizontal = 20.dp, vertical = 10.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                Text(
                    ChatRules.stepLine(step, words),
                    modifier = Modifier.weight(1f),
                    color = colors.onSurfaceVariant,
                    style = MaterialTheme.typography.bodyMedium,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                )
                if (active && index == lines.lastIndex) CircularProgressIndicator(Modifier.size(16.dp), strokeWidth = 2.dp)
            }
        }
    }
}

/** A line that is not the assistant's text: why a run ended, or that this is a new conversation. */
@Composable
internal fun NoteLine(note: ChatNote) {
    val text = when (note) {
        is ChatNote.StepLimit -> pluralStringResource(R.plurals.chat_end_steps, note.steps, note.steps)
        ChatNote.SizeLimit -> stringResource(R.string.chat_end_size)
        ChatNote.ToolsOff -> stringResource(R.string.chat_tools_off)
        ChatNote.Restarted -> stringResource(R.string.chat_restarted)
        ChatNote.NoAnswer -> stringResource(R.string.chat_no_answer)
        is ChatNote.HeldWith -> stringResource(R.string.chat_held_with, note.service, note.model)
    }
    NoticeBar(text, error = false)
}

@Composable
private fun subjectOf(message: AiMessageRef): String = message.subject.ifBlank { stringResource(R.string.chat_no_subject) }

/** Sender and current folder of a message, as one quiet line. */
@Composable
private fun fromLine(message: AiMessageRef): String = when {
    message.from.isBlank() -> message.mailboxName
    message.mailboxName.isBlank() -> message.from
    else -> stringResource(R.string.chat_from_folder, message.from, message.mailboxName)
}

/**
 * Something the user may open: a row that says what it opens, with a tonal button. A reply draft shows its text (no
 * recipients: those come from the app's reply rules when the draft is made). [busy]: a draft is being made.
 */
@Composable
internal fun LinkChip(link: AiLink, shape: Shape, busy: Boolean, onOpen: () -> Unit) {
    val colors = MaterialTheme.colorScheme
    Row(
        Modifier
            .fillMaxWidth()
            .clip(shape)
            .background(colors.surfaceContainerLow)
            .heightIn(min = 56.dp)
            .padding(start = 20.dp, end = 12.dp, top = 10.dp, bottom = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Column(Modifier.weight(1f)) {
            when (link) {
                is AiLink.Message -> {
                    Text(subjectOf(link.message), style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    Text(fromLine(link.message), color = colors.onSurfaceVariant, style = MaterialTheme.typography.bodyMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
                is AiLink.ReplyDraft -> {
                    Text(
                        stringResource(R.string.chat_reply_to, subjectOf(link.message)),
                        style = MaterialTheme.typography.bodyLarge,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                    Text(link.text.trim(), color = colors.onSurfaceVariant, style = MaterialTheme.typography.bodyMedium, maxLines = 4, overflow = TextOverflow.Ellipsis)
                }
            }
        }
        FilledTonalButton(onClick = onOpen, enabled = !busy, shape = CircleShape) {
            Text(stringResource(if (link is AiLink.ReplyDraft) R.string.chat_open_draft else R.string.chat_open_message))
        }
    }
}

/**
 * A suggested change. While it waits it is the full card: what it would do, every message or every field of the
 * entry it would touch, then Approve and Don't. Nothing is done before Approve. Afterwards it is one row that says what
 * happened, with Undo for as long as the chat lives.
 */
@Composable
internal fun ProposalCard(
    card: ChatItem.Card,
    words: ChatWords,
    onApprove: () -> Unit,
    onDecline: () -> Unit,
    onUndo: () -> Unit,
    /** "Accept, and accept the rest in this chat"; null where the card does not offer it. */
    onApproveAll: (() -> Unit)? = null,
) {
    if (ChatRules.isOpen(card.state)) OpenCard(card, words, onApprove, onDecline, onApproveAll) else ClosedCard(card, words, onApprove, onDecline, onUndo)
}

@Composable
private fun OpenCard(card: ChatItem.Card, words: ChatWords, onApprove: () -> Unit, onDecline: () -> Unit, onApproveAll: (() -> Unit)?) {
    val colors = MaterialTheme.colorScheme
    val change = card.proposal.change
    val applying = card.state == CardState.Applying
    Column(
        Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(Groups.OUTER_DP.dp))
            .background(colors.surfaceContainerHigh)
            .padding(20.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Text(ChatRules.title(change, words), color = colors.onSurface, style = MaterialTheme.typography.titleMedium)
        ChatRules.warning(card.proposal, words)?.let {
            Text(it, color = colors.error, style = MaterialTheme.typography.bodyMedium)
        }
        if (change is AiChange.AddEvent) EventFields(change, words) else MessageList(ChatRules.messages(change))
        if (applying) LinearProgressIndicator(Modifier.fillMaxWidth())
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            FilledTonalButton(onClick = onDecline, enabled = !applying, shape = CircleShape, modifier = Modifier.weight(1f).height(52.dp)) {
                Text(stringResource(R.string.chat_decline), style = MaterialTheme.typography.labelLarge)
            }
            Button(onClick = onApprove, enabled = !applying, shape = CircleShape, modifier = Modifier.weight(1f).height(52.dp)) {
                Text(stringResource(R.string.chat_approve), style = MaterialTheme.typography.labelLarge)
            }
        }
        if (onApproveAll != null) {
            TextButton(onClick = onApproveAll, enabled = !applying, modifier = Modifier.align(Alignment.End)) {
                Text(stringResource(R.string.chat_approve_all))
            }
        }
    }
}

/** Every message of the change: subject, then sender and the folder it is in now. Long lists scroll inside the card. */
@Composable
private fun MessageList(messages: List<AiMessageRef>) {
    val colors = MaterialTheme.colorScheme
    val modifier = if (ChatRules.scrollsInside(messages.size)) {
        Modifier.fillMaxWidth().heightIn(max = 280.dp).verticalScroll(rememberScrollState())
    } else {
        Modifier.fillMaxWidth()
    }
    Column(modifier, verticalArrangement = Arrangement.spacedBy(10.dp)) {
        messages.forEach { message ->
            Column {
                Text(subjectOf(message), color = colors.onSurface, style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(fromLine(message), color = colors.onSurfaceVariant, style = MaterialTheme.typography.bodyMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
        }
    }
}

/** Every field of the entry the card would add, nothing shortened. */
@Composable
private fun EventFields(change: AiChange.AddEvent, words: ChatWords) {
    val colors = MaterialTheme.colorScheme
    val locale = rememberLocale()
    val lines = remember(change, locale, words) {
        ChatRules.eventLines(change.input, change.calendarName, locale, ZoneId.systemDefault().id, words)
    }
    Column(Modifier.fillMaxWidth().heightIn(max = 360.dp).verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(10.dp)) {
        lines.forEach { line ->
            Column {
                Text(
                    stringResource(
                        when (line.field) {
                            ChatRules.EventField.TITLE -> R.string.chat_event_title
                            ChatRules.EventField.WHEN -> R.string.chat_event_when
                            ChatRules.EventField.DUE -> R.string.chat_event_due
                            ChatRules.EventField.CALENDAR -> R.string.chat_event_calendar
                            ChatRules.EventField.PLACE -> R.string.chat_event_place
                            ChatRules.EventField.DESCRIPTION -> R.string.chat_event_description
                            ChatRules.EventField.REMINDERS -> R.string.chat_event_reminders
                            ChatRules.EventField.REPEATS -> R.string.chat_event_repeats
                        },
                    ),
                    color = colors.onSurfaceVariant,
                    style = MaterialTheme.typography.labelMedium,
                )
                Text(line.value, color = colors.onSurface, style = MaterialTheme.typography.bodyLarge)
            }
        }
    }
}

/**
 * The chip "About: subject" of a chat opened from a conversation. The cross shows only while the first message has not
 * been sent: after that the text has gone to the service and cannot be taken back.
 */
@Composable
internal fun AboutChip(about: AiAbout, removable: Boolean, onRemove: () -> Unit) {
    val colors = MaterialTheme.colorScheme
    Row(
        Modifier
            .fillMaxWidth()
            .padding(horizontal = 12.dp, vertical = 4.dp)
            .clip(Groups.shape(0, 1))
            .background(colors.secondaryContainer)
            .heightIn(min = 48.dp)
            .padding(start = 20.dp, end = if (removable) 4.dp else 20.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f)) {
            Text(
                stringResource(R.string.chat_about, about.subject.ifBlank { stringResource(R.string.chat_about_no_subject) }),
                color = colors.onSecondaryContainer,
                style = MaterialTheme.typography.bodyLarge,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            if (about.isCut) {
                Text(stringResource(R.string.chat_about_cut), color = colors.onSecondaryContainer, style = MaterialTheme.typography.bodySmall)
            }
        }
        if (removable) {
            IconButton(onClick = onRemove) {
                Icon(Icons.Filled.Close, contentDescription = stringResource(R.string.chat_about_remove), tint = colors.onSecondaryContainer)
            }
        }
    }
}

/** The row a card becomes: what was done (or was not), and Undo or Try again where that is possible. */
@Composable
private fun ClosedCard(card: ChatItem.Card, words: ChatWords, onApprove: () -> Unit, onDecline: () -> Unit, onUndo: () -> Unit) {
    val colors = MaterialTheme.colorScheme
    val change = card.proposal.change
    val state = card.state
    val done = state is CardState.Applied || state == CardState.Undoing || state == CardState.Undone
    val status: String? = when (state) {
        CardState.Undone -> stringResource(R.string.chat_undone)
        CardState.Declined -> stringResource(R.string.chat_declined)
        is CardState.Applied -> if (state.automatically) stringResource(R.string.chat_done_automatically) else null
        is CardState.Failed -> state.message.resolve(LocalResources.current)
        else -> null
    }
    Row(
        Modifier
            .fillMaxWidth()
            .clip(Groups.shape(0, 1))
            .background(colors.surfaceContainerLow)
            .heightIn(min = 56.dp)
            .padding(start = 20.dp, end = 8.dp, top = 8.dp, bottom = 8.dp)
            .semantics { liveRegion = LiveRegionMode.Polite },
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Column(Modifier.weight(1f)) {
            Text(
                if (done) ChatRules.doneLine(change, words) else ChatRules.title(change, words),
                color = colors.onSurface,
                style = MaterialTheme.typography.bodyLarge,
            )
            if (status != null) {
                Text(status, color = if (state is CardState.Failed) colors.error else colors.onSurfaceVariant, style = MaterialTheme.typography.bodyMedium)
            }
        }
        when {
            state == CardState.Undoing -> Box(Modifier.padding(end = 12.dp)) { CircularProgressIndicator(Modifier.size(20.dp), strokeWidth = 2.dp) }
            ChatRules.undo(state) != null -> TextButton(onClick = onUndo) { Text(stringResource(R.string.action_undo)) }
            ChatRules.approve(state) != null -> {
                TextButton(onClick = onDecline) { Text(stringResource(R.string.chat_decline)) }
                TextButton(onClick = onApprove) { Text(stringResource(R.string.chat_try_again)) }
            }
        }
    }
}

/** The input at the bottom: a pill to type in and Send, which is Stop while an answer is being written. */
@Composable
internal fun InputPill(text: String, onText: (String) -> Unit, enabled: Boolean, running: Boolean, canSend: Boolean, onSend: () -> Unit, onStop: () -> Unit) {
    val colors = MaterialTheme.colorScheme
    Row(
        Modifier.fillMaxWidth().padding(start = 12.dp, end = 12.dp, top = 4.dp, bottom = 8.dp),
        verticalAlignment = Alignment.Bottom,
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        TextField(
            value = text,
            onValueChange = onText,
            modifier = Modifier.weight(1f),
            enabled = enabled,
            placeholder = { Text(stringResource(R.string.chat_input)) },
            keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Sentences),
            minLines = 1,
            maxLines = 6,
            shape = RoundedCornerShape(Groups.OUTER_DP.dp),
            colors = TextFieldDefaults.colors(
                focusedContainerColor = colors.surfaceContainerHigh,
                unfocusedContainerColor = colors.surfaceContainerHigh,
                disabledContainerColor = colors.surfaceContainerLow,
                focusedIndicatorColor = Color.Transparent,
                unfocusedIndicatorColor = Color.Transparent,
                disabledIndicatorColor = Color.Transparent,
            ),
        )
        if (running) {
            FilledTonalIconButton(onClick = onStop, modifier = Modifier.size(56.dp)) {
                Icon(Icons.Filled.Close, contentDescription = stringResource(R.string.chat_stop))
            }
        } else {
            FilledIconButton(onClick = onSend, enabled = canSend, modifier = Modifier.size(56.dp)) {
                Icon(Icons.AutoMirrored.Filled.Send, contentDescription = stringResource(R.string.chat_send))
            }
        }
    }
}
