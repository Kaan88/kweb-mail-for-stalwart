package ovh.kweb.mail.ui.attention

import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent
import android.net.Uri
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.CheckCircle
import androidx.compose.material.icons.filled.Close
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.SwipeToDismissBox
import androidx.compose.material3.SwipeToDismissBoxValue
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.material3.rememberSwipeToDismissBoxState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.painter.Painter
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.positionInParent
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalResources
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.graphics.vector.rememberVectorPainter
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import java.time.ZoneId
import java.util.Locale
import kotlinx.coroutines.flow.first
import ovh.kweb.mail.R
import ovh.kweb.mail.ui.Route
import ovh.kweb.mail.ui.assistant.AssistantRules
import ovh.kweb.mail.ui.assistant.serviceName
import ovh.kweb.mail.ui.calendar.EventPrefill
import ovh.kweb.mail.ui.list.Groups
import uniffi.kweb_core.AttentionRule
import uniffi.kweb_core.Finding
import uniffi.kweb_core.PhoneAction
import uniffi.kweb_core.TimeSlot

/** What the findings can open: a conversation, or the editor (the user saves it himself). */
class AttentionNav(
    val openThread: (threadId: String, messageId: String?) -> Unit,
    val openEditor: (Route.EditEvent) -> Unit,
    /** Shows the Tasks view of the Calendar tab. */
    val openTasks: () -> Unit = {},
    /** Opens the composer on a draft (a reply idea was tapped); nothing is sent from here. */
    val openCompose: (draftId: String) -> Unit = {},
)

/**
 * The sparkle: a tonal tertiary button with how many things were found. Not drawn at all for none ("a quiet day shows
 * nothing extra"). [size] 40 dp in a top bar, 32 dp inside a tile.
 */
@Composable
fun SparkleButton(count: Int, onClick: () -> Unit, modifier: Modifier = Modifier, size: androidx.compose.ui.unit.Dp = 40.dp) {
    if (count <= 0) return
    val colors = MaterialTheme.colorScheme
    val label = pluralStringResource(R.plurals.att_sparkle, count, count)
    Box(modifier.semantics { contentDescription = label }) {
        Box(
            Modifier.size(size).clip(CircleShape).background(colors.tertiaryContainer).clickable(onClick = onClick),
            contentAlignment = Alignment.Center,
        ) {
            Icon(painterResource(R.drawable.ic_sparkle), contentDescription = null, tint = colors.onTertiaryContainer, modifier = Modifier.size(size * 0.5f))
        }
        Box(
            Modifier
                .align(Alignment.TopEnd)
                .offset(x = 5.dp, y = (-4).dp)
                .heightIn(min = 17.dp)
                .widthIn(min = 17.dp)
                .clip(RoundedCornerShape(9.dp))
                .background(colors.primary)
                .padding(horizontal = 4.dp),
            contentAlignment = Alignment.Center,
        ) {
            Text(if (count > 9) "9+" else count.toString(), color = colors.onPrimary, fontSize = 10.5.sp, fontWeight = FontWeight.Bold)
        }
    }
}

/** The small mark beside a mail a finding is about: no button, no count. */
@Composable
fun SparkleMark(modifier: Modifier = Modifier, tint: Color = MaterialTheme.colorScheme.primary) {
    Icon(
        painterResource(R.drawable.ic_sparkle),
        contentDescription = stringResource(R.string.att_mark),
        tint = tint,
        modifier = modifier.size(13.dp),
    )
}

@Composable
private fun ruleIcon(rule: AttentionRule): Painter = when (rule) {
    AttentionRule.OVERLAP -> painterResource(R.drawable.ic_schedule)
    AttentionRule.NO_PLACE, AttentionRule.TIGHT_CHANGE -> painterResource(R.drawable.ic_place)
    AttentionRule.OPEN_INVITATION, AttentionRule.BUSY_DAY -> painterResource(R.drawable.ic_calendar)
    AttentionRule.OVERDUE, AttentionRule.NO_TIME_TODAY, AttentionRule.TOO_MANY -> rememberVectorPainter(Icons.Filled.CheckCircle)
    AttentionRule.MAIL_MAY_FINISH, AttentionRule.DATE_IN_MAIL, AttentionRule.DEADLINE_WORDS, AttentionRule.INVITATION_FILE -> painterResource(R.drawable.ic_mail)
    AttentionRule.WAITING_FOR_YOU, AttentionRule.NO_REPLY -> painterResource(R.drawable.ic_reply)
}

