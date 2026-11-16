package ovh.kweb.mail.ui.attention

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Close
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.Checkbox
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import java.time.Instant
import java.time.LocalDate
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle
import java.util.Locale
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.calendar.rememberLocale
import uniffi.kweb_core.AiFound
import uniffi.kweb_core.AiProposal
import uniffi.kweb_core.AiSparkle
import uniffi.kweb_core.AiVerdict
import uniffi.kweb_core.AiVerdictKind
import uniffi.kweb_core.EventInput

/** What the cards of a run can open; each screen says how (the editor, an event, a mail). */
class SparkleActions(
    /** The editor for a new event or task, filled in from [EventInput]. */
    val editNew: (EventInput) -> Unit,
    /** The editor for an existing event; [place] is put into its place field (the user saves it himself). */
    val editEvent: (eventId: String, recurrenceId: String?, place: String?) -> Unit,
    val openMail: (threadId: String, messageId: String) -> Unit,
)

/** The look of a result card: the tertiary container, as the sparkle button. */
@Composable
private fun RcCard(modifier: Modifier = Modifier, content: @Composable () -> Unit) {
    val colors = MaterialTheme.colorScheme
    Column(
        modifier.fillMaxWidth().clip(RoundedCornerShape(20.dp)).background(colors.tertiaryContainer).padding(start = 14.dp, end = 8.dp, top = 12.dp, bottom = 4.dp),
        verticalArrangement = Arrangement.spacedBy(3.dp),
    ) {
        androidx.compose.runtime.CompositionLocalProvider(androidx.compose.material3.LocalContentColor provides colors.onTertiaryContainer) { content() }
    }
}

@Composable
private fun Kind(text: String) {
    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(5.dp)) {
        Icon(painterResource(R.drawable.ic_sparkle), contentDescription = null, modifier = Modifier.size(14.dp))
        Text(text, fontSize = 11.sp, fontWeight = FontWeight.SemiBold)
    }
}

@Composable
private fun Actions(content: @Composable () -> Unit) {
    Row(Modifier.fillMaxWidth().padding(top = 2.dp), horizontalArrangement = Arrangement.spacedBy(2.dp, Alignment.End), verticalAlignment = Alignment.CenterVertically) { content() }
}

@Composable
private fun QuietButton(text: String, enabled: Boolean = true, onClick: () -> Unit) {
    TextButton(onClick = onClick, enabled = enabled, modifier = Modifier.heightIn(min = 36.dp)) { Text(text, fontSize = 13.sp, maxLines = 1) }
}

@Composable
private fun FilledButton(text: String, enabled: Boolean = true, busy: Boolean = false, onClick: () -> Unit) {
    Button(onClick = onClick, enabled = enabled && !busy, modifier = Modifier.heightIn(min = 36.dp)) {
        if (busy) CircularProgressIndicator(Modifier.size(14.dp), strokeWidth = 2.dp, color = MaterialTheme.colorScheme.onPrimary)
        Text(text, fontSize = 13.sp, maxLines = 1, modifier = if (busy) Modifier.padding(start = 8.dp) else Modifier)
    }
}

@Composable
private fun rememberWords(): ResourceSparkleWords {
    val resources = LocalResources.current
    return remember(resources) { ResourceSparkleWords(resources) }
}

/**
 * The result of one run, in place of the finding's card: cards to add, a place found, a verdict, reply ideas or a plain
 * line, then the cost note. A card is applied only by its own Add (the core's apply path); Edit opens the editor filled in.
 */
@Composable
fun SparkleRunBlock(run: SparkleRun, actions: SparkleActions, vm: SparkleRunViewModel, modifier: Modifier = Modifier) {
    when (run.phase) {
        RunPhase.RUNNING -> RunningCard(run, vm, modifier)
        RunPhase.FAILED -> FailedCard(run, vm, modifier)
        RunPhase.DONE -> DoneBlock(run, actions, vm, modifier)
    }
}