/** Does what a phone button does. Nothing here reaches the assistant or sends anything. */
@Stable
class FindingRunner(private val vm: AttentionViewModel, private val nav: AttentionNav, private val context: Context, private val zone: ZoneId) {
    /** The actions that act at once; Find a slot, Answer and Free time are drawn by the card itself and never come here. */
    fun run(f: Finding, action: PhoneAction) {
        when (action) {
            is PhoneAction.MoveEvent -> nav.openEditor(Route.EditEvent(action.eventId, null))
            is PhoneAction.OpenMail -> nav.openThread(action.threadId, action.messageId)
            is PhoneAction.OpenMails -> action.messageIds.firstOrNull()?.let { id -> vm.openMessage(id) { thread -> nav.openThread(thread, id) } }
            is PhoneAction.MoveToday -> vm.moveToday(action.taskIds)
            is PhoneAction.MarkDone -> vm.markDone(action.taskId)
            is PhoneAction.AddToCalendar -> openPrefill(action.prefill)
            is PhoneAction.AddTask -> openPrefill(action.prefill)
            is PhoneAction.Directions -> directions(action.from, action.to)
            is PhoneAction.FindSlot, is PhoneAction.Respond, is PhoneAction.FreeTime -> Unit
        }
    }

    /** What the cards of a sparkle run open: the editor (new, or an existing event with a place), a mail. */
    fun sparkleActions(): SparkleActions = SparkleActions(
        editNew = { input ->
            nav.openEditor(Route.EditEvent(null, input.start.take(10), prefill = SparkleRules.prefillOf(input)))
        },
        editEvent = { eventId, recurrenceId, place -> nav.openEditor(Route.EditEvent(eventId, null, place = place?.takeIf { it.isNotBlank() })) },
        openMail = { threadId, messageId -> nav.openThread(threadId, messageId) },
    )

    fun openDraft(draftId: String) = nav.openCompose(draftId)

    /** A tap on a tasks finding that has no mail to open: the Tasks view. */
    fun openTasks() = nav.openTasks()

    /** A slot chip: the event in the editor at that time (the user saves it himself). */
    fun openSlot(action: PhoneAction.FindSlot, slot: TimeSlot) {
        val start = AttentionRules.localStart(slot.start, zone)
        nav.openEditor(Route.EditEvent(action.eventId, start.take(10), startAt = start))
    }

    fun respond(action: PhoneAction.Respond, status: String) = vm.respond(action.eventId, status)

    private fun openPrefill(p: uniffi.kweb_core.Prefill) {
        nav.openEditor(Route.EditEvent(null, p.start.take(10), prefill = EventPrefill(p.title, p.start, p.allDay, p.isTask, p.durationMinutes.toInt())))
    }

    private fun directions(from: String, to: String) {
        val uri = Uri.parse("https://www.google.com/maps/dir/?api=1&origin=${Uri.encode(from)}&destination=${Uri.encode(to)}")
        val intent = Intent(Intent.ACTION_VIEW, uri).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        try {
            context.startActivity(intent)
        } catch (e: ActivityNotFoundException) {
            vm.say(R.string.att_no_maps)
        }
    }
}

@Composable
fun rememberFindingRunner(vm: AttentionViewModel, nav: AttentionNav): FindingRunner {
    val context = LocalContext.current
    return remember(vm, nav, context) { FindingRunner(vm, nav, context, ZoneId.systemDefault()) }
}

private enum class Mode { NONE, SLOTS, ANSWER, FREE }

/**
 * One finding as a card: what was found in a line, one more line, and at most two buttons (a phone action outlined, a
 * service action tonal with a sparkle before the verb). [onService] is where the service buttons are wired; the core
 * only returns a finding's service action when an assistant is set up, so the button is only there then.
 */
@Composable
fun FindingCard(
    finding: Finding,
    shape: androidx.compose.ui.graphics.Shape,
    runner: FindingRunner,
    clock: WordClock,
    modifier: Modifier = Modifier,
    container: Color = MaterialTheme.colorScheme.surfaceContainerLow,
    onService: (Finding) -> Unit = {},
    /** Its service button is not drawn: the result of that run is shown beside it. */
    hideService: Boolean = false,
) {
    val colors = MaterialTheme.colorScheme
    val resources = LocalResources.current
    val texts = remember(resources) { ResourceTexts(resources) }
    val buttons = remember(finding, hideService) { if (hideService) Buttons(finding.phone.take(2), null) else AttentionRules.buttons(finding) }
    var mode by remember(finding.id) { mutableStateOf(Mode.NONE) }
    val open = AttentionRules.cardAction(finding)
    Column(
        modifier.fillMaxWidth().clip(shape).background(container)
            .then(
                when {
                    open != null -> Modifier.clickable { runner.run(finding, open) }
                    AttentionRules.opensTasks(finding) -> Modifier.clickable { runner.openTasks() }
                    else -> Modifier
                },
            )
            .padding(horizontal = 12.dp, vertical = 10.dp),
        verticalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        Row(horizontalArrangement = Arrangement.spacedBy(11.dp), verticalAlignment = Alignment.Top) {
            Box(Modifier.size(34.dp).clip(RoundedCornerShape(12.dp)).background(colors.surfaceContainerHighest), contentAlignment = Alignment.Center) {
                Icon(ruleIcon(finding.rule), contentDescription = null, tint = colors.primary, modifier = Modifier.size(18.dp))
            }
            Column(Modifier.weight(1f)) {
                Text(AttentionWords.title(finding, clock, texts), style = MaterialTheme.typography.bodyMedium, fontWeight = FontWeight.Medium)
                AttentionWords.meta(finding, clock, texts)?.let {
                    Text(it, style = MaterialTheme.typography.bodySmall, color = colors.onSurfaceVariant, maxLines = 2, overflow = TextOverflow.Ellipsis)
                }
            }
        }
        when (mode) {
            Mode.SLOTS -> SlotChips(finding, clock, texts, runner)
            Mode.FREE -> FreeList(finding, clock, texts)
            Mode.ANSWER -> AnswerButtons(finding, runner)
            Mode.NONE -> Unit
        }
        if (buttons.phone.isNotEmpty() || buttons.service != null) {
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(6.dp, Alignment.End), verticalAlignment = Alignment.CenterVertically) {
                for (action in buttons.phone) {
                    OutlinedButton(
                        onClick = {
                            when (action) {
                                is PhoneAction.FindSlot -> mode = if (mode == Mode.SLOTS) Mode.NONE else Mode.SLOTS
                                is PhoneAction.Respond -> mode = if (mode == Mode.ANSWER) Mode.NONE else Mode.ANSWER
                                is PhoneAction.FreeTime -> mode = if (mode == Mode.FREE) Mode.NONE else Mode.FREE
                                else -> runner.run(finding, action)
                            }
                        },
                        modifier = Modifier.heightIn(min = 32.dp),
                        contentPadding = androidx.compose.foundation.layout.PaddingValues(horizontal = 14.dp),
                    ) { Text(AttentionWords.phoneLabel(action, finding, texts), fontSize = 12.5.sp, maxLines = 1) }
                }
                buttons.service?.let { service ->
                    FilledTonalButton(
                        onClick = { onService(finding) },
                        modifier = Modifier.heightIn(min = 32.dp),
                        colors = ButtonDefaults.filledTonalButtonColors(containerColor = colors.tertiaryContainer, contentColor = colors.onTertiaryContainer),
                        contentPadding = androidx.compose.foundation.layout.PaddingValues(horizontal = 14.dp),
                    ) {
                        Icon(painterResource(R.drawable.ic_sparkle), contentDescription = null, modifier = Modifier.size(16.dp))
                        Text(stringResource(AttentionWords.serviceLabel(service)), fontSize = 12.5.sp, maxLines = 1, modifier = Modifier.padding(start = 6.dp))
                    }
                }
            }
        }
    }
}