@Composable
private fun RunningCard(run: SparkleRun, vm: SparkleRunViewModel, modifier: Modifier) {
    RcCard(modifier) {
        val service = run.service.ifEmpty { stringResource(R.string.assistant_svc_other) }
        Kind(stringResource(R.string.att_run_running, service))
        LinearProgressIndicator(Modifier.fillMaxWidth().padding(top = 6.dp, end = 6.dp))
        Actions { QuietButton(stringResource(R.string.att_run_stop)) { vm.stop(run.id) } }
    }
}

@Composable
private fun FailedCard(run: SparkleRun, vm: SparkleRunViewModel, modifier: Modifier) {
    val colors = MaterialTheme.colorScheme
    Column(
        modifier.fillMaxWidth().clip(RoundedCornerShape(20.dp)).background(colors.errorContainer).padding(start = 14.dp, end = 8.dp, top = 12.dp, bottom = 4.dp),
        verticalArrangement = Arrangement.spacedBy(3.dp),
    ) {
        Text(run.error.orEmpty().ifBlank { stringResource(R.string.att_run_failed) }, style = MaterialTheme.typography.bodyMedium, color = colors.onErrorContainer)
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(2.dp, Alignment.End)) {
            TextButton(onClick = { vm.dismissRun(run.id) }) { Text(stringResource(R.string.att_run_dismiss), color = colors.onErrorContainer) }
            TextButton(onClick = { vm.retry(run.finding) }) { Text(stringResource(R.string.att_run_retry), color = colors.onErrorContainer) }
        }
    }
}

@Composable
private fun DoneBlock(run: SparkleRun, actions: SparkleActions, vm: SparkleRunViewModel, modifier: Modifier) {
    val result = run.result ?: return
    val locale = rememberLocale()
    val words = rememberWords()
    val colors = MaterialTheme.colorScheme
    val planning = run.task is AiSparkle.PlanMyDay
    var choosing by remember(run.runId) { mutableStateOf(false) }
    Column(modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(6.dp)) {
        if (planning && run.open.isNotEmpty()) {
            PlanCard(run, locale, actions, vm, onEdit = { choosing = true })
        } else {
            for (card in run.open) ProposalCard(run, card, locale, words, actions, vm)
        }
        if (run.extrasShown) {
            result.found?.let { FoundCard(run, it, actions, vm) }
            result.verdict?.let { VerdictCard(run, it, vm) }
            if (result.ideas.isNotEmpty()) IdeasCard(run, result.ideas, vm)
            if (result.text.isNotBlank() && result.found == null && result.ideas.isEmpty()) {
                RcCard {
                    Text(plainLine(run), style = MaterialTheme.typography.bodyMedium)
                    Actions { QuietButton(stringResource(R.string.att_run_ok)) { vm.dismissExtras(run.id) } }
                }
            }
        }
        if (!result.changesAllowed && run.open.isNotEmpty() && !planning) {
            Text(stringResource(R.string.att_run_view_only), style = MaterialTheme.typography.bodySmall, color = colors.onSurfaceVariant, modifier = Modifier.padding(horizontal = 4.dp))
        }
        val addAll = run.addable.size >= 2 && result.changesAllowed && !planning
        if (run.showCost || addAll) {
            Row(Modifier.fillMaxWidth().padding(horizontal = 2.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Box(Modifier.weight(1f)) { if (run.showCost) CostNote(run, locale, words) }
                if (addAll) {
                    FilledButton(
                        if (run.addable.size == 2) stringResource(R.string.att_run_add_both) else stringResource(R.string.att_run_add_n, run.addable.size),
                        enabled = run.busy.isEmpty(),
                    ) { vm.addAll(run.id, run.addable.map { it.id }) }
                }
            }
        }
    }
    if (choosing) {
        PlanLinesDialog(run, locale, onDismiss = { choosing = false }, onAdd = { keep -> choosing = false; vm.addChosen(run.id, keep) })
    }
}

/** "Sent 2 mails to OpenAI · gpt-5 at 09:41 · 2,412 tokens in, 318 out". */
@Composable
private fun CostNote(run: SparkleRun, locale: Locale, words: ResourceSparkleWords) {
    val result = run.result ?: return
    val time = remember(run.at, locale) {
        DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT).withLocale(locale).format(Instant.ofEpochSecond(run.at).atZone(ZoneId.systemDefault()))
    }
    val text = remember(run, locale) { SparkleRules.costNote(run.task, result, run.service, time, words, locale) }
    Row(horizontalArrangement = Arrangement.spacedBy(6.dp), verticalAlignment = Alignment.Top) {
        Icon(painterResource(R.drawable.ic_assistant), contentDescription = null, tint = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(top = 2.dp).size(14.dp))
        Text(text, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}

@Composable
private fun plainLine(run: SparkleRun): String = run.result?.text.orEmpty()

@Composable
private fun ProposalCard(run: SparkleRun, card: AiProposal, locale: Locale, words: ResourceSparkleWords, actions: SparkleActions, vm: SparkleRunViewModel) {
    val input = SparkleRules.inputOf(card) ?: return
    val allowed = run.result?.changesAllowed == true
    val whenText = SparkleRules.whenText(input, locale, words)
    val calendar = SparkleRules.calendarOf(card)
    val meta = listOf(whenText, calendar).filter { it.isNotEmpty() }.joinToString(" · ")
    RcCard {
        Kind(stringResource(if (input.isTask) R.string.att_run_new_task else R.string.att_run_new_event))
        Text(input.title, style = MaterialTheme.typography.bodyMedium, fontWeight = FontWeight.Medium)
        if (meta.isNotEmpty()) Text(meta, style = MaterialTheme.typography.bodySmall)
        run.errors[card.id]?.let { Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.error, fontWeight = FontWeight.Medium) }
        Actions {
            QuietButton(stringResource(R.string.att_run_dismiss), enabled = card.id !in run.busy) { vm.dismissCard(run.id, card.id) }
            QuietButton(stringResource(R.string.att_run_edit), enabled = card.id !in run.busy) {
                actions.editNew(input)
                vm.dismissCard(run.id, card.id)
            }
            if (allowed) FilledButton(stringResource(R.string.att_run_add), busy = card.id in run.busy) { vm.add(run.id, card) }
        }
    }
}

/** "Plan for today · 5 new events": one card, one line each. Edit lists the lines to drop single ones; Add adds the rest in one batch. */
@Composable
private fun PlanCard(run: SparkleRun, locale: Locale, actions: SparkleActions, vm: SparkleRunViewModel, onEdit: () -> Unit) {
    val allowed = run.result?.changesAllowed == true
    val day = remember(run.task) { (run.task as? AiSparkle.PlanMyDay)?.day.orEmpty() }
    val today = (run.task as? AiSparkle.PlanMyDay)?.today.orEmpty()
    val dayText = if (day == today) {
        stringResource(R.string.att_run_day_today)
    } else {
        runCatching { DateTimeFormatter.ofPattern("EEE d MMM", locale).format(LocalDate.parse(day)) }.getOrDefault(day)
    }
    val count = run.open.size
    RcCard {
        Kind(pluralStringResource(R.plurals.att_run_plan, count, dayText, count))
        Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
            for (card in run.open) {
                val input = SparkleRules.inputOf(card) ?: continue
                val line = SparkleRules.planLine(input, locale)
                if (allowed) {
                    Text(line, fontSize = 12.5.sp, lineHeight = 20.sp)
                } else {
                    // Changes are off: a line opens the editor with it filled in, and leaves the plan.
                    Text(
                        line,
                        fontSize = 12.5.sp,
                        lineHeight = 20.sp,
                        modifier = Modifier.fillMaxWidth().clickable { actions.editNew(input); vm.dismissCard(run.id, card.id) }.padding(vertical = 4.dp),
                    )
                }
            }
        }
        run.errors.values.firstOrNull()?.let { Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.error, fontWeight = FontWeight.Medium) }
        if (!allowed) Text(stringResource(R.string.att_run_view_only_plan), style = MaterialTheme.typography.bodySmall)
        Actions {
            QuietButton(stringResource(R.string.att_run_dismiss), enabled = run.busy.isEmpty()) { vm.dismissRun(run.id) }
            if (allowed) {
                QuietButton(stringResource(R.string.att_run_edit), enabled = run.busy.isEmpty(), onClick = onEdit)
                FilledButton(pluralStringResource(R.plurals.att_run_add_events, count, count), busy = run.busy.isNotEmpty()) { vm.addAll(run.id, run.open.map { it.id }) }
            }
        }
    }
}