@OptIn(androidx.compose.foundation.layout.ExperimentalLayoutApi::class)
@Composable
private fun SlotChips(f: Finding, clock: WordClock, texts: Texts, runner: FindingRunner) {
    val action = f.phone.filterIsInstance<PhoneAction.FindSlot>().firstOrNull() ?: return
    val colors = MaterialTheme.colorScheme
    Column(Modifier.padding(start = 45.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
        if (f.slots.isEmpty()) {
            Text(stringResource(R.string.att_slots_none), style = MaterialTheme.typography.bodySmall, color = colors.onSurfaceVariant)
        } else {
            val length = AttentionWords.length(f.slots.first().end - f.slots.first().start, texts)
            Text(stringResource(R.string.att_slots_help, length), style = MaterialTheme.typography.bodySmall, color = colors.onSurfaceVariant)
            FlowRow(horizontalArrangement = Arrangement.spacedBy(6.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                for (slot in f.slots) {
                    Box(
                        Modifier.heightIn(min = 30.dp).clip(RoundedCornerShape(8.dp)).background(colors.secondaryContainer)
                            .clickable { runner.openSlot(action, slot) }.padding(horizontal = 11.dp),
                        contentAlignment = Alignment.Center,
                    ) {
                        Text(AttentionWords.slotChip(slot.start, clock, f.slots.first().start), color = colors.onSecondaryContainer, fontSize = 12.sp, fontWeight = FontWeight.Medium)
                    }
                }
            }
        }
    }
}

@Composable
private fun FreeList(f: Finding, clock: WordClock, texts: Texts) {
    val colors = MaterialTheme.colorScheme
    Column(Modifier.padding(start = 45.dp)) {
        if (f.slots.isEmpty()) {
            Text(stringResource(R.string.att_slots_none), style = MaterialTheme.typography.bodySmall, color = colors.onSurfaceVariant)
        } else {
            for (slot in f.slots) Text(AttentionWords.range(slot.start, slot.end, clock, texts), style = MaterialTheme.typography.bodyMedium)
        }
    }
}

@Composable
private fun AnswerButtons(f: Finding, runner: FindingRunner) {
    val action = f.phone.filterIsInstance<PhoneAction.Respond>().firstOrNull() ?: return
    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(6.dp, Alignment.End)) {
        TextButton(onClick = { runner.respond(action, "declined") }) { Text(stringResource(R.string.att_decline)) }
        TextButton(onClick = { runner.respond(action, "tentative") }) { Text(stringResource(R.string.att_maybe)) }
        FilledTonalButton(onClick = { runner.respond(action, "accepted") }) { Text(stringResource(R.string.att_accept)) }
    }
}

/** Findings as one group of cards a small gap apart; a swipe sideways dismisses one and it stays away. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun FindingList(
    findings: List<Finding>,
    runner: FindingRunner,
    clock: WordClock,
    onDismiss: (Finding) -> Unit,
    modifier: Modifier = Modifier,
    container: Color = MaterialTheme.colorScheme.surfaceContainerLow,
    onService: (Finding) -> Unit = {},
    sparkle: SparkleRunViewModel = androidx.lifecycle.viewmodel.compose.viewModel(),
) {
    val runs by sparkle.runs.collectAsStateWithLifecycle()
    val start = remember(sparkle, onService) { { f: Finding -> onService(f); sparkle.start(f) } }
    Column(modifier, verticalArrangement = Arrangement.spacedBy(2.dp)) {
        findings.forEachIndexed { index, finding ->
            androidx.compose.runtime.key(finding.id) {
                val shape = Groups.shape(index, findings.size)
                val run = runs[finding.id]
                if (run != null) {
                    SparkleRunBlock(run, runner.sparkleActions(), sparkle)
                    // Check the mail: the verdict is beside the finding, whose Mark done stays the user's own change.
                    if (run.phase == RunPhase.DONE && run.task is uniffi.kweb_core.AiSparkle.CheckTheMail) {
                        FindingCard(finding, shape, runner, clock, container = container, hideService = true)
                    }
                } else {
                val state = rememberSwipeToDismissBoxState(
                    confirmValueChange = {
                        if (it != SwipeToDismissBoxValue.Settled) onDismiss(finding)
                        it != SwipeToDismissBoxValue.Settled
                    },
                )
                SwipeToDismissBox(
                    state = state,
                    backgroundContent = {
                        Box(Modifier.fillMaxWidth().clip(shape).background(MaterialTheme.colorScheme.surfaceContainerHighest).padding(horizontal = 20.dp), contentAlignment = Alignment.CenterEnd) {
                            Icon(Icons.Filled.Close, contentDescription = stringResource(R.string.att_dismiss), tint = MaterialTheme.colorScheme.onSurfaceVariant)
                        }
                    },
                ) {
                    FindingCard(finding, shape, runner, clock, container = container, onService = start)
                }
                }
            }
        }
    }
    SparkleDialog(sparkle) { id -> findings.any { it.id == id } }
}

@Composable
fun rememberWordClock(nowSeconds: Long): WordClock = remember(nowSeconds) { WordClock(ZoneId.systemDefault(), Locale.getDefault(), nowSeconds) }

/**
 * "Worth a look": a modal sheet with the findings grouped as the Home tiles are (Mail, Calendar, Tasks), scrolled to
 * [scrollTo]'s group. The footer names the service and model once, only when some finding has a service button.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun AttentionSheet(
    vm: AttentionViewModel,
    nav: AttentionNav,
    scrollTo: Group?,
    onClose: () -> Unit,
    onService: (Finding) -> Unit = {},
) {
    val state by vm.state.collectAsStateWithLifecycle()
    val closing = remember(nav, onClose) {
        AttentionNav(
            openThread = { t, m -> onClose(); nav.openThread(t, m) },
            openEditor = { r -> onClose(); nav.openEditor(r) },
            openTasks = { onClose(); nav.openTasks() },
            openCompose = { d -> onClose(); nav.openCompose(d) },
        )
    }
    val sparkle: SparkleRunViewModel = androidx.lifecycle.viewmodel.compose.viewModel()
    val snackbar = remember { androidx.compose.material3.SnackbarHostState() }
    SparkleEvents(sparkle, vm, snackbar, onOpenDraft = closing.openCompose)
    val runner = rememberFindingRunner(vm, closing)
    val colors = MaterialTheme.colorScheme
    val clock = rememberWordClock(state.at.takeIf { it > 0 } ?: (System.currentTimeMillis() / 1000))
    val sheet = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    val scroll = rememberScrollState()
    val offsets = remember { mutableStateMapOf<Group, Int>() }
    val groups = AttentionRules.groups(state.findings)
    // Nothing left to look at (everything was swiped or handled): the sheet goes.
    LaunchedEffect(state.loaded, groups.isEmpty()) { if (state.loaded && groups.isEmpty()) onClose() }
    LaunchedEffect(scrollTo) {
        if (scrollTo != null) {
            val y = snapshotFlow { offsets[scrollTo] }.first { it != null }
            scroll.scrollTo(y ?: 0)
        }
    }
    ModalBottomSheet(onDismissRequest = onClose, sheetState = sheet, containerColor = colors.surfaceContainerLow) {
        Column(Modifier.fillMaxWidth().navigationBarsPadding()) {
            Row(Modifier.fillMaxWidth().padding(start = 24.dp, end = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                Text(stringResource(R.string.att_sheet_title), style = MaterialTheme.typography.titleLarge, modifier = Modifier.weight(1f))
                IconButton(onClick = onClose) { Icon(Icons.Filled.Close, contentDescription = stringResource(R.string.att_close)) }
            }
            val found = remember(state.at) {
                java.time.format.DateTimeFormatter.ofLocalizedTime(java.time.format.FormatStyle.SHORT).format(java.time.Instant.ofEpochSecond(clock.nowSeconds).atZone(clock.zone))
            }
            Text(
                stringResource(R.string.att_sheet_sub, found),
                modifier = Modifier.padding(horizontal = 24.dp),
                style = MaterialTheme.typography.bodySmall,
                color = colors.onSurfaceVariant,
            )
            Column(
                Modifier.weight(1f, fill = false).verticalScroll(scroll).padding(horizontal = 12.dp, vertical = 8.dp),
                verticalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                for ((group, list) in groups) {
                    Row(
                        Modifier.padding(start = 12.dp, top = 8.dp).onGloballyPositioned { offsets[group] = it.positionInParent().y.toInt() },
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Text(stringResource(group.title), style = MaterialTheme.typography.labelLarge, color = colors.onSurfaceVariant, fontWeight = FontWeight.SemiBold)
                        Text(" · ${list.size}", style = MaterialTheme.typography.labelLarge, color = colors.onSurfaceVariant)
                    }
                    FindingList(list, runner, clock, onDismiss = vm::dismiss, container = colors.surfaceContainerHigh, onService = onService)
                }
            }
            // A result's Undo shows here: the screen's own snackbar is under the sheet.
            androidx.compose.material3.SnackbarHost(snackbar)
            val service = state.service
            if (service != null && state.findings.any { it.service != null }) {
                val name = AssistantRules.serviceOf(service.baseUrl).let {
                    if (it == AssistantRules.Service.OTHER) AssistantRules.hostOf(service.baseUrl).ifEmpty { stringResource(R.string.assistant_svc_other) } else stringResource(serviceName(it))
                }
                Row(Modifier.fillMaxWidth().padding(horizontal = 24.dp, vertical = 10.dp), horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.Top) {
                    Icon(painterResource(R.drawable.ic_sparkle), contentDescription = null, tint = colors.onSurfaceVariant, modifier = Modifier.size(16.dp))
                    Text(stringResource(R.string.att_sheet_foot, "$name · ${service.model}"), style = MaterialTheme.typography.bodySmall, color = colors.onSurfaceVariant)
                }
            }
        }
    }
}