@Composable
private fun PlanLinesDialog(run: SparkleRun, locale: Locale, onDismiss: () -> Unit, onAdd: (Set<String>) -> Unit) {
    val ticked = remember(run.runId) { mutableStateListOf<String>().apply { addAll(run.open.map { it.id }) } }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.att_run_edit_lines)) },
        text = {
            Column {
                for (card in run.open) {
                    val input = SparkleRules.inputOf(card) ?: continue
                    Row(
                        Modifier.fillMaxWidth().clickable { if (card.id in ticked) ticked.remove(card.id) else ticked.add(card.id) },
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Checkbox(checked = card.id in ticked, onCheckedChange = { if (it) ticked.add(card.id) else ticked.remove(card.id) })
                        Text(SparkleRules.planLine(input, locale), style = MaterialTheme.typography.bodyMedium)
                    }
                }
            }
        },
        confirmButton = {
            TextButton(onClick = { onAdd(ticked.toSet()) }, enabled = ticked.isNotEmpty()) {
                Text(pluralStringResource(R.plurals.att_run_add_events, ticked.size.coerceAtLeast(1), ticked.size))
            }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.cancel)) } },
    )
}

@Composable
private fun FoundCard(run: SparkleRun, found: AiFound, actions: SparkleActions, vm: SparkleRunViewModel) {
    val task = run.task as? AiSparkle.LookInMail
    RcCard {
        Kind(stringResource(R.string.att_run_found))
        // The model's words are data: plain text, never a link that opens by itself.
        Text(found.text, style = MaterialTheme.typography.bodyMedium, fontWeight = FontWeight.Medium, maxLines = 4, overflow = TextOverflow.Ellipsis)
        Text(
            stringResource(R.string.att_run_found_meta, found.message.subject.ifBlank { stringResource(R.string.list_no_subject) }, found.message.from),
            style = MaterialTheme.typography.bodySmall,
            maxLines = 2,
            overflow = TextOverflow.Ellipsis,
        )
        Actions {
            QuietButton(stringResource(R.string.att_run_dismiss)) { vm.dismissExtras(run.id) }
            QuietButton(stringResource(R.string.att_run_open_mail)) { actions.openMail(found.message.threadId, found.message.id) }
            if (task != null) FilledButton(stringResource(R.string.att_run_edit_event)) { actions.editEvent(task.eventId, task.recurrenceId, found.text) }
        }
    }
}

@Composable
private fun VerdictCard(run: SparkleRun, verdict: AiVerdict, vm: SparkleRunViewModel) {
    RcCard {
        Kind(stringResource(R.string.att_run_verdict))
        Text(
            stringResource(
                when (verdict.kind) {
                    AiVerdictKind.FINISHES -> R.string.att_run_verdict_finishes
                    AiVerdictKind.CHANGES -> R.string.att_run_verdict_changes
                    AiVerdictKind.UNRELATED -> R.string.att_run_verdict_unrelated
                },
            ),
            style = MaterialTheme.typography.bodyMedium,
            fontWeight = FontWeight.Medium,
        )
        if (verdict.reason.isNotBlank()) Text(verdict.reason, style = MaterialTheme.typography.bodySmall, maxLines = 4, overflow = TextOverflow.Ellipsis)
        Actions { QuietButton(stringResource(R.string.att_run_dismiss)) { vm.dismissExtras(run.id) } }
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun IdeasCard(run: SparkleRun, ideas: List<String>, vm: SparkleRunViewModel) {
    val colors = MaterialTheme.colorScheme
    RcCard {
        Kind(stringResource(R.string.att_run_ideas))
        FlowRow(horizontalArrangement = Arrangement.spacedBy(6.dp), verticalArrangement = Arrangement.spacedBy(6.dp), modifier = Modifier.padding(top = 2.dp, end = 6.dp)) {
            for (idea in ideas) {
                Text(
                    idea,
                    modifier = Modifier.clip(RoundedCornerShape(12.dp)).background(colors.surfaceContainerLowest).clickable { vm.useIdea(run.id, idea) }.padding(horizontal = 12.dp, vertical = 8.dp),
                    style = MaterialTheme.typography.bodyMedium,
                    color = colors.onSurface,
                )
            }
        }
        Text(stringResource(R.string.att_run_ideas_hint), style = MaterialTheme.typography.bodySmall)
        Actions { QuietButton(stringResource(R.string.att_run_dismiss)) { vm.dismissExtras(run.id) } }
    }
}

/**
 * The results of the runs whose findings belong to one screen that has no list of finding cards (the thread): each run in
 * an outlined box with a title and a close x, above the text.
 */
@Composable
fun SparkleBoxes(ids: Set<String>, actions: SparkleActions, vm: SparkleRunViewModel, modifier: Modifier = Modifier) {
    val runs by vm.runs.collectAsStateWithLifecycle()
    val mine = runs.values.filter { it.id in ids }
    if (mine.isEmpty()) return
    val colors = MaterialTheme.colorScheme
    Column(modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        for (run in mine) {
            androidx.compose.runtime.key(run.id) {
                Column(Modifier.fillMaxWidth().border(BorderStroke(1.dp, colors.outlineVariant), RoundedCornerShape(24.dp)).padding(start = 8.dp, end = 8.dp, bottom = 8.dp)) {
                    Row(Modifier.fillMaxWidth().padding(start = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                        Icon(painterResource(R.drawable.ic_sparkle), contentDescription = null, tint = colors.primary, modifier = Modifier.size(16.dp))
                        Text(
                            stringResource(if (run.task is AiSparkle.ReplyIdeas) R.string.att_run_ideas else R.string.att_run_box_dates),
                            modifier = Modifier.weight(1f).padding(start = 8.dp),
                            style = MaterialTheme.typography.titleSmall,
                            fontWeight = FontWeight.SemiBold,
                        )
                        IconButton(onClick = { if (run.phase == RunPhase.RUNNING) vm.stop(run.id) else vm.dismissRun(run.id) }) {
                            Icon(Icons.Filled.Close, contentDescription = stringResource(R.string.att_box_close))
                        }
                    }
                    SparkleRunBlock(run, actions, vm)
                }
            }
        }
    }
}

/** The question before a run, for the finding that asked it: what is sent and where, the same words as the one-shot actions. */
@Composable
fun SparkleDialog(vm: SparkleRunViewModel, mine: (String) -> Boolean) {
    val pending by vm.pending.collectAsStateWithLifecycle()
    val p = pending ?: return
    if (!mine(p.finding.id)) return
    val words = rememberWords()
    val scope = remember(p) { SparkleRules.scope(p.preview, p.task, words) }
    val host = p.preview.host
    val first = p.ask == SparkleRules.Ask.FIRST
    val text = buildString {
        append(
            if (first) {
                stringResource(R.string.assistant_consent_text, scope, host, p.preview.model)
            } else {
                stringResource(R.string.att_run_ask_text, scope, host, p.preview.model)
            },
        )
        if (p.chatYesMissing || p.preview.readsMail) append("\n\n").append(stringResource(R.string.att_run_ask_chat))
    }
    AlertDialog(
        onDismissRequest = vm::declinePending,
        icon = { Icon(painterResource(R.drawable.ic_sparkle), contentDescription = null) },
        title = { Text(stringResource(R.string.assistant_consent_title, host)) },
        text = { Text(text) },
        confirmButton = { TextButton(onClick = vm::confirm) { Text(stringResource(R.string.assistant_consent_send)) } },
        dismissButton = { TextButton(onClick = vm::declinePending) { Text(stringResource(R.string.cancel)) } },
    )
}
